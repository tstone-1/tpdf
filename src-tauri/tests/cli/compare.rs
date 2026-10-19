//! `compare` on documents built here, so the words each one holds are known:
//! the same words laid out on different lines and pages compare equal, and an
//! edit is found on the page and at the place it was made.
//!
//! Proved by hand, since `tests/cli.rs` is not selectable by `mutate_rust.py`
//! (2026-10-18): `cli/compare.rs` `Ok(if report.total_changes == 0 {` made
//! `Ok(if true {` turns 2 of the 6 `compare` cases red; and (2026-10-19)
//! `if side.without_text.len() == side.texts.len() {` made `if false {` turns
//! the two exit-3 cases red.
//! Run with `cargo test --test cli -- --filter compare`.
use super::{scratch, tool, Report};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;

/// A document of `pages`, each a list of lines set in Helvetica 12 from the
/// top-left of an A4 page.
fn document(path: &Path, pages: &[&[&str]]) {
    let mut doc = Document::with_version("1.7");
    let tree = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica", "Encoding"=>"WinAnsiEncoding",
    });
    let mut kids = Vec::new();
    for lines in pages {
        let mut content = String::from("BT /F1 12 Tf 14 TL 72 770 Td\n");
        for line in *lines {
            content.push_str(&format!("({line}) Tj T*\n"));
        }
        content.push_str("ET");
        let stream = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type"=>"Page", "Parent"=>tree,
            "MediaBox"=>vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Contents"=>stream,
            "Resources"=>dictionary! { "Font"=>dictionary! { "F1"=>font } },
        })));
    }
    let count = i64::try_from(kids.len()).unwrap();
    doc.objects.insert(
        tree,
        dictionary! {"Type"=>"Pages", "Kids"=>kids, "Count"=>count}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>tree});
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

pub(super) fn finds_what_changed_in_the_words(report: &mut Report) {
    let dir = scratch("compare");
    let old = dir.join("old.pdf");
    let reflowed = dir.join("reflowed.pdf");
    let edited = dir.join("edited.pdf");
    document(
        &old,
        &[
            &["Payment is due within 30 days", "of the invoice date."],
            &["Late payment carries interest", "at the statutory rate."],
        ],
    );
    // The same words, broken into lines and pages differently.
    document(
        &reflowed,
        &[
            &["Payment is due", "within 30 days of the invoice date. Late"],
            &["payment carries interest at the statutory rate."],
        ],
    );
    document(
        &edited,
        &[
            &["Payment is due within 45 days", "of the invoice date."],
            &["Late payment carries interest", "at the rate."],
        ],
    );
    let (old, reflowed, edited) = (
        old.display().to_string(),
        reflowed.display().to_string(),
        edited.display().to_string(),
    );

    let (code, out, err) = tool(&["compare", &old, &reflowed, "--json"], &[]);
    let same: serde_json::Value = serde_json::from_str(&out).unwrap_or_default();
    report.check(
        "compare: the same words on other lines and pages are no change, exit 0",
        code == 0
            && same["total_changes"] == 0
            && same["old"]["words"] == 18
            && same["new"]["words"] == 18,
        &format!("exit {code}: {out} {err}"),
    );
    report.check(
        "compare: stderr says only the words were compared",
        err.contains("the words were compared; pictures, drawings, formatting and layout were not"),
        &err,
    );

    let (code, out, err) = tool(&["compare", &old, &edited, "--json"], &[]);
    let found: serde_json::Value = serde_json::from_str(&out).unwrap_or_default();
    let changes = found["changes"].as_array().cloned().unwrap_or_default();
    let summary: Vec<(String, String, String, u64, u64)> = changes
        .iter()
        .map(|c| {
            (
                c["kind"].as_str().unwrap_or("?").to_string(),
                c["removed"].as_str().unwrap_or("?").to_string(),
                c["added"].as_str().unwrap_or("?").to_string(),
                c["old_page"].as_u64().unwrap_or(0),
                c["new_page"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    let wanted = vec![
        (
            "replaced".to_string(),
            "30".to_string(),
            "45".to_string(),
            1,
            1,
        ),
        (
            "removed".to_string(),
            "statutory".to_string(),
            String::new(),
            2,
            2,
        ),
    ];
    report.check(
        "compare: an edit is found as the change it is, on its page, exit 1",
        code == 1 && summary == wanted,
        &format!("exit {code}: {summary:?} {err}"),
    );
    // "30" is the sixth word of the first line: past the left margin, on the
    // first line, and about two digits wide at 12 pt.
    let rect = &changes
        .first()
        .map(|c| c["old_rects"][0]["rect"].clone())
        .unwrap_or_default();
    let at = |i: usize| rect[i].as_f64().unwrap_or(-1.0);
    report.check(
        "compare: the changed word's rectangle is where it is printed",
        at(0) > 150.0
            && at(0) < 260.0
            && at(1) > 50.0
            && at(1) < 80.0
            && at(2) > 8.0
            && at(2) < 20.0,
        &format!("{rect}"),
    );

    let (code, out, _) = tool(&["compare", &old, &edited], &[]);
    report.check(
        "compare: plain output is one wdiff line per change",
        code == 1
            && out
                == "1 -> 1: Payment is due within [-30-]{+45+} days of the invoice\n\
                    2 -> 2: carries interest at the [-statutory-] rate.\n",
        &out,
    );

    // A scan: pages with no text at all. Exit 0 would say "the same words".
    let scan = dir.join("scan.pdf");
    let partly = dir.join("partly.pdf");
    document(&scan, &[&[], &[]]);
    document(&partly, &[&["Payment is due within 30 days"], &[]]);
    let (scan, partly) = (scan.display().to_string(), partly.display().to_string());
    let (code, out, err) = tool(&["compare", &scan, &scan], &[]);
    report.check(
        "compare: a document with no text on any page is refused, exit 3",
        code == 3 && out.is_empty() && err.contains("none of its pages has text"),
        &format!("exit {code}: {out} {err}"),
    );
    let (code, _, err) = tool(&["compare", &old, &scan], &[]);
    report.check(
        "compare: either side with no text is refused, exit 3",
        code == 3 && err.contains("scan.pdf: none of its pages has text"),
        &format!("exit {code}: {err}"),
    );
    // The old document's second page against a page with no text: those words
    // are not compared rather than removed, and the run exits 1, since the
    // change may be on that page.
    let (code, out, err) = tool(&["compare", &old, &partly, "--json"], &[]);
    let facing: serde_json::Value = serde_json::from_str(&out).unwrap_or_default();
    report.check(
        "compare: words facing a page with no text are not compared, not removed, exit 1",
        // All 12 after "30 days": the end of page 1 has no match either, and
        // stands next to the scanned page, so it may be on it.
        code == 1
            && facing["total_changes"] == 0
            && facing["old"]["words_not_compared"] == 12
            && facing["new"]["words_not_compared"] == 0
            && err.contains("old.pdf: 12 words stand where")
            && err.contains("has a page with no text, and were not compared"),
        &format!("exit {code}: {out} {err}"),
    );

    let (code, _, err) = tool(&["compare", &partly, &partly], &[]);
    report.check(
        "compare: a page with no text is named on stderr, and the rest is compared",
        code == 0 && err.contains("page 2 has no text"),
        &format!("exit {code}: {err}"),
    );

    let (code, _, err) = tool(&["compare", &old], &[]);
    report.check(
        "compare: one document is a usage error",
        code == 2 && err.contains("takes two documents"),
        &format!("exit {code}: {err}"),
    );
}
