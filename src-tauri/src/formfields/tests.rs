use super::*;
use crate::forms::{scan, write, Change, Control, Value};

/// How the document under test holds its form.
#[derive(Clone, Copy, PartialEq)]
enum Held {
    /// No form at all.
    Absent,
    /// A dictionary written into the catalog, with its field list inside it.
    Direct,
    /// An object of its own, with the field list and the resources objects too.
    Indirect,
}

/// Two pages of 300 by 200 points; the second is turned a quarter by `turned`.
/// A form, when there is one, has a text field `taken` on the first page.
fn document(held: Held, turned: bool) -> Document {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let mut ids = Vec::new();
    for at in 0..2 {
        let mut page = dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 200.into()],
            "Resources" => Dictionary::new(),
        };
        if at == 1 && turned {
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
    let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages };
    if held != Held::Absent {
        let existing = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
            "T" => Object::string_literal("taken"),
            "Rect" => vec![10.into(), 10.into(), 110.into(), 30.into()], "F" => 4,
        });
        let annots = doc.add_object(vec![Object::Reference(existing)]);
        doc.get_dictionary_mut(ids[0])
            .unwrap()
            .set("Annots", annots);
        if held == Held::Direct {
            catalog.set(
                "AcroForm",
                dictionary! { "Fields" => vec![existing.into()] },
            );
        } else {
            let fields = doc.add_object(vec![Object::Reference(existing)]);
            let other = doc.add_object(dictionary! { "Type" => "Font", "BaseFont" => "Courier" });
            let fonts = doc.add_object(dictionary! { "Cour" => other });
            let resources = doc.add_object(dictionary! { "Font" => fonts });
            let form = doc.add_object(dictionary! {
                "Fields" => fields, "DR" => resources,
                "DA" => Object::string_literal("/Cour 9 Tf 0 g"),
            });
            catalog.set("AcroForm", form);
        }
    }
    let root = doc.add_object(catalog);
    doc.trailer.set("Root", root);
    reloaded(&mut doc)
}

/// The document as a reader meets it: written out and parsed again.
fn reloaded(doc: &mut Document) -> Document {
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    Document::load_mem(&bytes).unwrap()
}

fn field(name: &str, kind: Kind, page: u32, rect: [f64; 4]) -> NewField {
    NewField {
        name: name.into(),
        kind,
        page,
        rect,
        tooltip: None,
        required: false,
        max_length: None,
        border: false,
    }
}

fn three() -> Vec<NewField> {
    vec![
        NewField {
            tooltip: Some("Your full name".into()),
            required: true,
            max_length: Some(40),
            ..field("Name", Kind::Text, 0, [20.0, 30.0, 200.0, 20.0])
        },
        field("Remarks", Kind::Multiline, 1, [20.0, 60.0, 200.0, 80.0]),
        field("Agree", Kind::Checkbox, 0, [20.0, 120.0, 14.0, 14.0]),
    ]
}

fn form_of(doc: &Document) -> &Dictionary {
    let id = form_id(doc).expect("the form is an object of its own");
    doc.get_dictionary(id).unwrap()
}

#[test]
fn a_document_without_a_form_gets_one_and_the_fields_read_back() {
    for held in [Held::Absent, Held::Direct, Held::Indirect] {
        let mut doc = document(held, false);
        let had = scan(&doc).unwrap().widgets.len();
        add(&mut doc, &three()).unwrap();
        let doc = reloaded(&mut doc);
        let form = scan(&doc).unwrap();
        assert_eq!(form.widgets.len(), had + 3);
        let by = |name: &str| form.widgets.iter().find(|w| w.name == name).unwrap();

        let name = by("Name");
        assert!(matches!(name.control, Control::Text));
        assert_eq!(
            (name.page, name.multiline, name.max_length),
            (0, false, Some(40))
        );
        assert_eq!(name.value, Value::Text(String::new()));
        assert_eq!(name.reason, None, "a new field is one tpdf can answer");
        // 30 points from the top of a 200-point page, 20 high.
        assert_eq!(name.rect, [20.0, 150.0, 220.0, 170.0]);
        assert_eq!(name.display_rect, [20.0, 30.0, 220.0, 50.0]);
        assert_eq!(name.object, name.widget, "the field is its own widget");

        let remarks = by("Remarks");
        assert_eq!(
            (remarks.page, remarks.multiline, remarks.max_length),
            (1, true, None)
        );

        let agree = by("Agree");
        assert!(matches!(agree.control, Control::Checkbox));
        assert_eq!(agree.value, Value::Checked(false));
        assert_eq!(agree.reason, None);

        if held != Held::Absent {
            assert_eq!(by("taken").reason, None, "the field it had is still there");
        }
    }
}

#[test]
fn the_field_carries_what_other_readers_look_for() {
    let mut doc = document(Held::Absent, false);
    add(&mut doc, &three()).unwrap();
    let doc = reloaded(&mut doc);
    let form = scan(&doc).unwrap();
    let dict = |name: &str| {
        let id = form.widgets.iter().find(|w| w.name == name).unwrap().widget;
        doc.get_dictionary(id).unwrap()
    };
    let name = dict("Name");
    assert_eq!(name.get(b"FT").unwrap().as_name().unwrap(), b"Tx");
    // Required is bit 2; a single line has no bit 13.
    assert_eq!(name.get(b"Ff").unwrap().as_i64().unwrap(), 2);
    assert_eq!(name.get(b"F").unwrap().as_i64().unwrap(), 4, "it prints");
    assert_eq!(
        name.get(b"DA").unwrap().as_str().unwrap(),
        b"/Helv 0 Tf 0 g"
    );
    assert_eq!(
        crate::annots::decode_text_string(name.get(b"TU").unwrap().as_str().unwrap()),
        "Your full name"
    );
    assert!(name.get(b"P").unwrap().as_reference().is_ok());
    let normal = name
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap();
    let stream = doc
        .get_object(normal.as_reference().unwrap())
        .unwrap()
        .as_stream()
        .unwrap();
    assert_eq!(stream.content, b"/Tx BMC EMC");
    assert_eq!(
        stream.dict.get(b"BBox").unwrap().as_array().unwrap()[2]
            .as_float()
            .unwrap(),
        200.0
    );

    assert_eq!(
        dict("Remarks").get(b"Ff").unwrap().as_i64().unwrap(),
        1 << 12
    );
    assert!(!dict("Remarks").has(b"TU") && !dict("Remarks").has(b"MaxLen"));

    let agree = dict("Agree");
    assert_eq!(agree.get(b"FT").unwrap().as_name().unwrap(), b"Btn");
    assert_eq!(agree.get(b"Ff").unwrap().as_i64().unwrap(), 0);
    assert_eq!(agree.get(b"V").unwrap().as_name().unwrap(), b"Off");
    assert_eq!(agree.get(b"AS").unwrap().as_name().unwrap(), b"Off");
    let states = agree
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(states.has(b"Off") && states.has(b"Yes") && states.len() == 2);

    // The form names the font every field's default appearance asks for.
    let form = form_of(&doc);
    assert_eq!(
        form.get(b"DA").unwrap().as_str().unwrap(),
        b"/Helv 0 Tf 0 g"
    );
    let fonts = form
        .get(b"DR")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap();
    let helv = doc
        .get_dictionary(fonts.get(b"Helv").unwrap().as_reference().unwrap())
        .unwrap();
    assert_eq!(
        helv.get(b"BaseFont").unwrap().as_name().unwrap(),
        b"Helvetica"
    );
    assert_eq!(form.get(b"Fields").unwrap().as_array().unwrap().len(), 3);
}

#[test]
fn a_form_that_exists_keeps_what_it_had() {
    let mut doc = document(Held::Indirect, false);
    add(&mut doc, &three()).unwrap();
    let doc = reloaded(&mut doc);
    let form = form_of(&doc);
    // Its own default appearance and its own font stay; Helvetica joins them.
    assert_eq!(
        form.get(b"DA").unwrap().as_str().unwrap(),
        b"/Cour 9 Tf 0 g"
    );
    let fonts = form
        .get(b"DR")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(fonts.has(b"Cour") && fonts.has(b"Helv"));
    // The field list was an object of its own and is appended to where it is.
    let fields = form.get(b"Fields").unwrap().as_reference().unwrap();
    assert_eq!(doc.get_object(fields).unwrap().as_array().unwrap().len(), 4);
    // So was the first page's annotation list, which had one widget.
    let first = crate::pagetree::ordered_pages(&doc)[0];
    let annots = doc
        .get_dictionary(first)
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_reference()
        .unwrap();
    assert_eq!(doc.get_object(annots).unwrap().as_array().unwrap().len(), 3);
    let second = crate::pagetree::ordered_pages(&doc)[1];
    assert_eq!(
        doc.get_dictionary(second)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_field_just_added_can_be_answered() {
    let mut doc = document(Held::Absent, false);
    add(&mut doc, &three()).unwrap();
    let mut doc = reloaded(&mut doc);
    let form = scan(&doc).unwrap();
    let object = |name: &str| form.widgets.iter().find(|w| w.name == name).unwrap().object;
    write(
        &mut doc,
        &[
            Change {
                object: object("Name"),
                value: Value::Text("Ada Lovelace".into()),
            },
            Change {
                object: object("Agree"),
                value: Value::Checked(true),
            },
            Change {
                object: object("Remarks"),
                value: Value::Text("one\ntwo".into()),
            },
        ],
    )
    .unwrap();
    let doc = reloaded(&mut doc);
    let form = scan(&doc).unwrap();
    let value = |name: &str| {
        form.widgets
            .iter()
            .find(|w| w.name == name)
            .unwrap()
            .value
            .clone()
    };
    assert_eq!(value("Name"), Value::Text("Ada Lovelace".into()));
    assert_eq!(value("Agree"), Value::Checked(true));
    assert_eq!(value("Remarks"), Value::Text("one\ntwo".into()));
    // Ticked through the state the box was created with, not a second one.
    let agree = doc.get_dictionary(object("Agree")).unwrap();
    assert_eq!(agree.get(b"AS").unwrap().as_name().unwrap(), b"Yes");
}

#[test]
fn a_text_field_of_any_height_it_may_have_takes_one_line() {
    // From the least a text field may be to well past where twelve points
    // fits. Every one has to hold a short answer, and a height that cannot is a
    // field tpdf lets a reader make and then refuses to fill.
    let mut height = MIN_TEXT;
    while height <= 30.0 {
        let mut doc = document(Held::Absent, false);
        add(
            &mut doc,
            &[field("Name", Kind::Text, 0, [20.0, 30.0, 200.0, height])],
        )
        .unwrap();
        let mut doc = reloaded(&mut doc);
        let object = scan(&doc).unwrap().widgets[0].object;
        let answered = write(
            &mut doc,
            &[Change {
                object,
                value: Value::Text("Ada".into()),
            }],
        );
        assert_eq!(answered, Ok(()), "a field {height} points high");
        height += 0.25;
    }
}

/// The content of a widget's normal appearance, as text.
fn appearance_of(doc: &Document, widget: ObjectId) -> String {
    let normal = doc
        .get_dictionary(widget)
        .unwrap()
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_reference()
        .unwrap();
    let stream = doc.get_object(normal).unwrap().as_stream().unwrap();
    String::from_utf8_lossy(&stream.content).into_owned()
}

#[test]
fn a_text_field_asked_for_a_border_draws_one_and_declares_it() {
    let mut doc = document(Held::Absent, false);
    add(
        &mut doc,
        &[
            NewField {
                border: true,
                ..field("Framed", Kind::Text, 0, [20.0, 30.0, 200.0, 20.0])
            },
            field("Plain", Kind::Text, 0, [20.0, 60.0, 200.0, 20.0]),
            NewField {
                border: true,
                ..field("Box", Kind::Checkbox, 0, [20.0, 90.0, 14.0, 14.0])
            },
        ],
    )
    .unwrap();
    let doc = reloaded(&mut doc);
    let form = scan(&doc).unwrap();
    let id = |name: &str| form.widgets.iter().find(|w| w.name == name).unwrap().widget;
    // Inset by half the line, so the appearance's own box does not clip it.
    assert_eq!(
        appearance_of(&doc, id("Framed")),
        "0 G 1 w 0.5 0.5 199 19 re S /Tx BMC EMC"
    );
    let framed = doc.get_dictionary(id("Framed")).unwrap();
    let colour = framed
        .get(b"MK")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"BC")
        .unwrap();
    assert_eq!(colour.as_array().unwrap().len(), 1, "one number: grey");
    assert_eq!(colour.as_array().unwrap()[0].as_float().unwrap(), 0.0);
    let style = framed.get(b"BS").unwrap().as_dict().unwrap();
    assert_eq!(style.get(b"W").unwrap().as_float().unwrap(), 1.0);
    assert_eq!(style.get(b"S").unwrap().as_name().unwrap(), b"S");
    // The control: a field not asked for one has neither.
    assert_eq!(appearance_of(&doc, id("Plain")), "/Tx BMC EMC");
    let plain = doc.get_dictionary(id("Plain")).unwrap();
    assert!(!plain.has(b"MK") && !plain.has(b"BS"));
    // A checkbox draws its own box whatever is asked.
    let checkbox = doc.get_dictionary(id("Box")).unwrap();
    assert!(!checkbox.has(b"MK") && !checkbox.has(b"BS"));
}

#[test]
fn an_answer_keeps_the_border_the_field_declares() {
    let answered = |change: &dyn Fn(&mut Dictionary)| {
        let mut doc = document(Held::Absent, false);
        add(
            &mut doc,
            &[NewField {
                border: true,
                ..field("Name", Kind::Text, 0, [20.0, 30.0, 200.0, 20.0])
            }],
        )
        .unwrap();
        let mut doc = reloaded(&mut doc);
        let widget = scan(&doc).unwrap().widgets[0].widget;
        change(doc.get_dictionary_mut(widget).unwrap());
        write(
            &mut doc,
            &[Change {
                object: widget,
                value: Value::Text("Ada".into()),
            }],
        )
        .unwrap();
        appearance_of(&doc, widget)
    };
    let colour = |numbers: &[f32]| {
        dictionary! { "BC" => numbers.iter().map(|n| Object::Real(*n)).collect::<Vec<_>>() }
    };
    // As written: black, one point, after the white fill and before the text.
    let kept = answered(&|_| {});
    assert!(
        kept.starts_with(
            "q 1 1 1 rg 0 0 200 20 re f 0 G 1 w 0.5 0.5 199 19 re S 0 0 200 20 re W n"
        ),
        "{kept}"
    );
    assert!(kept.contains("Tj"), "the answer is drawn too: {kept}");
    // A colour another producer wrote, in each of the three spaces.
    assert!(answered(&|w| w.set("MK", colour(&[1.0, 0.0, 0.5]))).contains("1 0 0.5 RG 1 w"));
    assert!(answered(&|w| w.set("MK", colour(&[0.0, 0.2, 0.0, 1.0]))).contains("0 0.2 0 1 K 1 w"));
    assert!(answered(&|w| w.set("MK", colour(&[0.5]))).contains("0.5 G 1 w"));
    // A width another producer wrote, and the one point a missing width means.
    assert!(
        answered(&|w| w.set("BS", dictionary! { "W" => 2 })).contains("0 G 2 w 1 1 198 18 re S")
    );
    assert!(answered(&|w| {
        w.remove(b"BS");
    })
    .contains("0 G 1 w 0.5 0.5 199 19 re S"));
    // No colour, a colour that is none, or no width: no border is drawn.
    let none = |change: &dyn Fn(&mut Dictionary)| !answered(change).contains(" re S");
    assert!(none(&|w| {
        w.remove(b"MK");
    }));
    assert!(none(&|w| w.set("MK", Dictionary::new())));
    assert!(none(&|w| w.set("MK", colour(&[0.0, 0.0]))));
    assert!(none(&|w| w.set("MK", colour(&[1.5]))));
    assert!(none(&|w| w.set("MK", colour(&[-0.1, 0.0, 0.0]))));
    assert!(none(&|w| w.set("BS", dictionary! { "W" => 0 })));
    assert!(none(&|w| w.set("BS", dictionary! { "W" => 13 })));
}

#[test]
fn every_problem_is_named_and_nothing_is_written() {
    let mut doc = document(Held::Direct, true);
    let before = {
        let mut bytes = Vec::new();
        doc.clone().save_to(&mut bytes).unwrap();
        bytes
    };
    let cases: Vec<(NewField, &str)> = vec![
        (
            field("", Kind::Text, 0, [10.0, 10.0, 50.0, 20.0]),
            "a field needs a name",
        ),
        (
            field(" x", Kind::Text, 0, [10.0, 10.0, 50.0, 20.0]),
            "begins or ends with a space",
        ),
        (
            field("a.b", Kind::Text, 0, [10.0, 10.0, 50.0, 20.0]),
            "contains a period",
        ),
        (
            field("a\tb", Kind::Text, 0, [10.0, 10.0, 50.0, 20.0]),
            "control character",
        ),
        (
            field(&"n".repeat(256), Kind::Text, 0, [10.0, 10.0, 50.0, 20.0]),
            "at most 255",
        ),
        (
            field("taken", Kind::Text, 0, [10.0, 50.0, 50.0, 20.0]),
            "already has a field of this name",
        ),
        (
            field("far", Kind::Text, 2, [10.0, 10.0, 50.0, 20.0]),
            "there is no page 3; the document has 2",
        ),
        (
            field("turned", Kind::Text, 1, [10.0, 10.0, 50.0, 20.0]),
            "page 2 is turned",
        ),
        (
            field("thin", Kind::Text, 0, [10.0, 10.0, 50.0, 7.9]),
            "a text field needs at least 8 by 8",
        ),
        (
            field("narrow", Kind::Multiline, 0, [10.0, 10.0, 7.9, 50.0]),
            "a text field needs at least 8 by 8",
        ),
        (
            field("tiny", Kind::Checkbox, 0, [10.0, 10.0, 5.9, 6.0]),
            "a checkbox needs at least 6 by 6",
        ),
        (
            field("left", Kind::Text, 0, [-1.0, 10.0, 50.0, 20.0]),
            "not inside page 1, which is 300 by 200",
        ),
        (
            field("above", Kind::Text, 0, [10.0, -1.0, 50.0, 20.0]),
            "not inside page 1",
        ),
        (
            field("right", Kind::Text, 0, [260.0, 10.0, 41.0, 20.0]),
            "not inside page 1",
        ),
        (
            field("below", Kind::Text, 0, [10.0, 181.0, 50.0, 20.0]),
            "not inside page 1",
        ),
        (
            field("nan", Kind::Text, 0, [f64::NAN, 10.0, 50.0, 20.0]),
            "not four numbers",
        ),
        (
            NewField {
                max_length: Some(3),
                ..field("box", Kind::Checkbox, 0, [10.0, 10.0, 10.0, 10.0])
            },
            "a checkbox takes no characters",
        ),
        (
            NewField {
                max_length: Some(0),
                ..field("none", Kind::Text, 0, [10.0, 10.0, 50.0, 20.0])
            },
            "a number from 1 to 16384",
        ),
        (
            NewField {
                max_length: Some(16385),
                ..field("many", Kind::Text, 0, [10.0, 10.0, 50.0, 20.0])
            },
            "a number from 1 to 16384",
        ),
        (
            NewField {
                tooltip: Some("t".repeat(1025)),
                ..field("tip", Kind::Text, 0, [10.0, 10.0, 50.0, 20.0])
            },
            "a tooltip is at most 1024",
        ),
    ];
    for (bad, why) in &cases {
        // Beside a field that is fine, which must not be written either.
        let good = field("fine", Kind::Text, 0, [10.0, 100.0, 50.0, 20.0]);
        let said = add(&mut doc, &[good, bad.clone()]).unwrap_err();
        assert!(said.contains(why), "{:?}: {said}", bad.name);
        let mut after = Vec::new();
        doc.clone().save_to(&mut after).unwrap();
        assert_eq!(after, before, "{:?} changed the document", bad.name);
    }
    // Exactly at the limits is accepted.
    let edge = vec![
        field("edge", Kind::Text, 0, [0.0, 0.0, 300.0, 8.0]),
        field("corner", Kind::Checkbox, 0, [294.0, 194.0, 6.0, 6.0]),
        NewField {
            max_length: Some(16384),
            tooltip: Some("t".repeat(1024)),
            ..field(&"n".repeat(255), Kind::Text, 0, [10.0, 100.0, 50.0, 20.0])
        },
    ];
    assert_eq!(check(&doc, &edge).map(|placed| placed.len()), Ok(3));
}

#[test]
fn all_the_problems_are_reported_together() {
    let doc = document(Held::Direct, false);
    let problems = check(
        &doc,
        &[
            field("taken", Kind::Text, 0, [10.0, 50.0, 50.0, 20.0]),
            field("twice", Kind::Text, 0, [10.0, 50.0, 50.0, 20.0]),
            field("twice", Kind::Text, 5, [10.0, 80.0, 50.0, 20.0]),
        ],
    )
    .unwrap_err();
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems[1].contains("`twice` is named more than once"));
    assert!(problems[2].contains("there is no page 6"));
}

#[test]
fn a_page_displayed_from_an_inset_box_places_the_field_inside_it() {
    let mut doc = document(Held::Absent, false);
    let first = crate::pagetree::ordered_pages(&doc)[0];
    doc.get_dictionary_mut(first).unwrap().set(
        "CropBox",
        vec![50.into(), 40.into(), 250.into(), 160.into()],
    );
    // The displayed page is now 200 by 120, and the field is measured from its corner.
    assert!(add(
        &mut doc,
        &[field("out", Kind::Text, 0, [150.0, 10.0, 60.0, 20.0])]
    )
    .unwrap_err()
    .contains("which is 200 by 120"));
    add(
        &mut doc,
        &[field("in", Kind::Text, 0, [10.0, 10.0, 60.0, 20.0])],
    )
    .unwrap();
    let form = scan(&doc).unwrap();
    assert_eq!(form.widgets[0].rect, [60.0, 130.0, 120.0, 150.0]);
    assert_eq!(form.widgets[0].display_rect, [10.0, 10.0, 70.0, 30.0]);
}

#[test]
fn too_many_fields_and_an_xfa_form_are_refused_whole() {
    let mut doc = document(Held::Indirect, false);
    let many: Vec<NewField> = (0..=MAX_NEW)
        .map(|n| {
            field(
                &format!("f{n}"),
                Kind::Checkbox,
                0,
                [10.0, 10.0, 10.0, 10.0],
            )
        })
        .collect();
    assert!(add(&mut doc, &many)
        .unwrap_err()
        .contains("more than the 1000"));
    assert!(check(&doc, &many[..MAX_NEW]).is_ok());

    let form = form_id(&doc).unwrap();
    doc.get_dictionary_mut(form)
        .unwrap()
        .set("XFA", Object::Null);
    assert_eq!(
        add(
            &mut doc,
            &[field("x", Kind::Text, 0, [10.0, 50.0, 50.0, 20.0])]
        )
        .unwrap_err(),
        crate::forms::XFA_REFUSAL
    );
    // Nothing to add is nothing to refuse, whatever the document holds.
    assert_eq!(add(&mut doc, &[]), Ok(()));
}

#[test]
fn the_wire_names_are_the_ones_the_command_reads() {
    let parsed: NewField = serde_json::from_str(
        r#"{"name":"Name","kind":"multiline","page":0,"rect":[1,2,30,40],"required":true}"#,
    )
    .unwrap();
    assert_eq!(
        parsed,
        NewField {
            required: true,
            ..field("Name", Kind::Multiline, 0, [1.0, 2.0, 30.0, 40.0])
        }
    );
    for kind in ["text", "multiline", "checkbox"] {
        assert!(
            serde_json::from_str::<Kind>(&format!("\"{kind}\"")).is_ok(),
            "{kind}"
        );
    }
    // A misspelt key is refused, not read as its default.
    assert!(serde_json::from_str::<NewField>(
        r#"{"name":"N","kind":"text","page":0,"rect":[1,2,30,40],"tooltop":"x"}"#
    )
    .is_err());
}
