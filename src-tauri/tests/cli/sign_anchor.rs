//! `sign --anchor`: the rectangle is measured from text the page carries, and
//! text that is absent or there twice ends the run before the store is asked.
use super::sign_image::Counting;
use super::{certificate, now, scratch, signs, strings, Report, SUBJECT};
use lopdf::Document;
use std::path::Path;
use std::rc::Rc;

/// One page, 400 by 300, with *Signature:* once at (60, 200) and *Date here*
/// twice on one line, at x 60 and x 200, all in 12-point Helvetica. PDF space:
/// the baselines are 100 and 200 points down from the top.
fn worded_pdf() -> Vec<u8> {
    use lopdf::{dictionary, Object, Stream};
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 60 200 Td (Signature:) Tj ET\n\
          BT /F1 12 Tf 60 100 Td (Date here) Tj ET\n\
          BT /F1 12 Tf 200 100 Td (Date here) Tj ET"
            .to_vec(),
    ));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 400.into(), 300.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("saved");
    out
}

pub fn places_beside_text(report: &mut Report) {
    let now = now();
    let dir = scratch("sign-anchor");
    let s = |p: &Path| p.display().to_string();
    let worded = dir.join("worded.pdf");
    std::fs::write(&worded, worded_pdf()).expect("input");
    let store = || Counting {
        certificate: certificate(now),
        saved: None,
        asked: Rc::default(),
    };
    let line = |out: &Path, extra: &[&str]| {
        let mut args = strings(&[
            "sign",
            &s(&worded),
            "-o",
            &s(out),
            "--identity",
            SUBJECT,
            "--json",
            "--visible",
            "--size",
            "120,40",
        ]);
        args.extend(extra.iter().map(|a| (*a).to_string()));
        args
    };
    let rect_of = |stdout: &str| -> Vec<f64> {
        let json: serde_json::Value = serde_json::from_str(stdout).unwrap_or_default();
        json["appearance"]["rect"]
            .as_array()
            .map(|r| r.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
            .unwrap_or_default()
    };
    // The text's top is its baseline less the font's height above it: within
    // the 12 points above the baseline, whatever box the engine reports.
    let top_of = |baseline: f64, y: f64| y < baseline && y > baseline - 12.5;

    // One match, found whatever its case, and moved by the offset.
    let out = dir.join("unique.pdf");
    let (code, stdout, stderr) = signs(
        &line(&out, &["--anchor", "signature:", "--offset", "5,10"]),
        &store(),
        now,
    );
    let rect = rect_of(&stdout);
    report.check(
        "--anchor: the rectangle starts at the text's top-left corner, moved by --offset",
        code == 0
            && rect.len() == 4
            && (rect[0] - 65.0).abs() < 1.5
            && top_of(100.0, rect[1] - 10.0)
            && rect[2] == 120.0
            && rect[3] == 40.0,
        &format!("exit {code}, {rect:?}: {stderr}"),
    );

    // Control for the offset: without it, five left and ten up.
    let plain = dir.join("plain.pdf");
    let (code, stdout, stderr) = signs(&line(&plain, &["--anchor", "Signature:"]), &store(), now);
    let unmoved = rect_of(&stdout);
    report.check(
        "control: without --offset the rectangle starts at the text's corner",
        code == 0
            && unmoved.len() == 4
            && rect.len() == 4
            && (rect[0] - unmoved[0] - 5.0).abs() < 1e-3
            && (rect[1] - unmoved[1] - 10.0).abs() < 1e-3,
        &format!("exit {code}, {unmoved:?} against {rect:?}: {stderr}"),
    );

    // Text there twice: refused with the count, nothing written, nothing
    // asked of the store.
    let twice = dir.join("twice.pdf");
    let held = store();
    let (code, _, stderr) = signs(&line(&twice, &["--anchor", "Date here"]), &held, now);
    report.check(
        "--anchor: text found twice is refused (3), before the store, with nothing written",
        code == 3
            && stderr.contains("2 times")
            && stderr.contains("--anchor-match")
            && !twice.exists()
            && held.asked.identities.get() == 0
            && held.asked.signed.get() == 0,
        &format!(
            "exit {code}, listed {}: {stderr}",
            held.asked.identities.get()
        ),
    );

    // --anchor-match chooses: the first is at x 60, the second at x 200.
    for (nth, x) in [("1", 60.0), ("2", 200.0)] {
        let out = dir.join(format!("match-{nth}.pdf"));
        let (code, stdout, stderr) = signs(
            &line(&out, &["--anchor", "Date here", "--anchor-match", nth]),
            &store(),
            now,
        );
        let rect = rect_of(&stdout);
        report.check(
            &format!("--anchor-match {nth}: the rectangle starts at that match"),
            code == 0 && rect.len() == 4 && (rect[0] - x).abs() < 1.5 && top_of(200.0, rect[1]),
            &format!("exit {code}, {rect:?}: {stderr}"),
        );
    }

    // The two-inch anchor offset and the widget rectangle use physical points even
    // when the page dictionary uses two points per native unit.
    let scaled = dir.join("scaled.pdf");
    let mut doc = Document::load(&worded).expect("worded page");
    let page = doc.get_pages()[&1];
    doc.get_dictionary_mut(page).unwrap().set("UserUnit", 2);
    doc.save(&scaled).unwrap();
    for (name, placement) in [
        ("explicit", vec!["--rect", "600,80,120,40"]),
        (
            "anchored",
            vec![
                "--anchor",
                "Signature:",
                "--size",
                "120,40",
                "--offset",
                "144,10",
            ],
        ),
    ] {
        let out = dir.join(format!("{name}-scaled.pdf"));
        let mut args = strings(&[
            "sign",
            &s(&scaled),
            "-o",
            &s(&out),
            "--identity",
            SUBJECT,
            "--visible",
            "--json",
        ]);
        args.extend(strings(&placement));
        let (code, stdout, stderr) = signs(&args, &store(), now);
        let physical = rect_of(&stdout);
        let saved = Document::load(&out).ok().and_then(|doc| {
            let annots = doc
                .get_dictionary(doc.get_pages()[&1])
                .ok()?
                .get(b"Annots")
                .ok()?;
            let annot = doc.dereference(annots).ok()?.1.as_array().ok()?.last()?;
            let widget = doc.dereference(annot).ok()?.1.as_dict().ok()?;
            let numbers = |value: &lopdf::Object| {
                value
                    .as_array()
                    .ok()?
                    .iter()
                    .map(|n| n.as_float().ok().map(f64::from))
                    .collect::<Option<Vec<_>>>()
            };
            let rect = numbers(widget.get(b"Rect").ok()?)?;
            let ap = widget.get(b"AP").ok()?.as_dict().ok()?.get(b"N").ok()?;
            let stream = doc.dereference(ap).ok()?.1.as_stream().ok()?;
            let bbox = numbers(stream.dict.get(b"BBox").ok()?)?;
            Some((rect, bbox))
        });
        report.check(
            &format!("scaled {name} signature at the requested physical location"),
            code == 0
                && saved.as_ref().is_some_and(|(r, bbox)| {
                    *bbox == [0., 0., 120., 40.]
                        && r.len() == 4
                        && physical.len() == 4
                        && (physical[0] - r[0] * 2.).abs() < 0.01
                        && (physical[1] - (300. - r[3]) * 2.).abs() < 0.01
                        && (r[2] - r[0] - 60.).abs() < 0.01
                        && (r[3] - r[1] - 20.).abs() < 0.01
                        && if name == "explicit" {
                            physical[..2] == [600., 80.]
                        } else {
                            (physical[0] - (unmoved[0] * 2. + 144.)).abs() < 0.01
                                && (physical[1] - (unmoved[1] * 2. + 10.)).abs() < 0.01
                        }
                }),
            &format!("{saved:?}, {physical:?}, {stderr}"),
        );
    }

    // What cannot be placed: each refused (3) with nothing written.
    for (at, (what, extra, says)) in [
        (
            "a match the page does not have",
            &["--anchor", "Date here", "--anchor-match", "3"][..],
            "is not one of them",
        ),
        (
            "text the page does not have",
            &["--anchor", "Countersigned"][..],
            "has no text",
        ),
        (
            "an offset off the page",
            &["--anchor", "Signature:", "--offset", "-500,0"][..],
            "off the page",
        ),
        (
            "a page the document does not have",
            &["--anchor", "Signature:", "--page", "2"][..],
            "has 1 page",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let out = dir.join(format!("refused-{at}.pdf"));
        let held = store();
        let (code, _, stderr) = signs(&line(&out, extra), &held, now);
        report.check(
            &format!("--anchor, {what}: refused (3), before the store, with nothing written"),
            code == 3 && stderr.contains(says) && !out.exists() && held.asked.identities.get() == 0,
            &format!("exit {code}: {stderr}"),
        );
    }
}
