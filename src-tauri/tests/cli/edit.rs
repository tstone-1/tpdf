//! External edit requests checked with independent object/text reads.
use super::{
    forms::tool_with_stdin,
    pages::{fixture, names},
    scratch, tool, Report,
};
use lopdf::Document;
use serde_json::{json, Value};
use std::path::Path;

fn edit(source: &Path, output: &Path, ops: Value, flags: &[&str]) -> (i32, Value, String) {
    let source = source.display().to_string();
    let output = output.display().to_string();
    let mut args = vec!["edit", &source, "--plan", "-", "-o", &output, "--json"];
    args.extend_from_slice(flags);
    let (code, text, stderr) = tool_with_stdin(
        &args,
        &[],
        &json!({"schema":1,"operations":ops}).to_string(),
    );
    (
        code,
        serde_json::from_str(&text).expect("one JSON report"),
        stderr,
    )
}
fn annotations(path: &Path) -> Vec<(usize, String, String, Vec<f32>)> {
    let document = Document::load(path).unwrap();
    let mut found = Vec::new();
    for (page, id) in document.get_pages() {
        let dict = document.get_dictionary(id).unwrap();
        if let Ok(objects) = dict.get(b"Annots") {
            let objects = document.dereference(objects).unwrap().1.as_array().unwrap();
            for object in objects {
                let dict = document.dereference(object).unwrap().1.as_dict().unwrap();
                let kind =
                    String::from_utf8_lossy(dict.get(b"Subtype").unwrap().as_name().unwrap())
                        .into_owned();
                if kind == "Popup" {
                    continue;
                }
                let body = dict
                    .get(b"Contents")
                    .ok()
                    .and_then(|o| o.as_str().ok())
                    .map(tpdf_lib::annots::decode_text_string)
                    .unwrap_or_default();
                let rect = dict
                    .get(b"Rect")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_float().unwrap())
                    .collect();
                found.push((page as usize, kind, body, rect));
            }
        }
    }
    found
}

pub(super) fn operations(report: &mut Report) {
    let dir = scratch("edit-plans");
    let source = dir.join("source.pdf");
    fixture(&source, &["Alpha", "Bravo", "Charlie"]);
    let original = std::fs::read(&source).unwrap();
    let output = dir.join("edited.pdf");
    let operations = json!([
        {"op":"move_page","page":1,"to":3},
        {"op":"delete_page","page":2},
        {"op":"insert_blank","after":1,"width":300,"height":400},
        {"op":"rotate","page":3,"degrees":90},
        {"op":"annotate","page":2,"kind":"note","rect":[30,40,20,20],"text":"Synthetic note","author":"Test"},
        {"op":"undo"}, {"op":"redo"},
        {"op":"annotate","page":3,"kind":"square","rect":[20,30,80,40],"text":"Turned box"}
    ]);
    let (code, result, stderr) = edit(&source, &output, operations.clone(), &["--dry-run"]);
    report.check(
        "dry-run validates the combined plan and writes nothing",
        code == 0
            && result["written"] == false
            && result["pages"].as_array().unwrap().len() == 3
            && !output.exists(),
        &stderr,
    );
    let (code, result, stderr) = edit(&source, &output, operations, &[]);
    let annots = if output.exists() {
        annotations(&output)
    } else {
        Vec::new()
    };
    report.check(
        "edit composes moves, deletion, blank insertion, rotation and journal undo/redo",
        code == 0 && result["written"] == true && names(&output) == ["Bravo", "", "Alpha"],
        &stderr,
    );
    report.check(
        "annotations survive on a newly inserted and a moved/rotated page",
        annots
            .iter()
            .any(|a| a.0 == 2 && a.1 == "Text" && a.2 == "Synthetic note")
            && annots.iter().any(|a| {
                a.0 == 3 && a.1 == "Square" && a.2 == "Turned box" && a.3 == [40., 40., 80., 120.]
            }),
        &format!("{annots:?}"),
    );
    let (code, text, stderr) = tool(&["comments", &output.display().to_string(), "--json"], &[]);
    let comments: Value = serde_json::from_str(&text).unwrap();
    report.check(
        "comments exposes one-based pages, bodies and editable object identities",
        code == 0
            && comments["complete"] == true
            && comments["comments"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["page"] == 2 && c["body"] == "Synthetic note" && c["object"].is_array()),
        &stderr,
    );
    let items = comments["comments"].as_array().unwrap();
    let note = items
        .iter()
        .find(|c| c["body"] == "Synthetic note")
        .unwrap();
    let square = items.iter().find(|c| c["body"] == "Turned box").unwrap();
    let revised = dir.join("revised.pdf");
    let (code, _, stderr) = edit(
        &output,
        &revised,
        json!([
            {"op":"rewrite_comment","page":2,"object":note["object"],"text":"Revised note"},
            {"op":"delete_comment","page":3,"object":square["object"]}
        ]),
        &[],
    );
    let annots = if revised.exists() {
        annotations(&revised)
    } else {
        Vec::new()
    };
    report.check(
        "existing comments can be rewritten and deleted by their inspected identity",
        code == 0 && annots.len() == 1 && annots[0].2 == "Revised note",
        &stderr,
    );

    for (label, ops) in [
        (
            "invalid later step",
            json!([{"op":"rotate","page":1,"degrees":90},{"op":"delete_page","page":9}]),
        ),
        ("zero page", json!([{"op":"delete_page","page":0}])),
        (
            "last page deletion",
            json!([{"op":"delete_page","page":1},{"op":"delete_page","page":1},{"op":"delete_page","page":1}]),
        ),
        ("bad move", json!([{"op":"move_page","page":1,"to":4}])),
        (
            "off-page annotation",
            json!([{"op":"annotate","page":1,"kind":"note","rect":[199,0,10,10]}]),
        ),
        (
            "bad color",
            json!([{"op":"annotate","page":1,"kind":"square","rect":[0,0,10,10],"color":[2,0,0]}]),
        ),
        (
            "oversized page",
            json!([{"op":"insert_blank","after":0,"width":1e100,"height":10}]),
        ),
        ("unavailable undo", json!([{"op":"undo"}])),
        ("unknown operation", json!([{"op":"rotat","page":1}])),
        (
            "unknown field",
            json!([{"op":"rotate","page":1,"degrees":90,"typo":1}]),
        ),
        (
            "wrong comment page",
            json!([{"op":"delete_comment","page":1,"object":note["object"]}]),
        ),
    ] {
        let target = dir.join("protected.pdf");
        std::fs::write(&target, b"PREVIOUS OUTPUT").unwrap();
        let (code, result, stderr) = edit(&source, &target, ops, &["--force"]);
        report.check(
            &format!("{label} refuses the whole request and preserves the previous output"),
            code == 3
                && result["error"]["kind"] == "refused"
                && std::fs::read(&target).unwrap() == b"PREVIOUS OUTPUT",
            &stderr,
        );
    }

    // Each corner is asymmetric and every added quarter-turn is exercised.
    // The comment reader uses the saved /Rotate, independently of the inverse
    // transform that edit uses to address the GUI journal.
    for degrees in [90, 180, 270] {
        let target = dir.join(format!("turn-{degrees}.pdf"));
        let (code, _, stderr) = edit(
            &source,
            &target,
            json!([
                {"op":"rotate","page":1,"degrees":degrees},
                {"op":"annotate","page":1,"kind":"highlight","rect":[20,30,80,40],"text":"Spot"},
                {"op":"annotate","page":1,"kind":"ink","strokes":[[40,50,70,90]],"text":"Stroke"}
            ]),
            &[],
        );
        let (read, text, _) = tool(&["comments", &target.display().to_string(), "--json"], &[]);
        let comments: Value = serde_json::from_str(&text).unwrap();
        let matched = comments["comments"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|c| c["body"] == "Spot" && c["rect"] == json!([20., 30., 100., 70.]))
        });
        report.check(
            &format!("annotation coordinates follow the current display at {degrees} degrees"),
            code == 0 && read == 0 && matched,
            &format!("{stderr}; {comments}"),
        );
    }

    let (code, runs, stderr) = tool(&["text-runs", &source.display().to_string(), "--json"], &[]);
    let runs: Value = serde_json::from_str(&runs).unwrap();
    report.check(
        "text-runs discovers original text with a revision",
        code == 0
            && runs["runs"][0]["text"] == "Alpha"
            && runs["revision"].as_array().is_some_and(|r| r.len() == 32),
        &stderr,
    );
    let target = dir.join("text-edited.pdf");
    let op = json!({"op":"replace_text","page":3,"operator":runs["runs"][0]["operator"],"revision":runs["revision"],"original":"Alpha","replacement":"Alp"});
    let (code, _, stderr) = edit(
        &source,
        &target,
        json!([{"op":"move_page","page":1,"to":3},op]),
        &[],
    );
    report.check(
        "bounded text replacement follows a moved page and preserves its neighbours",
        code == 0 && target.exists() && names(&target) == ["Bravo", "Charlie", "Alp"],
        &stderr,
    );
    // The report names the font of each replacement that asked for one, and
    // of no other. `"auto"` on a font that has the characters keeps it.
    let named = |font: Option<&str>, name: &str| {
        let mut op = json!({"op":"replace_text","page":1,"operator":runs["runs"][0]["operator"],"revision":runs["revision"],"original":"Alpha","replacement":"Alp"});
        if let Some(font) = font {
            op["font"] = json!(font);
        }
        let target = dir.join(name);
        let (code, made, stderr) = edit(
            &source,
            &target,
            json!([{"op":"rotate","page":2,"degrees":90}, op]),
            &[],
        );
        (code, made["fonts"].clone(), stderr, target)
    };
    let (code, fonts, stderr, target) = named(Some("noto_sans_bold"), "font-bold.pdf");
    report.check(
        "a replacement that names a font reports the font it was set in",
        code == 0
            && fonts == json!([{"operation":2,"font":"Noto Sans Bold"}])
            && names(&target)[0] == "Alp",
        &format!("{fonts}; {stderr}"),
    );
    let (code, kept, stderr, _) = named(Some("auto"), "font-auto.pdf");
    let own = kept[0]["font"].as_str().unwrap_or_default().to_owned();
    report.check(
        "automatic on a font with the characters reports the document's own font",
        code == 0 && kept[0]["operation"] == 2 && !own.is_empty() && !own.contains("Noto"),
        &format!("{kept}; {stderr}"),
    );
    let (code, fonts, stderr, _) = named(None, "font-none.pdf");
    report.check(
        "control: a replacement that names no font reports none",
        code == 0 && fonts == json!([]),
        &format!("{fonts}; {stderr}"),
    );
    let plain = dir.join("font-plain.pdf");
    let (code, said, stderr) = tool_with_stdin(
        &[
            "edit",
            &source.display().to_string(),
            "--plan",
            "-",
            "-o",
            &plain.display().to_string(),
        ],
        &[],
        &json!({"schema":1,"operations":[{"op":"replace_text","page":1,"operator":runs["runs"][0]["operator"],"revision":runs["revision"],"original":"Alpha","replacement":"Alp","font":"noto_sans_italic"}]}).to_string(),
    );
    report.check(
        "the plain report says it in a line of its own",
        code == 0
            && said
                .lines()
                .any(|line| line == "operation 1: set in Noto Sans Italic"),
        &format!("{said}; {stderr}"),
    );
    let target = dir.join("stale.pdf");
    let (code, _, stderr) = edit(
        &source,
        &target,
        json!([{"op":"replace_text","page":1,"operator":runs["runs"][0]["operator"],"revision":vec![0;32],"original":"Alpha","replacement":"Alp"}]),
        &[],
    );
    report.check(
        "stale text revision refuses without an output",
        code == 3 && !target.exists(),
        &stderr,
    );

    let (code, _, stderr) = edit(
        &source,
        &target,
        json!([{"op":"replace_text","page":1,"operator":runs["runs"][0]["operator"],"revision":vec![0;32],"original":"Alpha","replacement":"Alp"}]),
        &["--dry-run"],
    );
    report.check(
        "dry-run validates text revisions inside the worker",
        code == 3 && !target.exists(),
        &stderr,
    );

    let kinds = [
        ("highlight", "Highlight"),
        ("underline", "Underline"),
        ("strikeout", "StrikeOut"),
        ("squiggly", "Squiggly"),
        ("note", "Text"),
        ("square", "Square"),
        ("ellipse", "Circle"),
        ("textbox", "FreeText"),
        ("stamp", "Stamp"),
        ("ink", "Ink"),
    ];
    let ops: Vec<_> = kinds
        .iter()
        .map(|(kind, _)| {
            let mut op =
                json!({"op":"annotate","page":1,"kind":kind,"text":kind,"rect":[20,20,150,50]});
            if *kind == "stamp" {
                op["stamp"] = json!("approved");
            }
            if *kind == "ink" {
                op.as_object_mut().unwrap().remove("rect");
                op["strokes"] = json!([[20, 20, 40, 50, 70, 30]]);
            }
            op
        })
        .collect();
    let target = dir.join("all-marks.pdf");
    let (code, _, stderr) = edit(&source, &target, json!(ops), &[]);
    let marks = if target.exists() {
        annotations(&target)
    } else {
        Vec::new()
    };
    report.check(
        "every supported annotation kind reaches the saved PDF with its own subtype and body",
        code == 0
            && marks.len() == kinds.len()
            && kinds
                .iter()
                .all(|(body, kind)| marks.iter().any(|m| m.1 == *kind && m.2 == *body)),
        &format!("{stderr}; {marks:?}"),
    );

    let encrypted = dir.join("encrypted.pdf");
    let encrypted_out = dir.join("encrypted-edited.pdf");
    if std::process::Command::new("qpdf")
        .args([
            "--encrypt",
            "synthetic-user",
            "synthetic-owner",
            "256",
            "--",
        ])
        .arg(&source)
        .arg(&encrypted)
        .status()
        .is_ok_and(|s| s.success())
    {
        let args = [
            "edit",
            &encrypted.display().to_string(),
            "--plan",
            "-",
            "-o",
            &encrypted_out.display().to_string(),
            "--password-env",
            "TPDF_TEST_EDIT_PASSWORD",
            "--json",
        ];
        let (code, _, stderr) = tool_with_stdin(
            &args,
            &[("TPDF_TEST_EDIT_PASSWORD", "synthetic-user")],
            &json!({"schema":1,"operations":[{"op":"rotate","page":1,"degrees":90}]}).to_string(),
        );
        let decrypted = dir.join("decrypted.pdf");
        let still_encrypted = std::process::Command::new("qpdf")
            .arg("--is-encrypted")
            .arg(&encrypted_out)
            .status()
            .is_ok_and(|s| s.success());
        let decrypted_ok = std::process::Command::new("qpdf")
            .args(["--password=synthetic-user", "--decrypt"])
            .arg(&encrypted_out)
            .arg(&decrypted)
            .status()
            .is_ok_and(|s| s.success());
        let good =
            still_encrypted && decrypted_ok && names(&decrypted) == ["Alpha", "Bravo", "Charlie"];
        report.check(
            "edit preserves encryption and the supplied password",
            code == 0 && good,
            &stderr,
        );
    } else {
        report.skip("edit preserves encryption", "qpdf is unavailable");
    }

    let (code, _, stderr) = edit(
        &source,
        &source,
        json!([{"op":"rotate","page":1,"degrees":90}]),
        &["--force"],
    );
    report.check(
        "edit never overwrites its input even with force",
        code != 0 && std::fs::read(&source).unwrap() == original,
        &stderr,
    );
    let signed = super::fixture("incr-signed.pdf").expect("signed fixture");
    let signed_out = dir.join("signed-out.pdf");
    let (code, _, stderr) = edit(
        &signed,
        &signed_out,
        json!([{"op":"rotate","page":1,"degrees":90}]),
        &[],
    );
    report.check(
        "edit refuses signed input without explicit invalidation",
        code == 3 && !signed_out.exists(),
        &stderr,
    );
    report.check(
        "all edit paths preserve the source and clean their staging directories",
        std::fs::read(&source).unwrap() == original
            && !std::fs::read_dir(&dir).unwrap().any(|p| {
                p.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tpdf-cli-")
            }),
        "source changed or staging leaked",
    );
    let _ = std::fs::remove_dir_all(dir);
}
