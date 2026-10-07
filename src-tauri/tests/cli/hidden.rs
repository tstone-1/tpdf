//! `tpdf hidden` through the shipped tool, on a document redacted the wrong
//! way several times over.
//!
//! The document is written here, so the test needs no fixture on disk and runs
//! on every machine. Each line of it is one case: hidden words the tool must
//! list, and visible words that an earlier design of the check took for
//! hidden. Helvetica is not embedded, so the renderer substitutes its own copy
//! and the covers are sized from an upper estimate of the text's width.

use std::path::Path;

use lopdf::{dictionary, Dictionary, Document, Object, Stream};

use super::{scratch, tool, Report};

const SIZE: f32 = 12.0;
const LEFT: f32 = 60.0;

/// An upper estimate of the width of `words` in Helvetica at [`SIZE`].
fn width_of(words: &str) -> f32 {
    0.62 * SIZE * words.len() as f32
}

fn show(x: f32, y: f32, words: &str) -> String {
    format!("BT /Helv {SIZE} Tf {x} {y} Td ({words}) Tj ET\n")
}

/// A rectangle around a line at baseline `y`, in the current fill colour.
fn cover(x: f32, y: f32, words: &str) -> String {
    let pad = 2.5;
    format!(
        "{} {} {} {} re f\n",
        x - pad,
        y - 3.0 - pad,
        width_of(words) + 2.0 * pad,
        SIZE + 2.0 + 2.0 * pad
    )
}

/// Rectangles drawn over the words: tight, loose, and not black.
fn drawn_over() -> String {
    let mut out = show(LEFT, 760.0, "This line is ordinary and visible");
    out += &show(LEFT, 730.0, "Name:");
    out += &show(LEFT + 40.0, 730.0, "Jane Example");
    out += "0 g\n";
    out += &cover(LEFT + 40.0, 730.0, "Jane Example");
    out += &show(LEFT, 700.0, "Account 12345678");
    out += "0 g 40 686 400 36 re f\n";
    out += &show(LEFT, 660.0, "Colour does not matter here");
    out += "0.55 0.05 0.05 rg\n";
    out += &cover(LEFT, 660.0, "Colour does not matter here");
    out += "0 g\n";
    out += &show(LEFT, 620.0, "The last line of page one is visible");
    out
}

/// Words nothing lies over, which the page still does not show.
fn never_painted() -> String {
    let mut out = show(LEFT, 760.0, "Page two begins with a visible line");
    out += "1 g\n";
    out += &show(LEFT, 730.0, "white on white words");
    out += "0 g\n";
    // In `q`/`Q`: the render mode is graphics state and outlives `ET`.
    out += &format!("q BT 3 Tr /Helv {SIZE} Tf {LEFT} 700 Td (render mode three words) Tj ET Q\n");
    out += &show(LEFT, 660.0, "Page two ends with a visible line");
    out
}

const UNDER_ANNOTATION: &str = "Covered by a square annotation";

fn under_an_annotation() -> String {
    show(LEFT, 760.0, "Page three begins with a visible line")
        + &show(LEFT, 730.0, UNDER_ANNOTATION)
        + &show(LEFT, 690.0, "Page three ends with a visible line")
}

/// Visible text a naive check takes for hidden, and one line off the page.
fn visible_lookalikes() -> String {
    let mut out = String::new();
    let mut y = 760.0;
    for words in [
        "________________________",
        "------------------------",
        "llllllllll IIIIIIIIII",
        "........................",
    ] {
        out += &show(LEFT, y, words);
        y -= 30.0;
    }
    out += "0.6 g\n";
    out += &show(LEFT, y, "Grey text is still text");
    y -= 30.0;
    out += "0.85 g\n";
    out += &cover(LEFT, y, "Text on a shaded cell");
    out += "0 g\n";
    out += &show(LEFT, y, "Text on a shaded cell");
    y -= 30.0;
    out += &cover(LEFT, y, "White text on a black bar");
    out += "1 g\n";
    out += &show(LEFT, y, "White text on a black bar");
    out += "0 g\n";
    y -= 30.0;
    out += &show(LEFT, y, "a b c d e f g");
    out += &show(-400.0, 300.0, "left in the margin");
    out
}

fn document(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let tree = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources = dictionary! { "Font" => dictionary! { "Helv" => font } };
    let mut kids = Vec::new();
    let mut page = |doc: &mut Document, content: String, extra: Dictionary| {
        let stream = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let mut dict = dictionary! {
            "Type" => "Page", "Parent" => tree,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => resources.clone(), "Contents" => stream,
        };
        dict.extend(&extra);
        let id = doc.add_object(dict);
        kids.push(Object::Reference(id));
        id
    };

    page(&mut doc, drawn_over(), dictionary! {});
    page(&mut doc, never_painted(), dictionary! {});

    let (x0, y0) = (LEFT - 3.0, 730.0 - 6.0);
    let (x1, y1) = (LEFT + width_of(UNDER_ANNOTATION) + 3.0, 730.0 + SIZE + 2.0);
    let rect = vec![x0.into(), y0.into(), x1.into(), y1.into()];
    let look = doc.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => rect.clone() },
        format!("0 g {x0} {y0} {} {} re f\n", x1 - x0, y1 - y0).into_bytes(),
    ));
    let square = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Square", "Rect" => rect, "F" => 4,
        "C" => vec![0.into(), 0.into(), 0.into()],
        "IC" => vec![0.into(), 0.into(), 0.into()],
        "AP" => dictionary! { "N" => look },
    });
    page(
        &mut doc,
        under_an_annotation(),
        dictionary! { "Annots" => vec![Object::Reference(square)] },
    );

    page(&mut doc, visible_lookalikes(), dictionary! {});
    // A page with no text: nothing to compare, and the report has to say so.
    page(
        &mut doc,
        "0 g 100 100 200 200 re f\n".into(),
        dictionary! {},
    );
    // Page one again, turned: the characters and the pixels must be read in
    // the same frame, whatever the page's own rotation.
    page(&mut doc, drawn_over(), dictionary! { "Rotate" => 90 });

    doc.objects.insert(
        tree,
        dictionary! { "Type" => "Pages", "Count" => 6, "Kids" => kids }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
    doc.trailer.set("Root", catalog);
    doc.save(path).expect("the document is written");
}

/// `(page, text, off_page)` for everything `found` lists.
fn listed(json: &str) -> Vec<(u64, String, bool)> {
    let report: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    report["found"]
        .as_array()
        .map(|found| {
            found
                .iter()
                .map(|f| {
                    (
                        f["page"].as_u64().unwrap_or(0),
                        f["text"].as_str().unwrap_or("?").to_string(),
                        f["off_page"].as_bool().unwrap_or(false),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn finds_what_the_page_does_not_show(report: &mut Report) {
    let dir = scratch("hidden");
    let source = dir.join("redacted-badly.pdf");
    document(&source);
    let input = source.display().to_string();

    let (code, json, stderr) = tool(&["hidden", &input, "--json"], &[]);
    report.check("hidden exits 1 when it finds text", code == 1, &stderr);
    let drawn = [
        "Jane Example",
        "Account 12345678",
        "Colour does not matter here",
    ];
    let mut expected: Vec<(u64, String, bool)> =
        drawn.iter().map(|t| (1, (*t).to_string(), false)).collect();
    expected.push((2, "white on white words".into(), false));
    expected.push((2, "render mode three words".into(), false));
    expected.push((3, UNDER_ANNOTATION.into(), false));
    expected.push((4, "left in the margin".into(), true));
    expected.extend(drawn.iter().map(|t| (6, (*t).to_string(), false)));
    // Within a page the order is the text's own, which on the turned page is
    // not top to bottom; what is held is which passages, on which page.
    let mut found = listed(&json);
    found.sort();
    expected.sort();
    report.check(
        "hidden lists every covered and unpainted passage, and nothing visible",
        found == expected,
        &format!("{found:?}"),
    );
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    report.check(
        "hidden names the page it could not compare, and counts what it compared",
        parsed["without_text"] == serde_json::json!([5])
            && parsed["not_compared"] == serde_json::json!([])
            && parsed["pages"] == 6
            && parsed["compared"].as_u64().unwrap_or(0) > 300,
        &json,
    );

    // The control: the page of look-alikes without its margin line has
    // nothing to list, so a tool that listed everything cannot pass above.
    let (code, json, stderr) = tool(&["hidden", &input, "--pages", "5", "--json"], &[]);
    report.check(
        "a page without text is exit 0 and is reported as not checked",
        code == 0 && listed(&json).is_empty() && json.contains("\"without_text\": [\n    5\n  ]"),
        &format!("{code} {stderr} {json}"),
    );
    let (code, plain, stderr) = tool(&["hidden", &input, "--pages", "4,5"], &[]);
    report.check(
        "plain output lists the passage and says what was not checked",
        code == 1
            && plain.contains("page 4, outside the page: left in the margin")
            && plain.contains("1 passage is in the file and not visible on the page, on 1 page")
            && plain.contains("1 page without text not checked (5)"),
        &format!("{code} {stderr} {plain}"),
    );
    let (code, _, stderr) = tool(&["hidden", &input, "--pages", "7"], &[]);
    report.check(
        "a page past the end is refused",
        code == 3 && stderr.contains("past the end"),
        &format!("{code} {stderr}"),
    );
}
