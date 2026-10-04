use lopdf::{dictionary, Dictionary, Document, Object, ObjectId};

use super::{apply, FieldEdit};
use crate::formfields::{add, Kind, NewField};
use crate::forms::{scan, write, Change, Value, Widget};

struct Fixture {
    doc: Document,
    pages: [ObjectId; 2],
}

/// Two pages of 300 by 200 points, the second turned a quarter by the
/// document. On the first: a framed text field `Name` answered `Ada`, a
/// checkbox `Agree`, a text field `Notes`; a field `Pair` with two widgets; a
/// read-only text field `Fixed`; and a signature field `Signed`. On the
/// second: a text field `Turned`.
fn fixture() -> Fixture {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let mut ids = Vec::new();
    for at in 0..2 {
        let mut page = dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 200.into()],
            "Resources" => Dictionary::new(),
        };
        if at == 1 {
            page.set("Rotate", 90);
        }
        ids.push(doc.add_object(page));
    }
    doc.objects.insert(
        pages,
        dictionary! {
            "Type" => "Pages", "Count" => 2,
            "Kids" => ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
        }
        .into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let field = |name: &str, kind: Kind, rect: [f64; 4], border: bool| NewField {
        options: Vec::new(),
        name: name.into(),
        kind,
        page: 0,
        rect,
        tooltip: None,
        required: false,
        max_length: None,
        border,
    };
    add(
        &mut doc,
        &[
            field("Name", Kind::Text, [20.0, 20.0, 100.0, 20.0], true),
            field("Agree", Kind::Checkbox, [20.0, 60.0, 12.0, 12.0], false),
            field("Notes", Kind::Text, [20.0, 90.0, 100.0, 20.0], false),
        ],
    )
    .expect("added");
    let form = doc
        .get_dictionary(catalog)
        .unwrap()
        .get(b"AcroForm")
        .and_then(Object::as_reference)
        .expect("a form of its own");
    // By hand, the shapes `add` does not write.
    let widget = |doc: &mut Document, extra: Dictionary, rect: [i64; 4], page: ObjectId| {
        let mut dict = dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "F" => 4, "P" => page,
            "Rect" => rect.iter().map(|v| Object::Integer(*v)).collect::<Vec<_>>(),
        };
        for (key, value) in extra {
            dict.set(key, value);
        }
        doc.add_object(dict)
    };
    let pair = doc.new_object_id();
    let first = widget(
        &mut doc,
        dictionary! { "Parent" => pair },
        [150, 150, 250, 170],
        ids[0],
    );
    let second = widget(
        &mut doc,
        dictionary! { "Parent" => pair },
        [150, 120, 250, 140],
        ids[0],
    );
    doc.objects.insert(
        pair,
        dictionary! {
            "FT" => "Tx", "T" => Object::string_literal("Pair"),
            "Kids" => vec![first.into(), second.into()],
        }
        .into(),
    );
    let fixed = widget(
        &mut doc,
        dictionary! { "FT" => "Tx", "T" => Object::string_literal("Fixed"), "Ff" => 1 },
        [150, 90, 250, 110],
        ids[0],
    );
    let signed = widget(
        &mut doc,
        dictionary! { "FT" => "Sig", "T" => Object::string_literal("Signed") },
        [150, 60, 250, 80],
        ids[0],
    );
    let turned = widget(
        &mut doc,
        dictionary! { "FT" => "Tx", "T" => Object::string_literal("Turned") },
        [20, 20, 120, 40],
        ids[1],
    );
    let push = |doc: &mut Document, owner: ObjectId, key: &[u8], items: &[ObjectId]| {
        let held = doc.get_dictionary(owner).unwrap().get(key).ok().cloned();
        let target = match held {
            Some(Object::Reference(id)) => id,
            // Written into the owner: moved into an object of its own, kept.
            Some(Object::Array(items)) => {
                let id = doc.add_object(items);
                doc.get_dictionary_mut(owner).unwrap().set(key, id);
                id
            }
            _ => {
                let id = doc.add_object(Vec::<Object>::new());
                doc.get_dictionary_mut(owner).unwrap().set(key, id);
                id
            }
        };
        let list = doc.get_object_mut(target).unwrap().as_array_mut().unwrap();
        list.extend(items.iter().copied().map(Object::Reference));
    };
    // `add` may have written the page's annotations into the page; move them
    // out so both shapes of a list are walked somewhere in this file.
    let annots = doc
        .get_dictionary(ids[0])
        .unwrap()
        .get(b"Annots")
        .cloned()
        .unwrap();
    if let Object::Array(items) = annots {
        let id = doc.add_object(items);
        doc.get_dictionary_mut(ids[0]).unwrap().set("Annots", id);
    }
    push(&mut doc, ids[0], b"Annots", &[first, second, fixed, signed]);
    push(&mut doc, ids[1], b"Annots", &[turned]);
    push(&mut doc, form, b"Fields", &[pair, fixed, signed, turned]);
    let name = scan(&doc)
        .unwrap()
        .widgets
        .iter()
        .find(|w| w.name == "Name")
        .unwrap()
        .object;
    write(
        &mut doc,
        &[Change {
            object: name,
            value: Value::Text("Ada".into()),
        }],
    )
    .expect("answered");
    Fixture {
        doc,
        pages: [ids[0], ids[1]],
    }
}

fn widgets(doc: &Document, name: &str) -> Vec<Widget> {
    scan(doc)
        .expect("a form")
        .widgets
        .into_iter()
        .filter(|w| w.name == name)
        .collect()
}

fn one(doc: &Document, name: &str) -> Widget {
    let mut found = widgets(doc, name);
    assert_eq!(found.len(), 1, "{name}");
    found.remove(0)
}

fn to(widget: &Widget, rect: [f32; 4]) -> FieldEdit {
    FieldEdit {
        widget: widget.widget,
        rect: Some(rect),
        name: None,
        remove: false,
    }
}

fn named(widget: &Widget, name: &str) -> FieldEdit {
    FieldEdit {
        widget: widget.widget,
        rect: None,
        name: Some(name.into()),
        remove: false,
    }
}

fn gone(widget: &Widget) -> FieldEdit {
    FieldEdit {
        widget: widget.widget,
        rect: None,
        name: None,
        remove: true,
    }
}

/// The appearance stream a widget is drawn with, and its box.
fn appearance(doc: &Document, widget: ObjectId) -> (ObjectId, Vec<f32>) {
    let ap = doc
        .get_dictionary(widget)
        .unwrap()
        .get(b"AP")
        .and_then(Object::as_dict)
        .expect("an appearance");
    let n = ap.get(b"N").unwrap();
    let id = n.as_reference().expect("a stream of its own");
    let bbox = doc
        .get_object(id)
        .unwrap()
        .as_stream()
        .unwrap()
        .dict
        .get(b"BBox")
        .and_then(Object::as_array)
        .unwrap()
        .iter()
        .map(|v| v.as_float().unwrap())
        .collect();
    (id, bbox)
}

fn bytes(doc: &Document) -> Vec<u8> {
    let mut out = Vec::new();
    doc.clone().save_to(&mut out).expect("serialises");
    out
}

#[test]
fn a_move_changes_where_the_widget_is_and_nothing_else_about_it() {
    let mut f = fixture();
    let name = one(&f.doc, "Name");
    assert_eq!(name.display_rect, [20.0, 20.0, 120.0, 40.0]);
    let drawn = appearance(&f.doc, name.widget);
    apply(&mut f.doc, &[to(&name, [150.0, 10.0, 250.0, 30.0])]).expect("moved");
    let now = one(&f.doc, "Name");
    assert_eq!(now.display_rect, [150.0, 10.0, 250.0, 30.0]);
    // On a page 200 high, 10 to 30 from the top is 170 to 190 from the bottom.
    assert_eq!(now.rect, [150.0, 170.0, 250.0, 190.0]);
    assert_eq!(now.value, Value::Text("Ada".into()));
    assert_eq!(
        appearance(&f.doc, now.widget),
        drawn,
        "the same size is drawn by the same stream"
    );
}

#[test]
fn a_resized_text_field_is_drawn_again_at_its_size_with_its_answer_and_its_line() {
    let mut f = fixture();
    let name = one(&f.doc, "Name");
    let (was, _) = appearance(&f.doc, name.widget);
    apply(&mut f.doc, &[to(&name, [20.0, 20.0, 220.0, 50.0])]).expect("resized");
    let now = one(&f.doc, "Name");
    let (stream, bbox) = appearance(&f.doc, now.widget);
    assert_ne!(stream, was);
    assert_eq!(bbox, [0.0, 0.0, 200.0, 30.0]);
    assert_eq!(now.value, Value::Text("Ada".into()));
    let body = String::from_utf8_lossy(
        &f.doc
            .get_object(stream)
            .unwrap()
            .as_stream()
            .unwrap()
            .content,
    )
    .into_owned();
    assert!(body.contains("<416461> Tj"), "the answer: {body}");
    assert!(body.contains(" re S"), "the line round it: {body}");
}

#[test]
fn a_resized_checkbox_keeps_the_appearance_it_had() {
    let mut f = fixture();
    let agree = one(&f.doc, "Agree");
    let before = f
        .doc
        .get_dictionary(agree.widget)
        .unwrap()
        .get(b"AP")
        .cloned()
        .unwrap();
    apply(&mut f.doc, &[to(&agree, [20.0, 60.0, 44.0, 84.0])]).expect("resized");
    let now = one(&f.doc, "Agree");
    assert_eq!(now.display_rect, [20.0, 60.0, 44.0, 84.0]);
    assert_eq!(
        f.doc
            .get_dictionary(now.widget)
            .unwrap()
            .get(b"AP")
            .unwrap(),
        &before
    );
}

#[test]
fn a_rename_gives_the_field_its_name_and_two_fields_can_swap() {
    let mut f = fixture();
    let name = one(&f.doc, "Name");
    apply(&mut f.doc, &[named(&name, "Full name")]).expect("renamed");
    assert_eq!(one(&f.doc, "Full name").widget, name.widget);
    assert!(widgets(&f.doc, "Name").is_empty());
    // A field with two widgets has one name: both follow.
    let pair = widgets(&f.doc, "Pair");
    apply(&mut f.doc, &[named(&pair[0], "Twins")]).expect("renamed");
    assert_eq!(widgets(&f.doc, "Twins").len(), 2);
    // A swap, which is each taking a name the other is giving up.
    let (notes, agree) = (one(&f.doc, "Notes"), one(&f.doc, "Agree"));
    apply(
        &mut f.doc,
        &[named(&notes, "Agree"), named(&agree, "Notes")],
    )
    .expect("swapped");
    assert_eq!(one(&f.doc, "Agree").widget, notes.widget);
    assert_eq!(one(&f.doc, "Notes").widget, agree.widget);
    // And a name freed by a removal in the same call can be taken.
    let (a, n) = (one(&f.doc, "Agree"), one(&f.doc, "Notes"));
    apply(&mut f.doc, &[gone(&a), named(&n, "Agree")]).expect("taken over");
    assert_eq!(one(&f.doc, "Agree").widget, n.widget);
}

#[test]
fn a_removal_takes_the_widget_off_its_page_and_the_field_out_of_the_form() {
    let mut f = fixture();
    let refs = |doc: &Document, owner: ObjectId, key: &[u8]| -> Vec<ObjectId> {
        let entry = doc.get_dictionary(owner).unwrap().get(key).unwrap();
        let (_, list) = doc.dereference(entry).unwrap();
        list.as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_reference().unwrap())
            .collect()
    };
    let form = |doc: &Document| {
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        doc.get_dictionary(root)
            .unwrap()
            .get(b"AcroForm")
            .unwrap()
            .as_reference()
            .unwrap()
    };
    let notes = one(&f.doc, "Notes");
    apply(&mut f.doc, &[gone(&notes)]).expect("removed");
    assert!(widgets(&f.doc, "Notes").is_empty());
    assert!(!refs(&f.doc, f.pages[0], b"Annots").contains(&notes.widget));
    assert!(!refs(&f.doc, form(&f.doc), b"Fields").contains(&notes.object));
    // The others are where they were.
    assert_eq!(one(&f.doc, "Name").value, Value::Text("Ada".into()));

    // One of two widgets: the field stays, with the other.
    let pair = widgets(&f.doc, "Pair");
    apply(&mut f.doc, &[gone(&pair[0])]).expect("removed");
    let left = widgets(&f.doc, "Pair");
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].widget, pair[1].widget);
    assert!(refs(&f.doc, form(&f.doc), b"Fields").contains(&pair[0].object));
    // The last one takes the field with it.
    apply(&mut f.doc, &[gone(&left[0])]).expect("removed");
    assert!(widgets(&f.doc, "Pair").is_empty());
    assert!(!refs(&f.doc, form(&f.doc), b"Fields").contains(&pair[0].object));
}

#[test]
fn a_form_written_into_the_catalog_loses_a_field_too() {
    let mut f = fixture();
    // The form and its field list, written into the catalog directly.
    let root = f.doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let form = f
        .doc
        .get_dictionary(root)
        .unwrap()
        .get(b"AcroForm")
        .unwrap()
        .as_reference()
        .unwrap();
    let mut held = f.doc.get_dictionary(form).unwrap().clone();
    let fields = held.get(b"Fields").unwrap().clone();
    let (_, list) = f.doc.dereference(&fields).unwrap();
    held.set("Fields", list.clone());
    f.doc
        .get_dictionary_mut(root)
        .unwrap()
        .set("AcroForm", held);
    let notes = one(&f.doc, "Notes");
    apply(&mut f.doc, &[gone(&notes)]).expect("removed");
    assert!(widgets(&f.doc, "Notes").is_empty());
    assert_eq!(scan(&f.doc).unwrap().widgets.len(), 7);
}

#[test]
fn a_change_that_cannot_be_made_changes_nothing() {
    let f = fixture();
    let before = bytes(&f.doc);
    let refused = |edits: &[FieldEdit]| {
        let mut doc = f.doc.clone();
        let why = apply(&mut doc, edits).expect_err("refused");
        assert_eq!(bytes(&doc), before, "{why}");
        why
    };
    let (name, notes, agree) = (
        one(&f.doc, "Name"),
        one(&f.doc, "Notes"),
        one(&f.doc, "Agree"),
    );
    // A change that could be made comes first each time, and must not be.
    let good = to(&notes, [20.0, 150.0, 120.0, 170.0]);
    let with = |bad: FieldEdit| refused(&[good.clone(), bad]);

    let unknown = FieldEdit {
        widget: (9999, 0),
        rect: None,
        name: None,
        remove: true,
    };
    assert!(with(unknown).contains("no longer in this document"));
    assert!(refused(&[good.clone(), good.clone()]).contains("changed twice"));
    assert!(with(gone(&one(&f.doc, "Signed"))).contains("signature field"));
    assert!(with(named(&one(&f.doc, "Signed"), "Other")).contains("signature field"));
    // Too small for its kind: 8 for a text field, 6 for a checkbox.
    assert!(with(to(&name, [20.0, 20.0, 27.0, 40.0])).contains("at least 8 by 8"));
    assert!(with(to(&agree, [20.0, 60.0, 25.0, 72.0])).contains("at least 6 by 6"));
    assert!(with(to(&name, [250.0, 20.0, 350.0, 40.0])).contains("not inside page 1"));
    assert!(with(to(&name, [20.0, f32::NAN, 120.0, 40.0])).contains("not four numbers"));
    assert!(with(to(&one(&f.doc, "Turned"), [20.0, 20.0, 60.0, 40.0])).contains("turned"));
    assert!(with(named(&name, "")).contains("needs a name"));
    assert!(with(named(&name, "a.b")).contains("period"));
    assert!(with(named(&name, "Agree")).contains("another field has this name"));
    assert!(refused(&[named(&name, "New"), named(&notes, "New")])
        .contains("another field has this name"));
    let pair = widgets(&f.doc, "Pair");
    assert!(refused(&[named(&pair[0], "One"), named(&pair[1], "Two")]).contains("two names"));
    // A field that loses one of its two widgets is still there, and so is its name.
    assert!(
        refused(&[gone(&pair[0]), named(&name, "Pair")]).contains("another field has this name")
    );
    // A read-only text field can be moved, and not resized.
    let fixed = one(&f.doc, "Fixed");
    assert!(with(to(&fixed, [150.0, 90.0, 280.0, 120.0])).contains("moved and not resized"));
    let mut doc = f.doc.clone();
    apply(&mut doc, &[to(&fixed, [160.0, 95.0, 260.0, 115.0])]).expect("moved");
    assert_eq!(one(&doc, "Fixed").display_rect, [160.0, 95.0, 260.0, 115.0]);
    // A name on a turned page is a name, and a removal there a removal.
    let mut doc = f.doc.clone();
    let turned = one(&doc, "Turned");
    apply(&mut doc, &[named(&turned, "Sideways")]).expect("renamed");
    let sideways = one(&doc, "Sideways");
    apply(&mut doc, &[gone(&sideways)]).expect("removed");
    // More than one call makes, and none at all.
    let many: Vec<FieldEdit> = (0..=super::MAX_EDITS as u32)
        .map(|n| FieldEdit {
            widget: (n + 5000, 0),
            rect: None,
            name: None,
            remove: true,
        })
        .collect();
    assert!(refused(&many).contains("more than the 1000"));
    let mut doc = f.doc.clone();
    apply(&mut doc, &[]).expect("nothing to do");
    assert_eq!(bytes(&doc), before);
}
