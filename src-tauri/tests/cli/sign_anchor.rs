//! `sign --anchor`: the rectangle is measured from text the page carries, and
//! text that is absent or there twice ends the run before the store is asked.
use super::sign_image::Counting;
use super::{certificate, now, scratch, signs, strings, Report, SUBJECT};
use std::path::Path;
use std::rc::Rc;

/// One page, 400 by 300, with *Signature:* once at (60, 200) and *Date here*
/// twice on one line, at x 60 and x 200, all in 12-point Helvetica. PDF space:
/// the baselines are 100 and 200 points down from the top.
fn worded_pdf() -> Vec<u8> {
    use lopdf::{dictionary, Document, Object, Stream};
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
