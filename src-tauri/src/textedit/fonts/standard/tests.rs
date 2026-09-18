use super::FONTS;
use crate::textedit::{self, Change};
use lopdf::{Document, Object};

fn with_font(base: &str, encoding: Option<&str>, content: &[u8]) -> Document {
    let mut doc = textedit::tests::with_content(content);
    let font = *doc
        .objects
        .iter()
        .find(|(_, o)| o.as_dict().is_ok_and(|d| d.get(b"BaseFont").is_ok()))
        .unwrap()
        .0;
    let dict = doc.get_dictionary_mut(font).unwrap();
    dict.set("BaseFont", Object::Name(base.as_bytes().to_vec()));
    match encoding {
        Some(name) => dict.set("Encoding", Object::Name(name.as_bytes().to_vec())),
        None => drop(dict.remove(b"Encoding")),
    }
    doc
}

// The generated Helvetica row agrees with the hand-written table that
// `annot-probe --mode text` checks against PDFium's own rendering.
#[test]
fn the_generated_helvetica_widths_match_the_rendered_ones() {
    let (_, table) = FONTS
        .iter()
        .find(|(name, _)| *name == b"Helvetica")
        .unwrap();
    for (code, width) in (32_u8..=126).chain(160..=255).zip(table) {
        let expected = crate::textbox::advance(&char::from(code).to_string(), 1000.);
        assert_eq!(f64::from(*width), expected, "code {code}");
    }
}

// A few widths every transcription of the Core 14 metrics agrees on, so a
// table shifted by one code, or rows swapped between fonts, is refused.
#[test]
fn the_generated_widths_are_the_adobe_metrics() {
    let width = |font: &[u8], code: usize| {
        let (_, table) = FONTS.iter().find(|(name, _)| *name == font).unwrap();
        table[if code < 127 {
            code - 32
        } else {
            code - 160 + 95
        }]
    };
    assert_eq!(FONTS.len(), 12);
    for (font, code, expected) in [
        (b"Times-Roman".as_slice(), 32, 250),
        (b"Times-Roman", b'a' as usize, 444),
        (b"Times-Roman", b'W' as usize, 944),
        (b"Times-Bold", b'a' as usize, 500),
        (b"Times-Italic", b'a' as usize, 500),
        (b"Times-BoldItalic", b'W' as usize, 889),
        (b"Helvetica-Bold", b'a' as usize, 556),
        (b"Helvetica-Bold", b'i' as usize, 278),
        (b"Times-Roman", 0xE9, 444),
        (b"Times-Roman", 0xDF, 500),
    ] {
        assert_eq!(
            width(font, code),
            expected,
            "{} {code}",
            String::from_utf8_lossy(font)
        );
    }
    for (name, table) in FONTS
        .iter()
        .filter(|(name, _)| name.starts_with(b"Courier"))
    {
        assert!(
            table.iter().all(|&w| w == 600),
            "{}",
            String::from_utf8_lossy(name)
        );
    }
}

// The arXiv side stamp: unembedded Times-Roman with its built-in encoding,
// turned a quarter. Its run measures by Times widths and can be edited.
#[test]
fn textedit_standard_times_roman_measures_and_edits() {
    let content =
        b"BT /F1 20 Tf 0 1 -1 0 32 40 Tm (arXiv:2003) Tj ET BT /F1 12 Tf 40 140 Td (SECOND) Tj ET";
    for encoding in [None, Some("WinAnsiEncoding")] {
        let mut doc = with_font("Times-Roman", encoding, content);
        let scan = textedit::scan(&doc, 0).unwrap();
        assert_eq!(scan.runs[0].text, "arXiv:2003");
        // a r X i v : 2 0 0 3 in Times-Roman: 444+333+722+278+500+278+500*4.
        assert!((scan.runs[0].advance - 20. * 4.555).abs() < 1e-9);
        textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: scan.revision,
                operator: scan.runs[0].operator,
                original: "arXiv:2003".into(),
                replacement: "arXiv:2001".into(),
            }],
        )
        .unwrap();
        assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "arXiv:2001");
    }
}

// arXiv's newer stamp turns the CTM inside the text block (`BT 0 1 -1 0 0 0 cm
// ... Tm`), so its run is read-only. Unembedded Times has no outlines here; its
// FontBBox is reserved instead: one em (its larger side, 1000) past both ends of
// the advance, and from -218 to 898 across it, inside the hit box's -250..1000.
#[test]
fn textedit_rotated_standard_stamp_stays_read_only_with_its_font_box_reserved() {
    let content = b"q BT 0 1 -1 0 0 0 cm 1 0 0 1 60 -32 Tm /F1 20 Tf (arXiv:2509) Tj ET Q BT /F1 12 Tf 40 140 Td (SECOND) Tj ET";
    let doc = with_font("Times-Roman", None, content);
    let inspection = textedit::inspect(&doc, 0).unwrap();
    assert_eq!(
        inspection
            .runs
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>(),
        ["SECOND"]
    );
    let [stamp] = inspection.preserved.as_slice() else {
        panic!("one preserved run");
    };
    // a r X i v : 2 5 0 9 in Times-Roman: 444+333+722+278+500+278+500*4.
    let advance = 20. * 4.555;
    let [x0, y0, x1, y1] = stamp.display_rect.map(f64::from);
    assert!(
        ((y1 - y0).abs() - (advance + 40.)).abs() < 1e-3,
        "{:?}",
        stamp.display_rect
    );
    assert!(
        ((x1 - x0).abs() - 25.).abs() < 1e-3,
        "{:?}",
        stamp.display_rect
    );
}

#[test]
fn textedit_standard_fonts_refuse_symbolic_and_unknown_names() {
    for base in ["Symbol", "ZapfDingbats", "Times", "times-roman", "Arial"] {
        let doc = with_font(base, None, b"BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
        assert!(
            textedit::scan(&doc, 0)
                .unwrap_err()
                .contains("requires a standard font"),
            "{base}"
        );
    }
    let mut doc = with_font("Helvetica", None, b"BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    for object in doc.objects.values_mut() {
        if let Ok(dict) = object.as_dict_mut() {
            dict.remove(b"BaseFont");
        }
    }
    assert!(textedit::scan(&doc, 0)
        .unwrap_err()
        .contains("requires a standard font"));
    let doc = with_font(
        "Courier",
        Some("MacRomanEncoding"),
        b"BT /F1 12 Tf 40 180 Td (FIRST) Tj ET",
    );
    assert!(textedit::scan(&doc, 0)
        .unwrap_err()
        .contains("unsupported standard font encoding"));
    let doc = with_font("Courier", None, b"BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    assert!((textedit::scan(&doc, 0).unwrap().runs[0].advance - 5. * 7.2).abs() < 1e-9);
}

// The stamp exactly as arXiv writes it. Swapping one digit for another of the
// same width summed to 337.74 against the scan's 337.73999999999995, and the
// ink check had no rounding allowance, so the edit was refused as too wide.
#[test]
fn textedit_an_equal_width_edit_fits_despite_rounding() {
    let plain = "(arXiv:2003.00976v2  [cs.SE]  5 Mar 2020)Tj";
    // A tightening kern: only the kept-kerning path fits, since a rewrite
    // drops the kern and overruns the advance by 1 unit at 20 pt.
    let kerned = "[(arXiv:2003.00976v2  [cs.SE]  5 Mar) 50 ( 2020)]TJ";
    for (show, base, from, to, fits) in [
        (plain, "Times-Roman", "5 Mar", "6 Mar", true),
        (plain, "Helvetica", "5 Mar", "6 Mar", true),
        (kerned, "Times-Roman", "5 Mar", "6 Mar", true),
        // A wider character is still refused: 5 is 500 in Times, W is 944.
        (plain, "Times-Roman", "5 Mar", "W Mar", false),
    ] {
        let content = format!("q\n0.5 G 0.5 g\nBT\n/F1 20 Tf 0 1 -1 0 32 237 Tm\n{show}\nET\nQ\n");
        let mut doc = with_font(base, None, content.as_bytes());
        let scan = textedit::scan(&doc, 0).unwrap();
        let result = textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: scan.revision,
                operator: scan.runs[0].operator,
                original: scan.runs[0].text.clone(),
                replacement: scan.runs[0].text.replace(from, to),
            }],
        );
        assert_eq!(result.is_ok(), fits, "{base} {to}: {result:?}");
    }
}
