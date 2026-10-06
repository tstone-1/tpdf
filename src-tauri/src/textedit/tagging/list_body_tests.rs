//! A list item whose body holds its paragraph as a block of its own, the shape
//! LibreOffice writes: `LI > LBody > paragraph`, the paragraph under `P` or
//! under the paragraph style's name, which the RoleMap makes a `P`.
use super::list_tests::{list, refused};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Document, Object, ObjectId};

fn offered(doc: &Document) -> Vec<String> {
    textedit::scan(doc, 0)
        .unwrap()
        .runs
        .into_iter()
        .map(|run| run.text)
        .collect()
}

/// The list fixture with each body's content moved into a paragraph named
/// `tag`. Returns the document, the page, the two bodies and their paragraphs.
fn styled(tag: &str) -> (Document, ObjectId, [ObjectId; 2], [ObjectId; 2]) {
    let (mut doc, ids, _, owners) = list();
    let bodies = [owners[1], owners[3]];
    let mut paragraphs = Vec::new();
    for (index, body) in bodies.into_iter().enumerate() {
        let paragraph = doc.add_object(dictionary! {
            "S" => tag, "P" => body, "Pg" => ids[0], "K" => (index * 2 + 1) as i64,
            "A" => dictionary! { "O" => "Layout", "Placement" => "Block", "StartIndent" => 18, "TextIndent" => -18 },
        });
        let body = doc.get_dictionary_mut(body).unwrap();
        body.set("K", paragraph);
        body.remove(b"Pg");
        paragraphs.push(paragraph);
    }
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![
                owners[0].into(),
                paragraphs[0].into(),
                owners[2].into(),
                paragraphs[1].into(),
            ]),
        ],
    );
    (doc, ids[0], bodies, [paragraphs[0], paragraphs[1]])
}

#[test]
fn textedit_list_bodies_hold_one_paragraph_under_its_own_or_a_producer_name() {
    for tag in ["P", "Standard", "H2"] {
        let (mut doc, _, _, _) = styled(tag);
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(offered(&doc), ["1.", "FIRST", "2.", "SECOND"], "{tag}");
        let objects = doc.objects.clone();
        textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: before.revision.clone(),
                operator: before.runs[1].operator,
                original: "FIRST".into(),
                replacement: "IN".into(),
            }],
        )
        .unwrap();
        assert_eq!(offered(&doc), ["1.", "IN", "2.", "SECOND"], "{tag}");
        // Every structure object, the role map and the parent tree are
        // written back as they were read.
        let kept = objects
            .iter()
            .filter(|(_, object)| {
                object
                    .as_dict()
                    .is_ok_and(|dict| dict.has(b"S") || dict.has(b"Nums") || dict.has(b"RoleMap"))
            })
            .collect::<Vec<_>>();
        assert!(kept.len() >= 10, "{tag}");
        for (id, object) in kept {
            assert_eq!(&doc.objects[id], object, "{tag} {id:?}");
        }
    }
    // A name the RoleMap does not make a text block is not a paragraph, and a
    // standard name keeps its own meaning: neither may sit in a body.
    for tag in ["Information", "Div", "Table", "LI", "Lbl", "LBody", "TD"] {
        refused(styled(tag).0);
    }
}

// The item is one block to a wrap: its label and its paragraph, exactly as
// when the body owns the words itself. The paragraph is not a block apart,
// which would let a wrap move it and leave its label behind.
#[test]
fn textedit_a_list_paragraph_belongs_to_its_item() {
    let (doc, _, bodies, _) = styled("Standard");
    let item = |body: ObjectId| {
        doc.get_dictionary(body)
            .unwrap()
            .get(b"P")
            .unwrap()
            .as_reference()
            .unwrap()
    };
    let page = textedit::inspect(&doc, 0).unwrap();
    let owners: Vec<_> = page
        .runs
        .runs
        .iter()
        .map(|run| page.blocks[&run.operator])
        .collect();
    assert_eq!(
        owners,
        [
            item(bodies[0]),
            item(bodies[0]),
            item(bodies[1]),
            item(bodies[1])
        ]
    );
}

#[test]
fn textedit_list_bodies_refuse_a_second_paragraph_and_words_beside_one() {
    // Two paragraphs in one body: both would be lines of one block to a wrap.
    let (mut doc, page, bodies, paragraphs) = styled("Standard");
    let second = doc.add_object(
        dictionary! { "S" => "Standard", "P" => bodies[0], "Pg" => page, "K" => Vec::<Object>::new() },
    );
    doc.get_dictionary_mut(bodies[0])
        .unwrap()
        .set("K", vec![paragraphs[0].into(), Object::Reference(second)]);
    refused(doc);
    // Control: the same second child as a sublist is no second paragraph.
    let (mut doc, _, bodies, paragraphs) = styled("Standard");
    let sublist =
        doc.add_object(dictionary! { "S" => "L", "P" => bodies[0], "K" => Vec::<Object>::new() });
    doc.get_dictionary_mut(bodies[0])
        .unwrap()
        .set("K", vec![paragraphs[0].into(), Object::Reference(sublist)]);
    assert_eq!(offered(&doc), ["1.", "FIRST", "2.", "SECOND"]);

    // An inline leaf of the body's own beside the paragraph.
    let (mut doc, page, bodies, paragraphs) = styled("Standard");
    let span = doc.add_object(
        dictionary! { "S" => "Span", "P" => bodies[0], "Pg" => page, "K" => Vec::<Object>::new() },
    );
    doc.get_dictionary_mut(bodies[0])
        .unwrap()
        .set("K", vec![paragraphs[0].into(), Object::Reference(span)]);
    refused(doc);
    // Control: the same leaf inside the paragraph is an ordinary Span.
    let (mut doc, page, _, paragraphs) = styled("Standard");
    let span = doc.add_object(
        dictionary! { "S" => "Span", "P" => paragraphs[0], "Pg" => page, "K" => Vec::<Object>::new() },
    );
    doc.get_dictionary_mut(paragraphs[0])
        .unwrap()
        .set("K", vec![Object::Integer(1), Object::Reference(span)]);
    assert_eq!(offered(&doc), ["1.", "FIRST", "2.", "SECOND"]);
}

// Marked content the body claims for itself beside its paragraph: the words
// would be lines of the paragraph to a wrap, and nothing says they are.
#[test]
fn textedit_list_bodies_refuse_their_own_words_beside_a_paragraph() {
    let (mut doc, page, bodies, paragraphs) = styled("Standard");
    // The first body takes the second paragraph's sequence as its own.
    let body = doc.get_dictionary_mut(bodies[0]).unwrap();
    body.set("K", vec![paragraphs[0].into(), Object::Integer(3)]);
    body.set("Pg", page);
    doc.get_dictionary_mut(paragraphs[1])
        .unwrap()
        .set("K", Vec::<Object>::new());
    let parents = doc
        .objects
        .iter()
        .find(|(_, object)| object.as_dict().is_ok_and(|dict| dict.has(b"Nums")))
        .map(|(id, _)| *id)
        .unwrap();
    let nums = doc.get_dictionary_mut(parents).unwrap();
    let slots = nums.get_mut(b"Nums").unwrap().as_array_mut().unwrap()[1]
        .as_array_mut()
        .unwrap();
    slots[3] = bodies[0].into();
    refused(doc);
    // Control: a body that owns words and holds no paragraph is the list the
    // editor has always read.
    assert_eq!(offered(&list().0), ["1.", "FIRST", "2.", "SECOND"]);
}

// A paragraph in a body holds leaves, as a paragraph in a cell does, and no
// further block: the recursion ends at the paragraph.
#[test]
fn textedit_a_list_paragraph_holds_leaves_and_no_block() {
    let (mut doc, page, _, paragraphs) = styled("Standard");
    let span = doc.add_object(dictionary! {
        "S" => "Span", "P" => paragraphs[0], "Pg" => page, "K" => 1,
        "Lang" => Object::string_literal("en-GB"),
    });
    doc.get_dictionary_mut(paragraphs[0])
        .unwrap()
        .set("K", span);
    let parents = doc
        .objects
        .iter()
        .find(|(_, object)| object.as_dict().is_ok_and(|dict| dict.has(b"Nums")))
        .map(|(id, _)| *id)
        .unwrap();
    doc.get_dictionary_mut(parents)
        .unwrap()
        .get_mut(b"Nums")
        .unwrap()
        .as_array_mut()
        .unwrap()[1]
        .as_array_mut()
        .unwrap()[1] = span.into();
    assert_eq!(offered(&doc), ["1.", "FIRST", "2.", "SECOND"]);
    for tag in ["Standard", "P", "LBody", "Div"] {
        let mut doc = doc.clone();
        doc.get_dictionary_mut(span).unwrap().set("S", tag);
        refused(doc);
    }
    // A label holds its words and no paragraph.
    let (mut doc, _, bodies, _) = styled("Standard");
    doc.get_dictionary_mut(bodies[0]).unwrap().set("S", "Lbl");
    refused(doc);
}

// LibreOffice puts a sublist in the body beside the paragraph. It goes back to
// the walk like a sublist beside the body's own words, and is neither a second
// paragraph nor words of the body.
#[test]
fn textedit_a_sublist_sits_beside_the_paragraph_in_a_list_body() {
    let (mut doc, _, bodies, paragraphs) = styled("Standard");
    // The second item becomes the only item of a list inside the first body.
    let second = doc
        .get_dictionary(bodies[1])
        .unwrap()
        .get(b"P")
        .unwrap()
        .as_reference()
        .unwrap();
    let outer = doc
        .get_dictionary(second)
        .unwrap()
        .get(b"P")
        .unwrap()
        .as_reference()
        .unwrap();
    let first = doc
        .get_dictionary(bodies[0])
        .unwrap()
        .get(b"P")
        .unwrap()
        .as_reference()
        .unwrap();
    let inner = doc.add_object(dictionary! { "S" => "L", "P" => bodies[0], "K" => second,
    "A" => dictionary! { "O" => "List", "ListNumbering" => "Disc" } });
    doc.get_dictionary_mut(outer).unwrap().set("K", first);
    doc.get_dictionary_mut(second).unwrap().set("P", inner);
    doc.get_dictionary_mut(bodies[0])
        .unwrap()
        .set("K", vec![paragraphs[0].into(), Object::Reference(inner)]);
    for edited in [1, 3] {
        let mut doc = doc.clone();
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(offered(&doc), ["1.", "FIRST", "2.", "SECOND"]);
        textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: before.revision.clone(),
                operator: before.runs[edited].operator,
                original: before.runs[edited].text.clone(),
                replacement: "IN".into(),
            }],
        )
        .unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        for index in 0..4 {
            if index == edited {
                assert_eq!(after.runs[index].text, "IN");
            } else {
                assert_eq!(after.runs[index], before.runs[index]);
            }
        }
    }
    // Each level's text belongs to its own item.
    let page = textedit::inspect(&doc, 0).unwrap();
    let owners: Vec<_> = page
        .runs
        .runs
        .iter()
        .map(|run| page.blocks[&run.operator])
        .collect();
    assert_eq!(owners, [first, first, second, second]);
}

// What pins a paragraph pins it in a body too, and what pins the body reaches
// the paragraph inside it. The label is neither's.
#[test]
fn textedit_a_pinned_list_paragraph_keeps_its_text_and_leaves_the_label() {
    let (mut doc, _, _, paragraphs) = styled("Standard");
    doc.get_dictionary_mut(paragraphs[0]).unwrap().set(
        "A",
        dictionary! { "O" => "Layout", "Placement" => "Block", "TextAlign" => "Justify" },
    );
    assert_eq!(offered(&doc), ["1.", "2.", "SECOND"]);
    let (mut doc, _, bodies, _) = styled("Standard");
    doc.get_dictionary_mut(bodies[1])
        .unwrap()
        .set("Alt", Object::string_literal("SYNTHETIC"));
    assert_eq!(offered(&doc), ["1.", "FIRST", "2."]);
    // Its own ink bounds are refused on a paragraph here as anywhere.
    let (mut doc, _, _, paragraphs) = styled("Standard");
    doc.get_dictionary_mut(paragraphs[0]).unwrap().set(
        "A",
        dictionary! { "O" => "Layout", "BBox" => vec![0.into(), 0.into(), 300.into(), 240.into()] },
    );
    assert_eq!(
        textedit::scan(&doc, 0).unwrap_err(),
        "unsupported BBox metadata in tagged layout attributes"
    );
}
