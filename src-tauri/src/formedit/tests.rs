use lopdf::{dictionary, Dictionary, Document, Object, ObjectId};

use super::{apply, FieldEdit, Props};
use crate::formfields::{add, Kind, NewField};
use crate::forms::{scan, write, Align, Change, Value, Widget};

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
        text_size: None,
        default_value: None,
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
        dictionary! {
            "FT" => "Sig", "T" => Object::string_literal("Signed"),
            "V" => dictionary! { "Type" => "Sig" },
        },
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
        value: None,
        widget: widget.widget,
        rect: Some(rect),
        name: None,
        remove: false,
        props: Default::default(),
    }
}

fn named(widget: &Widget, name: &str) -> FieldEdit {
    FieldEdit {
        value: None,
        widget: widget.widget,
        rect: None,
        name: Some(name.into()),
        remove: false,
        props: Default::default(),
    }
}

fn gone(widget: &Widget) -> FieldEdit {
    FieldEdit {
        value: None,
        widget: widget.widget,
        rect: None,
        name: None,
        remove: true,
        props: Default::default(),
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
fn a_rename_to_a_name_outside_ascii_reads_back_as_that_name() {
    // The control: a name inside ASCII reads the same in either encoding, so
    // it says nothing about which one was written.
    let mut f = fixture();
    let name = one(&f.doc, "Name");
    apply(&mut f.doc, &[named(&name, "Groesse")]).expect("renamed");
    assert_eq!(one(&f.doc, "Groesse").widget, name.widget);

    // A text string with no byte-order mark is PDFDocEncoding to every
    // reader, so UTF-8 bytes written bare come back as two letters for each
    // of these.
    apply(&mut f.doc, &[named(&name, "Gr\u{f6}\u{df}e")]).expect("renamed");
    assert_eq!(one(&f.doc, "Gr\u{f6}\u{df}e").widget, name.widget);
    assert!(widgets(&f.doc, "Groesse").is_empty());
    // And the name is taken, for the check that reads the file's own string.
    let notes = one(&f.doc, "Notes");
    let refused = apply(&mut f.doc, &[named(&notes, "Gr\u{f6}\u{df}e")]).unwrap_err();
    assert!(refused.contains("another field has this name"), "{refused}");
    assert_eq!(one(&f.doc, "Notes").widget, notes.widget);
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
        value: None,
        widget: (9999, 0),
        rect: None,
        name: None,
        remove: true,
        props: Default::default(),
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
    // Off a turned page is off it as it is displayed: 200 across, 300 down.
    assert!(with(to(&one(&f.doc, "Turned"), [150.0, 20.0, 250.0, 40.0]))
        .contains("which is 200 by 300"));
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
            value: None,
            widget: (n + 5000, 0),
            rect: None,
            name: None,
            remove: true,
            props: Default::default(),
        })
        .collect();
    assert!(refused(&many).contains("more than the 1000"));
    let mut doc = f.doc.clone();
    apply(&mut doc, &[]).expect("nothing to do");
    assert_eq!(bytes(&doc), before);
}

fn with(widget: &Widget, props: Props) -> FieldEdit {
    FieldEdit {
        value: None,
        widget: widget.widget,
        rect: None,
        name: None,
        remove: false,
        props,
    }
}

/// The body of the stream a widget is drawn with.
fn drawn(doc: &Document, widget: ObjectId) -> String {
    let (stream, _) = appearance(doc, widget);
    String::from_utf8_lossy(&doc.get_object(stream).unwrap().as_stream().unwrap().content)
        .into_owned()
}

/// A dropdown `Colour` offering Red, Green and Blue, holding Green.
fn with_colours(f: &mut Fixture) -> Widget {
    add(
        &mut f.doc,
        &[NewField {
            text_size: None,
            default_value: None,
            options: vec!["Red".into(), "Green".into(), "Blue".into()],
            name: "Colour".into(),
            kind: Kind::Dropdown,
            page: 0,
            rect: [20.0, 120.0, 100.0, 20.0],
            tooltip: None,
            required: false,
            max_length: None,
            border: false,
        }],
    )
    .expect("a dropdown");
    let colour = one(&f.doc, "Colour");
    write(
        &mut f.doc,
        &[Change {
            object: colour.object,
            value: Value::Selection(vec![1]),
        }],
    )
    .expect("chosen");
    one(&f.doc, "Colour")
}

fn labels(widget: &Widget) -> Vec<String> {
    match &widget.control {
        crate::forms::Control::Choice { options, .. } => {
            options.iter().map(|o| o.label.clone()).collect()
        }
        _ => Vec::new(),
    }
}

#[test]
fn a_tooltip_and_the_two_flags_are_written_on_the_field_and_draw_nothing() {
    let mut f = fixture();
    let name = one(&f.doc, "Name");
    assert!(name.tooltip.is_empty() && !name.required && !name.read_only);
    let was = appearance(&f.doc, name.widget);
    apply(
        &mut f.doc,
        &[with(
            &name,
            Props {
                tooltip: Some("Your full name".into()),
                required: Some(true),
                read_only: Some(true),
                ..Props::default()
            },
        )],
    )
    .expect("set");
    let now = one(&f.doc, "Name");
    assert_eq!(now.tooltip, "Your full name");
    assert!(now.required && now.read_only);
    assert_eq!(now.reason.as_deref(), Some(crate::forms::READ_ONLY));
    assert_eq!(now.value, Value::Text("Ada".into()));
    assert_eq!(appearance(&f.doc, now.widget), was, "nothing is redrawn");
    // Each is taken off again, one at a time: the other flag stays.
    apply(
        &mut f.doc,
        &[with(
            &now,
            Props {
                tooltip: Some(String::new()),
                read_only: Some(false),
                ..Props::default()
            },
        )],
    )
    .expect("lifted");
    let now = one(&f.doc, "Name");
    assert!(now.tooltip.is_empty() && now.required && !now.read_only);
    assert!(!f.doc.get_dictionary(now.object).unwrap().has(b"TU"));
    apply(
        &mut f.doc,
        &[with(
            &now,
            Props {
                required: Some(false),
                ..Props::default()
            },
        )],
    )
    .expect("not required");
    assert!(!one(&f.doc, "Name").required);
}

#[test]
fn a_limit_is_set_and_lifted_and_refused_when_the_answer_is_longer() {
    let mut f = fixture();
    let name = one(&f.doc, "Name");
    let most = |n: u32| Props {
        max_length: Some(n),
        ..Props::default()
    };
    let before = bytes(&f.doc);
    let why = apply(&mut f.doc, &[with(&name, most(2))]).expect_err("Ada is three");
    assert!(
        why.contains("holds 3 characters") && why.contains("the 2 it"),
        "{why}"
    );
    assert_eq!(bytes(&f.doc), before);
    apply(&mut f.doc, &[with(&name, most(3))]).expect("exactly as long");
    assert_eq!(one(&f.doc, "Name").max_length, Some(3));
    apply(&mut f.doc, &[with(&name, most(0))]).expect("lifted");
    assert_eq!(one(&f.doc, "Name").max_length, None);
    assert!(!f.doc.get_dictionary(name.object).unwrap().has(b"MaxLen"));
}

#[test]
fn an_alignment_draws_the_field_again_with_its_answer_at_that_side() {
    let mut f = fixture();
    let name = one(&f.doc, "Name");
    assert_eq!(name.align, Align::Left);
    let (was, _) = appearance(&f.doc, name.widget);
    let aligned = |to: Align| Props {
        align: Some(to),
        ..Props::default()
    };
    // The alignment it has already: written, and nothing drawn.
    apply(&mut f.doc, &[with(&name, aligned(Align::Left))]).expect("left");
    assert_eq!(appearance(&f.doc, name.widget).0, was);
    // A field 100 wide, its answer at twelve points.
    let advance = crate::textbox::advance("Ada", 12.0);
    for (to, x) in [
        (Align::Right, 100.0 - 2.0 - advance),
        (Align::Center, (100.0 - advance) / 2.0),
        (Align::Left, 2.0),
    ] {
        let held = one(&f.doc, "Name");
        apply(&mut f.doc, &[with(&held, aligned(to))]).expect("aligned");
        let now = one(&f.doc, "Name");
        assert_eq!(now.align, to);
        assert_eq!(now.value, Value::Text("Ada".into()));
        let body = drawn(&f.doc, now.widget);
        assert!(
            body.contains(&format!("1 0 0 1 {x} ")) && body.contains("<416461> Tj"),
            "{to:?} at {x}: {body}"
        );
        assert!(body.contains(" re S"), "its line is kept: {body}");
    }
}

#[test]
fn a_field_on_a_turned_page_is_moved_and_resized_where_a_reader_sees_it() {
    let mut f = fixture();
    let turned = one(&f.doc, "Turned");
    // The fixture's widget says nothing of a turn; one made for a turned
    // page does, and that is the one a resize has to draw the right way up.
    f.doc
        .get_dictionary_mut(turned.widget)
        .unwrap()
        .set("MK", dictionary! { "R" => 90 });
    // A turn is whole quarters, counted round; anything else is none.
    for (degrees, turns) in [
        (135_i64, 0_u8),
        (-90, 3),
        (450, 1),
        (360, 0),
        (180, 2),
        (90, 1),
    ] {
        f.doc
            .get_dictionary_mut(turned.widget)
            .unwrap()
            .set("MK", dictionary! { "R" => degrees });
        assert_eq!(one(&f.doc, "Turned").turns, turns, "{degrees}");
    }
    let turned = one(&f.doc, "Turned");
    assert_eq!(turned.turns, 1);
    // Moved: where it was asked to go, as the page is displayed.
    let asked = [30.0, 50.0, 130.0, 70.0];
    apply(&mut f.doc, &[to(&turned, asked)]).expect("moved and resized");
    let now = one(&f.doc, "Turned");
    assert_eq!(now.display_rect, asked);
    // In the page's own space it lies on its side: 20 wide, 100 high.
    assert_eq!(
        (now.rect[2] - now.rect[0], now.rect[3] - now.rect[1]),
        (20.0, 100.0)
    );
    // Drawn again 100 by 20, turned by the matrix.
    let (stream, _) = appearance(&f.doc, now.widget);
    let dict = &f.doc.get_object(stream).unwrap().as_stream().unwrap().dict;
    let numbers = |key: &[u8]| -> Vec<f32> {
        dict.get(key)
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_float().unwrap())
            .collect()
    };
    assert_eq!(numbers(b"BBox"), [0.0, 0.0, 100.0, 20.0]);
    assert_eq!(numbers(b"Matrix"), [0.0, 1.0, -1.0, 0.0, 20.0, 0.0]);
}

/// The default appearance a field declares for itself.
fn declared(doc: &Document, field: ObjectId) -> String {
    let dict = doc.get_dictionary(field).unwrap();
    String::from_utf8_lossy(dict.get(b"DA").unwrap().as_str().unwrap()).into_owned()
}

#[test]
fn a_text_size_is_declared_on_the_field_and_its_answer_is_drawn_at_it() {
    let mut f = fixture();
    let name = one(&f.doc, "Name");
    assert_eq!(name.text_size, None);
    assert!(drawn(&f.doc, name.widget).contains("/F0 12 Tf"));
    let (was, _) = appearance(&f.doc, name.widget);
    let sized = |size: f32| Props {
        text_size: Some(size),
        ..Props::default()
    };
    // The size it has already, said as nought: written, and nothing drawn.
    apply(&mut f.doc, &[with(&name, sized(0.0))]).expect("as it was");
    assert_eq!(appearance(&f.doc, name.widget).0, was);
    // Nine points: declared, read back, and the answer drawn at it.
    apply(&mut f.doc, &[with(&name, sized(9.0))]).expect("nine");
    let now = one(&f.doc, "Name");
    assert_eq!(now.text_size, Some(9.0));
    assert_eq!(declared(&f.doc, now.object), "/Helv 9 Tf 0 g");
    assert_eq!(now.value, Value::Text("Ada".into()));
    let body = drawn(&f.doc, now.widget);
    assert!(
        body.contains("/F0 9 Tf") && body.contains("<416461> Tj"),
        "{body}"
    );
    assert!(body.contains(" re S"), "its line is kept: {body}");
    // The same size again draws nothing.
    let (nine, _) = appearance(&f.doc, now.widget);
    apply(&mut f.doc, &[with(&now, sized(9.0))]).expect("nine again");
    assert_eq!(appearance(&f.doc, now.widget).0, nine);
    // A size the field is too low for is declared, and drawn at what fits:
    // a field twenty high holds a line of fifteen points.
    apply(&mut f.doc, &[with(&now, sized(40.0))]).expect("forty");
    assert_eq!(one(&f.doc, "Name").text_size, Some(40.0));
    assert!(drawn(&f.doc, now.widget).contains("/F0 15 Tf"));
    // Nought is the size that follows the field again.
    apply(&mut f.doc, &[with(&now, sized(0.0))]).expect("nought");
    assert_eq!(one(&f.doc, "Name").text_size, None);
    assert_eq!(declared(&f.doc, now.object), "/Helv 0 Tf 0 g");
    assert!(drawn(&f.doc, now.widget).contains("/F0 12 Tf"));
    // The font and the colour another program declared are kept.
    f.doc
        .get_dictionary_mut(now.object)
        .unwrap()
        .set("DA", Object::string_literal("/TiRo 11 Tf 0 0 1 rg"));
    assert_eq!(one(&f.doc, "Name").text_size, Some(11.0));
    apply(&mut f.doc, &[with(&now, sized(8.5))]).expect("eight and a half");
    assert_eq!(declared(&f.doc, now.object), "/TiRo 8.5 Tf 0 0 1 rg");
    // A dropdown has a text size too.
    let colour = with_colours(&mut f);
    apply(&mut f.doc, &[with(&colour, sized(7.0))]).expect("a dropdown");
    assert_eq!(one(&f.doc, "Colour").text_size, Some(7.0));
    assert!(drawn(&f.doc, colour.widget).contains("/F0 7 Tf"));
}

#[test]
fn a_field_with_no_text_size_of_its_own_has_the_forms() {
    let mut f = fixture();
    let catalog = f.doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let form = f
        .doc
        .get_dictionary(catalog)
        .unwrap()
        .get(b"AcroForm")
        .and_then(Object::as_reference)
        .unwrap();
    let name = one(&f.doc, "Name");
    f.doc
        .get_dictionary_mut(form)
        .unwrap()
        .set("DA", Object::string_literal("/Helv 10 Tf 0 g"));
    // Its own says nought, which is its own answer.
    assert_eq!(one(&f.doc, "Name").text_size, None);
    f.doc.get_dictionary_mut(name.object).unwrap().remove(b"DA");
    assert_eq!(one(&f.doc, "Name").text_size, Some(10.0));
    // Set from there, the field gets one of its own and the form keeps its.
    let sized = Props {
        text_size: Some(6.0),
        ..Props::default()
    };
    apply(&mut f.doc, &[with(&name, sized)]).expect("six");
    assert_eq!(declared(&f.doc, name.object), "/Helv 6 Tf 0 g");
    assert_eq!(declared(&f.doc, form), "/Helv 10 Tf 0 g");
    // A checkbox has no text and so no size.
    assert_eq!(one(&f.doc, "Agree").text_size, None);
}

#[test]
fn a_default_value_is_kept_for_a_reset_and_answers_a_field_that_holds_nothing() {
    let mut f = fixture();
    let starting = |default: &str| Props {
        default_value: Some(default.into()),
        ..Props::default()
    };
    // A field that holds nothing is answered with it.
    let notes = one(&f.doc, "Notes");
    assert_eq!(notes.value, Value::Text(String::new()));
    assert_eq!(notes.default_value, "");
    apply(&mut f.doc, &[with(&notes, starting("n/a"))]).expect("a default");
    let now = one(&f.doc, "Notes");
    assert_eq!(now.default_value, "n/a");
    assert_eq!(now.value, Value::Text("n/a".into()));
    assert!(drawn(&f.doc, now.widget).contains("<6e2f61> Tj"));
    // A field that holds an answer keeps it, and is not drawn again.
    let name = one(&f.doc, "Name");
    let (was, _) = appearance(&f.doc, name.widget);
    apply(&mut f.doc, &[with(&name, starting("Bob"))]).expect("a default");
    let now = one(&f.doc, "Name");
    assert_eq!(now.default_value, "Bob");
    assert_eq!(now.value, Value::Text("Ada".into()));
    assert_eq!(appearance(&f.doc, name.widget).0, was);
    // Empty takes the default off and leaves the answer.
    apply(&mut f.doc, &[with(&now, starting(""))]).expect("off");
    let now = one(&f.doc, "Name");
    assert_eq!(now.default_value, "");
    assert_eq!(now.value, Value::Text("Ada".into()));
    assert!(!f.doc.get_dictionary(now.object).unwrap().has(b"DV"));
    // Within the limit the same change sets.
    let limited = Props {
        max_length: Some(4),
        ..starting("four")
    };
    apply(&mut f.doc, &[with(&now, limited)]).expect("within the limit");
    assert_eq!(one(&f.doc, "Name").default_value, "four");
}

#[test]
fn a_read_only_field_is_aligned_and_is_read_only_afterwards() {
    let mut f = fixture();
    let fixed = one(&f.doc, "Fixed");
    assert!(fixed.read_only);
    apply(
        &mut f.doc,
        &[with(
            &fixed,
            Props {
                align: Some(Align::Center),
                ..Props::default()
            },
        )],
    )
    .expect("aligned");
    let now = one(&f.doc, "Fixed");
    assert_eq!(now.align, Align::Center);
    assert!(now.read_only, "the flag is put back");
    // And one made read-only in the same change is drawn first.
    let name = one(&f.doc, "Name");
    apply(
        &mut f.doc,
        &[with(
            &name,
            Props {
                align: Some(Align::Right),
                read_only: Some(true),
                ..Props::default()
            },
        )],
    )
    .expect("both");
    let now = one(&f.doc, "Name");
    assert!(now.read_only && now.align == Align::Right);
    assert!(drawn(&f.doc, now.widget).contains("<416461> Tj"));
}

#[test]
fn new_choices_keep_what_was_chosen_where_they_still_have_it() {
    let mut f = fixture();
    let colour = with_colours(&mut f);
    assert_eq!(colour.value, Value::Selection(vec![1]));
    let offering = |options: &[&str]| Props {
        options: Some(options.iter().map(|o| (*o).to_string()).collect()),
        ..Props::default()
    };
    apply(
        &mut f.doc,
        &[with(&colour, offering(&["Blue", "Teal", "Green"]))],
    )
    .expect("new choices");
    let now = one(&f.doc, "Colour");
    assert_eq!(labels(&now), ["Blue", "Teal", "Green"]);
    assert_eq!(
        now.value,
        Value::Selection(vec![2]),
        "Green, where it now is"
    );
    // "Green" in the hexadecimal of the font's encoding.
    assert!(drawn(&f.doc, now.widget).contains("<477265656e> Tj"));
    // Choices without it: the field holds nothing and shows nothing.
    apply(&mut f.doc, &[with(&now, offering(&["Black", "White"]))]).expect("others");
    let now = one(&f.doc, "Colour");
    assert_eq!(labels(&now), ["Black", "White"]);
    assert_eq!(now.value, Value::Selection(Vec::new()));
    let field = f.doc.get_dictionary(now.object).unwrap();
    assert!(
        !field.has(b"V"),
        "no answer is left naming a choice that is gone"
    );
    assert!(!drawn(&f.doc, now.widget).contains("477265656e"));
}

#[test]
fn a_choice_the_field_already_offered_keeps_the_value_it_exports() {
    let mut f = fixture();
    let colour = with_colours(&mut f);
    let pair = |export: &str, label: &str| {
        Object::Array(vec![
            crate::forms::pdf_string(export),
            crate::forms::pdf_string(label),
        ])
    };
    {
        let field = f.doc.get_dictionary_mut(colour.object).unwrap();
        field.set(
            "Opt",
            vec![pair("r", "Red"), pair("g", "Green"), pair("b", "Blue")],
        );
        field.set("V", crate::forms::pdf_string("g"));
        field.remove(b"I");
    }
    let colour = one(&f.doc, "Colour");
    assert_eq!(colour.value, Value::Selection(vec![1]));
    apply(
        &mut f.doc,
        &[with(
            &colour,
            Props {
                options: Some(vec!["Teal".into(), "Green".into()]),
                ..Props::default()
            },
        )],
    )
    .expect("new choices");
    let now = one(&f.doc, "Colour");
    let crate::forms::Control::Choice { options, .. } = &now.control else {
        panic!("a choice");
    };
    let read: Vec<(&str, &str)> = options
        .iter()
        .map(|o| (o.export.as_str(), o.label.as_str()))
        .collect();
    assert_eq!(read, [("Teal", "Teal"), ("g", "Green")]);
    assert_eq!(now.value, Value::Selection(vec![1]));
    let held = f.doc.get_dictionary(now.object).unwrap().get(b"V").unwrap();
    assert_eq!(
        crate::annots::decode_text_string(held.as_str().unwrap()),
        "g"
    );
}

#[test]
fn a_property_the_field_cannot_have_is_refused_and_nothing_is_written() {
    let mut f = fixture();
    with_colours(&mut f);
    let before = bytes(&f.doc);
    let mut refused = |name: &str, at: usize, props: Props| {
        let widget = widgets(&f.doc, name).remove(at);
        let why = apply(&mut f.doc, &[with(&widget, props)]).expect_err("refused");
        assert_eq!(bytes(&f.doc), before, "{why}");
        why
    };
    let most = Props {
        max_length: Some(5),
        ..Props::default()
    };
    let centred = Props {
        align: Some(Align::Center),
        ..Props::default()
    };
    let offering = |options: &[&str]| Props {
        options: Some(options.iter().map(|o| (*o).to_string()).collect()),
        ..Props::default()
    };
    let sized = |size: f32| Props {
        text_size: Some(size),
        ..Props::default()
    };
    let starting = |default: &str| Props {
        default_value: Some(default.into()),
        ..Props::default()
    };
    assert!(refused("Agree", 0, sized(9.0)).contains("has a text size"));
    for size in [3.9, 144.5, -9.0, f32::NAN, f32::INFINITY] {
        assert!(
            refused("Name", 0, sized(size)).contains("a text size is 4 to 144 points"),
            "{size}"
        );
    }
    assert!(refused("Agree", 0, starting("x")).contains("only a text field has a default"));
    assert!(refused("Colour", 0, starting("Red")).contains("only a text field has a default"));
    assert!(refused("Name", 0, starting("one\ntwo")).contains("takes one line"));
    assert!(refused("Name", 0, starting("bell\u{7}")).contains("control character"));
    // Longer than the limit the same change sets, and than no limit never.
    let limited = Props {
        max_length: Some(3),
        ..starting("four")
    };
    assert!(refused("Name", 0, limited).contains("more than the field takes"));
    // An answer that would not be drawn visibly is refused as an answer is.
    let long = "a long default value that a field of a hundred points cannot show";
    assert!(refused("Notes", 0, starting(long)).contains("does not fit visibly"));
    assert!(refused("Agree", 0, most.clone()).contains("only a text field has a most"));
    assert!(refused("Colour", 0, most).contains("only a text field has a most"));
    assert!(refused("Agree", 0, centred).contains("has text to align"));
    assert!(refused("Name", 0, offering(&["A"])).contains("only a field of choices"));
    assert!(refused("Colour", 0, offering(&[])).contains("at least one choice"));
    assert!(refused("Colour", 0, offering(&["A", "A"])).contains("there twice"));
    assert!(refused(
        "Name",
        0,
        Props {
            tooltip: Some("x".repeat(super::MAX_TOOLTIP + 1)),
            ..Props::default()
        }
    )
    .contains("at most 1024"));
    assert!(refused(
        "Name",
        0,
        Props {
            max_length: Some(super::MAX_LENGTH + 1),
            ..Props::default()
        }
    )
    .contains("up to 16384"));
    assert!(refused(
        "Signed",
        0,
        Props {
            required: Some(true),
            ..Props::default()
        }
    )
    .contains("signature field"));
}

#[test]
fn a_field_with_two_widgets_has_one_set_of_properties() {
    let mut f = fixture();
    let pair = widgets(&f.doc, "Pair");
    let tip = |text: &str| Props {
        tooltip: Some(text.into()),
        ..Props::default()
    };
    let before = bytes(&f.doc);
    let why = apply(
        &mut f.doc,
        &[with(&pair[0], tip("one")), with(&pair[1], tip("two"))],
    )
    .expect_err("two tooltips");
    assert!(why.contains("two sets of properties"), "{why}");
    assert_eq!(bytes(&f.doc), before);
    // The same for both is one change, and both widgets read it.
    apply(
        &mut f.doc,
        &[with(&pair[0], tip("one")), with(&pair[1], tip("one"))],
    )
    .expect("the same");
    assert!(widgets(&f.doc, "Pair").iter().all(|w| w.tooltip == "one"));
    // Set under a widget that is removed in the same change: the field stays,
    // with its other widget, and has the property.
    let pair = widgets(&f.doc, "Pair");
    let mut going = with(&pair[0], tip("kept"));
    going.remove = true;
    apply(&mut f.doc, &[going]).expect("removed, and set");
    assert_eq!(one(&f.doc, "Pair").tooltip, "kept");
    // And when the whole field goes, its properties go with it.
    let name = one(&f.doc, "Name");
    let mut going = with(&name, tip("lost"));
    going.remove = true;
    apply(&mut f.doc, &[going]).expect("removed");
    assert!(widgets(&f.doc, "Name").is_empty());
}

#[test]
fn properties_lay_over_each_other_part_by_part() {
    let mut held = Props {
        tooltip: Some("first".into()),
        required: Some(true),
        ..Props::default()
    };
    assert!(Props::default().is_empty() && !held.is_empty());
    held.merge(&Props {
        tooltip: Some(String::new()),
        max_length: Some(4),
        align: Some(Align::Right),
        options: Some(vec!["A".into()]),
        read_only: Some(false),
        text_size: Some(9.0),
        default_value: Some("n/a".into()),
        ..Props::default()
    });
    assert_eq!(
        held,
        Props {
            tooltip: Some(String::new()),
            required: Some(true),
            read_only: Some(false),
            max_length: Some(4),
            align: Some(Align::Right),
            options: Some(vec!["A".into()]),
            text_size: Some(9.0),
            default_value: Some("n/a".into()),
        }
    );
    // A later change that names nothing leaves all of it.
    let whole = held.clone();
    held.merge(&Props::default());
    assert_eq!(held, whole);
    assert!(Props {
        tooltip: Some("a\tb".into()),
        ..Props::default()
    }
    .problem("f")
    .is_some_and(|why| why.contains("control character")));
    assert!(Props {
        tooltip: Some("two\nlines".into()),
        ..Props::default()
    }
    .problem("f")
    .is_none());
}

#[test]
fn a_field_tpdf_cannot_draw_keeps_its_alignment_and_takes_the_rest() {
    let mut f = fixture();
    let notes = one(&f.doc, "Notes");
    // A comb field: one tpdf does not fill, so one it cannot draw.
    f.doc
        .get_dictionary_mut(notes.object)
        .unwrap()
        .set("Ff", 1_i64 << 24);
    let notes = one(&f.doc, "Notes");
    assert_eq!(notes.reason.as_deref(), Some(crate::forms::COMB));
    let before = bytes(&f.doc);
    let why = apply(
        &mut f.doc,
        &[with(
            &notes,
            Props {
                align: Some(Align::Right),
                ..Props::default()
            },
        )],
    )
    .expect_err("not drawn");
    assert!(why.contains("keeps its alignment, text size"), "{why}");
    assert_eq!(bytes(&f.doc), before);
    apply(
        &mut f.doc,
        &[with(
            &notes,
            Props {
                tooltip: Some("In boxes".into()),
                ..Props::default()
            },
        )],
    )
    .expect("a tooltip draws nothing");
    assert_eq!(one(&f.doc, "Notes").tooltip, "In boxes");
}

#[test]
fn a_field_with_no_alignment_of_its_own_has_the_forms() {
    let mut f = fixture();
    assert_eq!(one(&f.doc, "Name").align, Align::Left);
    let catalog = f.doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let form = f
        .doc
        .get_dictionary(catalog)
        .unwrap()
        .get(b"AcroForm")
        .and_then(Object::as_reference)
        .unwrap();
    f.doc.get_dictionary_mut(form).unwrap().set("Q", 2);
    assert_eq!(one(&f.doc, "Name").align, Align::Right);
    let name = one(&f.doc, "Name").object;
    f.doc.get_dictionary_mut(name).unwrap().set("Q", 1);
    assert_eq!(one(&f.doc, "Name").align, Align::Center);
    assert_eq!(one(&f.doc, "Notes").align, Align::Right);
}

/// The fixture with a group `Pay` of three radio buttons, `Card`, `Cash` and
/// `Later`, the second one chosen.
fn with_buttons() -> (Document, Vec<Widget>) {
    let mut doc = fixture().doc;
    let button = |value: &str, top: f64| NewField {
        text_size: None,
        default_value: None,
        options: vec![value.into()],
        name: "Pay".into(),
        kind: Kind::Radio,
        page: 0,
        rect: [220.0, top, 12.0, 12.0],
        tooltip: None,
        required: false,
        max_length: None,
        border: false,
    };
    add(
        &mut doc,
        &[
            button("Card", 20.0),
            button("Cash", 40.0),
            button("Later", 60.0),
        ],
    )
    .expect("added");
    let group = widgets(&doc, "Pay")[0].object;
    write(
        &mut doc,
        &[Change {
            object: group,
            value: Value::Selection(vec![1]),
        }],
    )
    .expect("chosen");
    let buttons = widgets(&doc, "Pay");
    (doc, buttons)
}

fn valued(widget: &Widget, value: &str) -> FieldEdit {
    FieldEdit {
        value: Some(value.into()),
        widget: widget.widget,
        rect: None,
        name: None,
        remove: false,
        props: Default::default(),
    }
}

/// The names of the states a button has for one of its looks.
fn looks(doc: &Document, widget: ObjectId, look: &[u8]) -> Vec<String> {
    let ap = doc.get_dictionary(widget).unwrap().get(b"AP").unwrap();
    let ap = doc.dereference(ap).unwrap().1.as_dict().unwrap();
    let Ok(states) = ap.get(look) else {
        return Vec::new();
    };
    let states = doc.dereference(states).unwrap().1.as_dict().unwrap();
    let mut names: Vec<String> = states
        .iter()
        .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
        .collect();
    names.sort();
    names
}

fn name_at(doc: &Document, id: ObjectId, key: &[u8]) -> String {
    doc.get_dictionary(id)
        .unwrap()
        .get(key)
        .and_then(Object::as_name)
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .unwrap_or_default()
}

#[test]
fn a_radio_buttons_value_is_renamed_in_its_looks_and_in_what_the_group_holds() {
    let (mut doc, buttons) = with_buttons();
    let states = |doc: &Document| match &widgets(doc, "Pay")[0].control {
        crate::forms::Control::Radio { states, .. } => states
            .iter()
            .map(|state| String::from_utf8_lossy(state).into_owned())
            .collect::<Vec<_>>(),
        other => panic!("{other:?}"),
    };
    assert_eq!(states(&doc), ["Card", "Cash", "Later"]);
    assert_eq!(name_at(&doc, buttons[0].object, b"V"), "Cash");

    // A second look for the chosen button, held in place where the first is
    // an object of its own or the other way round: both are renamed.
    {
        let ap = doc
            .get_dictionary(buttons[1].widget)
            .unwrap()
            .get(b"AP")
            .unwrap();
        let ap = doc.dereference(ap).unwrap().1.as_dict().unwrap();
        let normal = doc.dereference(ap.get(b"N").unwrap()).unwrap().1.clone();
        let held_in_place = matches!(ap.get(b"N"), Ok(Object::Dictionary(_)));
        let down = if held_in_place {
            Object::Reference(doc.add_object(normal))
        } else {
            normal
        };
        let ap = match doc
            .get_dictionary(buttons[1].widget)
            .unwrap()
            .get(b"AP")
            .unwrap()
            .clone()
        {
            Object::Reference(id) => doc.get_dictionary_mut(id).unwrap(),
            _ => doc
                .get_dictionary_mut(buttons[1].widget)
                .unwrap()
                .get_mut(b"AP")
                .and_then(Object::as_dict_mut)
                .unwrap(),
        };
        ap.set("D", down);
    }
    assert_eq!(looks(&doc, buttons[1].widget, b"D"), ["Cash", "Off"]);
    // What the group holds after a reset is that button too.
    doc.get_dictionary_mut(buttons[0].object)
        .unwrap()
        .set("DV", Object::Name(b"Cash".to_vec()));

    // The chosen button: its looks, what it shows, and what the group holds.
    // A button that is not chosen: its looks, and nothing else.
    apply(
        &mut doc,
        &[
            valued(&buttons[1], "Bank transfer"),
            valued(&buttons[2], "Never"),
        ],
    )
    .expect("revalued");
    assert_eq!(states(&doc), ["Card", "Bank transfer", "Never"]);
    assert_eq!(
        looks(&doc, buttons[1].widget, b"N"),
        ["Bank transfer", "Off"]
    );
    assert_eq!(
        looks(&doc, buttons[1].widget, b"D"),
        ["Bank transfer", "Off"]
    );
    assert_eq!(name_at(&doc, buttons[1].widget, b"AS"), "Bank transfer");
    assert_eq!(name_at(&doc, buttons[0].object, b"V"), "Bank transfer");
    assert_eq!(name_at(&doc, buttons[0].object, b"DV"), "Bank transfer");
    assert_eq!(looks(&doc, buttons[2].widget, b"N"), ["Never", "Off"]);
    assert_eq!(name_at(&doc, buttons[2].widget, b"AS"), "Off");
    assert_eq!(looks(&doc, buttons[0].widget, b"N"), ["Card", "Off"]);
    // The group still holds the second button, and takes another answer.
    let after = widgets(&doc, "Pay");
    assert_eq!(after[0].value, Value::Selection(vec![1]));
    assert!(after.iter().all(|w| w.reason.is_none()));
    write(
        &mut doc,
        &[Change {
            object: buttons[0].object,
            value: Value::Selection(vec![2]),
        }],
    )
    .expect("answered");
    assert_eq!(name_at(&doc, buttons[0].object, b"V"), "Never");

    // The value it has already is no change at all.
    let before = bytes(&doc);
    apply(&mut doc, &[valued(&buttons[0], "Card")]).expect("nothing to do");
    assert_eq!(bytes(&doc), before);
}

#[test]
fn a_value_a_button_cannot_take_is_refused_and_nothing_is_written() {
    let (doc, buttons) = with_buttons();
    let refused = |doc: &Document, edits: &[FieldEdit]| {
        let mut copy = doc.clone();
        let why = apply(&mut copy, edits).expect_err("refused");
        assert_eq!(bytes(&copy), bytes(doc), "{why}");
        why
    };
    assert!(refused(&doc, &[valued(&buttons[0], "Cash")])
        .contains("already has a button with the value `Cash`"));
    assert!(refused(&doc, &[valued(&buttons[0], "Off")]).contains("`Off` is what a group holds"));
    assert!(refused(&doc, &[valued(&buttons[0], "")]).contains("Pay"));
    let name = one(&doc, "Name");
    assert!(refused(&doc, &[valued(&name, "x")]).contains("only a radio button has a value"));
    // With another change beside it, neither is made.
    assert!(
        refused(&doc, &[named(&name, "Other"), valued(&buttons[0], "Cash")])
            .contains("already has")
    );

    // A group that lists what its buttons export keeps its state names.
    let mut listed = doc.clone();
    listed.get_dictionary_mut(buttons[0].object).unwrap().set(
        "Opt",
        vec![
            Object::string_literal("a"),
            Object::string_literal("b"),
            Object::string_literal("c"),
        ],
    );
    assert!(
        refused(&listed, &[valued(&buttons[0], "New")]).contains("lists what its buttons export")
    );

    // Two buttons with one value are chosen together, and stay so.
    let mut shared = doc.clone();
    for look in [b"N".as_slice(), b"D"] {
        let ap = shared
            .get_dictionary(buttons[2].widget)
            .unwrap()
            .get(b"AP")
            .unwrap()
            .clone();
        let ap = match ap {
            Object::Reference(id) => shared.get_dictionary_mut(id).unwrap(),
            _ => shared
                .get_dictionary_mut(buttons[2].widget)
                .unwrap()
                .get_mut(b"AP")
                .and_then(Object::as_dict_mut)
                .unwrap(),
        };
        let Ok(states) = ap.get_mut(look) else {
            continue;
        };
        let states = match states {
            Object::Reference(id) => {
                let id = *id;
                shared.get_dictionary_mut(id).unwrap()
            }
            other => other.as_dict_mut().unwrap(),
        };
        let drawing = states.remove(b"Later").unwrap();
        states.set("Cash", drawing);
    }
    assert!(refused(&shared, &[valued(&buttons[1], "New")]).contains("chosen together"));
    // The value such a button has already is still no change, and no refusal.
    let before = bytes(&shared);
    apply(&mut shared, &[valued(&buttons[1], "Cash")]).expect("nothing to do");
    assert_eq!(bytes(&shared), before);

    // A removed button's value is not looked at.
    let mut copy = doc.clone();
    let mut going = valued(&buttons[0], "Cash");
    going.remove = true;
    apply(&mut copy, &[going]).expect("removed");
    assert_eq!(widgets(&copy, "Pay").len(), 2);
}

#[test]
fn an_empty_signature_field_is_moved_resized_renamed_and_removed_like_any_field() {
    let mut doc = fixture().doc;
    add(
        &mut doc,
        &[NewField {
            text_size: None,
            default_value: None,
            options: Vec::new(),
            name: "Approve".into(),
            kind: Kind::Signature,
            page: 0,
            rect: [20.0, 120.0, 100.0, 30.0],
            tooltip: None,
            required: false,
            max_length: None,
            border: true,
        }],
    )
    .expect("added");
    let empty = one(&doc, "Approve");
    assert!(matches!(
        empty.control,
        crate::forms::Control::Signature { signed: false }
    ));
    let drawing = |doc: &Document, widget: ObjectId| {
        let (id, bbox) = appearance(doc, widget);
        let stream = doc.get_object(id).unwrap().as_stream().unwrap();
        (bbox, String::from_utf8_lossy(&stream.content).into_owned())
    };
    assert_eq!(drawing(&doc, empty.widget).0, [0.0, 0.0, 100.0, 30.0]);

    // Moved: the same drawing, somewhere else.
    let before = appearance(&doc, empty.widget).0;
    apply(&mut doc, &[to(&empty, [40.0, 120.0, 140.0, 150.0])]).expect("moved");
    assert_eq!(
        one(&doc, "Approve").display_rect,
        [40.0, 120.0, 140.0, 150.0]
    );
    assert_eq!(appearance(&doc, empty.widget).0, before);

    // Resized: its border is drawn for the size it now has.
    apply(&mut doc, &[to(&empty, [40.0, 120.0, 200.0, 160.0])]).expect("resized");
    let (bbox, content) = drawing(&doc, empty.widget);
    assert_eq!(bbox, [0.0, 0.0, 160.0, 40.0]);
    assert!(content.contains("0.5 0.5 159 39 re S"), "{content}");
    // Still a place for a signature, and too small a one is refused.
    assert!(matches!(
        one(&doc, "Approve").control,
        crate::forms::Control::Signature { signed: false }
    ));
    let why =
        apply(&mut doc.clone(), &[to(&empty, [40.0, 120.0, 200.0, 140.0])]).expect_err("too small");
    assert!(why.contains("24"), "{why}");

    apply(&mut doc, &[named(&empty, "Approved by")]).expect("renamed");
    let renamed = one(&doc, "Approved by");
    apply(&mut doc, &[gone(&renamed)]).expect("removed");
    assert!(widgets(&doc, "Approved by").is_empty());

    // The one that holds a signature is none of these.
    let signed = one(&doc, "Signed");
    for edit in [
        to(&signed, [150.0, 60.0, 250.0, 90.0]),
        named(&signed, "x"),
        gone(&signed),
    ] {
        let why = apply(&mut doc.clone(), &[edit]).expect_err("refused");
        assert!(why.contains("signed signature field"), "{why}");
    }
}
