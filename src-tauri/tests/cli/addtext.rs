//! `page-numbers` and `watermark` on a document built here: one upright page
//! and one shown turned a quarter, so where the text lands can be asked of
//! PDFium through `search`, in the page as displayed.
use super::{scratch, tool, Report};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;

/// Two A4 pages with a line of Helvetica each, the second `/Rotate 90`.
fn document(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let tree = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica", "Encoding"=>"WinAnsiEncoding",
    });
    let mut kids = Vec::new();
    for (line, turn) in [("The first page.", 0), ("The second page.", 90)] {
        // A half-size `cm` the page never takes back, and a second `q` left
        // open: the page's own line lands at 72, 770 at 12 pt, and text drawn
        // after it without closing both would come out half-size and moved.
        let content = format!("q 0.5 0 0 0.5 0 0 cm BT /F1 24 Tf 144 1540 Td ({line}) Tj ET q");
        let stream = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type"=>"Page", "Parent"=>tree, "Rotate"=>turn,
            "MediaBox"=>vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Contents"=>stream,
            "Resources"=>dictionary! { "Font"=>dictionary! { "F1"=>font } },
        })));
    }
    doc.objects.insert(
        tree,
        dictionary! {"Type"=>"Pages", "Kids"=>kids, "Count"=>2}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>tree});
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

/// Where `search` finds `text` in `path`: `(page, [x, y, width, height])` of
/// its first rectangle, in the page as displayed.
fn found(path: &str, text: &str) -> Option<(u64, [f64; 4])> {
    let (_, out, _) = tool(&["search", path, "--text", text, "--json"], &[]);
    let report: serde_json::Value = serde_json::from_str(&out).ok()?;
    let rect = &report["files"][0]["matches"][0]["rects"][0];
    let numbers: Vec<f64> = rect["rect"]
        .as_array()?
        .iter()
        .filter_map(serde_json::Value::as_f64)
        .collect();
    Some((rect["page"].as_u64()?, numbers.try_into().ok()?))
}

pub(super) fn adds_text_to_every_page(report: &mut Report) {
    let dir = scratch("addtext");
    let source = dir.join("source.pdf");
    document(&source);
    let before = std::fs::read(&source).unwrap();
    let input = source.display().to_string();
    let numbered = dir.join("numbered.pdf").display().to_string();
    let marked = dir.join("marked.pdf").display().to_string();

    let (code, out, err) = tool(&["page-numbers", &input, "-o", &numbered, "--json"], &[]);
    let said: serde_json::Value = serde_json::from_str(&out).unwrap_or_default();
    report.check(
        "page-numbers writes a copy and reads every page back",
        code == 0 && said["written"] == serde_json::json!([1, 2]) && said["first"] == "Page 1 of 2",
        &format!("exit {code}: {out} {err}"),
    );
    report.check(
        "page-numbers leaves the input as it was",
        std::fs::read(&source).unwrap() == before,
        "the input changed",
    );
    // At the bottom centre of each page as displayed: A4 upright is 595 by
    // 842, and turned it is 842 by 595.
    let at = |page: u64, width: f64, height: f64, hit: Option<(u64, [f64; 4])>| {
        hit.is_some_and(|(p, [x, y, w, h])| {
            p == page
                && (x + w / 2.0 - width / 2.0).abs() < 2.0
                && y + h > height - 50.0
                && y + h < height - 30.0
        })
    };
    let first = found(&numbered, "Page 1 of 2");
    report.check(
        "the number is at the bottom centre of an upright page",
        at(1, 595.0, 842.0, first),
        &format!("{first:?}"),
    );
    let second = found(&numbered, "Page 2 of 2");
    report.check(
        "and of a page shown turned, upright as it is displayed",
        at(2, 842.0, 595.0, second),
        &format!("{second:?}"),
    );
    // The page's own text is still found where it was: the `q` put in front
    // of the content did not change what the content draws.
    let own = found(&numbered, "The first page.");
    report.check(
        "the page's own text stays where it was",
        own.is_some_and(|(_, [x, y, _, _])| (x - 72.0).abs() < 2.0 && y > 55.0 && y < 75.0),
        &format!("{own:?}"),
    );

    let (code, out, err) = tool(
        &[
            "watermark",
            &input,
            "-o",
            &marked,
            "--text",
            "DRAFT COPY",
            "--pages",
            "2",
            "--json",
        ],
        &[],
    );
    let said: serde_json::Value = serde_json::from_str(&out).unwrap_or_default();
    report.check(
        "watermark writes the pages it was given and no other",
        code == 0
            && said["written"] == serde_json::json!([2])
            && found(&marked, "DRAFT").is_some_and(|(p, _)| p == 2),
        &format!("exit {code}: {out} {err}"),
    );

    // Beyond Latin-1, the embedded Noto fonts, read back by PDFium through
    // their `/ToUnicode` maps.
    let greek = dir.join("greek.pdf").display().to_string();
    let (code, out, err) = tool(
        &[
            "page-numbers",
            &input,
            "-o",
            &greek,
            "--text",
            "Σελίδα {page} από {pages}",
        ],
        &[],
    );
    report.check(
        "Greek page numbers are written and extract as typed",
        code == 0 && found(&greek, "Σελίδα 2 από 2").is_some_and(|(p, _)| p == 2),
        &format!("exit {code}: {out} {err}"),
    );
    let chinese = dir.join("chinese.pdf").display().to_string();
    let (code, out, err) = tool(
        &["watermark", &input, "-o", &chinese, "--text", "草稿"],
        &[],
    );
    report.check(
        "a Chinese watermark is written and extracts as typed",
        code == 0 && found(&chinese, "草稿").is_some_and(|(p, _)| p == 1),
        &format!("exit {code}: {out} {err}"),
    );

    let refused = dir.join("refused.pdf");
    let (code, _, err) = tool(
        &[
            "page-numbers",
            &input,
            "-o",
            &refused.display().to_string(),
            "--text",
            "עמוד {page}",
        ],
        &[],
    );
    report.check(
        "text no bundled font can draw is refused, with nothing written",
        code == 2 && err.contains("has no font for") && !refused.exists(),
        &format!("exit {code}: {err}"),
    );
}
