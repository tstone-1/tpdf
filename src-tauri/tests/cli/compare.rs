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
use std::fmt::Write;
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
            if line.is_ascii() {
                writeln!(content, "({line}) Tj T*").unwrap();
            } else {
                content.push_str("/Span <</ActualText <FEFF");
                for unit in line.encode_utf16() {
                    write!(content, "{unit:04X}").unwrap();
                }
                content.push_str(">>> BDC (X) Tj EMC T*\n");
            }
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

fn repeated_words_beside_a_blank_cover(dir: &Path, report: &mut Report) {
    let old = dir.join("repeated.pdf");
    let changed = dir.join("reordered.pdf");
    let covered = dir.join("reordered-with-cover.pdf");
    let unchanged = dir.join("same-with-cover.pdf");
    let paid = ["paid"; 20].join(" ");
    let due = ["due"; 20].join(" ");
    let paid = [paid.as_str(); 55];
    let due = [due.as_str(); 55];
    document(&old, &[&paid, &due]);
    document(&changed, &[&due, &paid]);
    document(&covered, &[&[], &due, &paid]);
    document(&unchanged, &[&[], &paid, &due]);
    for (left, right, exit, changed_passages, unread_side) in [
        (&old, &old, 0, 0, None),
        (&old, &unchanged, 0, 0, None),
        (&old, &changed, 1, 1, None),
        (&old, &covered, 1, 0, Some("old")),
        (&covered, &old, 1, 0, Some("new")),
    ] {
        let (code, out, err) = tool(
            &[
                "compare",
                left.to_str().unwrap(),
                right.to_str().unwrap(),
                "--json",
            ],
            &[],
        );
        let result: serde_json::Value = serde_json::from_str(&out).unwrap();
        report.check(
            "compare preserves word order beside a blank cover",
            code == exit
                && result["total_changes"] == changed_passages
                && ["old", "new"].iter().all(|&side| {
                    result[side]["words"] == 2200
                        && result[side]["words_not_compared"]
                            == if unread_side == Some(side) { 2200 } else { 0 }
                }),
            &format!("{left:?} vs {right:?}: exit {code}: {out} {err}"),
        );
    }
}

fn nearby_changes_and_wordless_pages(dir: &Path, report: &mut Report) {
    let old = dir.join("nearby-old.pdf");
    let new = dir.join("nearby-new.pdf");
    let compare = || {
        let (exit, out, err) = tool(
            &[
                "compare",
                old.to_str().unwrap(),
                new.to_str().unwrap(),
                "--json",
            ],
            &[],
        );
        (
            exit,
            serde_json::from_str::<serde_json::Value>(&out).unwrap_or_default(),
            err,
        )
    };
    document(&old, &[&["Fee 10 and tax 5 due"]]);
    document(&new, &[&["Fee 20 and tax 9 due"]]);
    let (exit, changed, err) = compare();
    report.check(
        "compare: context stops before a neighboring change",
        exit == 1
            && changed["total_changes"] == 2
            && changed["changes"][0]["after"] == "and tax"
            && changed["changes"][1]["before"] == "and tax",
        &format!("exit {exit}: {changed} {err}"),
    );

    document(&old, &[&["Header"], &["Terms changed"]]);
    document(&new, &[&["Header"], &["\u{ad}"]]);
    let (exit, incomplete, err) = compare();
    report.check(
        "compare: a page with only a soft hyphen has no comparable words",
        exit == 1
            && incomplete["total_changes"] == 0
            && incomplete["old"]["words_not_compared"] == 2
            && incomplete["new"]["pages_without_text"] == serde_json::json!([2])
            && err.contains("page 2 has no text"),
        &format!("exit {exit}: {incomplete} {err}"),
    );
    for reverse in [false, true] {
        let (readable, scanned) = if reverse { (&new, &old) } else { (&old, &new) };
        document(readable, &[&["Header"], &["passed"], &["Footer"]]);
        document(scanned, &[&["Header"], &[], &["failed"], &["Footer"]]);
        let (exit, result, err) = compare();
        let (side, other, field) = if reverse {
            ("new", "old", "removed")
        } else {
            ("old", "new", "added")
        };
        report.check(
            "compare: retain a definite change opposite a wholly readable document",
            exit == 1
                && result["total_changes"] == 1
                && result["changes"][0][field] == "failed"
                && result["changes"][0]["before"] == "Header"
                && result["changes"][0]["after"] == "Footer"
                && result[side]["words_not_compared"] == 1
                && result[other]["words_not_compared"] == 0
                && !err.contains(&format!(
                    "where {} has a page with no text",
                    readable.display()
                )),
            &format!("{result} {err}"),
        );
    }

    document(&new, &[&["\u{ad}"]]);
    let (exit, _, err) = compare();
    report.check(
        "compare: a whole document of separators has no words to compare",
        exit == 3 && err.contains("none of its pages has text"),
        &format!("exit {exit}: {err}"),
    );
}

/// A fixed physical crop and type size; only their native coordinates vary.
fn positioned(path: &Path, unit: i32, points: f64, shows: &str) {
    document(path, &[&["base"]]);
    let mut doc = Document::load(path).unwrap();
    let page = doc.get_pages()[&1];
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Synthetic def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfrange <00> <FF> <0000> endbfrange endcmap CMapName currentdict /CMap defineresource pop end end";
    let cmap = doc.add_object(Stream::new(dictionary! {}, cmap.to_vec()));
    let font = doc.add_object(
        dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica",
        "Encoding"=>"WinAnsiEncoding","ToUnicode"=>cmap},
    );
    let size = points / f64::from(unit);
    let content = format!(
        "BT /F1 {size} Tf {} {} Td {shows} ET",
        80 / unit,
        120 / unit
    );
    let stream = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let dict = doc.get_dictionary_mut(page).unwrap();
    dict.set("Contents", stream);
    dict.set("Resources", dictionary! {"Font"=>dictionary!{"F1"=>font}});
    dict.set("UserUnit", unit);
    for (key, coords) in [
        ("MediaBox", [0, 0, 480, 640]),
        ("CropBox", [16, 24, 416, 624]),
    ] {
        dict.set(
            key,
            Object::Array(coords.map(|n| Object::from(n / unit)).to_vec()),
        );
    }
    doc.save(path).unwrap();
}

fn physical_rectangles(dir: &Path, report: &mut Report) {
    let old = dir.join("older.pdf");
    let new = dir.join("newer.pdf");
    positioned(&old, 2, 12., "(,) Tj");
    positioned(&new, 8, 12., "(.) Tj");
    let (code, out, err) = tool(
        &[
            "compare",
            old.to_str().unwrap(),
            new.to_str().unwrap(),
            "--json",
        ],
        &[],
    );
    let result: serde_json::Value = serde_json::from_str(&out).unwrap();
    for (path, unit, word, key) in [(&old, 2, ",", "old_rects"), (&new, 8, ".", "new_rects")] {
        let rects = &result["changes"][0][key];
        let rect = &rects[0]["rect"];
        let (found, printed, why) = tool(
            &["search", path.to_str().unwrap(), "--text", word, "--json"],
            &[],
        );
        let search: serde_json::Value = serde_json::from_str(&printed).unwrap();
        report.check(
            "compare and search locate the punctuation in physical points",
            code == 1
                && found == 0
                && rects == &search["files"][0]["matches"][0]["rects"]
                && rect[0].as_f64().is_some_and(|x| (63.0..68.0).contains(&x))
                && rect[1]
                    .as_f64()
                    .is_some_and(|y| (499.0..505.0).contains(&y))
                && rect[2].as_f64().is_some_and(|w| w >= 0.5),
            &format!("unit {unit}: {rect} {err} {why}"),
        );
        let (planned, _, why) = tool(
            &[
                "search",
                path.to_str().unwrap(),
                "--text",
                word,
                "--annotate",
                "highlight",
            ],
            &[],
        );
        report.check(
            "search writes a highlight plan for punctuation above the physical minimum",
            planned == 0,
            &why,
        );
        if rect.is_array() {
            let marked = dir.join(format!("marked-{unit}.pdf"));
            let ops = serde_json::json!([{"op":"annotate","page":1,"kind":"square","rect":rect}]);
            let (written, _, why) = super::edit::edit(path, &marked, ops, &[]);
            let at = if written == 0 {
                super::edit::annotations(&marked)
            } else {
                Vec::new()
            };
            report.check(
                "edit places the reported rectangle over the glyph on the cropped sheet",
                at.first().is_some_and(|a| {
                    let r = &a.3;
                    r.len() == 4
                        && (79.0..84.0).contains(&(r[0] * unit as f32))
                        && (116.0..122.0).contains(&(r[1] * unit as f32))
                }),
                &format!("{at:?} {why}"),
            );
        }
    }
    positioned(&new, 8, 1., "(.) Tj");
    let (found, out, err) = tool(
        &["search", new.to_str().unwrap(), "--text", ".", "--json"],
        &[],
    );
    let search: serde_json::Value = serde_json::from_str(&out).unwrap();
    report.check(
        "a genuinely subthreshold dot still has no usable rectangle",
        found == 0 && search["files"][0]["matches"][0]["rects"] == serde_json::json!([]),
        &format!("{out} {err}"),
    );
}

/// Reuse one worker and its cached page across rendering and text extraction.
fn repeated_read(path: &Path, report: &mut Report) {
    use tpdf_lib::worker::{Reply, Request, Worker};
    let mut worker = Worker::spawn(path, &super::library_dir()).unwrap();
    assert!(
        worker
            .call(&Request::Open {
                lazy_geometry: true
            })
            .unwrap()
            .ok
    );
    let picture = |worker: &mut Worker| {
        let reply = worker
            .call(&Request::Tile {
                rid: 0,
                page: 0,
                scale: 1.,
                turns: 0,
                invert: false,
                x: 32,
                y: 464,
                width: 200,
                height: 96,
                png: false,
                crop: None,
                annotations: false,
                printing: false,
            })
            .unwrap();
        assert!(reply.ok, "{}", reply.error);
        worker.tile.as_slice()[..reply.bytes].to_vec()
    };
    let before = picture(&mut worker);
    let expected: Vec<u32> = "discre\u{ad}tionary".chars().map(u32::from).collect();
    let mut both = true;
    for _ in 0..2 {
        let response = worker
            .call(&Request::Text {
                page: 0,
                crop: None,
            })
            .unwrap();
        both &= matches!(response.reply, Some(Reply::Text(text)) if text.codes == expected);
    }
    report.check(
        "repeated reads recover the soft hyphen and leave both printed lines intact",
        both && before.chunks_exact(4).any(|p| p[0] < 128) && before == picture(&mut worker),
        "",
    );
}

fn hyphenated_words(dir: &Path, report: &mut Report) {
    let first = dir.join("source-word.pdf");
    let second = dir.join("reflowed-word.pdf");
    for (source, candidate, expected) in [
        ("(discretionary) Tj", r"(discre\255tionary) Tj", 0),
        (
            "(discretionary) Tj",
            r"(discre\255) Tj 0 -16 Td (tionary) Tj",
            0,
        ),
        ("(discretionary) Tj", "(discre) Tj 0 -16 Td (tionary) Tj", 1),
        (
            "(discretionary) Tj",
            "(discre-) Tj 0 -16 Td (tionary) Tj",
            1,
        ),
        (
            "(discre-tionary) Tj",
            "(discre-) Tj 0 -16 Td (tionary) Tj",
            0,
        ),
    ] {
        positioned(&first, 1, 12., source);
        positioned(&second, 1, 12., candidate);
        let (code, out, err) = tool(
            &[
                "compare",
                first.to_str().unwrap(),
                second.to_str().unwrap(),
                "--json",
            ],
            &[],
        );
        let result: serde_json::Value = serde_json::from_str(&out).unwrap();
        report.check(
            "a discretionary wrap compares equal while hard hyphens and word boundaries survive",
            code == expected && result["total_changes"] == expected,
            &format!("{candidate}: {out} {err}"),
        );
    }
    positioned(&second, 1, 12., r"(discre\255) Tj 0 -16 Td (tionary) Tj");
    repeated_read(&second, report);
}

/// Exercise the selected font through the same worker the CLI uses.
fn indirect_font_mapping(dir: &Path, fixture: &Path, report: &mut Report) {
    for null_map in [false, true] {
        let mut doc = Document::load(fixture).unwrap();
        let subtype = doc.add_object(Object::Name(b"Type0".to_vec()));
        let null = doc.add_object(Object::Null);
        let mut changed = false;
        for object in doc.objects.values_mut() {
            let Ok(font) = object.as_dict_mut() else {
                continue;
            };
            if font.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type0")
                && !font.has(b"ToUnicode")
            {
                font.set("Subtype", subtype);
                if null_map {
                    font.set("ToUnicode", null);
                }
                changed = true;
            }
        }
        assert!(
            changed,
            "fixture must draw composite text without a Unicode map"
        );
        let path = dir.join(format!("indirect-font-{null_map}.pdf"));
        doc.save(&path).unwrap();
        let path = path.to_string_lossy();
        let (exit, _, err) = tool(&["compare", &path, &path, "--json"], &[]);
        report.check(
            &format!(
                "compare refuses page one's indirect composite font (null Unicode map {null_map})"
            ),
            exit == 3 && err.contains("page 1 has text with a guessed character encoding"),
            &format!("exit {exit}: {err}"),
        );
    }
}

pub(super) fn finds_what_changed_in_the_words(report: &mut Report) {
    let dir = scratch("compare");
    nearby_changes_and_wordless_pages(&dir, report);
    repeated_words_beside_a_blank_cover(&dir, report);
    physical_rectangles(&dir, report);
    hyphenated_words(&dir, report);
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

    if let Some(unmapped) = super::fixture("encodings.pdf") {
        indirect_font_mapping(&dir, &unmapped, report);
        let (exit, out, err) = tool(
            &["compare", &old, &unmapped.to_string_lossy(), "--json"],
            &[],
        );
        report.check(
            "compare: guessed text is refused before reporting a word change",
            exit == 3
                && serde_json::from_str::<serde_json::Value>(&out).is_ok_and(|body| {
                    body["error"]["kind"] == "refused" && body.get("changes").is_none()
                })
                && err.contains("page 1 has text with a guessed character encoding"),
            &format!("exit {exit}: {out} {err}"),
        );
    }

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
