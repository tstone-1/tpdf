//! Form fields written out in place rather than as objects of their own.
//!
//! A field may be a dictionary directly inside `/AcroForm /Fields` or inside
//! another field's `/Kids`. It has no object id, so `forget` cannot name it,
//! and the walk used to stop at it: its value stayed, and so did every field
//! under it.

use lopdf::{dictionary, Document, Object, ObjectId};
use std::collections::HashSet;

use super::{covered_fields, drop_fields, drop_inline_fields};

/// A one-page document whose form holds `fields`, with the annotations
/// `build` makes on its page.
fn form_of(fields: Vec<Object>, build: impl FnOnce(&mut Document) -> Vec<Object>) -> Document {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let annots = build(&mut doc);
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        "Annots" => annots,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
        }),
    );
    let form = doc.add_object(dictionary! { "Fields" => fields });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages, "AcroForm" => form,
    });
    doc.trailer.set("Root", catalog);
    doc
}

/// Whether `needle` is anywhere in the document as written.
///
/// After the objects nothing reaches are dropped, as the sweep every redaction
/// runs drops them: a forgotten widget's appearance stream is one of those.
fn holds(doc: &mut Document, needle: &[u8]) -> bool {
    doc.prune_objects();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialise");
    bytes.windows(needle.len()).any(|window| window == needle)
}

/// The two passes in the order the rewrite runs them.
fn remove(doc: &mut Document, taken: &[String]) -> (Vec<ObjectId>, usize) {
    let doomed = covered_fields(doc, taken, &HashSet::new());
    drop_fields(doc, &doomed).expect("drop the fields");
    let inline = drop_inline_fields(doc, taken);
    (doomed, inline)
}

fn widget(doc: &mut Document, answer: &str) -> ObjectId {
    let look = doc.add_object(lopdf::Stream::new(
        dictionary! { "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 100.into(), 20.into()] },
        format!("BT ({answer}) Tj ET").into_bytes(),
    ));
    doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Rect" => vec![0.into(), 0.into(), 100.into(), 20.into()],
        "AP" => dictionary! { "N" => look },
    })
}

/// An inline field holding the taken text goes, and its widget with it.
#[test]
fn an_inline_field_holding_taken_text_goes_with_its_widget() {
    let mut drawn = None;
    let mut doc = form_of(Vec::new(), |doc| {
        let id = widget(doc, "ACCOUNT 12345678");
        drawn = Some(id);
        vec![id.into()]
    });
    let drawn = drawn.expect("the widget");
    let kept = doc.add_object(dictionary! {
        "T" => Object::string_literal("other"),
        "FT" => "Tx",
        "V" => Object::string_literal("nothing taken here"),
    });
    let inline = dictionary! {
        "T" => Object::string_literal("account"),
        "FT" => "Tx",
        "V" => Object::string_literal("ACCOUNT 12345678"),
        "Kids" => vec![drawn.into()],
    };
    let form = doc
        .catalog()
        .expect("catalog")
        .get(b"AcroForm")
        .and_then(Object::as_reference)
        .expect("the form");
    doc.get_object_mut(form)
        .and_then(Object::as_dict_mut)
        .expect("the form")
        .set("Fields", vec![Object::Dictionary(inline), kept.into()]);

    let (doomed, inline) = remove(&mut doc, &["ACCOUNT 12345678".to_string()]);

    assert_eq!(doomed, vec![drawn], "the widget under the inline field");
    assert_eq!(inline, 1, "the inline field itself");
    assert!(
        !holds(&mut doc, b"12345678"),
        "no copy of the answer is left"
    );
    // The control: the field that holds nothing taken stays.
    assert!(holds(&mut doc, b"nothing taken here"));
}

/// A referenced field under an inline parent is reached and judged.
///
/// The parent holds no value; the kid does. With the walk stopping at an
/// inline entry, the kid was never looked at.
#[test]
fn a_field_under_an_inline_parent_is_reached() {
    let mut doc = form_of(Vec::new(), |_| Vec::new());
    let kid = doc.add_object(dictionary! {
        "T" => Object::string_literal("number"),
        "FT" => "Tx",
        "V" => Object::string_literal("ACCOUNT 12345678"),
    });
    let parent = dictionary! {
        "T" => Object::string_literal("group"),
        "Kids" => vec![kid.into()],
    };
    let form = doc
        .catalog()
        .expect("catalog")
        .get(b"AcroForm")
        .and_then(Object::as_reference)
        .expect("the form");
    doc.get_object_mut(form)
        .and_then(Object::as_dict_mut)
        .expect("the form")
        .set("Fields", vec![Object::Dictionary(parent)]);

    let (doomed, inline) = remove(&mut doc, &["ACCOUNT 12345678".to_string()]);

    assert_eq!(doomed, vec![kid]);
    assert_eq!(inline, 1, "the parent, its `/Kids` now empty");
    assert!(!holds(&mut doc, b"12345678"));
    assert!(
        !doc.catalog().expect("catalog").has(b"AcroForm"),
        "a form with no field left goes"
    );
}

/// An inline field holding nothing taken is left as it was.
#[test]
fn an_inline_field_holding_nothing_taken_stays() {
    let mut doc = form_of(
        vec![Object::Dictionary(dictionary! {
            "T" => Object::string_literal("name"),
            "FT" => "Tx",
            "V" => Object::string_literal("PUBLIC ANSWER"),
        })],
        |_| Vec::new(),
    );

    let (doomed, inline) = remove(&mut doc, &["ACCOUNT 12345678".to_string()]);

    assert!(doomed.is_empty());
    assert_eq!(inline, 0);
    assert!(holds(&mut doc, b"PUBLIC ANSWER"));
}
