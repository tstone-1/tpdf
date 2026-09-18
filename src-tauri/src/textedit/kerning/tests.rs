use crate::textedit::{self, tests::with_content, Change};
use lopdf::{content::Content, Object};

// Replace the first run and return the written show's operand.
fn write(content: &str, original: &str, replacement: &str) -> Result<Object, String> {
    let mut doc = with_content(content.as_bytes());
    let scan = textedit::scan(&doc, 0)?;
    assert_eq!(scan.runs[0].text, original);
    textedit::write(
        &mut doc,
        &[Change {
            layout: None,
            page: 0,
            revision: scan.revision,
            operator: scan.runs[0].operator,
            original: original.into(),
            replacement: replacement.into(),
        }],
    )?;
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, replacement);
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let content = Content::decode_strict(&doc.get_page_content(page)).unwrap();
    Ok(content
        .operations
        .iter()
        .find(|op| op.operator == "TJ" || op.operator == "Tj")
        .unwrap()
        .operands[0]
        .clone())
}

fn array(items: Vec<Object>) -> Object {
    Object::Array(items)
}

#[test]
fn textedit_kerning_keeps_unchanged_ends_and_drops_the_changed_pairs_kern() {
    // The kern between B and C belonged to a pair that is gone; the one
    // between D and E still joins kept glyphs.
    assert_eq!(
        write(
            "BT /F1 12 Tf 40 180 Td [(AB) -50 (CD) 30 (EF)] TJ ET",
            "ABCDEF",
            "ABXDEF"
        )
        .unwrap(),
        array(vec![
            Object::string_literal("ABXD"),
            Object::Integer(30),
            Object::string_literal("EF"),
        ])
    );
    // The same at the other end: the kern before the kept "CD" joined B to C.
    assert_eq!(
        write(
            "BT /F1 12 Tf 40 180 Td [(AB) -50 (CD)] TJ ET",
            "ABCD",
            "AXCD"
        )
        .unwrap(),
        array(vec![Object::string_literal("AXCD")])
    );
    // Several numbers in a row are left to the ordinary rewrite.
    assert_eq!(
        write(
            "BT /F1 12 Tf 40 180 Td [(AB) -50 -50 (CD)] TJ ET",
            "ABCD",
            "ABCE"
        )
        .unwrap(),
        array(vec![Object::string_literal("ABCE")])
    );
}

// W W W tightened by two kerns: V is narrower than W, but not by the 400
// units the kerns saved, so only the version that keeps them fits.
#[test]
fn textedit_kerning_lets_a_kerned_line_take_an_edit_that_fits_only_with_its_kerns() {
    assert_eq!(
        write(
            "BT /F1 12 Tf 40 180 Td [(W) 200 (W) 200 (W)] TJ ET",
            "WWW",
            "WWV"
        )
        .unwrap(),
        array(vec![
            Object::string_literal("W"),
            Object::Integer(200),
            Object::string_literal("WV"),
        ])
    );
}

// A kept kern can widen: here keeping the 1000-unit gap after A leaves no room
// for W, while the rewrite without it does fit.
#[test]
fn textedit_kerning_gives_way_to_the_rewrite_when_kept_kerns_do_not_fit() {
    assert_eq!(
        write(
            "BT /F1 12 Tf 40 180 Td [(A) -1000 (BC)] TJ ET",
            "ABC",
            "ABW"
        )
        .unwrap(),
        array(vec![Object::string_literal("ABW")])
    );
    // When neither fits, the refusal is the ordinary one.
    assert_eq!(
        write("BT /F1 12 Tf 40 180 Td [(A) 100 (BC)] TJ ET", "ABC", "AWW").unwrap_err(),
        "replacement would exceed the original text advance"
    );
}
