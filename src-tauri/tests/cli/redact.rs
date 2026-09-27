//! `tpdf redact` against real workers and real documents.
//!
//! Part of `tests/cli.rs`'s binary, which is its own worker --- and, for this
//! file, its own OCR worker too, since the application's redaction path runs
//! the OCR gate. The helpers it uses (`tool`, `fixture`, `scratch`) are that
//! file's.
//!
//! Five things, each with the control that keeps it from passing vacuously:
//!
//! 1. **What it finds is gone and what it does not find is not.** A document
//!    built here carries an e-mail address, an IBAN, a code word on two pages
//!    (one of them cropped) and a line reachable only by a rectangle, beside two
//!    `CONTROL-KEEP` lines nothing matches. By `--text`, by `--pattern`, by
//!    `--regions` and all three at once, the written file's text --- read by
//!    the built tool, by Poppler's `pdftotext`, and byte for byte through
//!    `qpdf --qdf` --- holds none of the matched strings and both controls.
//!    Control: the same three readers find every matched string in the input.
//! 2. **`--dry-run` writes nothing, and says what the write then does**: its
//!    regions, removals and pages equal the real run's.
//! 3. **The verdict is the application's.** For three documents, the regions
//!    the tool marks are marked again in this process and handed to
//!    `redaction::redact_copy_asked` --- the body of the window's *Redact and
//!    save as* --- and the tool's `verified` and every reason equal what that
//!    returns, and its exit code is 0 exactly when that is verified.
//!    `text-marked.pdf` is the fixture the application reports *not verified*
//!    for a reason that is not the platform's: an annotation keeps a copy of the
//!    removed line. Its exit code is 1, and the file is kept.
//! 4. **Refusals write nothing**: a signed document without
//!    `--invalidate-signatures` (and with it, the copy's signature no longer
//!    verifies), an XFA form, an output that exists, an output that is the
//!    input under another name, a locked document without its password (and
//!    with it, the copy stays encrypted).
//! 5. With `TPDF_REDACT_PROBE=<dir>`, the built document, its redacted copy and
//!    the strings expected gone and kept are left there for pypdf
//!    (`scripts/redact_pdf_check.py`).

use std::path::{Path, PathBuf};
use std::process::Command;

use lopdf::{dictionary, Document, Object, Stream};
use tpdf_lib::cli::redact::{match_regions, search_pages};
use tpdf_lib::cli::report::SearchKind;
use tpdf_lib::render::{Backend, RenderService};
use tpdf_lib::search::{Options, Prepared};

use super::{fixture, library_dir, scratch, tool, Report};

/// Each line of each page: the words `contacts_pdf` draws, one text object a
/// line, at `(x, y)` in points up from the bottom.
const PAGE_ONE: [(&str, i64); 5] = [
    ("Contact: jane.doe@example.com", 700),
    ("IBAN DE89 3704 0044 0532 0130 00 for payroll", 670),
    ("CONTROL-KEEP alpha", 640),
    ("Codeword Rumpelstilzchen", 610),
    ("Sign here: J. Doe", 580),
];
const PAGE_TWO: [(&str, i64); 3] = [
    ("Second copy: jane.doe@example.com", 700),
    ("CONTROL-KEEP beta", 670),
    ("Codeword Rumpelstilzchen", 640),
];

/// The e-mail pattern the README shows.
pub(super) const EMAIL: &str = r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}";
/// The IBAN-shaped pattern the README shows.
pub(super) const IBAN: &str = r"\b[A-Z]{2}[0-9]{2}(?: ?[A-Z0-9]{4}){3,7}(?: ?[A-Z0-9]{1,3})?\b";

/// Two pages of Helvetica, each line its own `BT ... ET`, so a removal that
/// takes a whole operation takes one line and never its neighbour. The second
/// page has a crop box that does not start at the sheet's corner, which is
/// what makes `--regions`' "as displayed" a claim that can be wrong.
pub(super) fn contacts_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = Vec::new();
    for (lines, crop) in [
        (&PAGE_ONE[..], None),
        (&PAGE_TWO[..], Some([36, 36, 576, 756])),
    ] {
        let mut body = String::new();
        for (text, y) in lines {
            body.push_str(&format!("BT /F1 12 Tf 72 {y} Td ({text}) Tj ET\n"));
        }
        let content = doc.add_object(Stream::new(dictionary! {}, body.into_bytes()));
        let mut page = dictionary! {
            "Type" => "Page", "Parent" => pages, "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content,
        };
        if let Some(crop) = crop {
            page.set(
                "CropBox",
                crop.iter().map(|v| Object::Integer(*v)).collect::<Vec<_>>(),
            );
        }
        kids.push(Object::Reference(doc.add_object(page)));
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// One page drawing the same Form XObject twice, and a line of its own.
///
/// The removal leaves a form the page draws more than once --- taking the one
/// drawing a region covers would leave the other, and the object itself ---
/// so the copy's text still holds the word, which is the case the tool's
/// search of the written file exists for.
pub(super) fn shared_form_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let form = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 300.into(), 20.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        },
        b"BT /F1 12 Tf 0 4 Td (Codeword Rumpelstilzchen) Tj ET".to_vec(),
    ));
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q 1 0 0 1 72 700 cm /Fm1 Do Q\nq 1 0 0 1 72 600 cm /Fm1 Do Q\n\
          BT /F1 12 Tf 72 500 Td (CONTROL-KEEP gamma) Tj ET\n"
            .to_vec(),
    ));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font },
            "XObject" => dictionary! { "Fm1" => form },
        },
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// `contacts_pdf` with an `/AcroForm` carrying an `/XFA` packet.
fn xfa_pdf() -> Vec<u8> {
    let mut doc = Document::load_mem(&contacts_pdf()).expect("parses");
    let packet = doc.add_object(Stream::new(dictionary! {}, b"<xdp:xdp/>".to_vec()));
    let form = doc.add_object(dictionary! { "Fields" => Vec::<Object>::new(), "XFA" => packet });
    let root = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("root");
    doc.get_dictionary_mut(root)
        .expect("catalog")
        .set("AcroForm", form);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

fn parsed(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).unwrap_or(serde_json::Value::Null)
}

/// The built tool's text for `path`.
fn text_of(path: &Path, env: &[(&str, &str)], extra: &[&str]) -> String {
    let at = path.display().to_string();
    let mut args = vec!["text", at.as_str()];
    args.extend_from_slice(extra);
    tool(&args, env).1
}

/// Poppler's reading, or `None` when it is not installed.
fn pdftotext(path: &Path) -> Option<String> {
    let out = Command::new("pdftotext")
        .args([path.display().to_string().as_str(), "-"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The file's bytes with every stream uncompressed, or `None` without qpdf.
fn qdf(path: &Path, dir: &Path) -> Option<Vec<u8>> {
    let out = dir.join(format!("{}.qdf", path.file_name()?.to_string_lossy()));
    let status = Command::new("qpdf")
        .args(["--qdf", "--object-streams=disable"])
        .arg(path)
        .arg(&out)
        .status()
        .ok()?;
    // 0 or 3 (warnings): the file was written.
    (status.code() == Some(0) || status.code() == Some(3))
        .then(|| std::fs::read(&out).ok())
        .flatten()
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

/// What each reader finds of `gone` and `kept` in `path`: a list of
/// complaints, empty when every matched string is absent and every control
/// present, for every reader that is installed.
fn readers(path: &Path, dir: &Path, gone: &[&str], kept: &[&str]) -> Vec<String> {
    let mut wrong = Vec::new();
    let mut texts: Vec<(&str, Vec<u8>)> = vec![("tpdf text", text_of(path, &[], &[]).into_bytes())];
    if let Some(poppler) = pdftotext(path) {
        texts.push(("pdftotext", poppler.into_bytes()));
    }
    if let Some(bytes) = qdf(path, dir) {
        texts.push(("qpdf --qdf", bytes));
    }
    for (reader, text) in &texts {
        for word in gone {
            if contains(text, word) {
                wrong.push(format!("{reader} still finds {word:?}"));
            }
        }
        for word in kept {
            if !contains(text, word) {
                wrong.push(format!("{reader} lost the control {word:?}"));
            }
        }
    }
    wrong
}

/// Which readers are installed, for the report's names.
fn installed() -> String {
    let mut have = vec!["tpdf text"];
    if Command::new("pdftotext").arg("-v").output().is_ok() {
        have.push("pdftotext");
    }
    if Command::new("qpdf").arg("--version").output().is_ok() {
        have.push("qpdf --qdf");
    }
    have.join(", ")
}

/// A page's text as the service's workers extract it.
fn wait<T: Send + 'static, E: Send + 'static + From<String>>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, E>) + Send>),
) -> Result<T, E> {
    let (tx, rx) = std::sync::mpsc::channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    rx.recv_timeout(std::time::Duration::from_secs(120))
        .unwrap_or_else(|_| Err(E::from("the render service did not answer".to_string())))
}

/// The regions file for `line` on `page` (counted from 1), measured from the
/// page's own extraction: each character's box, in the displayed page's
/// points, unioned, as `x, y, w, h`.
fn region_for(path: &Path, page: u32, line: &str) -> serde_json::Value {
    let service = RenderService::start_with(library_dir(), Backend::Worker);
    let info = wait(|r| service.open(path.to_path_buf(), true, None, r)).expect("opens");
    let text = wait(|r| service.text(info.id, page - 1, None, r)).expect("text");
    let codes: Vec<char> = text
        .codes
        .iter()
        .map(|c| char::from_u32(*c).unwrap_or('\u{fffd}'))
        .collect();
    let joined: String = codes.iter().collect();
    let start = joined.find(line).expect("the line is on the page");
    let start = joined[..start].chars().count();
    let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for at in start..start + line.chars().count() {
        let q = &text.boxes[at * 4..at * 4 + 4];
        if q[2] > q[0] {
            l = l.min(q[0]);
            t = t.min(q[1]);
            r = r.max(q[2]);
            b = b.max(q[3]);
        }
    }
    serde_json::json!({ "page": page, "rect": [l, t, r - l, b - t] })
}

/// The application's path, in this process, on the regions the tool marks for
/// `queries`: marked on an `Edits`, asked, written, verified.
fn app_path(
    path: &Path,
    out: &Path,
    queries: &[(SearchKind, &str)],
    password: Option<&str>,
) -> Result<tpdf_lib::redact::Applied, String> {
    use tpdf_lib::redaction::{ask_redactions, redact_copy_asked};
    let service = RenderService::start_with(library_dir(), Backend::Worker);
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let hashing = file.try_clone().map_err(|e| e.to_string())?;
    let info = wait(|r| {
        service.open_handed(
            path.to_path_buf(),
            Some(file),
            true,
            password.map(str::to_string),
            r,
        );
    })
    .map_err(|refusal| refusal.reason)?;
    let count = u32::try_from(info.page_count).map_err(|e| e.to_string())?;
    let compiled: Vec<(SearchKind, String, Prepared)> = queries
        .iter()
        .map(|(kind, q)| {
            let options = Options {
                regex: *kind == SearchKind::Pattern,
                ..Options::default()
            };
            (
                *kind,
                (*q).to_string(),
                Prepared::new(q, options).expect("compiles"),
            )
        })
        .collect();
    let pages: Vec<u32> = (1..=count).collect();
    let found = search_pages(&compiled, &pages, |at| {
        wait(|r| service.text(info.id, at, None, r))
    })?;
    let edits = tpdf_lib::edits::Edits::default();
    edits.open(
        info.id,
        count,
        Some(tpdf_lib::fingerprint::Opened {
            file: hashing,
            what: path.to_path_buf(),
        }),
    );
    for (page, areas) in match_regions(&found) {
        for area in areas {
            edits.redact(info.id, u64::from(page) + 1, area)?;
        }
    }
    let asked = tauri::async_runtime::block_on(ask_redactions(&edits, &service, info.id))?;
    tauri::async_runtime::block_on(redact_copy_asked(
        &service,
        library_dir(),
        info.id,
        asked,
        path.display().to_string(),
        out.display().to_string(),
        None,
    ))
    .map_err(tpdf_lib::redaction::Stopped::into_message)
}

/// The tool's arguments for `queries`.
fn query_args(queries: &[(SearchKind, &str)]) -> Vec<String> {
    queries
        .iter()
        .flat_map(|(kind, q)| {
            [
                match kind {
                    SearchKind::Text => "--text".to_string(),
                    SearchKind::Pattern => "--pattern".to_string(),
                },
                (*q).to_string(),
            ]
        })
        .collect()
}

fn run(args: &[String], env: &[(&str, &str)]) -> (i32, serde_json::Value, String) {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = tool(&refs, env);
    (code, parsed(&stdout), stderr)
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

/// 1 and 2: what goes, what stays, and what a dry run predicts.
pub(super) fn removes_what_it_finds(report: &mut Report) {
    let dir = scratch("redact");
    let input = dir.join("contacts.pdf");
    std::fs::write(&input, contacts_pdf()).expect("contacts");
    let kept = ["CONTROL-KEEP alpha", "CONTROL-KEEP beta"];
    let secrets = [
        "jane.doe@example.com",
        "DE89 3704 0044 0532 0130 00",
        "Rumpelstilzchen",
        "Sign here: J. Doe",
    ];

    // The control: every reader finds every string in the input.
    let before = readers(&input, &dir, &[], &secrets);
    report.check(
        &format!("control: {} find every string in the input", installed()),
        before.is_empty(),
        &before.join("; "),
    );

    let regions_file = dir.join("regions.json");
    std::fs::write(
        &regions_file,
        serde_json::json!([region_for(&input, 1, "Sign here: J. Doe")]).to_string(),
    )
    .expect("regions");

    let cases: [(&str, Vec<String>, Vec<&str>); 4] = [
        (
            "by --text",
            query_args(&[(SearchKind::Text, "rumpelstilzchen")]),
            vec!["Rumpelstilzchen"],
        ),
        (
            "by --pattern, an e-mail address and an IBAN",
            query_args(&[(SearchKind::Pattern, EMAIL), (SearchKind::Pattern, IBAN)]),
            vec!["jane.doe@example.com", "DE89 3704 0044 0532 0130 00"],
        ),
        (
            "by --regions",
            vec!["--regions".into(), s(&regions_file)],
            vec!["Sign here: J. Doe"],
        ),
        (
            "by all three at once",
            [
                query_args(&[
                    (SearchKind::Text, "Rumpelstilzchen"),
                    (SearchKind::Pattern, EMAIL),
                    (SearchKind::Pattern, IBAN),
                ]),
                vec!["--regions".into(), s(&regions_file)],
            ]
            .concat(),
            secrets.to_vec(),
        ),
    ];
    let probe = std::env::var_os("TPDF_REDACT_PROBE").map(PathBuf::from);
    for (at, (what, args, gone)) in cases.iter().enumerate() {
        let out = dir.join(format!("redacted-{at}.pdf"));
        let line = [
            vec![
                "redact".to_string(),
                s(&input),
                "-o".into(),
                s(&out),
                "--json".into(),
            ],
            args.clone(),
        ]
        .concat();
        let (code, json, stderr) = run(&line, &[]);
        report.check(
            &format!("{what}: the copy is written and the report says so"),
            (code == 0 || code == 1)
                && json["written"] == true
                && json["verified"] == (code == 0)
                && out.exists(),
            &format!("exit {code}: {stderr}"),
        );
        // Nothing on stderr: a verdict is not an error, and every worker the
        // run spawned shares this stream --- including a pre-spawned spare
        // that ends without a document (`docs/TRAPS.md`).
        report.check(
            &format!("{what}: nothing is printed on stderr"),
            stderr.is_empty(),
            &stderr,
        );
        let wrong = readers(&out, &dir, gone, &kept);
        report.check(
            &format!("{what}: no reader finds a matched string, and every one finds both controls"),
            wrong.is_empty(),
            &wrong.join("; "),
        );
        // The other secrets are untouched: the removal took what was asked.
        let others: Vec<&str> = secrets
            .iter()
            .copied()
            .filter(|x| !gone.contains(x))
            .collect();
        let over = readers(&out, &dir, &[], &others);
        report.check(
            &format!("{what}: what was not asked for is still there"),
            over.is_empty(),
            &over.join("; "),
        );

        // 2: a dry run of the same line writes nothing and predicts this run.
        let dry_out = dir.join(format!("dry-{at}.pdf"));
        let mut dry = line.clone();
        dry[3] = s(&dry_out);
        dry.push("--dry-run".into());
        let (dry_code, dry_json, _) = run(&dry, &[]);
        let same = |key: &str| dry_json[key] == json[key];
        let pages_agree = dry_json["pages"]
            .as_array()
            .zip(json["pages"].as_array())
            .is_some_and(|(a, b)| {
                a.len() == b.len()
                    && a.iter().zip(b).all(|(x, y)| {
                        [
                            "page",
                            "hits",
                            "regions",
                            "text_removals",
                            "form_text_removals",
                            "image_removals",
                            "taking",
                            "left",
                        ]
                        .iter()
                        .all(|k| x[k] == y[k])
                    })
            });
        report.check(
            &format!("{what}: a dry run writes nothing and counts what the write took"),
            dry_code == 0
                && !dry_out.exists()
                && dry_json["written"] == false
                && dry_json["dry_run"] == true
                && same("regions")
                && same("removals")
                && same("searches")
                && pages_agree
                && json["regions"].as_u64().is_some_and(|n| n > 0),
            &format!("exit {dry_code}: {dry_json} against {json}"),
        );
        if let (Some(probe), 3) = (&probe, at) {
            let _ = std::fs::create_dir_all(probe);
            let _ = std::fs::copy(&input, probe.join("contacts.pdf"));
            let _ = std::fs::copy(&out, probe.join("contacts-redacted.pdf"));
            let _ = std::fs::write(
                probe.join("contacts-expected.json"),
                serde_json::json!({ "gone": secrets, "kept": kept }).to_string(),
            );
        }
    }

    // Nothing matched: nothing written, exit 0, and the report says zero.
    let none = dir.join("none.pdf");
    let (code, json, stderr) = run(
        &[
            "redact".into(),
            s(&input),
            "-o".into(),
            s(&none),
            "--text".into(),
            "Zanzibar".into(),
            "--json".into(),
        ],
        &[],
    );
    report.check(
        "a query that matches nothing is not an error: exit 0, zero matches, nothing written",
        code == 0
            && json["written"] == false
            && json["searches"][0]["matches"] == 0
            && json["regions"] == 0
            && !none.exists(),
        &format!("exit {code}: {stderr}{json}"),
    );

    // `--pages` limits the search: page 2's copy of the word stays.
    let first = dir.join("first-page.pdf");
    let (code, json, stderr) = run(
        &[
            "redact".into(),
            s(&input),
            "-o".into(),
            s(&first),
            "--text".into(),
            "Rumpelstilzchen".into(),
            "--pages".into(),
            "1".into(),
            "--json".into(),
        ],
        &[],
    );
    let page_one = text_of(&first, &[], &["--pages", "1"]);
    let page_two = text_of(&first, &[], &["--pages", "2"]);
    report.check(
        "--pages 1 removes the word from page 1 and leaves page 2's",
        (code == 0 || code == 1)
            && json["searches"][0]["matches"] == 1
            && !page_one.contains("Rumpelstilzchen")
            && page_two.contains("Rumpelstilzchen"),
        &format!("exit {code}: {stderr}"),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A document, where it is, and what to search it for.
type Parity<'a> = (String, PathBuf, Vec<(SearchKind, &'a str)>);

/// 3: the tool's verdict is the application's, on three documents.
pub(super) fn verdict_is_the_applications(report: &mut Report) {
    let dir = scratch("redact-parity");
    let contacts = dir.join("contacts.pdf");
    std::fs::write(&contacts, contacts_pdf()).expect("contacts");
    let shared = dir.join("shared-form.pdf");
    std::fs::write(&shared, shared_form_pdf()).expect("shared form");
    let mut cases: Vec<Parity<'_>> = vec![
        (
            "contacts.pdf".into(),
            contacts,
            vec![
                (SearchKind::Pattern, EMAIL),
                (SearchKind::Text, "Rumpelstilzchen"),
            ],
        ),
        (
            "shared-form.pdf".into(),
            shared,
            vec![(SearchKind::Text, "Rumpelstilzchen")],
        ),
    ];
    for (name, query) in [
        ("text-base14.pdf", "4711-0815"),
        ("text-marked.pdf", "4711-0815"),
    ] {
        match fixture(name) {
            Some(path) => cases.push((name.into(), path, vec![(SearchKind::Text, query)])),
            None => report.skip(&format!("the parity check on {name}"), "not generated"),
        }
    }
    for (name, path, queries) in &cases {
        let theirs_out = dir.join(format!("app-{name}"));
        let ours_out = dir.join(format!("tool-{name}"));
        let app = app_path(path, &theirs_out, queries, None);
        let line = [
            vec![
                "redact".to_string(),
                s(path),
                "-o".into(),
                s(&ours_out),
                "--json".into(),
            ],
            query_args(queries),
        ]
        .concat();
        let (code, json, stderr) = run(&line, &[]);
        let Ok(app) = app else {
            report.check(
                &format!("{name}: the application's path ran"),
                false,
                &format!("{app:?}"),
            );
            continue;
        };
        let all: Vec<String> = json["reasons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r.as_str().map(str::to_string))
            .collect();
        // The tool's own reader, a search of the written file, can only add
        // reasons; everything else is the application's, in its order.
        let (own, reasons): (Vec<String>, Vec<String>) = all
            .iter()
            .cloned()
            .partition(|r| r.contains("searching the written file for"));
        let clean = app.verified && own.is_empty();
        report.check(
            &format!(
                "{name}: verified is {clean} and every other reason is the application's, word for word"
            ),
            json["verified"] == clean && reasons == app.why && json["regions"] == app.regions,
            &format!(
                "exit {code}: {stderr}\n       tool {all:?}\n       app  {:?}",
                app.why
            ),
        );
        let leaves = *name == "shared-form.pdf";
        report.check(
            &format!(
                "{name}: the search of the written file {}",
                if leaves {
                    "finds the word in the form the removal left, beside the application's reason"
                } else {
                    "finds nothing"
                }
            ),
            own.is_empty() != leaves
                && (!leaves || app.why.iter().any(|r| r.contains("drawn 2 time(s)"))),
            &format!("{own:?} beside {:?}", app.why),
        );
        report.check(
            &format!("{name}: the exit code is {}", if clean { 0 } else { 1 }),
            code == if clean { 0 } else { 1 } && ours_out.exists(),
            &format!("exit {code}"),
        );
        if *name == "text-marked.pdf" {
            report.check(
                "text-marked.pdf: not verified for a copy the removal leaves, which is not the platform's doing",
                !app.verified && reasons.iter().any(|r| r.contains("is still in the file")),
                &format!("{reasons:?}"),
            );
        }
        if !app.verified
            && app
                .why
                .iter()
                .all(|r| r.contains("could not be shown unreadable"))
        {
            println!(
                "[INFO] {name}: every reason is the OCR gate's --- no fixture here can reach exit 0 \
                 while the platform's engine cannot read (docs/TRAPS.md)"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// 4: refusals, and what the two opt-ins do.
pub(super) fn refusals_write_nothing(report: &mut Report) {
    let dir = scratch("redact-refused");
    let input = dir.join("contacts.pdf");
    std::fs::write(&input, contacts_pdf()).expect("contacts");
    let out = dir.join("out.pdf");
    let redact = |extra: &[&str], output: &Path| -> (i32, String) {
        let mut args = vec!["redact".to_string(), s(&input), "-o".into(), s(output)];
        args.extend(extra.iter().map(|a| (*a).to_string()));
        let (code, _, stderr) = run(&args, &[]);
        (code, stderr)
    };

    // An output that exists: refused and left alone, then replaced with --force.
    std::fs::write(&out, b"already here").expect("existing");
    let (code, stderr) = redact(&["--text", "Rumpelstilzchen"], &out);
    report.check(
        "an output that exists is refused (3) and left as it was",
        code == 3 && std::fs::read(&out).ok().as_deref() == Some(&b"already here"[..]),
        &format!("exit {code}: {stderr}"),
    );
    let (code, stderr) = redact(&["--text", "Rumpelstilzchen", "--force"], &out);
    report.check(
        "--force replaces it",
        (code == 0 || code == 1) && std::fs::read(&out).is_ok_and(|b| b.starts_with(b"%PDF")),
        &format!("exit {code}: {stderr}"),
    );
    let _ = std::fs::remove_file(&out);

    // The input under another name.
    let linked = dir.join("linked.pdf");
    let same = std::fs::hard_link(&input, &linked).is_ok();
    let (code, stderr) = redact(&["--text", "Rumpelstilzchen", "--force"], &linked);
    report.check(
        "an output that is the input under another name is a malformed line (2)",
        same && code == 2 && stderr.contains("under another name"),
        &format!("exit {code}: {stderr}"),
    );

    // XFA, for the write and for the dry run.
    let xfa = dir.join("xfa.pdf");
    std::fs::write(&xfa, xfa_pdf()).expect("xfa");
    for dry in [false, true] {
        let mut args = vec![
            "redact".to_string(),
            s(&xfa),
            "-o".into(),
            s(&out),
            "--text".into(),
            "Rumpelstilzchen".into(),
        ];
        if dry {
            args.push("--dry-run".into());
        }
        let (code, _, stderr) = run(&args, &[]);
        report.check(
            &format!(
                "an XFA form is refused (3){}, nothing written",
                if dry { " by a dry run too" } else { "" }
            ),
            code == 3 && stderr.contains("XFA") && !out.exists(),
            &format!("exit {code}: {stderr}"),
        );
    }

    // A signed document.
    match fixture("incr-signed.pdf") {
        None => report.skip(
            "the signed-document checks",
            "incr-signed.pdf is not generated",
        ),
        Some(signed) => {
            let args = |extra: &[&str]| -> Vec<String> {
                let mut a = vec![
                    "redact".to_string(),
                    s(&signed),
                    "-o".into(),
                    s(&out),
                    "--text".into(),
                    "page 1".into(),
                    "--json".into(),
                ];
                a.extend(extra.iter().map(|x| (*x).to_string()));
                a
            };
            let (code, _, stderr) = run(&args(&[]), &[]);
            report.check(
                "a signed document is refused (3) and nothing is written",
                code == 3 && stderr.contains("--invalidate-signatures") && !out.exists(),
                &format!("exit {code}: {stderr}"),
            );
            let (code, json, stderr) = run(&args(&["--invalidate-signatures"]), &[]);
            let (_, verified, _) = tool(&["verify", "--json", &s(&out)], &[]);
            let verdicts: Vec<String> = parsed(&verified)["files"][0]["signatures"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|x| {
                    x["integrity"]["verdict"]
                        .as_str()
                        .unwrap_or("?")
                        .to_string()
                })
                .collect();
            report.check(
                "with --invalidate-signatures it is written, says so, and its signature no longer reads intact",
                (code == 0 || code == 1)
                    && json["signatures_invalidated"] == 1
                    && !verdicts.is_empty()
                    && verdicts.iter().all(|v| v != "intact"),
                &format!("exit {code}: {stderr} {verdicts:?}"),
            );
            let _ = std::fs::remove_file(&out);
        }
    }

    // A locked document: refused without its password, redacted with it, and
    // the copy is still encrypted.
    match fixture("incr-encrypted-pw.pdf") {
        None => report.skip(
            "the password checks",
            "incr-encrypted-pw.pdf is not generated",
        ),
        Some(locked) => {
            let base = vec![
                "redact".to_string(),
                s(&locked),
                "-o".into(),
                s(&out),
                "--text".into(),
                "page 2".into(),
            ];
            let (code, _, stderr) = run(&base, &[]);
            report.check(
                "a locked document without its password is refused (3)",
                code == 3 && stderr.contains("--password-env") && !out.exists(),
                &format!("exit {code}: {stderr}"),
            );
            let with = [
                base.clone(),
                vec!["--password-env".into(), "TPDF_IT_PASSWORD".into()],
            ]
            .concat();
            let (code, _, stderr) = run(&with, &[("TPDF_IT_PASSWORD", "swordfish")]);
            let encrypted = Command::new("qpdf")
                .args(["--is-encrypted", &s(&out)])
                .status()
                .map(|st| st.code() == Some(0))
                .ok();
            let text = text_of(
                &out,
                &[("TPDF_IT_PASSWORD", "swordfish")],
                &["--password-env", "TPDF_IT_PASSWORD"],
            );
            report.check(
                "with its password it is redacted, the copy is still encrypted, and the word is gone",
                (code == 0 || code == 1)
                    && encrypted.unwrap_or(true)
                    && !text.contains("page 2")
                    && text.contains("page 1"),
                &format!("exit {code}: {stderr} encrypted {encrypted:?}, text {text:?}"),
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
