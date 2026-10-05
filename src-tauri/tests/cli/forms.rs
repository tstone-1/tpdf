//! `tpdf fields` and `tpdf fill` against real workers and real documents.
//!
//! Part of `tests/cli.rs`'s binary, which is its own worker; the helpers it
//! uses (`tool`, `signs`, `fixture`, the software key) are that file's.
//!
//! Three things, each with the control that keeps it from passing vacuously:
//!
//! 1. **`fields --json` is what the in-process form reader says**, field for
//!    field: `cli::fields::report` over `DocumentGraph::form` in this process,
//!    on `form.pdf`, a signed fixture and two forms built here --- one shaped
//!    as `forms::tests::fixture` (an inherited field on two pages and a
//!    checkbox), one with every control and every reason a field is not
//!    editable. Control: the reports between them hold every kind and every
//!    reason, so a report that dropped one cannot agree.
//! 2. **`fill` writes every kind of answer and reads back as asked**, through
//!    the built tool, from a file and from stdin; every refusal on its own,
//!    and together, writes nothing; a signed document is refused; the output
//!    rules are `sign`'s; an encrypted form is filled with its password and
//!    stays encrypted; and a filled form signed with the software key reads
//!    back intact.
//! 3. With `TPDF_FILL_PROBE=<dir>`, the filled files are left there for the
//!    independent readers `BUILD.md` names (pypdf, PDFKit).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

use super::{bindings, differing, fixture, now, scratch, signs, strings, tool, Report};

fn text(s: &str) -> Object {
    let mut bytes = vec![0xfe, 0xff];
    bytes.extend(s.encode_utf16().flat_map(u16::to_be_bytes));
    Object::String(bytes, lopdf::StringFormat::Hexadecimal)
}

fn appearance(doc: &mut Document, w: i64, h: i64, body: &[u8]) -> ObjectId {
    doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), w.into(), h.into()],
            "Resources" => Dictionary::new(),
        },
        body.to_vec(),
    ))
}

/// A widget's rectangle.
fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Object {
    Object::Array(vec![x0.into(), y0.into(), x1.into(), y1.into()])
}

/// Pages of 400 x 500 points, the catalog and the form, around `fields` and
/// the widgets each page carries.
fn assembled(
    mut doc: Document,
    pages_id: ObjectId,
    annots: Vec<Vec<ObjectId>>,
    fields: Vec<ObjectId>,
) -> Vec<u8> {
    let mut kids = Vec::new();
    for on_page in annots {
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 400.into(), 500.into()],
            "Resources" => Dictionary::new(),
            "Annots" => on_page.into_iter().map(Object::Reference).collect::<Vec<_>>(),
        });
        kids.push(Object::Reference(page));
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }.into(),
    );
    let root = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id,
        "AcroForm" => dictionary! {
            "Fields" => fields.into_iter().map(Object::Reference).collect::<Vec<_>>(),
        },
    });
    doc.trailer.set("Root", root);
    // Each widget names its page, as a producer writes it.
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for page in pages {
        let annots: Vec<ObjectId> = doc
            .get_dictionary(page)
            .and_then(|d| d.get(b"Annots"))
            .and_then(Object::as_array)
            .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
            .unwrap_or_default();
        for annot in annots {
            if let Ok(w) = doc.get_dictionary_mut(annot) {
                w.set("P", page);
            }
        }
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// `forms::tests::fixture`'s shape: `ACME.answer`, a text field whose type and
/// flags are on its parent, with a widget on each of two pages, and a checkbox.
/// `scripts/form_pdfkit_check.swift` reads exactly this form, filled.
pub(super) fn acme_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let parent = doc.add_object(dictionary! { "FT" => "Tx", "T" => text("ACME"), "Ff" => 0 });
    let field = doc.add_object(dictionary! {
        "Parent" => parent, "T" => text("answer"), "V" => text("OLD"), "MaxLen" => 20,
    });
    let mut widgets = Vec::new();
    for _ in 0..2 {
        widgets.push(doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Parent" => field,
            "Rect" => rect(20, 70, 180, 100), "F" => 4,
        }));
    }
    doc.get_dictionary_mut(field).expect("field").set(
        "Kids",
        widgets
            .iter()
            .copied()
            .map(Object::Reference)
            .collect::<Vec<_>>(),
    );
    doc.get_dictionary_mut(parent)
        .expect("parent")
        .set("Kids", vec![Object::Reference(field)]);
    let ap = appearance(&mut doc, 20, 20, b"q Q");
    let check = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "T" => text("consent"),
        "Rect" => rect(20, 20, 40, 40), "F" => 4, "V" => "Off", "AS" => "Off",
        "AP" => dictionary! { "N" => dictionary! { "Accepted" => ap, "Off" => ap } },
    });
    assembled(
        doc,
        pages,
        vec![vec![widgets[0], check], vec![widgets[1]]],
        vec![parent, check],
    )
}

/// Every control `forms.rs` fills, and one field for every reason it will not.
pub(super) fn every_control_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let mut first = Vec::new();
    let mut second = Vec::new();
    let mut fields = Vec::new();

    // The inherited text field, a widget on each page.
    let parent = doc.add_object(dictionary! { "FT" => "Tx", "T" => text("ACME"), "Ff" => 0 });
    let answer = doc.add_object(dictionary! {
        "Parent" => parent, "T" => text("answer"), "V" => text("OLD"), "MaxLen" => 20,
    });
    let mut kids = Vec::new();
    for page in [&mut first, &mut second] {
        let w = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Parent" => answer,
            "Rect" => rect(20, 70, 180, 100), "F" => 4,
        });
        page.push(w);
        kids.push(Object::Reference(w));
    }
    doc.get_dictionary_mut(answer)
        .expect("answer")
        .set("Kids", kids);
    doc.get_dictionary_mut(parent)
        .expect("parent")
        .set("Kids", vec![Object::Reference(answer)]);
    fields.push(parent);

    // A checkbox.
    let off = appearance(&mut doc, 20, 20, b"q Q");
    let on = appearance(&mut doc, 20, 20, b"q 0 0 0 rg 4 4 12 12 re f Q");
    let consent = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "T" => text("consent"),
        "Rect" => rect(20, 20, 40, 40), "F" => 4, "V" => "Off", "AS" => "Off",
        "AP" => dictionary! { "N" => dictionary! { "Accepted" => on, "Off" => off } },
    });
    first.push(consent);
    fields.push(consent);

    // A radio group, one button on each page.
    let radio = doc.add_object(dictionary! {
        "FT" => "Btn", "Ff" => 1 << 15, "T" => text("delivery"), "V" => "First",
    });
    let mut buttons = Vec::new();
    for (i, state) in ["First", "Second"].iter().enumerate() {
        let off = appearance(&mut doc, 20, 20, b"q 0 0 0 RG 1 1 18 18 re S Q");
        let on = appearance(
            &mut doc,
            20,
            20,
            b"q 0 0 0 RG 1 1 18 18 re S 5 5 10 10 re f Q",
        );
        let mut states = dictionary! { "Off" => off };
        states.set(*state, on);
        let button = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Parent" => radio, "F" => 4,
            "Rect" => rect(200, 20, 220, 40), "AP" => dictionary! { "N" => states },
            "AS" => if i == 0 { "First" } else { "Off" },
        });
        buttons.push(Object::Reference(button));
        if i == 0 {
            first.push(button);
        } else {
            second.push(button);
        }
    }
    doc.get_dictionary_mut(radio)
        .expect("radio")
        .set("Kids", buttons);
    fields.push(radio);

    // Choices: a dropdown with a shared export value, a multiple-selection
    // list, a single list and an editable dropdown.
    let pair = |e: &str, l: &str| Object::Array(vec![text(e), text(l)]);
    let choice =
        |doc: &mut Document, name: &str, flags: i64, at: Object, opt: Vec<Object>, v: Object| {
            doc.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Widget", "FT" => "Ch", "T" => text(name),
                "Ff" => flags, "F" => 4, "Rect" => at, "Opt" => opt, "V" => v,
            })
        };
    let combo = choice(
        &mut doc,
        "delivery_choice",
        1 << 17,
        rect(60, 150, 220, 180),
        vec![
            pair("SAME", "First label"),
            pair("SAME", "Second label"),
            pair("OTHER", "Third label"),
        ],
        text("OTHER"),
    );
    let items = choice(
        &mut doc,
        "items",
        1 << 21,
        rect(60, 200, 220, 280),
        vec![
            pair("A", "Alpha label"),
            pair("B", "Beta label"),
            pair("C", "Gamma label"),
        ],
        text("B"),
    );
    first.extend([combo, items]);
    let plain = vec![text("Alpha"), text("Beta"), text("Gamma")];
    let single = choice(
        &mut doc,
        "single_item",
        0,
        rect(60, 220, 220, 290),
        plain.clone(),
        text("Alpha"),
    );
    let custom = choice(
        &mut doc,
        "custom_choice",
        (1 << 17) | (1 << 18),
        rect(60, 320, 220, 350),
        plain,
        text("Alpha"),
    );
    second.extend([single, custom]);
    fields.extend([combo, items, single, custom]);

    // A multiline text field.
    let notes = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx", "T" => text("notes"),
        "Ff" => 1 << 12, "F" => 4, "Rect" => rect(20, 360, 380, 460), "V" => text(""),
    });
    second.push(notes);
    fields.push(notes);

    // One text field for every reason one is not editable.
    for (i, (name, flags, annotation_flags)) in [
        ("ro", 1, 4),
        ("pin", 1 << 13, 4),
        ("upload", 1 << 20, 4),
        ("code", 1 << 24, 4),
        ("rich", 1 << 25, 4),
        ("secret", 0, 2),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 300 + 25 * i as i64;
        let field = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx", "T" => text(name),
            "Ff" => flags, "F" => annotation_flags, "Rect" => rect(240, y, 390, y + 20),
            "V" => text("kept"), "MaxLen" => 6,
        });
        first.push(field);
        fields.push(field);
    }
    assembled(doc, pages, vec![first, second], fields)
}

/// `cli::fields::report` for `path`, built in this process.
fn fields_here(
    bindings: tpdf_lib::progressive::Bindings,
    path: &Path,
    password: Option<&str>,
) -> Result<serde_json::Value, String> {
    let document = tpdf_lib::document::OpenDocument::open(bindings, path, password)
        .map_err(|refusal| refusal.reason)?;
    let form = document.graph().form()?;
    serde_json::to_value(tpdf_lib::cli::fields::report(
        &path.display().to_string(),
        &form,
    ))
    .map_err(|e| e.to_string())
}

fn parsed(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).unwrap_or_default()
}

pub(super) fn fields_agree(report: &mut Report) {
    let Some(bindings) = bindings() else {
        report.check("PDFium binds in this process", false, "no library");
        return;
    };
    let dir = scratch("fields");
    let acme = dir.join("acme.pdf");
    let every = dir.join("every-control.pdf");
    std::fs::write(&acme, acme_pdf()).expect("acme");
    std::fs::write(&every, every_control_pdf()).expect("every control");
    let mut documents = vec![acme, every];
    for name in ["form.pdf", "signed-nested-field.pdf"] {
        match fixture(name) {
            Some(path) => documents.push(path),
            None => report.skip(&format!("fields of {name}"), "not generated"),
        }
    }
    let mut kinds = std::collections::BTreeSet::new();
    let mut reasons = std::collections::BTreeSet::new();
    let mut inherited = false;
    for path in &documents {
        let at = path.display().to_string();
        let (code, stdout, stderr) = tool(&["fields", "--json", &at], &[]);
        let theirs = parsed(&stdout);
        let ours = fields_here(bindings, path, None).unwrap_or_default();
        for f in theirs["fields"].as_array().into_iter().flatten() {
            kinds.insert(f["kind"].as_str().unwrap_or("?").to_string());
            if let Some(why) = f["not_editable"].as_str() {
                reasons.insert(why.to_string());
            }
            inherited |= f["name"] == "ACME.answer";
        }
        report.check(
            &format!("{at}: fields --json is what the in-process form reader says"),
            code == 0
                && theirs["fields"].as_array().is_some_and(|f| !f.is_empty())
                && theirs == ours,
            &format!(
                "exit {code}: {stderr}\n       {}",
                differing(&theirs, &ours).join("\n       ")
            ),
        );
    }
    let want_kinds = [
        "checkbox",
        "choice_combo",
        "choice_list",
        "other",
        "radio",
        "text",
    ];
    let want_reasons = [
        "comb",
        "file_select",
        "hidden",
        "password",
        "read_only",
        "rich_text",
    ];
    report.check(
        "control: the reports between them hold every kind, and an inherited field",
        want_kinds.iter().all(|k| kinds.contains(*k)) && inherited,
        &format!("{kinds:?}, inherited {inherited}"),
    );
    report.check(
        "control: and every reason a text field is not editable",
        want_reasons.iter().all(|r| reasons.contains(*r)),
        &format!("{reasons:?}"),
    );
    // The plain text, once, and a document with no form.
    let (code, stdout, _) = tool(&["fields", &documents[0].display().to_string()], &[]);
    report.check(
        "fields without --json names each field with its value",
        code == 0
            && stdout.contains("ACME.answer (text): \"OLD\"")
            && stdout.contains("consent (checkbox): false"),
        &stdout,
    );
    let empty = dir.join("plain.pdf");
    std::fs::write(&empty, super::plain_pdf()).expect("plain");
    let (code, stdout, _) = tool(&["fields", "--json", &empty.display().to_string()], &[]);
    report.check(
        "a document with no form lists no fields and exits 0",
        code == 0
            && parsed(&stdout)["fields"]
                .as_array()
                .is_some_and(Vec::is_empty),
        &stdout,
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every editable control in `every_control_pdf`, answered.
const ANSWERS: &str = r#"{
  "ACME.answer": "Grüße",
  "consent": true,
  "delivery": "Second",
  "delivery_choice": "OTHER",
  "items": ["A", "C"],
  "single_item": "Gamma",
  "custom_choice": "My own",
  "notes": "Line one\nLine two"
}"#;

fn answers() -> serde_json::Value {
    serde_json::from_str(ANSWERS).expect("answers")
}

/// The built tool with `stdin` fed from `input`.
pub(super) fn tool_with_stdin(
    args: &[&str],
    env: &[(&str, &str)],
    input: &str,
) -> (i32, String, String) {
    use std::io::Write as _;
    let mut child = Command::new(env!("CARGO_BIN_EXE_tpdf-cli"))
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the built tool runs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("answers written");
    let out = child.wait_with_output().expect("it ends");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The values `fields --json` reads from `path`, by name.
fn values_of(
    path: &Path,
    env: &[(&str, &str)],
    extra: &[&str],
) -> serde_json::Map<String, serde_json::Value> {
    let at = path.display().to_string();
    let mut args = vec!["fields", "--json"];
    args.extend(extra);
    args.push(&at);
    let (_, stdout, _) = tool(&args, env);
    parsed(&stdout)["fields"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            (
                f["name"].as_str().unwrap_or("").to_string(),
                f["value"].clone(),
            )
        })
        .collect()
}

/// Whether every answer in `answers` is what `path` now says.
fn says(
    path: &Path,
    answers: &serde_json::Value,
    env: &[(&str, &str)],
    extra: &[&str],
) -> Result<(), String> {
    let found = values_of(path, env, extra);
    let wrong: Vec<String> = answers
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(name, want)| found.get(*name) != Some(want))
        .map(|(name, want)| format!("{name}: asked {want}, reads {:?}", found.get(name)))
        .collect();
    if found.is_empty() || !wrong.is_empty() {
        return Err(format!("{} fields read; {}", found.len(), wrong.join("; ")));
    }
    Ok(())
}

pub(super) fn fill_round_trips(report: &mut Report) {
    let dir = scratch("fill");
    let s = |p: &Path| p.display().to_string();
    let every = dir.join("every-control.pdf");
    std::fs::write(&every, every_control_pdf()).expect("every control");
    let answers_file = dir.join("answers.json");
    std::fs::write(&answers_file, answers().to_string()).expect("answers");
    let probe = std::env::var_os("TPDF_FILL_PROBE").map(PathBuf::from);

    // --- every kind of answer, from a file ---------------------------------
    let filled = dir.join("filled.pdf");
    let (code, stdout, stderr) = tool(
        &[
            "fill",
            &s(&every),
            "-o",
            &s(&filled),
            "--values",
            &s(&answers_file),
            "--json",
        ],
        &[],
    );
    let json = parsed(&stdout);
    report.check(
        "fill writes every kind of answer and says so",
        code == 0
            && json["written"] == true
            && json["problems"].as_array().is_some_and(Vec::is_empty),
        &format!("exit {code}: {stderr}{stdout}"),
    );
    let reported: serde_json::Map<String, serde_json::Value> = json["fields"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            (
                f["name"].as_str().unwrap_or("").to_string(),
                f["value"].clone(),
            )
        })
        .collect();
    report.check(
        "its report reads back every answer as given",
        serde_json::Value::Object(reported.clone()) == answers(),
        &format!("{reported:?}"),
    );
    report.check(
        "a separate fields run on the written file reads every answer as given",
        says(&filled, &answers(), &[], &[]).is_ok(),
        &says(&filled, &answers(), &[], &[])
            .err()
            .unwrap_or_default(),
    );
    let before = values_of(&every, &[], &[]);
    let after = values_of(&filled, &[], &[]);
    let untouched: Vec<&String> = before
        .keys()
        .filter(|name| answers().get(name.as_str()).is_none())
        .filter(|name| before.get(*name) != after.get(*name))
        .collect();
    report.check(
        "every field it was not asked to fill reads as it did",
        before.len() == 14 && after.len() == 14 && untouched.is_empty(),
        &format!(
            "{} before, {} after, changed {untouched:?}",
            before.len(),
            after.len()
        ),
    );
    report.check(
        "control: the input still says what it said, and is not what was asked",
        says(&every, &answers(), &[], &[]).is_err()
            && before.get("consent") == Some(&serde_json::json!(false)),
        "the input reads as the answers",
    );

    // --- from stdin ------------------------------------------------------------
    let piped = dir.join("piped.pdf");
    let (code, _, stderr) = tool_with_stdin(
        &["fill", &s(&every), "-o", &s(&piped), "--values", "-"],
        &[],
        &answers().to_string(),
    );
    report.check(
        "--values - reads the answers from stdin",
        code == 0 && says(&piped, &answers(), &[], &[]).is_ok(),
        &format!("exit {code}: {stderr}"),
    );

    // --- every refusal, on its own ------------------------------------------------
    let long = "W".repeat(300);
    let refusals: Vec<(&str, String, &str)> = vec![
        (
            "a name no field has",
            r#"{"nobody": "x"}"#.into(),
            "unknown",
        ),
        (
            "a field answered twice",
            r#"{"consent": true, "consent": false}"#.into(),
            "ambiguous",
        ),
        (
            "an export value two options share",
            r#"{"delivery_choice": "SAME"}"#.into(),
            "ambiguous",
        ),
        ("a read-only field", r#"{"ro": "x"}"#.into(), "not_editable"),
        ("a password field", r#"{"pin": "x"}"#.into(), "not_editable"),
        (
            "a file-select field",
            r#"{"upload": "x"}"#.into(),
            "not_editable",
        ),
        ("a comb field", r#"{"code": "x"}"#.into(), "not_editable"),
        (
            "a rich-text field",
            r#"{"rich": "x"}"#.into(),
            "not_editable",
        ),
        (
            "a hidden field",
            r#"{"secret": "x"}"#.into(),
            "not_editable",
        ),
        ("a wrong type", r#"{"consent": "yes"}"#.into(), "type"),
        (
            "a list answered with a string",
            r#"{"items": "A"}"#.into(),
            "type",
        ),
        (
            "an option the radio group lacks",
            r#"{"delivery": "Third"}"#.into(),
            "option",
        ),
        (
            "an option the list lacks",
            r#"{"items": ["A", "Z"]}"#.into(),
            "option",
        ),
        (
            "characters Helvetica cannot draw",
            r#"{"ACME.answer": "日本"}"#.into(),
            "characters",
        ),
        (
            "more than the field's maximum length",
            r#"{"ACME.answer": "twenty-one characters"}"#.into(),
            "length",
        ),
        (
            "a line break in a one-line field",
            r#"{"custom_choice": "one\ntwo"}"#.into(),
            "line",
        ),
        (
            "an answer that does not fit",
            format!(r#"{{"custom_choice": "{long}"}}"#),
            "layout",
        ),
    ];
    for (n, (what, answers, kind)) in refusals.iter().enumerate() {
        let file = dir.join(format!("refused-{n}.json"));
        std::fs::write(&file, answers).expect("answers");
        let out = dir.join(format!("refused-{n}.pdf"));
        let (code, stdout, stderr) = tool(
            &[
                "fill",
                &s(&every),
                "-o",
                &s(&out),
                "--values",
                &s(&file),
                "--json",
            ],
            &[],
        );
        let json = parsed(&stdout);
        let problems = json["problems"].as_array().cloned().unwrap_or_default();
        report.check(
            &format!("refused, and nothing written: {what}"),
            code == 3
                && !out.exists()
                && json["written"] == false
                && problems.len() == 1
                && problems[0]["problem"] == *kind
                && stderr.contains(problems[0]["field"].as_str().unwrap_or("\u{0}")),
            &format!("exit {code}: {stderr}{stdout}"),
        );
    }
    // Together, every problem is listed, in the order given, beside a fine answer.
    let together = format!(
        r#"{{"nobody": "x", "single_item": "Gamma", "ro": "x", "consent": "yes", "delivery": "Third",
             "ACME.answer": "日本", "custom_choice": "{long}"}}"#
    );
    let existing = dir.join("existing.pdf");
    std::fs::write(&existing, b"keep me").expect("existing");
    let file = dir.join("together.json");
    std::fs::write(&file, &together).expect("answers");
    let (code, stdout, stderr) = tool(
        &[
            "fill",
            &s(&every),
            "-o",
            &s(&existing),
            "--values",
            &s(&file),
            "--force",
            "--json",
        ],
        &[],
    );
    let kinds: Vec<String> = parsed(&stdout)["problems"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            format!(
                "{}:{}",
                p["field"].as_str().unwrap_or(""),
                p["problem"].as_str().unwrap_or("")
            )
        })
        .collect();
    report.check(
        "several problems are all listed, in order, and nothing is written, even with --force",
        code == 3
            && kinds
                == [
                    "nobody:unknown",
                    "ro:not_editable",
                    "consent:type",
                    "delivery:option",
                    "ACME.answer:characters",
                    "custom_choice:layout",
                ]
            && std::fs::read(&existing).ok().as_deref() == Some(b"keep me".as_slice())
            && stderr.contains("6 answers were refused"),
        &format!("exit {code}: {kinds:?}\n{stderr}"),
    );

    // --- the output's rules, as `sign` and `text` have them -----------------------
    let (code, _, stderr) = tool(
        &[
            "fill",
            &s(&every),
            "-o",
            &s(&existing),
            "--values",
            &s(&answers_file),
        ],
        &[],
    );
    report.check(
        "an existing output is refused without --force, and left alone",
        code == 3
            && stderr.contains("--force")
            && std::fs::read(&existing).ok().as_deref() == Some(b"keep me".as_slice()),
        &format!("exit {code}: {stderr}"),
    );
    let (code, _, stderr) = tool(
        &[
            "fill",
            &s(&every),
            "-o",
            &s(&existing),
            "--values",
            &s(&answers_file),
            "--force",
        ],
        &[],
    );
    report.check(
        "with --force it is replaced by the filled copy",
        code == 0 && says(&existing, &answers(), &[], &[]).is_ok(),
        &format!("exit {code}: {stderr}"),
    );
    let alias = dir.join("alias.pdf");
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&every, &alias);
    #[cfg(windows)]
    let linked = std::fs::hard_link(&every, &alias);
    let original = std::fs::read(&every).expect("input");
    let (code, _, stderr) = tool(
        &[
            "fill",
            &s(&every),
            "-o",
            &s(&alias),
            "--values",
            &s(&answers_file),
            "--force",
        ],
        &[],
    );
    report.check(
        "an output that is the input under another name is refused (2), and the input is untouched",
        linked.is_ok() && code == 2 && std::fs::read(&every).ok() == Some(original),
        &format!("exit {code}: {stderr}"),
    );
    // --force replaces a regular file and nothing else: not a directory, and
    // not whatever a link names, which the command line never did.
    let directory = dir.join("a-directory");
    std::fs::create_dir(&directory).expect("directory");
    #[allow(unused_mut)]
    let mut targets = vec![("a directory", directory)];
    let victim = dir.join("victim.txt");
    std::fs::write(&victim, b"not this run's").expect("victim");
    #[cfg(unix)]
    {
        let link = dir.join("link.pdf");
        std::os::unix::fs::symlink(&victim, &link).expect("link");
        targets.push(("a link", link));
    }
    for (what, target) in &targets {
        let (code, _, stderr) = tool(
            &[
                "fill",
                &s(&every),
                "-o",
                &s(target),
                "--values",
                &s(&answers_file),
                "--force",
            ],
            &[],
        );
        report.check(
            &format!("--force does not fill onto {what} (3), and what it names is as it was"),
            code == 3
                && stderr.contains("--force for a regular file")
                && std::fs::read(&victim).ok().as_deref() == Some(b"not this run's".as_slice())
                && target.symlink_metadata().is_ok_and(|data| !data.is_file()),
            &format!("exit {code}: {stderr}"),
        );
    }
    report.check(
        "fill leaves no staging directory beside its outputs",
        std::fs::read_dir(&dir).expect("scratch").all(|entry| {
            !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".tpdf-cli-")
        }),
        "a .tpdf-cli- directory is left",
    );

    // --- a signed document ------------------------------------------------------------
    let now = now();
    let store = super::TestStore {
        certificate: super::certificate(now),
        misdirected: false,
    };
    let acme = dir.join("acme.pdf");
    std::fs::write(&acme, acme_pdf()).expect("acme");
    let signed = dir.join("acme-signed.pdf");
    let (code, _, stderr) = signs(
        &strings(&[
            "sign",
            &s(&acme),
            "-o",
            &s(&signed),
            "--identity",
            super::SUBJECT,
        ]),
        &store,
        now,
    );
    let acme_answers = dir.join("acme.json");
    std::fs::write(
        &acme_answers,
        r#"{"ACME.answer": "Grüße", "consent": true}"#,
    )
    .expect("answers");
    let refused_out = dir.join("signed-filled.pdf");
    let (fill_code, _, fill_err) = tool(
        &[
            "fill",
            &s(&signed),
            "-o",
            &s(&refused_out),
            "--values",
            &s(&acme_answers),
        ],
        &[],
    );
    report.check(
        "a signed form is refused, says to fill before signing, and nothing is written",
        code == 0 && fill_code == 3 && fill_err.contains("tpdf sign") && !refused_out.exists(),
        &format!("sign exit {code}: {stderr}; fill exit {fill_code}: {fill_err}"),
    );
    match fixture("incr-certified-2.pdf") {
        Some(certified) => {
            let out = dir.join("certified-filled.pdf");
            let (code, _, stderr) = tool(
                &[
                    "fill",
                    &s(&certified),
                    "-o",
                    &s(&out),
                    "--values",
                    &s(&acme_answers),
                ],
                &[],
            );
            report.check(
                "a document certified for form filling is refused too: this writer rewrites it",
                code == 3 && stderr.contains("is signed") && !out.exists(),
                &format!("exit {code}: {stderr}"),
            );
        }
        None => report.skip(
            "the certified document",
            "incr-certified-2.pdf is not generated",
        ),
    }

    // --- fill, then sign ------------------------------------------------------------------
    let acme_filled = dir.join("acme-filled.pdf");
    let (code, _, stderr) = tool(
        &[
            "fill",
            &s(&acme),
            "-o",
            &s(&acme_filled),
            "--values",
            &s(&acme_answers),
        ],
        &[],
    );
    let then_signed = dir.join("acme-filled-signed.pdf");
    let (sign_code, _, sign_err) = signs(
        &strings(&[
            "sign",
            &s(&acme_filled),
            "-o",
            &s(&then_signed),
            "--identity",
            super::SUBJECT,
        ]),
        &store,
        now,
    );
    let verdicts = super::read_back(&then_signed).map(|(lines, _)| lines);
    let acme_want: serde_json::Value =
        serde_json::from_str(r#"{"ACME.answer": "Grüße", "consent": true}"#).expect("json");
    report.check(
        "a filled form signed with the software key reads back intact, answers and all",
        code == 0
            && sign_code == 0
            && verdicts
                .as_ref()
                .is_ok_and(|v| v.len() == 1 && v[0].1 == "intact")
            && says(&then_signed, &acme_want, &[], &[]).is_ok(),
        &format!("fill exit {code}: {stderr}; sign exit {sign_code}: {sign_err}; {verdicts:?}"),
    );

    // --- an encrypted form ------------------------------------------------------------------
    let encrypted = dir.join("encrypted.pdf");
    let made = Command::new("qpdf")
        .args([
            "--encrypt",
            "swordfish",
            "owner-secret",
            "256",
            "--",
            &s(&every),
            &s(&encrypted),
        ])
        .status()
        .is_ok_and(|status| status.success());
    if made {
        let password = [("TPDF_IT_PASSWORD", "swordfish")];
        let out = dir.join("encrypted-filled.pdf");
        let (code, stdout, stderr) = tool(
            &[
                "fill",
                &s(&encrypted),
                "-o",
                &s(&out),
                "--values",
                &s(&answers_file),
                "--password-env",
                "TPDF_IT_PASSWORD",
            ],
            &password,
        );
        report.check(
            "an encrypted form is filled with its password from the environment",
            code == 0
                && says(
                    &out,
                    &answers(),
                    &password,
                    &["--password-env", "TPDF_IT_PASSWORD"],
                )
                .is_ok()
                && !stdout.contains("swordfish")
                && !stderr.contains("swordfish"),
            &format!("exit {code}: {stderr}"),
        );
        let (locked, _, why) = tool(&["fields", &s(&out)], &[]);
        let still = Command::new("qpdf")
            .args(["--is-encrypted", &s(&out)])
            .status()
            .ok()
            .and_then(|status| status.code());
        report.check(
            "and the filled copy is still encrypted: locked without the password",
            locked == 3 && why.contains("--password-env") && still == Some(0),
            &format!("exit {locked}: {why}; qpdf {still:?}"),
        );
        let (code, _, stderr) = tool(
            &[
                "fill",
                &s(&encrypted),
                "-o",
                &s(&dir.join("nopw.pdf")),
                "--values",
                &s(&answers_file),
            ],
            &[],
        );
        report.check(
            "without its password, fill refuses it (3) and writes nothing",
            code == 3 && stderr.contains("--password-env") && !dir.join("nopw.pdf").exists(),
            &format!("exit {code}: {stderr}"),
        );
    } else {
        report.skip("the encrypted form", "qpdf is not installed");
    }

    if let Some(probe) = probe {
        let _ = std::fs::create_dir_all(&probe);
        for (from, to) in [
            (&every, "every-control.pdf"),
            (&filled, "every-control-filled.pdf"),
            (&answers_file, "every-control-answers.json"),
            (&acme, "acme.pdf"),
            (&acme_filled, "acme-filled.pdf"),
            (&then_signed, "acme-filled-signed.pdf"),
        ] {
            let _ = std::fs::copy(from, probe.join(to));
        }
        println!("[INFO] filled files left in {}", probe.display());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
