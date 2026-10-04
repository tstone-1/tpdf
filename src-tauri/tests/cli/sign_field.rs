//! `sign --field`: the signature goes into an empty signature field the
//! document has, and no field is added. The fields are made by `form`.
use super::forms::tool_with_stdin;
use super::{
    certificate, now, plain_pdf, read_back, scratch, signs, strings, Report, TestStore, SUBJECT,
};
use std::path::Path;

/// A field's `/T`, which is PDFDocEncoding or UTF-16 behind a byte-order mark.
fn name_of(dict: &lopdf::Dictionary) -> String {
    let raw = dict
        .get(b"T")
        .and_then(lopdf::Object::as_str)
        .unwrap_or_default();
    match raw.strip_prefix(&[0xFE, 0xFF]) {
        Some(wide) => String::from_utf16_lossy(
            &wide
                .chunks(2)
                .map(|pair| u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]))
                .collect::<Vec<_>>(),
        ),
        None => String::from_utf8_lossy(raw).into_owned(),
    }
}

/// `(name, whether it holds a signature)` of every signature field, by name.
fn signature_fields(path: &Path) -> Vec<(String, bool)> {
    let doc = lopdf::Document::load(path).expect("document");
    let mut found: Vec<(String, bool)> = doc
        .objects
        .values()
        .filter_map(|object| {
            let dict = object.as_dict().ok()?;
            (dict.get(b"FT").ok()?.as_name().ok()? == b"Sig")
                .then(|| (name_of(dict), dict.has(b"V")))
        })
        .collect();
    found.sort();
    found
}

/// The strings an appearance under a field called `name` shows.
fn drawn_in(path: &Path, name: &str) -> (Vec<f32>, String) {
    let doc = lopdf::Document::load(path).expect("document");
    let field = doc
        .objects
        .values()
        .filter_map(|object| object.as_dict().ok())
        .find(|dict| dict.has(b"T") && name_of(dict) == name)
        .expect("the field");
    let rect = field
        .get(b"Rect")
        .and_then(lopdf::Object::as_array)
        .expect("rect")
        .iter()
        .map(|v| v.as_float().expect("number"))
        .collect();
    let form = field
        .get(b"AP")
        .and_then(lopdf::Object::as_dict)
        .and_then(|ap| ap.get(b"N"))
        .and_then(lopdf::Object::as_reference)
        .expect("an appearance");
    let stream = doc
        .get_object(form)
        .and_then(lopdf::Object::as_stream)
        .expect("stream");
    let content = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    (rect, String::from_utf8_lossy(&content).into_owned())
}

pub fn signs_the_field(report: &mut Report) {
    let now = now();
    let dir = scratch("sign-field");
    let s = |p: &Path| p.display().to_string();
    let plain = dir.join("plain.pdf");
    std::fs::write(&plain, plain_pdf()).expect("input");
    let form = dir.join("form.pdf");
    let (code, _, stderr) = tool_with_stdin(
        &["form", &s(&plain), "-o", &s(&form), "--fields", "-"],
        &[],
        r#"[{"name": "Approved", "kind": "signature", "page": 1, "rect": [40, 40, 200, 60], "border": true},
            {"name": "Witness", "kind": "signature", "page": 1, "rect": [40, 140, 200, 60]},
            {"name": "Name", "kind": "text", "page": 1, "rect": [40, 220, 200, 20]}]"#,
    );
    report.check(
        "control: form makes two empty signature fields",
        code == 0
            && signature_fields(&form) == [("Approved".into(), false), ("Witness".into(), false)],
        &format!("exit {code}; {stderr}; {:?}", signature_fields(&form)),
    );
    let store = TestStore {
        certificate: certificate(now),
        misdirected: false,
    };
    let line = |input: &Path, out: &Path, extra: &[&str]| {
        let mut args = strings(&[
            "sign",
            &s(input),
            "-o",
            &s(out),
            "--identity",
            SUBJECT,
            "--json",
        ]);
        args.extend(extra.iter().map(|a| (*a).to_string()));
        args
    };

    // Into the first field, with an appearance: drawn in the field's rectangle.
    let was = drawn_in(&form, "Approved");
    let once = dir.join("once.pdf");
    let (code, stdout, stderr) = signs(
        &line(
            &form,
            &once,
            &["--field", "Approved", "--visible", "--no-image"],
        ),
        &store,
        now,
    );
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    let now_drawn = drawn_in(&once, "Approved");
    let read = read_back(&once);
    report.check(
        "sign --field signs the field the document has, and adds none",
        code == 0
            && json["field"] == "Approved"
            && signature_fields(&once) == [("Approved".into(), true), ("Witness".into(), false)]
            && read
                .as_ref()
                .is_ok_and(|(lines, _)| lines == &[("Approved".to_string(), "intact".to_string())]),
        &format!(
            "exit {code}; {stderr}; {:?}; {read:?}",
            signature_fields(&once)
        ),
    );
    report.check(
        "with --visible it is drawn in the field's rectangle, which does not move",
        now_drawn.0 == was.0
            && was.0 == [40.0, 200.0, 240.0, 260.0]
            && now_drawn.1.contains("Tj")
            && !was.1.contains("Tj")
            && json["appearance"]["rect"] == serde_json::json!([40.0, 40.0, 200.0, 60.0])
            && json["appearance"]["page"] == 1,
        &format!("{:?} to {:?}; {}", was.0, now_drawn.0, json["appearance"]),
    );

    // The second field, without an appearance, in the document signed once:
    // both signatures intact, and still two fields.
    let twice = dir.join("twice.pdf");
    let (code, stdout, stderr) = signs(&line(&once, &twice, &["--field", "Witness"]), &store, now);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    let read = read_back(&twice);
    report.check(
        "a second field is signed after the first, and both signatures are intact",
        code == 0
            && json["field"] == "Witness"
            && signature_fields(&twice) == [("Approved".into(), true), ("Witness".into(), true)]
            && read.as_ref().is_ok_and(|(lines, _)| {
                lines.len() == 2 && lines.iter().all(|(_, verdict)| verdict == "intact")
            }),
        &format!("exit {code}; {stderr}; {read:?}"),
    );

    // Refusals: 3 for what the document says, 2 for a line that contradicts
    // itself, and no file either way.
    let refused = dir.join("refused.pdf");
    for (what, input, extra, exit, said) in [
        (
            "a name the document does not have",
            &form,
            vec!["--field", "Nobody"],
            3,
            "has no field called `Nobody`",
        ),
        (
            "the same name with an appearance, before any certificate is listed",
            &form,
            vec!["--field", "Nobody", "--visible"],
            3,
            "has no field called `Nobody`",
        ),
        (
            "a field that is not a signature field",
            &form,
            vec!["--field", "Name"],
            3,
            "`Name` is not a signature field",
        ),
        (
            "a field that already holds a signature",
            &once,
            vec!["--field", "Approved"],
            3,
            "`Approved` already holds a signature",
        ),
        (
            "a rectangle beside a field",
            &form,
            vec!["--field", "Approved", "--visible", "--rect", "1,1,100,50"],
            2,
            "leave `--rect` out",
        ),
        (
            "a page beside a field",
            &form,
            vec!["--field", "Approved", "--visible", "--page", "1"],
            2,
            "leave `--page` out",
        ),
    ] {
        let (code, _, stderr) = signs(&line(input, &refused, &extra), &store, now);
        report.check(
            &format!("sign --field refuses {what}"),
            code == exit && stderr.contains(said) && !refused.exists(),
            &format!("exit {code}; {stderr}"),
        );
    }
}
