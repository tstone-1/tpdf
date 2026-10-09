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

/// The document at `path` with one more revision, which gives the field
/// called `name` another rectangle and writes nothing else.
fn with_the_field_moved(path: &Path, name: &str) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("the signed document");
    let prev = lopdf::Document::load_mem(&bytes).expect("it parses");
    let (id, mut field) = prev
        .objects
        .iter()
        .find_map(|(id, object)| {
            let dict = object.as_dict().ok()?;
            (dict.has(b"T") && name_of(dict) == name).then(|| (*id, dict.clone()))
        })
        .expect("the field");
    field.set("Rect", vec![0.into(), 0.into(), 300.into(), 300.into()]);
    let mut incremental = lopdf::IncrementalDocument::create_from(bytes.clone(), prev);
    incremental.new_document.set_object(id, field);
    let mut out = Vec::new();
    incremental.save_to(&mut out).expect("the revision saves");
    assert_eq!(&out[..bytes.len()], bytes, "a revision appended");
    out
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

    // What the first signature is told followed it: the second, in a place
    // the document it signed already had. No page object is written for
    // that, and the page is not one a signature fails to cover.
    let found = super::verify_appendix::signatures(&twice);
    // Only the first, so that `judged` reads its appendix and not the
    // second's, which has none.
    let first: Vec<serde_json::Value> = found
        .iter()
        .filter(|s| s["appended_bytes"].as_u64().is_some_and(|n| n > 0))
        .cloned()
        .collect();
    let after_first = first
        .first()
        .map(|s| s["appendix"].clone())
        .unwrap_or_default();
    let (_, text, _) = super::tool(&["verify", &s(&twice)], &[]);
    report.check(
        "a second signature in a prepared field is not a page rewritten after the first",
        found.len() == 2
            && after_first["unread"] == false
            && after_first["pages_touched"] == 0
            && after_first["pages_listing"] == serde_json::json!([])
            && after_first["sentence"] == "another signature, and no page was rewritten"
            && first.len() == 1
            && super::verify_appendix::judged(&first) == Some((false, 0))
            && !text.contains("After the last signature"),
        &format!("{after_first}\n{text}"),
    );
    // Control: the same field moved after the first signature, by a revision
    // written here. One page, which no signature covers.
    let moved = dir.join("moved.pdf");
    std::fs::write(&moved, with_the_field_moved(&once, "Witness")).expect("the moved copy");
    let found = super::verify_appendix::signatures(&moved);
    let (code, text, _) = super::tool(&["verify", "--strict", &s(&moved)], &[]);
    report.check(
        "control: the empty field moved after the signature is a page rewritten",
        code == 1
            && super::verify_appendix::judged(&found) == Some((false, 1))
            && text.contains("After the last signature: 1 page was rewritten"),
        &format!("exit {code}; {found:?}\n{text}"),
    );

    // The window's own path to the worker: the render service's request, which
    // the application's signing command makes with the field a reader pressed.
    // The command-line tool above asks a worker of its own another way.
    {
        use tpdf_lib::render::{Backend, RenderService};
        let service = RenderService::start_with(super::library_dir(), Backend::Worker);
        let (tx, rx) = std::sync::mpsc::channel();
        service.open_handed(
            form.clone(),
            None,
            true,
            None,
            Box::new(move |opened| {
                let _ = tx.send(opened.map(|info| info.id).map_err(|why| why.reason));
            }),
        );
        let opened = rx
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("opened");
        let ask = |field: &str| {
            let (tx, rx) = std::sync::mpsc::channel();
            match &opened {
                Ok(doc) => service.prepare_signature(
                    *doc,
                    now,
                    None,
                    field.to_string(),
                    Box::new(move |made| {
                        let _ = tx.send(made);
                    }),
                ),
                Err(why) => {
                    let _ = tx.send(Err(why.clone()));
                }
            }
            rx.recv_timeout(std::time::Duration::from_secs(60))
                .expect("answered")
        };
        let original = std::fs::read(&form).expect("form");
        let fields_after = |update: &[u8]| {
            let whole = dir.join("prepared.pdf");
            std::fs::write(&whole, [original.as_slice(), update].concat()).expect("written");
            signature_fields(&whole)
        };
        let into = ask("Witness");
        report.check(
            "the window's request signs the field it names, through a pooled worker",
            into.as_ref().is_ok_and(|unsigned| {
                unsigned.field == "Witness"
                    && fields_after(&unsigned.update)
                        == [("Approved".into(), false), ("Witness".into(), true)]
            }),
            &format!("{:?}", into.as_ref().map(|u| u.field.clone())),
        );
        let own = ask("");
        report.check(
            "and with no field named it makes one of its own",
            own.as_ref().is_ok_and(|unsigned| {
                unsigned.field == "Signature1" && fields_after(&unsigned.update).len() == 3
            }),
            &format!("{:?}", own.as_ref().map(|u| u.field.clone())),
        );
        let missing = ask("Nobody");
        report.check(
            "and a name the document does not have is the worker's refusal",
            missing
                .as_ref()
                .is_err_and(|why| why.contains("has no field called `Nobody`")),
            &format!("{:?}", missing.as_ref().map(|u| u.field.clone())),
        );
    }

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
