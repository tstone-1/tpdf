//! Does the comment scan read what a reviewer wrote, and refuse what it cannot?
//!
//! `annots.rs` is unit-tested against synthetic documents it builds itself,
//! which is the right place for the decoders and the wrong place for everything
//! else: a document written by the same process that reads it agrees with itself
//! about the parts both got wrong. `testdata/comments.pdf` is written by a
//! generator that knows nothing about the scan, and this asserts the scan
//! against the manifest that generator emitted.
//!
//! Three modes:
//!
//! * `--mode read` --- prints every comment, so a human can look at one.
//!
//! * `--mode check` --- asserts `testdata/comments-corpus.json`: the bodies,
//!   the authors in three encodings, the dates, the reply that names its parent,
//!   the rectangle on a rotated page, the `/Annots` array that is an indirect
//!   reference, and every bound reported rather than silently applied.
//!
//! * `--mode clean` --- the control, and it is not optional. Run against a
//!   document with no annotations at all, it asserts zero comments and zero
//!   limits. Without it, a scan that returned nothing for everything would pass
//!   "the hostile page is cut short" and "the link is not a comment" perfectly.
//!
//! Usage:
//!   comments-probe <file.pdf> [--mode read|check|clean] [--manifest PATH] [--lib DIR]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tpdf_lib::document::OpenDocument;

use tpdf_lib::annots::{Comment, Comments, Kind};
use tpdf_lib::progressive::{self};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Read,
    Check,
    Clean,
}

struct Args {
    file: PathBuf,
    mode: Mode,
    manifest: PathBuf,
    library: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let file = args
        .next()
        .ok_or("usage: comments-probe <file.pdf> [...]")?;
    let mut parsed = Args {
        file: PathBuf::from(file),
        mode: Mode::Read,
        manifest: PathBuf::from("testdata/comments-corpus.json"),
        library: PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR),
    };

    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--mode" => {
                parsed.mode = match value.as_str() {
                    "read" => Mode::Read,
                    "check" => Mode::Check,
                    "clean" => Mode::Clean,
                    other => return Err(format!("unknown mode: {other}")),
                }
            }
            "--manifest" => parsed.manifest = PathBuf::from(value),
            "--lib" => parsed.library = PathBuf::from(value),
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    Ok(parsed)
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(2);
        }
    };

    match run(&args) {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(2);
        }
    }
}

fn run(args: &Args) -> Result<bool, String> {
    // Opened through PDFium as well as scanned, deliberately: the viewer only
    // ever asks for comments on a document it has open, so a fixture PDFium
    // refuses is one this answer would never be wanted for. It is also where
    // the page count comes from, which is what bounds the scan.
    let bindings = bind(&args.library)?;
    let document = OpenDocument::open(bindings, &args.file, None)?;
    let comments = document.graph().comments(document.page_count() as usize)?;

    match args.mode {
        Mode::Read => {
            read(&comments);
            Ok(true)
        }
        Mode::Check => check(args, &comments),
        Mode::Clean => Ok(clean(&comments)),
    }
}

fn bind(library: &Path) -> Result<progressive::Bindings, String> {
    use pdfium_render::prelude::Pdfium;
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = Pdfium::bind_to_library(&path)
        .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;
    Ok(progressive::bindings_of(Box::leak(Box::new(Pdfium::new(
        bound,
    )))))
}

fn read(comments: &Comments) {
    println!(
        "{} comments, scanned in {:.2} ms",
        comments.items.len(),
        comments.scan_ms
    );
    for item in &comments.items {
        let body: String = item.body.chars().take(60).collect();
        println!(
            "  #{:<4} p{:<3} {:<14} {:<12} {:<18} {}{}",
            item.id,
            item.page + 1,
            format!("{:?}", item.kind),
            if item.author.is_empty() {
                "(anonymous)"
            } else {
                &item.author
            },
            item.date.clone().unwrap_or_else(|| "(no date)".into()),
            item.reply_to
                .map(|id| format!("re #{id}: "))
                .unwrap_or_default(),
            body.replace('\n', "\\n"),
        );
    }
    println!("{}", limits_line(comments));
}

fn limits_line(comments: &Comments) -> String {
    let limits = &comments.limits;
    format!(
        "limits: crowded_pages={} over_budget={} bodies_clipped={} unknown_kinds={} \
         unreadable={} cycles={} pages_missed={}",
        limits.crowded_pages,
        limits.over_budget,
        limits.bodies_clipped,
        limits.unknown_kinds,
        limits.unreadable,
        limits.cycles,
        limits.pages_missed
    )
}

/// Records a check's outcome, in the shape every other probe here uses.
struct Report {
    passed: usize,
    failed: usize,
}

impl Report {
    fn check(&mut self, ok: bool, name: &str, detail: &str) {
        if ok {
            self.passed += 1;
            println!("[OK]   {name:<52} {detail}");
        } else {
            self.failed += 1;
            println!("[FAIL] {name:<52} {detail}");
        }
    }

    /// A check this fixture cannot exercise. Printed, never omitted --- for the
    /// reason every probe here states: a check that quietly disappears on some
    /// inputs cannot be told apart from one that ran.
    fn skip(&mut self, name: &str, why: &str) {
        println!("[SKIP] {name:<52} not applicable -- {why}");
    }
}

/// The control: a document with no annotations reports none, and cuts nothing.
fn clean(comments: &Comments) -> bool {
    let mut report = Report {
        passed: 0,
        failed: 0,
    };
    report.check(
        comments.items.is_empty(),
        "a document with no annotations has no comments",
        &format!("{} found", comments.items.len()),
    );
    report.check(
        !comments.limits.any(),
        "and reports nothing cut",
        &limits_line(comments),
    );
    println!(
        "\n{}/{} checks passed",
        report.passed,
        report.passed + report.failed
    );
    report.failed == 0
}

fn check(args: &Args, comments: &Comments) -> Result<bool, String> {
    let text = std::fs::read_to_string(&args.manifest)
        .map_err(|e| format!("could not read {}: {e}", args.manifest.display()))?;
    let manifest: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("manifest is not JSON: {e}"))?;
    // Keyed by file name, like `outline-manifest.json`: one sidecar describes
    // both fixtures, so this reads the section for the file it was handed
    // rather than being told which manifest goes with which document.
    let name = args
        .file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("the file has no name")?;
    let pages = manifest
        .get(name)
        .and_then(|entry| entry.get("pages"))
        .ok_or_else(|| format!("{name} is not in {}", args.manifest.display()))?
        .clone();

    let mut report = Report {
        passed: 0,
        failed: 0,
    };

    let on = |page: u32| -> Vec<&Comment> {
        comments
            .items
            .iter()
            .filter(|item| item.page == page)
            .collect()
    };
    let expect = |page: &str, key: &str| -> Option<&serde_json::Value> {
        pages.get(page).and_then(|value| value.get(key))
    };

    // ------------------------------------------------------------- page 0
    let ordinary = on(0);
    if let Some(count) = expect("0", "comments").and_then(serde_json::Value::as_u64) {
        report.check(
            ordinary.len() as u64 == count,
            "the ordinary page reports every comment and nothing else",
            &format!("wanted {count}, got {}", ordinary.len()),
        );
    }
    if let Some(kinds) = expect("0", "kinds").and_then(serde_json::Value::as_array) {
        let wanted: Vec<String> = kinds
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect();
        let got: Vec<String> = ordinary.iter().map(|item| kind_name(item.kind)).collect();
        report.check(
            got == wanted,
            "the kinds are the document's, in document order",
            &format!("wanted {wanted:?}, got {got:?}"),
        );
    }
    if let Some(body) = expect("0", "note_body").and_then(serde_json::Value::as_str) {
        report.check(
            ordinary.iter().any(|item| item.body == body),
            "a sticky note's body is read",
            body,
        );
    }
    if let Some(body) = expect("0", "reply_body").and_then(serde_json::Value::as_str) {
        let reply = ordinary.iter().find(|item| item.body == body);
        let parent = reply
            .and_then(|item| item.reply_to)
            .and_then(|id| comments.items.iter().find(|other| other.id == id));
        report.check(
            parent.is_some_and(|parent| {
                expect("0", "note_body")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|wanted| parent.body == wanted)
            }),
            "a reply names the comment it answers",
            &match (reply, parent) {
                (Some(_), Some(parent)) => {
                    format!("replies to #{} by {}", parent.id, parent.author)
                }
                (Some(_), None) => "the reply names no parent".into(),
                (None, _) => "the reply is missing entirely".into(),
            },
        );
    }
    if let Some(authors) = expect("0", "authors").and_then(serde_json::Value::as_array) {
        let got: BTreeSet<&str> = ordinary.iter().map(|item| item.author.as_str()).collect();
        for author in authors.iter().filter_map(serde_json::Value::as_str) {
            report.check(
                got.contains(author),
                "an author is read",
                &format!("{author} in {got:?}"),
            );
        }
    }
    // The one that matters most on this page: a `/Link` with a `/URI` and a form
    // `/Widget` both carry text, and neither is a comment. A scan keyed on
    // `/Contents` alone would report both, and the URL would then be document
    // text in the sidebar --- which `docs/THREAT-MODEL.md` T8 is about.
    if let Some(absent) = expect("0", "absent").and_then(serde_json::Value::as_array) {
        let haystack: String = comments
            .items
            .iter()
            .map(|item| format!("{} {} {}", item.author, item.body, item.subject))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in absent.iter().filter_map(serde_json::Value::as_str) {
            report.check(
                !haystack.contains(needle),
                "text from something that is not a comment stays out",
                needle,
            );
        }
    }

    // ------------------------------------------------------------- page 1
    let strings = on(1);
    if let Some(count) = expect("1", "comments").and_then(serde_json::Value::as_u64) {
        report.check(
            strings.len() as u64 == count,
            "every string on the encodings page is read",
            &format!("wanted {count}, got {}", strings.len()),
        );
    }
    for (key, name) in [
        (
            "utf16_author",
            "a UTF-16BE author survives, astral character and all",
        ),
        ("pdfdoc_author", "a PDFDocEncoded author survives"),
    ] {
        if let Some(wanted) = expect("1", key).and_then(serde_json::Value::as_str) {
            report.check(
                strings.iter().any(|item| item.author == wanted),
                name,
                wanted,
            );
        }
    }
    for (key, name) in [
        ("pdfdoc_body", "PDFDocEncoding is not Latin-1"),
        (
            "utf8_body",
            "a UTF-8 body survives, byte-order mark removed",
        ),
        ("paragraph_body", "a body keeps its paragraphs"),
    ] {
        if let Some(wanted) = expect("1", key).and_then(serde_json::Value::as_str) {
            let got = strings.iter().find(|item| item.body == wanted);
            report.check(
                got.is_some(),
                name,
                &format!(
                    "{:?} in {:?}",
                    wanted,
                    strings
                        .iter()
                        .map(|item| item.body.chars().take(40).collect::<String>())
                        .collect::<Vec<_>>()
                ),
            );
        }
    }
    if let Some(wanted) = expect("1", "offset_date").and_then(serde_json::Value::as_str) {
        report.check(
            strings
                .iter()
                .any(|item| item.date.as_deref() == Some(wanted)),
            "a date with a timezone offset is read in its own zone",
            wanted,
        );
    }
    if expect("1", "bad_date_is_absent").is_some() {
        // The control for every date above: a parser that answered for anything
        // would satisfy them all.
        report.check(
            strings.iter().any(|item| item.date.is_none()),
            "a string that is not a date produces no date",
            &format!(
                "{:?}",
                strings
                    .iter()
                    .map(|item| item.date.clone())
                    .collect::<Vec<_>>()
            ),
        );
    }

    // ------------------------------------------------------------- page 2
    let hostile = on(2);
    if let Some(crowd) = expect("2", "crowd").and_then(serde_json::Value::as_u64) {
        report.check(
            (hostile.len() as u64) < crowd,
            "a crowded page is cut at the bound",
            &format!("{} of {crowd} kept", hostile.len()),
        );
        report.check(
            comments.limits.crowded_pages > 0,
            "and says so, rather than cutting silently",
            &limits_line(comments),
        );
    }
    if expect("2", "body_is_clipped").is_some() {
        report.check(
            comments.limits.bodies_clipped > 0,
            "an oversized body is clipped and reported",
            &format!("{} clipped", comments.limits.bodies_clipped),
        );
    }
    if expect("2", "cycle_broken").is_some() {
        report.check(
            comments.limits.cycles > 0,
            "a reply cycle is broken and reported",
            &format!("{} cut", comments.limits.cycles),
        );
    }
    if let Some(body) = expect("2", "hidden_body").and_then(serde_json::Value::as_str) {
        let hidden = hostile.iter().find(|item| item.body == body);
        report.check(
            hidden.is_some_and(|item| item.hidden),
            "a hidden comment is listed and marked hidden",
            &match hidden {
                Some(item) => format!("hidden={}", item.hidden),
                None => "not listed at all".into(),
            },
        );
    }
    // Against the count the fixture states rather than "more than none": the
    // three malformed entries sit before a per-page bound that would swallow
    // them, and a fixture that stopped delivering them would still satisfy a
    // `> 0` written for the fourth.
    match expect("2", "unreadable").and_then(serde_json::Value::as_u64) {
        Some(wanted) => report.check(
            comments.limits.unreadable as u64 == wanted,
            "every entry that cannot be read is counted",
            &format!("wanted {wanted}, got {}", comments.limits.unreadable),
        ),
        None => report.skip(
            "every entry that cannot be read is counted",
            "this fixture has no malformed entries",
        ),
    }

    // -------------------------------------------- the rotated fixture, page 1
    //
    // A file of its own --- see `make_comments_pdf.py` --- so on `comments.pdf`
    // the manifest has no rectangle here and these two do not run. That is the
    // one place this probe skips silently rather than printing a reason: the
    // whole check block belongs to a different document, and a `[SKIP]` for it
    // in every `comments.pdf` run would be noise about a file that was not
    // being examined.
    let turned = on(0);
    if let Some(wanted) = expect("0", "rect").and_then(serde_json::Value::as_array) {
        let wanted: Vec<f32> = wanted
            .iter()
            .filter_map(serde_json::Value::as_f64)
            .map(|value| value as f32)
            .collect();
        let got = turned.first().map(|item| item.rect);
        // The whole rectangle rather than one edge, because the failure this is
        // for is a *transposition*: a mapping that swapped x and y would still
        // put the marker near the top of a rotated page and nowhere near the
        // right place across it.
        report.check(
            got.is_some_and(|rect| {
                wanted.len() == 4
                    && rect
                        .iter()
                        .zip(&wanted)
                        .all(|(got, want)| (got - want).abs() < 0.5)
            }),
            "a rotated page places a rectangle in display space",
            &format!("wanted {wanted:?}, got {got:?}"),
        );
    }
    if let Some(size) = expect("0", "displayed_size").and_then(serde_json::Value::as_array) {
        let width = size
            .first()
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0) as f32;
        let height = size
            .get(1)
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0) as f32;
        let first = turned.first();
        report.check(
            first.is_some_and(|item| item.rect[2] <= width + 0.5 && item.rect[3] <= height + 0.5),
            "and inside the displayed page",
            &match first {
                Some(item) => format!(
                    "{:?} against a displayed page of {width} x {height}",
                    item.rect
                ),
                None => "the rotated page reported no comment".into(),
            },
        );
    }

    // ------------------------------------------------------------- page 3
    if let Some(body) = expect("3", "body").and_then(serde_json::Value::as_str) {
        report.check(
            on(3).iter().any(|item| item.body == body),
            "an indirect /Annots array is resolved",
            body,
        );
    }

    // ------------------------------------------------------- whole document
    report.check(
        acyclic(comments),
        "no reply chain loops, whatever the file said",
        &format!("{} comments walked", comments.items.len()),
    );
    report.check(
        comments
            .items
            .iter()
            .all(|item| item.reply_to.is_none_or(|id| id < item.id)),
        "a reply's parent is a comment that exists, earlier in the list",
        "every reply_to resolves",
    );

    let total = report.passed + report.failed;
    println!("\n{}/{total} checks passed", report.passed);
    Ok(report.failed == 0)
}

/// Walks every reply chain to its root, refusing to loop.
///
/// The property `annots.rs` promises the frontend --- that a consumer can follow
/// `reply_to` without a visited set of its own --- and the only way to check it
/// is to be that consumer. The step bound is what keeps this check from becoming
/// the hang it is testing for.
fn acyclic(comments: &Comments) -> bool {
    for item in &comments.items {
        let mut at = item.reply_to;
        let mut steps = 0;
        while let Some(id) = at {
            if id == item.id || steps > comments.items.len() {
                return false;
            }
            let Some(parent) = comments.items.iter().find(|other| other.id == id) else {
                return false;
            };
            at = parent.reply_to;
            steps += 1;
        }
    }
    true
}

/// The serialised name of a kind, which is what the manifest speaks.
///
/// Through serde rather than a second table: the manifest is written against the
/// names the frontend receives, and a `match` here would be a copy of the one
/// `#[serde(rename_all)]` generates --- agreeing with it until somebody renamed
/// a variant in one place.
fn kind_name(kind: Kind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{kind:?}"))
}
