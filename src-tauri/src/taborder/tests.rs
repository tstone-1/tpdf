use lopdf::{dictionary, Document, Object, ObjectId};

use super::{count, in_order, sort};

/// A page of 300 by 200 points whose annotations are these, in this order.
/// Each is a name and a rectangle in the page's own space, from the bottom
/// left; a name that starts with `note` is a comment and not a widget.
fn page_of(listed: &[(&str, [i64; 4])], indirect: bool, rotate: i64) -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let page = doc.new_object_id();
    let entries: Vec<Object> = listed
        .iter()
        .map(|(name, rect)| {
            let widget = !name.starts_with("note");
            Object::Reference(doc.add_object(dictionary! {
                "Type" => "Annot",
                "Subtype" => if widget { "Widget" } else { "Text" },
                "T" => Object::string_literal(*name),
                "Rect" => rect.iter().map(|v| Object::Integer(*v)).collect::<Vec<_>>(),
            }))
        })
        .collect();
    let annots: Object = if indirect {
        doc.add_object(entries).into()
    } else {
        entries.into()
    };
    doc.objects.insert(
        page,
        dictionary! {
            "Type" => "Page", "Parent" => pages, "Rotate" => rotate,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 200.into()],
            "Annots" => annots,
        }
        .into(),
    );
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Count" => 1, "Kids" => vec![Object::Reference(page)] }
            .into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    (doc, page)
}

/// The names of the page's annotations, in the order it lists them.
fn names(doc: &Document, page: ObjectId) -> Vec<String> {
    let held = doc.get_dictionary(page).unwrap().get(b"Annots").unwrap();
    let entries = doc.dereference(held).unwrap().1.as_array().unwrap();
    entries
        .iter()
        .map(|entry| {
            let dict = doc.get_dictionary(entry.as_reference().unwrap()).unwrap();
            String::from_utf8_lossy(dict.get(b"T").unwrap().as_str().unwrap()).into_owned()
        })
        .collect()
}

fn tabs(doc: &Document, page: ObjectId) -> Option<Vec<u8>> {
    doc.get_dictionary(page)
        .unwrap()
        .get(b"Tabs")
        .ok()
        .map(|tabs| tabs.as_name().unwrap().to_vec())
}

// Two rows of two, a third row of one. `[left, bottom, right, top]`.
const A: (&str, [i64; 4]) = ("a", [20, 160, 120, 180]);
const B: (&str, [i64; 4]) = ("b", [150, 160, 250, 180]);
const C: (&str, [i64; 4]) = ("c", [20, 120, 120, 140]);
const D: (&str, [i64; 4]) = ("d", [150, 120, 250, 140]);
const E: (&str, [i64; 4]) = ("e", [20, 80, 120, 100]);

#[test]
fn fields_are_listed_in_rows_from_the_top_and_each_row_from_the_left() {
    for indirect in [false, true] {
        let (mut doc, page) = page_of(&[E, D, A, C, B], indirect, 0);
        assert!(!in_order(&doc, page));
        assert_eq!(count(&doc, page), 5);
        assert_eq!(sort(&mut doc, page), Ok(true));
        assert_eq!(names(&doc, page), ["a", "b", "c", "d", "e"], "{indirect}");
        assert_eq!(tabs(&doc, page).as_deref(), Some(b"R".as_slice()));
        assert!(in_order(&doc, page));
        // Asked again, nothing moves.
        assert_eq!(sort(&mut doc, page), Ok(false));
        assert_eq!(names(&doc, page), ["a", "b", "c", "d", "e"]);
    }
}

#[test]
fn a_comment_keeps_its_place_in_the_list_and_the_fields_take_the_fields_places() {
    let note = ("note 1", [200, 20, 220, 40]);
    let other = ("note 2", [0, 190, 10, 200]);
    let (mut doc, page) = page_of(&[D, note, A, other, B], false, 0);
    assert_eq!(count(&doc, page), 3);
    assert_eq!(sort(&mut doc, page), Ok(true));
    assert_eq!(names(&doc, page), ["a", "note 1", "b", "note 2", "d"]);
}

#[test]
fn a_box_beside_a_taller_field_is_in_that_fields_row() {
    // A field 40 high, and a checkbox whose top is below the field's top
    // and whose middle is inside it: one row, the field first as it is left.
    let tall = ("tall", [20, 140, 120, 180]);
    let tick = ("tick", [150, 150, 162, 162]);
    // And one whose middle is below the field: the next row, though it is
    // further left than the checkbox.
    let below = ("below", [130, 120, 142, 132]);
    let (mut doc, page) = page_of(&[below, tick, tall], false, 0);
    sort(&mut doc, page).unwrap();
    assert_eq!(names(&doc, page), ["tall", "tick", "below"]);
    // A row is ordered from the left whichever is higher by a little.
    let high = ("high", [150, 165, 250, 185]);
    let low = ("low", [20, 160, 120, 180]);
    let (mut doc, page) = page_of(&[high, low], false, 0);
    sort(&mut doc, page).unwrap();
    assert_eq!(names(&doc, page), ["low", "high"]);
}

#[test]
fn fields_at_one_place_keep_the_order_they_had() {
    let first = ("first", [20, 160, 120, 180]);
    let second = ("second", [20, 160, 120, 180]);
    let (mut doc, page) = page_of(&[first, second, A], false, 0);
    assert!(in_order(&doc, page));
    assert_eq!(sort(&mut doc, page), Ok(false));
    assert_eq!(names(&doc, page), ["first", "second", "a"]);
    // In order already, the page is still told how its fields are tabbed.
    assert_eq!(tabs(&doc, page).as_deref(), Some(b"R".as_slice()));
}

#[test]
fn the_rows_are_the_ones_a_reader_sees_on_a_turned_page() {
    // Turned a quarter clockwise, the page's left edge is the reader's top:
    // what is furthest left in the file is the first row, and within a row
    // the reader's left is the file's bottom.
    let (mut doc, page) = page_of(&[A, B, C, D, E], false, 90);
    sort(&mut doc, page).unwrap();
    assert_eq!(names(&doc, page), ["e", "c", "a", "d", "b"]);
}

#[test]
fn a_page_with_no_fields_is_in_order_and_is_left_as_it_is() {
    let note = ("note 1", [200, 20, 220, 40]);
    let (mut doc, page) = page_of(&[note], false, 0);
    assert!(in_order(&doc, page));
    assert_eq!(count(&doc, page), 0);
    assert_eq!(sort(&mut doc, page), Ok(false));
    assert_eq!(tabs(&doc, page), None);
    // And one with no list at all.
    let (mut doc, page) = page_of(&[], false, 0);
    doc.get_dictionary_mut(page).unwrap().remove(b"Annots");
    assert!(in_order(&doc, page));
    assert_eq!(sort(&mut doc, page), Ok(false));
}

#[test]
fn a_widget_with_no_readable_rectangle_keeps_its_slot() {
    let (mut doc, page) = page_of(&[D, ("broken", [0, 0, 0, 0]), A], false, 0);
    let held = doc
        .get_dictionary(page)
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap()[1]
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(held)
        .unwrap()
        .set("Rect", vec![Object::Integer(1)]);
    assert_eq!(count(&doc, page), 2);
    sort(&mut doc, page).unwrap();
    assert_eq!(names(&doc, page), ["a", "broken", "d"]);
}
