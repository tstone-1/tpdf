//! `tpdf form` through the shipped tool.
//!
//! The tool's own `fields` and `fill` read the copy back, which is the route a
//! reader takes next; `lopdf`, loaded here, reads the objects the tool wrote
//! without going through `forms::scan`; and `qpdf`, where it is installed,
//! checks the file as a second opinion that shares no code with the writer.

use std::process::Command;

use super::forms::{every_control_pdf, tool_with_stdin};
use super::{fixture, scratch, tool, Report};

const LIST: &str = r#"[
  {"name": "Name", "kind": "text", "page": 1, "rect": [72, 100, 250, 20],
   "tooltip": "Your full name", "required": true, "max_length": 40},
  {"name": "Remarks", "kind": "multiline", "page": 1, "rect": [72, 140, 250, 80]},
  {"name": "Agree", "kind": "checkbox", "page": 1, "rect": [72, 240, 14, 14]}
]"#;

fn parsed(json: &str) -> serde_json::Value {
    serde_json::from_str(json).unwrap_or_default()
}

/// The fields of `path` as `tpdf fields --json` lists them, by name.
fn listed(path: &str, key: Option<&str>) -> Vec<serde_json::Value> {
    let (_, out, _) = match key {
        Some(key) => tool(
            &["fields", path, "--json", "--password-env", "KEY"],
            &[("KEY", key)],
        ),
        None => tool(&["fields", path, "--json"], &[]),
    };
    parsed(&out)["fields"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn named<'a>(fields: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
    fields
        .iter()
        .find(|f| f["name"] == name)
        .unwrap_or(&serde_json::Value::Null)
}

pub(super) fn adds_fields_that_can_be_filled(report: &mut Report) {
    // `rotated.pdf` twice over: its first page is upright and the other three
    // are turned, and it is a fixture every runner generates, which a document
    // embedding a system font is not.
    let (Some(plain), Some(turned), Some(locked), Some(signed)) = (
        fixture("rotated.pdf"),
        fixture("rotated.pdf"),
        fixture("incr-encrypted-pw.pdf"),
        fixture("incr-signed.pdf"),
    ) else {
        report.skip("form", "testdata/ is not generated");
        return;
    };
    let dir = scratch("form");
    let at = |name: &str| dir.join(name).display().to_string();
    let (plain, turned, locked, signed) = (
        plain.display().to_string(),
        turned.display().to_string(),
        locked.display().to_string(),
        signed.display().to_string(),
    );
    let list = at("fields.json");
    std::fs::write(&list, LIST).expect("the list");

    // A document with no form gets one.
    let made = at("made.pdf");
    let (code, json, stderr) = tool(
        &["form", &plain, "-o", &made, "--fields", &list, "--json"],
        &[],
    );
    let result = parsed(&json);
    report.check(
        "form reports each field added, with its page and place",
        code == 0
            && result["command"] == "form"
            && result["fields_before"] == 0
            && result["fields_after"] == 3
            && result["added"].as_array().is_some_and(|a| a.len() == 3)
            && result["added"][0]["name"] == "Name"
            && result["added"][0]["kind"] == "text"
            && result["added"][0]["multiline"] == false
            && result["added"][0]["page"] == 1
            && result["added"][0]["rect"] == serde_json::json!([72.0, 100.0, 250.0, 20.0])
            && result["added"][1]["multiline"] == true
            && result["added"][2]["kind"] == "checkbox",
        &format!("exit {code}; {stderr}; {json}"),
    );
    let fields = listed(&made, None);
    report.check(
        "fields lists them, empty and answerable, with the limit asked for",
        fields.len() == 3
            && fields
                .iter()
                .all(|f| f["editable"] == true && f["not_editable"].is_null())
            && named(&fields, "Name")["kind"] == "text"
            && named(&fields, "Name")["value"] == ""
            && named(&fields, "Name")["max_length"] == 40
            && named(&fields, "Name")["multiline"] == false
            && named(&fields, "Remarks")["multiline"] == true
            && named(&fields, "Remarks")["max_length"].is_null()
            && named(&fields, "Agree")["kind"] == "checkbox"
            && named(&fields, "Agree")["value"] == false,
        &format!("{fields:?}"),
    );

    // The objects, read without the tool's own form reader.
    let doc = lopdf::Document::load(&made).expect("the copy parses");
    let form = doc
        .catalog()
        .and_then(|c| c.get(b"AcroForm"))
        .and_then(|o| doc.dereference(o))
        .and_then(|(_, o)| o.as_dict());
    let kids: Vec<&lopdf::Dictionary> = form
        .as_ref()
        .ok()
        .and_then(|f| f.get(b"Fields").ok())
        .and_then(|o| o.as_array().ok())
        .into_iter()
        .flatten()
        .filter_map(|o| doc.dereference(o).ok().and_then(|(_, o)| o.as_dict().ok()))
        .collect();
    let has_font = form
        .as_ref()
        .ok()
        .and_then(|f| f.get(b"DR").ok())
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Font").ok())
        .and_then(|o| o.as_dict().ok())
        .is_some_and(|fonts| fonts.has(b"Helv"));
    let name = kids.first();
    let rect: Vec<f32> = name
        .and_then(|d| d.get(b"Rect").ok())
        .and_then(|o| o.as_array().ok())
        .into_iter()
        .flatten()
        .filter_map(|o| o.as_float().ok())
        .collect();
    report.check(
        "the copy holds a form with its font, and each field is a widget with an appearance",
        kids.len() == 3
            && has_font
            && kids.iter().all(|d| {
                d.get(b"Subtype").and_then(|o| o.as_name()).ok() == Some(b"Widget".as_slice())
                    && d.has(b"AP")
                    && d.has(b"P")
                    && d.get(b"F").and_then(|o| o.as_i64()).ok() == Some(4)
            })
            // 100 points from the top of a 792-point page, 20 high.
            && rect == [72.0, 672.0, 322.0, 692.0]
            && name.and_then(|d| d.get(b"Ff").ok()).and_then(|o| o.as_i64().ok()) == Some(2)
            && kids[1].get(b"Ff").and_then(|o| o.as_i64()).ok() == Some(1 << 12),
        &format!("{} fields; font {has_font}; rect {rect:?}", kids.len()),
    );
    match Command::new("qpdf").args(["--check", &made]).output() {
        Ok(out) => report.check(
            "qpdf finds nothing wrong with the copy",
            out.status.code() == Some(0),
            &String::from_utf8_lossy(&out.stderr),
        ),
        Err(_) => report.skip(
            "qpdf checks the copy with new fields",
            "qpdf is unavailable",
        ),
    }

    // The fields take answers, which is what they are for.
    let filled = at("filled.pdf");
    let (code, _, stderr) = tool_with_stdin(
        &["fill", &made, "-o", &filled, "--values", "-"],
        &[],
        r#"{"Name": "Ada Lovelace", "Remarks": "one\ntwo", "Agree": true}"#,
    );
    let answered = listed(&filled, None);
    report.check(
        "fill answers the new fields and they read back",
        code == 0
            && named(&answered, "Name")["value"] == "Ada Lovelace"
            && named(&answered, "Remarks")["value"] == "one\ntwo"
            && named(&answered, "Agree")["value"] == true,
        &format!("exit {code}; {stderr}; {answered:?}"),
    );
    // A dropdown: added with its choices, listed with them, and answered
    // with one of them and with nothing else.
    let chooser = at("chooser.pdf");
    let (code, json, stderr) = tool_with_stdin(
        &["form", &plain, "-o", &chooser, "--fields", "-", "--json"],
        &[],
        r#"[{"name": "Country", "kind": "dropdown", "page": 1, "rect": [72, 100, 160, 20],
             "options": ["Germany", "France", "Spain"]}]"#,
    );
    let listed_now = listed(&chooser, None);
    let country = named(&listed_now, "Country");
    report.check(
        "form adds a dropdown, and fields lists it with its choices and nothing chosen",
        code == 0
            && parsed(&json)["added"][0]["kind"] == "choice_combo"
            && country["value"].is_null()
            && country.to_string().contains("Germany")
            && country.to_string().contains("Spain"),
        &format!("exit {code}; {stderr}; {json}; {country}"),
    );
    let chosen = at("chosen.pdf");
    let (code, _, stderr) = tool_with_stdin(
        &["fill", &chooser, "-o", &chosen, "--values", "-"],
        &[],
        r#"{"Country": "France"}"#,
    );
    let after = listed(&chosen, None);
    report.check(
        "fill chooses one of them and it reads back",
        code == 0 && named(&after, "Country")["value"] == "France",
        &format!("exit {code}; {stderr}; {after:?}"),
    );
    let (code, _, stderr) = tool_with_stdin(
        &["fill", &chooser, "-o", &at("unlisted.pdf"), "--values", "-"],
        &[],
        r#"{"Country": "Italy"}"#,
    );
    report.check(
        "an answer that is not one of its choices is refused",
        code == 3 && !std::path::Path::new(&at("unlisted.pdf")).exists(),
        &format!("exit {code}; {stderr}"),
    );
    let (code, _, stderr) = tool_with_stdin(
        &["form", &plain, "-o", &at("none.pdf"), "--fields", "-"],
        &[],
        r#"[{"name": "Country", "kind": "dropdown", "page": 1, "rect": [72, 100, 160, 20]}]"#,
    );
    report.check(
        "a dropdown with no choices is refused",
        code == 3
            && stderr.contains("needs at least one choice")
            && !std::path::Path::new(&at("none.pdf")).exists(),
        &format!("exit {code}; {stderr}"),
    );

    let (code, _, stderr) = tool_with_stdin(
        &["fill", &made, "-o", &at("long.pdf"), "--values", "-"],
        &[],
        &format!(r#"{{"Name": "{}"}}"#, "x".repeat(41)),
    );
    report.check(
        "the limit a field was given is one fill holds an answer to",
        code == 3 && !std::path::Path::new(&at("long.pdf")).exists(),
        &format!("exit {code}; {stderr}"),
    );

    // A document that has a form keeps what it had.
    let every = at("every.pdf");
    std::fs::write(&every, every_control_pdf()).expect("every control");
    let before = listed(&every, None);
    let more = at("more.pdf");
    let (code, json, stderr) = tool_with_stdin(
        &["form", &every, "-o", &more, "--fields", "-", "--json"],
        &[],
        r#"[{"name": "Extra", "kind": "checkbox", "page": 1, "rect": [10, 10, 12, 12]}]"#,
    );
    let after = listed(&more, None);
    let kept = before.iter().all(|was| {
        let now = named(&after, was["name"].as_str().unwrap_or_default());
        now["kind"] == was["kind"]
            && now["value"] == was["value"]
            && now["options"] == was["options"]
    });
    report.check(
        "a form that exists gains the field and keeps every field and answer it had",
        code == 0
            && !before.is_empty()
            && after.len() == before.len() + 1
            && kept
            && named(&after, "Extra")["kind"] == "checkbox"
            && parsed(&json)["fields_before"] == before.len()
            && parsed(&json)["fields_after"] == before.len() + 1,
        &format!(
            "exit {code}; {stderr}; {} then {}",
            before.len(),
            after.len()
        ),
    );
    let taken = before[0]["name"].as_str().unwrap_or_default().to_string();
    let (code, _, stderr) = tool_with_stdin(
        &["form", &every, "-o", &at("twice.pdf"), "--fields", "-"],
        &[],
        &format!(
            r#"[{{"name": "Fine", "kind": "text", "page": 1, "rect": [10, 30, 60, 20]}},
                {{"name": {}, "kind": "text", "page": 1, "rect": [10, 60, 60, 20]}},
                {{"name": "Off", "kind": "text", "page": 1, "rect": [10, 60, 9000, 20]}}]"#,
            serde_json::Value::String(taken.split('.').next().unwrap_or_default().to_string())
        ),
    );
    report.check(
        "every problem is named at once and nothing is written",
        code == 3
            && stderr.contains("already has a field of this name")
            && stderr.contains("`Off`: its rectangle is not inside page 1")
            && !std::path::Path::new(&at("twice.pdf")).exists(),
        &format!("exit {code}; {stderr}"),
    );

    // Refusals that leave no file.
    let field = |page: u32| {
        format!(r#"[{{"name": "N", "kind": "text", "page": {page}, "rect": [72, 100, 200, 20]}}]"#)
    };
    let refused = at("refused.pdf");
    let gone = || !std::path::Path::new(&refused).exists();
    let (code, _, stderr) = tool_with_stdin(
        &["form", &turned, "-o", &refused, "--fields", "-"],
        &[],
        &field(2),
    );
    report.check(
        "a page the document turns is refused, and says so",
        code == 3 && stderr.contains("page 2 is turned") && gone(),
        &format!("exit {code}; {stderr}"),
    );
    let (code, _, stderr) = tool_with_stdin(
        &["form", &turned, "-o", &refused, "--fields", "-"],
        &[],
        &field(5),
    );
    report.check(
        "a page the document does not have is refused",
        code == 3 && stderr.contains("there is no page 5; the document has 4") && gone(),
        &format!("exit {code}; {stderr}"),
    );
    let (code, _, stderr) = tool_with_stdin(
        &["form", &signed, "-o", &refused, "--fields", "-"],
        &[],
        &field(1),
    );
    report.check(
        "a signed document is refused unless its signatures may be invalidated",
        code == 3 && stderr.contains("--invalidate-signatures") && gone(),
        &format!("exit {code}; {stderr}"),
    );
    let allowed = at("allowed.pdf");
    let (code, json, stderr) = tool_with_stdin(
        &[
            "form",
            &signed,
            "-o",
            &allowed,
            "--fields",
            "-",
            "--invalidate-signatures",
            "--json",
        ],
        &[],
        &field(1),
    );
    report.check(
        "with the flag it is written, and the report counts the signatures",
        code == 0
            && parsed(&json)["signatures_invalidated"]
                .as_u64()
                .is_some_and(|n| n > 0)
            && named(&listed(&allowed, None), "N")["kind"] == "text",
        &format!("exit {code}; {stderr}; {json}"),
    );

    // A document behind a password stays behind it.
    let secret = at("secret.pdf");
    let (code, _, stderr) = tool_with_stdin(
        &[
            "form",
            &locked,
            "-o",
            &secret,
            "--fields",
            "-",
            "--password-env",
            "KEY",
        ],
        &[("KEY", "swordfish")],
        &field(1),
    );
    let (without, _, _) = tool(&["fields", &secret], &[]);
    report.check(
        "a protected document gets the field and still asks for its password",
        code == 0
            && without == 3
            && named(&listed(&secret, Some("swordfish")), "N")["kind"] == "text",
        &format!("exit {code}; {stderr}; without the password {without}"),
    );

    // The source is never touched, and an existing output is not replaced unasked.
    let (code, _, stderr) = tool(&["form", &plain, "-o", &made, "--fields", &list], &[]);
    report.check(
        "an output that exists is not replaced without --force",
        code != 0 && listed(&made, None).len() == 3,
        &format!("exit {code}; {stderr}"),
    );
    report.check(
        "the source has no form afterwards either",
        listed(&plain, None).is_empty(),
        "",
    );

    // A text size and a default value: the field is made holding its default.
    let sized = at("sized.pdf");
    let (code, json, stderr) = tool_with_stdin(
        &["form", &plain, "-o", &sized, "--fields", "-", "--json"],
        &[],
        r#"[{"name": "Ref", "kind": "text", "page": 1, "rect": [72, 100, 250, 20],
             "text_size": 9, "default_value": "n/a"}]"#,
    );
    let doc = lopdf::Document::load(&sized).ok();
    let written = doc.as_ref().and_then(|doc| {
        doc.objects.values().find_map(|object| {
            let dict = object.as_dict().ok()?;
            let named = dict.get(b"T").ok()?.as_str().ok()?;
            (named.ends_with(b"\0R\0e\0f")).then(|| {
                (
                    dict.get(b"DA")
                        .and_then(lopdf::Object::as_str)
                        .map(<[u8]>::to_vec)
                        .ok(),
                    dict.has(b"DV"),
                )
            })
        })
    });
    report.check(
        "form makes a field with its text size, holding its default value",
        code == 0
            && named(&listed(&sized, None), "Ref")["value"] == "n/a"
            && named(&listed(&sized, None), "Ref")["editable"] == true
            && written == Some((Some(b"/Helv 9 Tf 0 g".to_vec()), true)),
        &format!("exit {code}; {stderr}; {json}; {written:?}"),
    );
    let refused = at("refused.pdf");
    let (code, _, stderr) = tool_with_stdin(
        &["form", &plain, "-o", &refused, "--fields", "-"],
        &[],
        r#"[{"name": "Ref", "kind": "text", "page": 1, "rect": [72, 100, 40, 12],
             "default_value": "a default value far too long for forty points"},
            {"name": "Box", "kind": "checkbox", "page": 1, "rect": [72, 140, 14, 14],
             "text_size": 9}]"#,
    );
    report.check(
        "a default that does not fit and a text size on a checkbox are both named, and nothing is written",
        code != 0
            && stderr.contains("its default value")
            && stderr.contains("no text to size")
            && !std::path::Path::new(&refused).exists(),
        &format!("exit {code}; {stderr}"),
    );
}
