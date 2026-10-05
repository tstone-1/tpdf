use super::list_tests::refused;
use super::tests::{fixture, CONTENT};
use crate::textedit::{self, Change, EditFont, Layout};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

pub(super) fn table() -> (Document, [ObjectId; 9]) {
    let (mut doc, ids) = fixture(CONTENT);
    let table = doc.add_object(dictionary! { "S" => "Table", "P" => ids[2], "Pg" => ids[0] });
    let mut rows = Vec::new();
    for cell in [ids[3], ids[4]] {
        let row = doc.add_object(dictionary! { "S" => "TR", "P" => table, "K" => cell });
        let cell = doc.get_dictionary_mut(cell).unwrap();
        cell.set("S", "TD");
        cell.set("P", row);
        cell.set(
            "A",
            vec![
                Object::Dictionary(
                    dictionary! { "O" => "Table", "Headers" => Vec::<Object>::new() },
                ),
                Object::Dictionary(dictionary! { "O" => "Table", "RowSpan" => 1 }),
                Object::Dictionary(dictionary! { "O" => "Table", "ColSpan" => 1 }),
            ],
        );
        rows.push(row);
    }
    doc.get_dictionary_mut(table)
        .unwrap()
        .set("K", vec![2.into(), rows[0].into(), rows[1].into()]);
    doc.get_dictionary_mut(ids[2]).unwrap().set("K", table);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![ids[3].into(), ids[4].into(), table.into()]),
        ],
    );
    let content = String::from_utf8(CONTENT.to_vec())
        .unwrap()
        .replace("/Standard", "/TD");
    let content = format!("/Table <</MCID 2>> BDC 40 120 100 1 re f EMC {content}");
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    (
        doc,
        [
            ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], table, rows[0], rows[1],
        ],
    )
}

#[test]
fn textedit_tables_preserve_cells_borders_and_structure() {
    for target in 0..2 {
        for replacement in ["IN", ""] {
            let (mut doc, ids) = table();
            let runs = textedit::scan(&doc, 0).unwrap();
            assert_eq!(runs.runs.len(), 2);
            let edit = Change {
                layout: None,
                page: 0,
                revision: runs.revision,
                operator: runs.runs[target].operator,
                original: runs.runs[target].text.clone(),
                replacement: replacement.into(),
            };
            let before = doc.objects.clone();
            textedit::write(&mut doc, &[edit]).unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(after.runs.len(), 2);
            assert_eq!(after.runs[target].text, replacement);
            let other = after
                .runs
                .iter()
                .find(|run| run.text == runs.runs[1 - target].text)
                .unwrap();
            assert_eq!(other.display_rect, runs.runs[1 - target].display_rect);
            for (id, object) in before {
                if id != ids[0] {
                    assert_eq!(doc.objects[&id], object);
                }
            }
            assert!(String::from_utf8(doc.get_page_content(ids[0]))
                .unwrap()
                .contains("40 120 100 1 re f"));
        }
    }
}

#[test]
fn textedit_tables_accept_bounded_cell_attribute_representations() {
    for representation in 0..5 {
        let (mut doc, ids) = table();
        let attrs =
            Object::Dictionary(dictionary! { "O" => "Table", "RowSpan" => 1, "ColSpan" => 1 });
        let attrs = match representation {
            0 => attrs,
            1 => Object::Array(vec![attrs]),
            2 => doc.add_object(attrs).into(),
            3 => Object::Array(vec![doc.add_object(attrs).into()]),
            _ => doc.add_object(Object::Array(vec![attrs])).into(),
        };
        doc.get_dictionary_mut(ids[3]).unwrap().set("A", attrs);
        assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
    }
    let (mut doc, ids) = table();
    doc.get_dictionary_mut(ids[3]).unwrap().remove(b"A");
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
}

#[test]
fn textedit_tables_preserve_merged_cells_and_paragraph_leaf_ownership() {
    for nested_span in [false, true] {
        for span in [1, 2, 128] {
            let (mut doc, ids) = table();
            let cell = ids[3];
            let paragraph =
                doc.add_object(dictionary! { "S" => "P", "P" => cell, "Pg" => ids[0], "K" => 0 });
            let owner = if nested_span {
                let leaf = doc.add_object(
                    dictionary! { "S" => "Span", "P" => paragraph, "Pg" => ids[0], "K" => 0 },
                );
                doc.get_dictionary_mut(paragraph).unwrap().set("K", leaf);
                leaf
            } else {
                paragraph
            };
            let cell = doc.get_dictionary_mut(cell).unwrap();
            cell.set("K", paragraph);
            cell.set(
                "A",
                dictionary! { "O" => "Table", "RowSpan" => span, "ColSpan" => span },
            );
            doc.get_dictionary_mut(ids[5]).unwrap().set(
                "Nums",
                vec![
                    0.into(),
                    Object::Array(vec![owner.into(), ids[4].into(), ids[6].into()]),
                ],
            );
            let bytes = String::from_utf8(doc.get_page_content(ids[0])).unwrap();
            let bytes = bytes.replacen(
                "/TD << /MCID 0",
                if nested_span {
                    "/Span << /MCID 0"
                } else {
                    "/P << /MCID 0"
                },
                1,
            );
            let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.into_bytes()));
            doc.get_dictionary_mut(ids[0])
                .unwrap()
                .set("Contents", stream);
            let before = doc.objects.clone();
            let runs = textedit::scan(&doc, 0).unwrap();
            textedit::write(
                &mut doc,
                &[Change {
                    page: 0,
                    revision: runs.revision,
                    operator: runs.runs[0].operator,
                    original: "FIRST".into(),
                    replacement: "IN".into(),
                    layout: None,
                }],
            )
            .unwrap();
            assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
            for (id, object) in before {
                if id != ids[0] {
                    assert_eq!(doc.objects[&id], object);
                }
            }
            let mut wrong_owner = doc.clone();
            wrong_owner
                .get_dictionary_mut(owner)
                .unwrap()
                .set("P", ids[2]);
            refused(wrong_owner);
            let mut nested = doc.clone();
            nested
                .get_dictionary_mut(paragraph)
                .unwrap()
                .set("S", "Table");
            refused(nested);
        }
    }
}

#[test]
fn textedit_tables_refuse_spans_metadata_and_attribute_ambiguity() {
    for key in ["RowSpan", "ColSpan"] {
        for value in [
            0.into(),
            129.into(),
            (-1).into(),
            Object::Real(1.0),
            "1".into(),
            Object::Null,
        ] {
            let (mut doc, ids) = table();
            let mut attrs = dictionary! { "O" => "Table" };
            attrs.set(key, value);
            doc.get_dictionary_mut(ids[3]).unwrap().set("A", attrs);
            refused(doc);
        }
    }
    let attrs = Object::Dictionary(dictionary! { "O" => "Table", "RowSpan" => 1 });
    for value in [
        Object::Array(vec![]),
        Object::Array(vec![attrs.clone(); 4]),
        Object::Array(vec![attrs.clone(); 2]),
        Object::Array(vec![attrs, 0.into()]),
        Object::Array(
            vec![dictionary! { "O" => "Table", "Headers" => Vec::<Object>::new() }.into(); 2],
        ),
        dictionary! { "O" => "Table", "Headers" => Object::Null }.into(),
        dictionary! { "O" => "Layout", "RowSpan" => 1 }.into(),
        dictionary! { "O" => "Table", "Scope" => "Column" }.into(),
        dictionary! { "O" => "Table", "Headers" => vec![Object::string_literal("SYNTHETIC")] }
            .into(),
        dictionary! { "O" => "Table", "Width" => 100 }.into(),
        Object::Null,
    ] {
        let (mut doc, ids) = table();
        doc.get_dictionary_mut(ids[3]).unwrap().set("A", value);
        refused(doc);
    }
    let (mut doc, ids) = table();
    let cyclic = doc.new_object_id();
    doc.objects.insert(cyclic, cyclic.into());
    doc.get_dictionary_mut(ids[3]).unwrap().set("A", cyclic);
    refused(doc);
}

#[test]
fn textedit_tables_refuse_invalid_hierarchy_and_container_text() {
    for (index, role) in [
        (6, "Div"),
        (7, "Div"),
        (3, "P"),
        (3, "THead"),
        (7, "Table"),
        (6, "TR"),
    ] {
        let (mut doc, ids) = table();
        doc.get_dictionary_mut(ids[index]).unwrap().set("S", role);
        refused(doc);
    }
    // A row's layout attributes are refused, all but the one that says it is
    // placed as a block; the test below owns that case. A table's are not
    // refused, and the two tests after it own that one.
    for attributes in [
        dictionary! { "O" => "Layout", "Placement" => "Inline" },
        dictionary! { "O" => "Layout", "Placement" => "Block", "TextAlign" => "Center" },
        dictionary! { "O" => "Layout", "Placement" => "Block", "SpaceBefore" => 4 },
        dictionary! { "O" => "Layout" },
    ] {
        let (mut doc, ids) = table();
        doc.get_dictionary_mut(ids[7]).unwrap().set("A", attributes);
        refused(doc);
    }
    let (mut doc, ids) = table();
    let text = doc.get_page_content(ids[0]);
    let text = String::from_utf8(text)
        .unwrap()
        .replace("40 120 100 1 re f", "BT /F1 12 Tf 40 100 Td (FIRST) Tj ET");
    let stream = doc.add_object(Stream::new(Dictionary::new(), text.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    refused(doc);
}

fn without_border(doc: &mut Document, ids: &[ObjectId; 9]) {
    doc.get_dictionary_mut(ids[6])
        .unwrap()
        .set("K", vec![Object::Reference(ids[7]), ids[8].into()]);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![0.into(), Object::Array(vec![ids[3].into(), ids[4].into()])],
    );
    let content = String::from_utf8(CONTENT.to_vec())
        .unwrap()
        .replace("/Standard", "/TD");
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
}

#[test]
fn textedit_tables_require_rows_and_cells_in_their_own_containers() {
    let (mut doc, ids) = table();
    without_border(&mut doc, &ids);
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
    let mut orphan_rows = doc.clone();
    orphan_rows
        .get_dictionary_mut(ids[6])
        .unwrap()
        .set("S", "Div");
    refused(orphan_rows);
    let mut orphan_cells = doc.clone();
    orphan_cells
        .get_dictionary_mut(ids[7])
        .unwrap()
        .set("S", "Div");
    orphan_cells
        .get_dictionary_mut(ids[8])
        .unwrap()
        .set("S", "Div");
    orphan_cells
        .get_dictionary_mut(ids[6])
        .unwrap()
        .set("S", "Div");
    refused(orphan_cells);
    // Removing cell attributes leaves P otherwise supported: only the row's
    // child-role check can reject a paragraph disguised as a cell.
    for cell in [ids[3], ids[4]] {
        doc.get_dictionary_mut(cell).unwrap().set("S", "P");
        doc.get_dictionary_mut(cell).unwrap().remove(b"A");
    }
    // Keep the stream tags consistent with the replacement paragraph roles.
    let bytes = String::from_utf8(CONTENT.to_vec())
        .unwrap()
        .replace("/Standard", "/P");
    let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    refused(doc);
}

// The border MCID becomes a paragraph of its own beside the table, so the page
// still holds editable text when the table's cells stop offering theirs.
fn beside(doc: &mut Document, ids: &[ObjectId; 9]) {
    let outside = doc.add_object(
        dictionary! { "S" => "Standard", "P" => ids[2], "Pg" => ids[0], "K" => vec![Object::Integer(2)] },
    );
    doc.get_dictionary_mut(ids[6])
        .unwrap()
        .set("K", vec![Object::Reference(ids[7]), ids[8].into()]);
    doc.get_dictionary_mut(ids[2])
        .unwrap()
        .set("K", vec![Object::Reference(ids[6]), outside.into()]);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![ids[3].into(), ids[4].into(), outside.into()]),
        ],
    );
    let content = String::from_utf8(doc.get_page_content(ids[0]))
        .unwrap()
        .replace(
            "/Table <</MCID 2>> BDC 40 120 100 1 re f EMC",
            "/Standard <</MCID 2>> BDC BT /F1 12 Tf 40 100 Td (THIRD) Tj ET EMC",
        );
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
}

fn bounded(entries: Dictionary) -> (Document, [ObjectId; 9]) {
    let (mut doc, ids) = table();
    doc.get_dictionary_mut(ids[6]).unwrap().set("A", entries);
    beside(&mut doc, &ids);
    (doc, ids)
}

fn rectangle() -> Object {
    vec![0.into(), 0.into(), 200.into(), 200.into()].into()
}

fn offered(doc: &Document) -> Vec<String> {
    textedit::scan(doc, 0)
        .unwrap()
        .runs
        .iter()
        .map(|run| run.text.clone())
        .collect()
}

// A change to the run with this text, in the box the editor opens on it.
fn change(doc: &Document, original: &str, replacement: &str, layout: bool) -> Change {
    let scan = textedit::scan(doc, 0).unwrap();
    let run = scan.runs.iter().find(|run| run.text == original).unwrap();
    Change {
        layout: layout.then(|| Layout::opened(run, EditFont::Original)),
        page: 0,
        revision: scan.revision,
        operator: run.operator,
        original: original.into(),
        replacement: replacement.into(),
    }
}

// The write's verdict; a refused one has changed nothing.
fn attempt(doc: &Document, change: Change) -> Result<Document, String> {
    let mut copy = doc.clone();
    match textedit::write(&mut copy, &[change]) {
        Ok(()) => Ok(copy),
        Err(error) => {
            assert_eq!(copy.objects, doc.objects);
            Err(error)
        }
    }
}

// A table whose stated bounds are these, in the fixture `bounded` builds: the
// first cell's text has its origin at (40, 180) and the second's at (40, 140),
// each glyph 7.2 pt wide with ink as wide as that and 8.4 pt high from the
// baseline, so the first cell's ink is [40, 180, 76, 188.4]; the paragraph
// beside the table is at (40, 100).
fn stating(bounds: [i64; 4]) -> Document {
    bounded(stated(bounds)).0
}

fn stated(bounds: [i64; 4]) -> Dictionary {
    dictionary! { "O" => "Layout", "BBox" => bounds.map(Object::from).to_vec() }
}

const LEAVES_ALONG: &str = "There is no room for more text on this line: the table states its bounds, and the text would leave them. Shorten the text or reduce the font size.";
const LEAVES: &str = "The table states its bounds, and this text would leave them. Reduce the box or font size to keep the text inside.";

// ISO 32000-1 Table 344. Acrobat and LibreOffice write a table's own ink
// bounds beside its placement, and the structure tree is written back as it
// was. Until 2026-10-05 a table that stated them made its cells read-only,
// which was every table LibreOffice exports; now the cells are offered and an
// edit is held inside the bounds, so they still enclose the table. This test
// asserted the read-only cells before; its reason, that the bounds stay true,
// is the one it checks now.
#[test]
fn textedit_bounded_tables_offer_their_cells_and_keep_their_ink_bounds() {
    // A table that declares only a placement changes nothing.
    let (plain, _) = bounded(dictionary! { "O" => "Layout", "Placement" => "Block" });
    assert_eq!(offered(&plain), ["THIRD", "FIRST", "SECOND"]);
    for entries in [
        dictionary! { "O" => "Layout", "BBox" => rectangle() },
        dictionary! {
            "O" => "Layout", "Placement" => "Block", "Width" => 100, "Height" => "Auto",
            "BBox" => rectangle(),
        },
    ] {
        let (doc, ids) = bounded(entries);
        assert_eq!(offered(&doc), ["THIRD", "FIRST", "SECOND"]);
        // Shorter with no box, longer in the editor's box, and the paragraph
        // beside the table: each is written, and every object but the page
        // and its new content is the object it was, the table's among them.
        for (original, replacement, layout) in [
            ("FIRST", "IN", false),
            ("FIRST", "FIRSTFIRST", true),
            ("THIRD", "THIRDTHIRD", true),
        ] {
            let written = attempt(&doc, change(&doc, original, replacement, layout)).unwrap();
            let after = offered(&written);
            assert!(after.contains(&replacement.to_owned()), "{after:?}");
            for (id, object) in &doc.objects {
                if *id != ids[0] {
                    assert_eq!(&written.objects[id], object);
                }
            }
            assert!(written.get_dictionary(ids[6]).unwrap().has(b"A"));
        }
        // The paragraph beside the table grows exactly as it does without
        // the bounds: they are the table's, and it is not in the table.
        let beside = |doc: &Document| {
            let written = attempt(doc, change(doc, "THIRD", "THIRDTHIRD", true)).unwrap();
            textedit::scan(&written, 0).unwrap().runs
        };
        assert_eq!(beside(&doc), beside(&plain));
    }
}

// An edit that would put ink outside the stated bounds is refused, on every
// side, and each has a control that stays inside and is written.
#[test]
fn textedit_bounded_tables_refuse_text_that_would_leave_their_bounds() {
    // Along the line, in the box the editor opens: twelve glyphs end at 126.4.
    let doc = stating([30, 130, 120, 200]);
    assert_eq!(
        attempt(&doc, change(&doc, "FIRST", "FIRSTFIRSTFI", true)).unwrap_err(),
        LEAVES_ALONG
    );
    assert!(attempt(&doc, change(&doc, "FIRST", "FIRSTFIRST", true)).is_ok());
    // The same text without the bounds has the room.
    let (plain, _) = bounded(dictionary! { "O" => "Layout", "Placement" => "Block" });
    assert!(attempt(&plain, change(&plain, "FIRST", "FIRSTFIRSTFI", true)).is_ok());
    // The ink itself is held, on each side, where no limit along the line
    // has shortened the text first.
    for (side, (tight, loose)) in SIDES.into_iter().enumerate() {
        let (doc, ids) = bounded(stated(tight));
        assert_eq!(leaving(side, doc, &ids).unwrap_err(), LEAVES, "{side}");
        let (doc, ids) = bounded(stated(loose));
        assert!(leaving(side, doc, &ids).is_ok(), "{side}");
    }
}

// For each side of a table's bounds, left, bottom, right and top: bounds the
// edit `leaving` makes on that side would cross, and bounds it stays inside.
const SIDES: [([i64; 4], [i64; 4]); 4] = [
    ([30, 130, 200, 200], [20, 130, 200, 200]),
    ([30, 170, 200, 200], [30, 160, 200, 200]),
    ([30, 130, 120, 200], [30, 130, 200, 200]),
    ([30, 130, 200, 189], [30, 130, 200, 191]),
];

// An edit of the first cell whose ink moves out on one side of [40, 180, 76,
// 188.4] and on no other.
fn leaving(side: usize, mut doc: Document, ids: &[ObjectId; 9]) -> Result<Document, String> {
    if side == 0 {
        // Before the line: a centred one starts half its growth earlier, at 22.
        doc.get_dictionary_mut(ids[2]).unwrap().set(
            "A",
            dictionary! { "O" => "Layout", "TextAlign" => "Center" },
        );
        return attempt(&doc, change(&doc, "FIRST", "FIRSTFIRST", true));
    }
    let replacement = ["", "FIRST\nFIRST", "FIRSTFIRSTFI", "FIRST"][side];
    let mut change = change(&doc, "FIRST", replacement, true);
    let layout = change.layout.as_mut().unwrap();
    match side {
        // Below: a second line, 15 pt under the first, with ink from 165.
        1 => (layout.wrap, layout.height) = (true, 30.),
        // After the line, in a box the reader sized, which no limit along the
        // line shortens: twelve glyphs of ink end at 126.4.
        2 => (layout.grow, layout.width) = (false, 120.),
        // Above: at half the size the text sits 6 pt higher in its box, and
        // its ink reaches 190.2 where it reached 188.4.
        _ => layout.size = 6.,
    }
    attempt(&doc, change)
}

// Bounds the source's own ink already crosses refuse no edit for that ink:
// the text may not reach past them further than it did, on any side.
#[test]
fn textedit_bounded_tables_allow_the_ink_their_text_already_had_outside() {
    // Each of these bounds cuts through the first cell's ink on one side:
    // left, bottom, right, top.
    for bounds in [
        [50, 130, 200, 200],
        [30, 184, 200, 200],
        [30, 130, 60, 200],
        [30, 130, 200, 184],
    ] {
        let doc = stating(bounds);
        assert_eq!(offered(&doc), ["THIRD", "FIRST", "SECOND"], "{bounds:?}");
        // The same width at the source's own positions, with and without a
        // box, and a shorter text.
        for (replacement, layout) in [("TSRIF", true), ("TSRIF", false), ("IF", false)] {
            let written = attempt(&doc, change(&doc, "FIRST", replacement, layout));
            assert!(written.is_ok(), "{bounds:?} {replacement} {written:?}");
        }
    }
    // Longer text keeps the ink the source had across the line: the bounds on
    // the left, below and above are crossed exactly as far as they were.
    for bounds in [
        [50, 130, 200, 200],
        [30, 184, 200, 200],
        [30, 130, 200, 184],
    ] {
        let doc = stating(bounds);
        let written = attempt(&doc, change(&doc, "FIRST", "FIRSTF", true));
        assert!(written.is_ok(), "{bounds:?} {written:?}");
    }
    // Past bounds it already crossed on the right it may not grow at all,
    let doc = stating([30, 130, 60, 200]);
    assert_eq!(
        attempt(&doc, change(&doc, "FIRST", "FIRSTF", true)).unwrap_err(),
        LEAVES_ALONG
    );
    // while text laid out afresh that ends before the source's ink did is
    // written: five glyphs at 10 pt end at 70, the source's at 76, and seven
    // would end at 82.
    let smaller = |doc: &Document, replacement: &str| {
        let mut change = change(doc, "FIRST", replacement, true);
        change.layout.as_mut().unwrap().size = 10.;
        attempt(doc, change)
    };
    assert!(smaller(&doc, "FIRST").is_ok());
    assert_eq!(smaller(&doc, "FIRSTFI").unwrap_err(), LEAVES_ALONG);
    // Stroked text reaches half its line's width past its outlines, and that
    // is ink it already had: 2 pt on every side of bounds drawn at the
    // outlines themselves. Longer text crosses three of them as far as it did,
    // and six glyphs at 10 pt end at 76 and reach 78 as the source did.
    let stroked = |entries: Dictionary| {
        let (mut doc, ids) = bounded(entries);
        let content = String::from_utf8(doc.get_page_content(ids[0]))
            .unwrap()
            .replace("(FIRST) Tj", "1 Tr 4 w (FIRST) Tj");
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        doc.get_dictionary_mut(ids[0])
            .unwrap()
            .set("Contents", stream);
        doc
    };
    let doc = stroked(dictionary! {
        "O" => "Layout",
        "BBox" => vec![40.into(), 180.into(), 200.into(), Object::Real(188.4)],
    });
    assert!(attempt(&doc, change(&doc, "FIRST", "FIRSTF", true)).is_ok());
    let doc = stroked(stated([30, 130, 76, 200]));
    assert!(smaller(&doc, "FIRSTF").is_ok());
    assert_eq!(smaller(&doc, "FIRSTFI").unwrap_err(), LEAVES_ALONG);
}

// Two cells on one line: growing the first pushes the second along, and the
// second may not be pushed out of the bounds its table states either.
#[test]
fn textedit_bounded_tables_hold_the_text_an_edit_pushes() {
    let pushing = |bounds: [i64; 4]| {
        let (mut doc, ids) = bounded(stated(bounds));
        let content = String::from_utf8(doc.get_page_content(ids[0]))
            .unwrap()
            .replace("40 140 Td (SECOND)", "84 180 Td (SECOND)");
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        doc.get_dictionary_mut(ids[0])
            .unwrap()
            .set("Contents", stream);
        let pushed = attempt(&doc, change(&doc, "FIRST", "FIRSTFIRST", true));
        pushed.map(|written| {
            let runs = textedit::scan(&written, 0).unwrap().runs;
            runs.iter().find(|run| run.text == "SECOND").unwrap().matrix[4]
        })
    };
    // The second cell's text ends at 127.2 and has to move on by 35.2.
    assert!(pushing([30, 130, 200, 200]).unwrap() > 110.);
    assert_eq!(pushing([30, 130, 150, 200]).unwrap_err(), LEAVES_ALONG);
}

// A paragraph in a cell wraps as it does anywhere, and the line the wrap
// moves down may not leave the bounds either. The paragraph is `wrap_tests`'
// own: three lines from (20, 200) at a 14 pt pitch, whose last moves to 158.
#[test]
fn textedit_bounded_tables_hold_the_lines_a_wrap_moves() {
    use crate::textedit::wrap_tests::{paragraph, LAST, LONGER, WIDEST};
    let wrapping = |bounds: Option<[i64; 4]>| {
        let mut doc = paragraph(52.);
        let catalog = doc.catalog().unwrap();
        let root = catalog
            .get(b"StructTreeRoot")
            .unwrap()
            .as_reference()
            .unwrap();
        let document = doc.get_dictionary(root).unwrap().get(b"K").unwrap();
        let document = document.as_array().unwrap()[0].as_reference().unwrap();
        let kids = doc.get_dictionary(document).unwrap().get(b"K").unwrap();
        let kids = kids.as_array().unwrap().clone();
        let block = kids[0].as_reference().unwrap();
        let (table, row, cell) = (
            doc.new_object_id(),
            doc.new_object_id(),
            doc.new_object_id(),
        );
        let mut stated = dictionary! { "S" => "Table", "P" => document, "K" => row };
        if let Some(bounds) = bounds {
            stated.set("A", self::stated(bounds));
        }
        doc.objects.insert(table, stated.into());
        doc.objects.insert(
            row,
            dictionary! { "S" => "TR", "P" => table, "K" => cell }.into(),
        );
        doc.objects.insert(
            cell,
            dictionary! { "S" => "TD", "P" => row, "K" => block }.into(),
        );
        doc.get_dictionary_mut(block).unwrap().set("P", cell);
        doc.get_dictionary_mut(document)
            .unwrap()
            .set("K", vec![Object::Reference(table), kids[1].clone()]);
        let runs = textedit::scan(&doc, 0).unwrap().runs;
        let index = runs.iter().position(|run| run.text == WIDEST).unwrap();
        let change = crate::textedit::layout_tests::in_default_box(&doc, index, LONGER);
        attempt(&doc, change).map(|written| {
            let runs = textedit::scan(&written, 0).unwrap().runs;
            runs.iter().find(|run| run.text == LAST).unwrap().matrix[5]
        })
    };
    // The control: the table without bounds, and bounds with the room.
    assert!((wrapping(None).unwrap() - 158.).abs() < 0.0001);
    assert!((wrapping(Some([10, 150, 300, 215])).unwrap() - 158.).abs() < 0.0001);
    // The moved line's box reaches down to 155.
    assert_eq!(wrapping(Some([10, 160, 300, 215])).unwrap_err(), LEAVES);
}

// The bounds are the nearest table's, read from its attributes and its
// classes; a table that states them twice keeps its text inside both.
#[test]
fn textedit_bounded_tables_read_every_bounds_their_table_states() {
    let grown = |doc: &Document| attempt(doc, change(doc, "FIRST", "FIRSTFIRSTFI", true));
    let wide = || dictionary! { "O" => "Layout", "BBox" => rectangle() };
    let narrow = || stated([30, 130, 120, 200]);
    // Either corner order states the same rectangle (ISO 32000-1 7.9.5).
    for corners in [
        [120, 130, 30, 200],
        [30, 200, 120, 130],
        [120, 200, 30, 130],
    ] {
        let doc = stating(corners);
        assert_eq!(grown(&doc).unwrap_err(), LEAVES_ALONG);
        assert!(attempt(&doc, change(&doc, "FIRST", "FIRSTFIRST", true)).is_ok());
    }
    // Two attribute objects, in either order: the text stays inside both, on
    // every side, and inside both is as far as either lets it go.
    for (side, (tight, loose)) in SIDES.into_iter().enumerate() {
        for (objects, refused) in [
            ([stated(loose), stated(tight)], true),
            ([stated(tight), stated(loose)], true),
            ([stated(loose), stated(loose)], false),
        ] {
            let (mut doc, ids) = bounded(wide());
            doc.get_dictionary_mut(ids[6])
                .unwrap()
                .set("A", objects.map(Object::Dictionary).to_vec());
            let written = leaving(side, doc, &ids);
            assert_eq!(written.is_err(), refused, "{side} {written:?}");
        }
    }
    // A class, alone and beside the table's own attributes.
    for own in [None, Some(wide())] {
        let (mut doc, ids) = bounded(wide());
        doc.get_dictionary_mut(ids[1])
            .unwrap()
            .set("ClassMap", dictionary! { "Tight" => narrow() });
        let table = doc.get_dictionary_mut(ids[6]).unwrap();
        table.set("C", Object::Name(b"Tight".to_vec()));
        match own {
            Some(own) => table.set("A", own),
            None => {
                table.remove(b"A");
            }
        }
        assert_eq!(grown(&doc).unwrap_err(), LEAVES_ALONG);
    }
    // The control: the wide bounds alone have the room.
    assert!(grown(&bounded(wide()).0).is_ok());
    // A paragraph in the cell is the table's text too.
    let (mut doc, ids) = bounded(narrow());
    let paragraph =
        doc.add_object(dictionary! { "S" => "P", "P" => ids[3], "Pg" => ids[0], "K" => 0 });
    doc.get_dictionary_mut(ids[3]).unwrap().set("K", paragraph);
    let slots = doc
        .get_dictionary_mut(ids[5])
        .unwrap()
        .get_mut(b"Nums")
        .unwrap();
    slots.as_array_mut().unwrap()[1].as_array_mut().unwrap()[0] = paragraph.into();
    assert_eq!(grown(&doc).unwrap_err(), LEAVES_ALONG);
}

// The bounds of a figure still keep its text as it is: only a table's cells
// are edited inside the bounds it states.
#[test]
fn textedit_bounded_figures_keep_their_text_read_only() {
    let (mut doc, ids) = bounded(dictionary! { "O" => "Layout", "BBox" => rectangle() });
    // The control, and the change a reader would aim at the paragraph beside
    // the table while it is one.
    assert_eq!(offered(&doc), ["THIRD", "FIRST", "SECOND"]);
    let aimed = change(&doc, "THIRD", "IN", false);
    assert!(attempt(&doc, aimed.clone()).is_ok());
    // That paragraph becomes a figure stating its bounds, its text in a Span.
    let document = doc.get_dictionary(ids[2]).unwrap();
    let outside = document.get(b"K").unwrap().as_array().unwrap()[1]
        .as_reference()
        .unwrap();
    let span =
        doc.add_object(dictionary! { "S" => "Span", "P" => outside, "Pg" => ids[0], "K" => 2 });
    let slots = doc
        .get_dictionary_mut(ids[5])
        .unwrap()
        .get_mut(b"Nums")
        .unwrap();
    slots.as_array_mut().unwrap()[1].as_array_mut().unwrap()[2] = span.into();
    let figure = doc.get_dictionary_mut(outside).unwrap();
    figure.set("S", "Figure");
    figure.set("K", span);
    figure.set(
        "A",
        dictionary! { "O" => "Layout", "BBox" => vec![30.into(), 90.into(), 200.into(), 120.into()] },
    );
    assert_eq!(offered(&doc), ["FIRST", "SECOND"]);
    assert!(attempt(&doc, aimed).is_err());
}

#[test]
fn textedit_bounded_tables_refuse_malformed_bounds_and_placements() {
    for entries in [
        dictionary! { "O" => "Table", "BBox" => rectangle() },
        dictionary! { "O" => "Layout", "BBox" => vec![0.into(), 0.into(), 9.into()] },
        dictionary! { "O" => "Layout", "BBox" => vec![0.into(), 9.into(), 9.into(), "Top".into()] },
        dictionary! { "O" => "Layout", "BBox" => Object::Name(b"Auto".to_vec()) },
        dictionary! { "O" => "Layout", "Placement" => "Middle" },
        dictionary! { "O" => "Layout", "Width" => -1 },
        dictionary! { "O" => "Layout", "Height" => "Some" },
        dictionary! { "O" => "Layout", "StartIndent" => "Wide" },
        dictionary! { "O" => "Layout", "WritingMode" => "TbRl" },
        dictionary! { "O" => "Layout", "TextAlign" => "Center" },
    ] {
        let (doc, _) = bounded(entries);
        refused(doc);
    }
    // ISO 32000-1 7.9.5: a rectangle may name either pair of opposite
    // corners (InDesign gives the top first). Accepted, and the cells are
    // offered as under any bounds; what the corners mean is asserted in
    // `textedit_bounded_tables_read_every_bounds_their_table_states`. This
    // asserted that such bounds still pinned, which no table's do now.
    for corners in [[9, 0, 0, 9], [0, 9, 9, 0]] {
        let (doc, _) =
            bounded(dictionary! { "O" => "Layout", "BBox" => corners.map(Object::from).to_vec() });
        assert_eq!(offered(&doc), ["THIRD", "FIRST", "SECOND"]);
    }
    // Block indents and spacing are allocation, accepted here as on a paragraph.
    let (doc, _) = bounded(dictionary! { "O" => "Layout", "StartIndent" => 12, "SpaceAfter" => 6 });
    assert!(textedit::scan(&doc, 0).is_ok());
}

// LibreOffice 26.2 writes a placement on every row and a Layout object on every
// cell, beside the cell's Table object. Refusing them refused every page of a
// document with one table in it.
#[test]
fn textedit_tables_keep_a_row_placed_as_a_block_and_a_cells_own_size() {
    let (mut doc, ids) = table();
    for row in [ids[7], ids[8]] {
        doc.get_dictionary_mut(row)
            .unwrap()
            .set("A", dictionary! { "O" => "Layout", "Placement" => "Block" });
    }
    let size = dictionary! { "O" => "Layout", "Placement" => "Inline", "Width" => 9.326, "Height" => 2.438 };
    doc.get_dictionary_mut(ids[3]).unwrap().set(
        "A",
        vec![
            Object::Dictionary(size.clone()),
            Object::Dictionary(dictionary! { "O" => "Table", "ColSpan" => 2 }),
        ],
    );
    doc.get_dictionary_mut(ids[4]).unwrap().set("A", size);
    let before = doc.objects.clone();
    assert_eq!(offered(&doc), ["FIRST", "SECOND"]);
    assert_eq!(doc.objects, before, "reading changes nothing");

    // What a cell's Layout object may say is held as tightly as its Table one.
    for attributes in [
        dictionary! { "O" => "Layout", "Width" => -1 },
        dictionary! { "O" => "Layout", "Height" => "Tall" },
        dictionary! { "O" => "Layout", "Placement" => "Before" },
        dictionary! { "O" => "Layout", "TextAlign" => "Center" },
        dictionary! { "O" => "Layout", "BBox" => rectangle() },
    ] {
        let (mut doc, ids) = table();
        doc.get_dictionary_mut(ids[3]).unwrap().set("A", attributes);
        refused(doc);
    }
    // And it is given once.
    let (mut doc, ids) = table();
    doc.get_dictionary_mut(ids[3]).unwrap().set(
        "A",
        vec![
            Object::Dictionary(dictionary! { "O" => "Layout", "Width" => 1 }),
            Object::Dictionary(dictionary! { "O" => "Layout", "Height" => 1 }),
        ],
    );
    refused(doc);
}

// A paragraph in a cell under a name of the producer's own: LibreOffice tags it
// Standard, or with the paragraph style's name, and the RoleMap makes it a P.
#[test]
fn textedit_tables_read_a_cells_paragraph_through_the_role_map() {
    let aliased = |tag: &str| {
        let (mut doc, ids) = table();
        let cell = ids[3];
        let paragraph =
            doc.add_object(dictionary! { "S" => tag, "P" => cell, "Pg" => ids[0], "K" => 0 });
        doc.get_dictionary_mut(cell).unwrap().set("K", paragraph);
        doc.get_dictionary_mut(ids[5]).unwrap().set(
            "Nums",
            vec![
                0.into(),
                Object::Array(vec![paragraph.into(), ids[4].into(), ids[6].into()]),
            ],
        );
        let bytes = String::from_utf8(doc.get_page_content(ids[0])).unwrap();
        let bytes = bytes.replacen("/TD << /MCID 0", &format!("/{tag} << /MCID 0"), 1);
        let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.into_bytes()));
        doc.get_dictionary_mut(ids[0])
            .unwrap()
            .set("Contents", stream);
        doc
    };
    // The control: a literal P has always been read.
    assert_eq!(offered(&aliased("P")), ["FIRST", "SECOND"]);
    assert_eq!(offered(&aliased("Standard")), ["FIRST", "SECOND"]);
    // A name the RoleMap does not make a text block is not one.
    refused(aliased("Information"));
}
