//! Pictures inside a Form XObject: what a region takes of them, what stays
//! because it is shared, and what the removal does to the form's stream.

use lopdf::content::Content;
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

use super::{
    aggregate, covered, leave_shared, notes_for, remove_form_images, RegionPlan, SharedDraws,
    Unhandled,
};
use crate::objects::{FormObject, PageObject};

/// How a form in the fixture holds the names of what it draws.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Held {
    /// A dictionary in the form's own stream dictionary.
    Inline,
    /// The form's `/Resources` is an object of its own.
    Resources,
    /// And so is the `/XObject` list inside it.
    List,
    /// The form has no resources and draws through the page's.
    Page,
}

struct Fixture {
    doc: Document,
    page: ObjectId,
    /// The forms the page draws, in order.
    forms: Vec<ObjectId>,
    /// `pictures[form][n]` is the n-th picture that form draws.
    pictures: Vec<Vec<ObjectId>>,
}

fn picture(doc: &mut Document, tag: u8) -> ObjectId {
    doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
        },
        vec![tag],
    ))
}

/// A page drawing one form per entry of `per_form`, each form drawing that many
/// pictures with a line of text before the first and a rule after the last.
fn fixture(per_form: &[usize], held: Held) -> Fixture {
    let mut doc = Document::with_version("1.7");
    let mut page_xobjects = Dictionary::new();
    let mut drawn = String::new();
    let mut forms = Vec::new();
    let mut pictures = Vec::new();
    for (at, count) in per_form.iter().enumerate() {
        let mut names = Dictionary::new();
        let mut body = format!("BT (f{at}) Tj ET\n");
        let mut here = Vec::new();
        for n in 0..*count {
            let id = picture(&mut doc, (at * 16 + n) as u8);
            let name = format!("Im{at}x{n}");
            body.push_str(&format!("q 10 0 0 10 {} 0 cm /{name} Do Q\n", n * 20));
            names.set(name.clone(), Object::Reference(id));
            if held == Held::Page {
                page_xobjects.set(name, Object::Reference(id));
            }
            here.push(id);
        }
        body.push_str("0 0 m 50 0 l S\n");
        let mut dict = dictionary! { "Type" => "XObject", "Subtype" => "Form" };
        match held {
            Held::Inline => {
                dict.set(
                    "Resources",
                    dictionary! { "XObject" => Object::Dictionary(names) },
                );
            }
            Held::Resources => {
                let resources =
                    doc.add_object(dictionary! { "XObject" => Object::Dictionary(names) });
                dict.set("Resources", resources);
            }
            Held::List => {
                let list = doc.add_object(Object::Dictionary(names));
                let resources = doc.add_object(dictionary! { "XObject" => list });
                dict.set("Resources", resources);
            }
            Held::Page => {}
        }
        let id = doc.add_object(Stream::new(dict, body.into_bytes()));
        page_xobjects.set(format!("Fm{at}"), Object::Reference(id));
        drawn.push_str(&format!("/Fm{at} Do\n"));
        forms.push(id);
        pictures.push(here);
    }
    let content = doc.add_object(Stream::new(dictionary! {}, drawn.into_bytes()));
    let pages_id = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content,
        "Resources" => dictionary! { "XObject" => Object::Dictionary(page_xobjects) },
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    Fixture {
        doc,
        page,
        forms,
        pictures,
    }
}

/// What a form's content still does, one operator name per operation, with the
/// name a `Do` draws beside it.
fn operators(doc: &Document, form: ObjectId) -> Vec<String> {
    let stream = doc.get_object(form).unwrap().as_stream().unwrap();
    let body = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    Content::decode(&body)
        .unwrap()
        .operations
        .into_iter()
        .map(
            |operation| match (operation.operator.as_str(), operation.operands.first()) {
                ("Do", Some(Object::Name(name))) => format!("Do {}", String::from_utf8_lossy(name)),
                (operator, _) => operator.to_string(),
            },
        )
        .collect()
}

/// The names a form's own XObject list still holds, or the page's when the
/// form has none.
fn names_held(doc: &Document, page: ObjectId, form: ObjectId) -> Vec<String> {
    let stream = doc.get_object(form).unwrap().as_stream().unwrap();
    let holder = match stream.dict.get(b"Resources") {
        Ok(resources) => doc
            .dereference(resources)
            .unwrap()
            .1
            .as_dict()
            .unwrap()
            .clone(),
        Err(_) => doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap()
            .clone(),
    };
    let list = doc.dereference(holder.get(b"XObject").unwrap()).unwrap().1;
    let mut names: Vec<String> = list
        .as_dict()
        .unwrap()
        .iter()
        .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
        .filter(|name| name.starts_with("Im"))
        .collect();
    names.sort();
    names
}

#[test]
fn a_picture_is_taken_out_of_the_form_that_draws_it_and_nothing_else_is() {
    for held in [Held::Inline, Held::Resources, Held::List] {
        let mut f = fixture(&[2, 3], held);
        let before_other = operators(&f.doc, f.forms[0]);
        let took = remove_form_images(&mut f.doc, f.page, &[(0, 2), (1, 3)], 1, &[1])
            .unwrap_or_else(|why| panic!("{held:?}: {why}"));
        assert_eq!((took.removed, took.shows_before), (1, 3), "{held:?}");
        // The second of the three draws is gone; the text, the two other
        // pictures, the state around each and the rule are where they were.
        assert_eq!(
            operators(&f.doc, f.forms[1]),
            [
                "BT", "Tj", "ET", "q", "cm", "Do Im1x0", "Q", "q", "cm", "Q", "q", "cm",
                "Do Im1x2", "Q", "m", "l", "S"
            ],
            "{held:?}"
        );
        assert_eq!(
            operators(&f.doc, f.forms[0]),
            before_other,
            "{held:?}: the other form"
        );
        // And nothing names the picture any more, so the sweep can take its
        // bytes --- except through the page's list, which is the page's.
        assert_eq!(
            names_held(&f.doc, f.page, f.forms[1]),
            ["Im1x0", "Im1x2"],
            "{held:?}"
        );
    }
}

#[test]
fn a_picture_named_by_a_list_that_is_not_the_forms_alone_keeps_its_name() {
    // A form with no resources of its own draws through the page's list, which
    // the page and every other such form read too. The draw goes; the name is
    // not this form's to take.
    let mut legacy = fixture(&[1, 1], Held::Page);
    remove_form_images(&mut legacy.doc, legacy.page, &[(0, 1), (1, 1)], 0, &[0]).expect("removed");
    assert!(!operators(&legacy.doc, legacy.forms[0]).contains(&"Do Im0x0".to_string()));
    assert!(names_held(&legacy.doc, legacy.page, legacy.forms[0]).contains(&"Im0x0".to_string()));

    // Two forms pointing at one resources object, or at one list: the picture
    // has one name there and either form may draw it.
    for held in [Held::Resources, Held::List] {
        let mut f = fixture(&[1], held);
        let form = f.forms[0];
        let resources = f
            .doc
            .get_object(form)
            .unwrap()
            .as_stream()
            .unwrap()
            .dict
            .get(b"Resources")
            .unwrap()
            .as_reference()
            .unwrap();
        let shared = match held {
            Held::List => f
                .doc
                .get_dictionary(resources)
                .unwrap()
                .get(b"XObject")
                .unwrap()
                .as_reference()
                .unwrap(),
            _ => resources,
        };
        // Something else in the document refers to the same object.
        f.doc.add_object(dictionary! { "Too" => shared });
        remove_form_images(&mut f.doc, f.page, &[(0, 1)], 0, &[0]).expect("removed");
        assert!(
            !operators(&f.doc, form).contains(&"Do Im0x0".to_string()),
            "{held:?}"
        );
        assert_eq!(
            names_held(&f.doc, f.page, form),
            ["Im0x0"],
            "{held:?}: the name is kept"
        );
    }
    // The control is the first test: the same shapes, unshared, lose the name.
}

#[test]
fn several_pictures_of_one_form_go_in_one_call_whatever_order_they_are_named_in() {
    let mut f = fixture(&[3], Held::Inline);
    let took = remove_form_images(&mut f.doc, f.page, &[(4, 3)], 4, &[2, 0, 2]).expect("removed");
    assert_eq!(took.removed, 2, "naming one twice removes it once");
    let left: Vec<String> = operators(&f.doc, f.forms[0])
        .into_iter()
        .filter(|operator| operator.starts_with("Do"))
        .collect();
    assert_eq!(left, ["Do Im0x1"]);
    assert_eq!(names_held(&f.doc, f.page, f.forms[0]), ["Im0x1"]);
}

#[test]
fn a_removal_that_cannot_be_right_changes_nothing_and_says_why() {
    let untouched = |f: &Fixture| operators(&f.doc, f.forms[0]);
    let refused = |f: &mut Fixture, forms: &[(usize, usize)], at: usize, ordinals: &[usize]| {
        let before = untouched(f);
        let why = remove_form_images(&mut f.doc, f.page, forms, at, ordinals).unwrap_err();
        assert_eq!(untouched(f), before, "a refusal changed the form: {why}");
        why
    };
    let mut f = fixture(&[2], Held::Inline);
    // PDFium counted another number of forms, or of pictures in this one.
    assert!(refused(&mut f, &[(0, 2), (1, 0)], 0, &[0]).contains("draws 1 form XObject(s)"));
    assert!(
        refused(&mut f, &[(0, 5)], 0, &[0]).contains("draws 2 picture(s) and PDFium reported 5")
    );
    // A form or a picture that is not there.
    assert!(refused(&mut f, &[(0, 2)], 9, &[0]).contains("object 9 is not one of the 1 form(s)"));
    assert!(refused(&mut f, &[(0, 2)], 0, &[7]).contains("there is no picture 7"));

    // The form is drawn twice by the page.
    let mut twice = fixture(&[1], Held::Inline);
    let content = twice.doc.get_page_contents(twice.page)[0];
    twice
        .doc
        .get_object_mut(content)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .set_plain_content(b"/Fm0 Do\n/Fm0 Do\n".to_vec());
    assert!(refused(&mut twice, &[(0, 1), (1, 1)], 0, &[0])
        .contains("inside a form that this document draws 2"));
}

#[test]
fn a_picture_drawn_elsewhere_too_loses_its_draw_in_the_form() {
    // Drawn twice by the form: the marked draw goes, the other keeps the name.
    let mut repeated = fixture(&[1], Held::Inline);
    let form = repeated.forms[0];
    repeated
        .doc
        .get_object_mut(form)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .set_plain_content(b"/Im0x0 Do\n/Im0x0 Do\n".to_vec());
    let took =
        remove_form_images(&mut repeated.doc, repeated.page, &[(0, 2)], 0, &[0]).expect("removed");
    assert_eq!(took.removed, 1);
    assert_eq!(operators(&repeated.doc, form), ["Do Im0x0"]);
    assert_eq!(names_held(&repeated.doc, repeated.page, form), ["Im0x0"]);
    // The second draw takes the name with it.
    remove_form_images(&mut repeated.doc, repeated.page, &[(0, 1)], 0, &[0]).expect("removed");
    assert!(names_held(&repeated.doc, repeated.page, form).is_empty());

    // Named from somewhere else in the document as well: the form's own list
    // loses the name, and the picture is still referred to from there.
    let mut elsewhere = fixture(&[1], Held::Inline);
    let shared = elsewhere.pictures[0][0];
    let also = elsewhere.doc.add_object(dictionary! { "Also" => shared });
    remove_form_images(&mut elsewhere.doc, elsewhere.page, &[(0, 1)], 0, &[0]).expect("removed");
    assert!(names_held(&elsewhere.doc, elsewhere.page, elsewhere.forms[0]).is_empty());
    assert!(elsewhere.doc.get_dictionary(also).unwrap().has(b"Also"));
}

fn objects_for(forms: usize) -> Vec<PageObject> {
    (0..forms)
        .map(|_| PageObject {
            bounds: [0.0, 0.0, 600.0, 700.0],
            kind: "form".to_string(),
        })
        .collect()
}

fn form_with(at: usize, images: &[[f32; 4]]) -> FormObject {
    FormObject {
        paths: Vec::new(),
        at,
        text: Vec::new(),
        images: images.to_vec(),
        unreachable: Vec::new(),
    }
}

#[test]
fn a_region_takes_the_pictures_it_touches_and_only_those() {
    let objects = objects_for(2);
    let forms = [
        form_with(0, &[[10.0, 10.0, 50.0, 50.0], [200.0, 200.0, 300.0, 300.0]]),
        form_with(1, &[[40.0, 40.0, 90.0, 90.0]]),
    ];
    // Touching is enough: a picture is taken whole.
    let plan = covered(&objects, &forms, [45.0, 45.0, 60.0, 60.0]);
    assert_eq!(plan.form_images, vec![(0, 0), (1, 0)]);
    assert!(plan.is_complete(), "{:?}", plan.unhandled);
    assert!(
        plan.images.is_empty(),
        "these are not the page's own pictures"
    );
    // The control: a region over none of them takes none.
    let plan = covered(&objects, &forms, [100.0, 100.0, 150.0, 150.0]);
    assert!(plan.form_images.is_empty() && plan.is_complete());
    let plan = covered(&objects, &forms, [250.0, 250.0, 260.0, 260.0]);
    assert_eq!(plan.form_images, vec![(0, 1)]);
}

#[test]
fn a_picture_stays_with_a_form_drawn_more_than_once_and_goes_from_one_that_is_not() {
    let objects = objects_for(2);
    let forms = [
        form_with(0, &[[10.0, 10.0, 50.0, 50.0], [20.0, 20.0, 60.0, 60.0]]),
        form_with(1, &[[10.0, 10.0, 50.0, 50.0]]),
    ];
    let region = [15.0, 15.0, 45.0, 45.0];
    let planned = || covered(&objects, &forms, region);
    assert_eq!(planned().form_images, vec![(0, 0), (0, 1), (1, 0)]);

    // Nobody asked about the forms' pictures: all three go, and nothing is said.
    let mut plan = planned();
    leave_shared(&mut plan, &SharedDraws::unknown(0, 2), &objects, &forms);
    assert_eq!(plan.form_images.len(), 3);
    assert!(plan.is_complete() && plan.shared.is_empty());

    // The first form is drawn three times: changing it changes places nobody
    // marked, so both its pictures stay, reported once, as the form.
    let mut plan = planned();
    let shared = SharedDraws {
        forms: vec![Some(3), None],
        ..SharedDraws::unknown(0, 2)
    };
    leave_shared(&mut plan, &shared, &objects, &forms);
    assert_eq!(plan.form_images, vec![(1, 0)]);
    assert_eq!(
        plan.unhandled,
        vec![Unhandled {
            at: 0,
            kind: "form".to_string(),
            drawn: Some(3)
        }]
    );
    assert!(plan.shared.is_empty());

    // One picture of the first form is drawn twice and the form once: all
    // three draws go, and the one whose picture stays in the file is said.
    let mut plan = planned();
    let shared = SharedDraws {
        form_images: vec![vec![None, Some(2)], vec![None]],
        ..SharedDraws::unknown(0, 2)
    };
    leave_shared(&mut plan, &shared, &objects, &forms);
    assert_eq!(plan.form_images, vec![(0, 0), (0, 1), (1, 0)]);
    assert!(plan.is_complete(), "{:?}", plan.unhandled);
    assert_eq!(
        plan.shared,
        vec![Unhandled {
            at: 0,
            kind: "image".to_string(),
            drawn: Some(2)
        }]
    );
}

#[test]
fn the_document_is_asked_how_often_each_picture_of_each_form_is_drawn() {
    let mut f = fixture(&[2, 1], Held::Resources);
    // The second picture of the first form is named from somewhere else too.
    let shared = f.pictures[0][1];
    f.doc.add_object(dictionary! { "Also" => shared });
    let asked = SharedDraws::unknown(0, 2).with_form_images(&f.doc, f.page, &[2, 1]);
    assert_eq!(asked.form_images, vec![vec![None, Some(2)], vec![None]]);
    // A count PDFium and the content disagree on answers nothing for that
    // form and still answers the other.
    let asked = SharedDraws::unknown(0, 2).with_form_images(&f.doc, f.page, &[5, 1]);
    assert_eq!(asked.form_images, vec![vec![None; 5], vec![None]]);
    // A number of forms that disagrees answers nothing at all.
    let asked = SharedDraws::unknown(0, 3).with_form_images(&f.doc, f.page, &[2, 1, 4]);
    assert_eq!(
        asked.form_images,
        vec![vec![None; 2], vec![None], vec![None; 4]]
    );
}

#[test]
fn a_blocks_picture_is_said_to_stay_when_the_block_draws_it_twice_or_reads_a_list_not_its_own() {
    // The block draws its one picture twice: taking one draw leaves the other.
    let mut twice = fixture(&[1], Held::Inline);
    let form = twice.forms[0];
    twice
        .doc
        .get_object_mut(form)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .set_plain_content(b"/Im0x0 Do\n/Im0x0 Do\n".to_vec());
    let asked = SharedDraws::unknown(0, 1).with_form_images(&twice.doc, twice.page, &[2]);
    assert_eq!(asked.form_images, vec![vec![Some(2), Some(2)]]);

    // A block with no resources of its own reads the page's list, which is
    // not its alone: the name stays, so the picture is said to stay.
    let legacy = fixture(&[1], Held::Page);
    let asked = SharedDraws::unknown(0, 1).with_form_images(&legacy.doc, legacy.page, &[1]);
    assert_eq!(asked.form_images, vec![vec![Some(2)]]);

    // The control: its own list, one draw, nothing to say.
    let own = fixture(&[1], Held::Inline);
    let asked = SharedDraws::unknown(0, 1).with_form_images(&own.doc, own.page, &[1]);
    assert_eq!(asked.form_images, vec![vec![None]]);
}

#[test]
fn the_pages_plan_carries_the_pictures_of_every_region_once() {
    let region = |form_images: Vec<(usize, usize)>| RegionPlan {
        form_images,
        form_image_objects: vec![(2, 3)],
        ..RegionPlan::default()
    };
    let one = aggregate(
        0,
        vec![[0.0; 4]; 2],
        vec![region(vec![(2, 1), (2, 0)]), region(vec![(2, 1)])],
        None,
    );
    assert_eq!(one.planned.form_images, vec![(2, 0), (2, 1)]);
    assert_eq!(one.planned.form_image_objects, vec![(2, 3)]);
    assert_eq!(
        one.shows, 2,
        "two removals, and a picture two regions touch is one"
    );
    assert_eq!(one.summary().images, 2);
}

#[test]
fn a_picture_taken_off_a_page_and_still_in_the_file_is_a_note_and_not_a_concern() {
    let stays = Unhandled {
        at: 4,
        kind: "image".to_string(),
        drawn: Some(12),
    };
    let region = |shared: Vec<Unhandled>| RegionPlan {
        shared,
        ..RegionPlan::default()
    };
    // Two regions over the one picture, and a third over nothing shared.
    let page = aggregate(
        2,
        vec![[0.0; 4]; 3],
        vec![
            region(vec![stays.clone()]),
            region(vec![stays.clone()]),
            region(Vec::new()),
        ],
        None,
    );
    assert_eq!(
        page.notes,
        vec![format!("page 3: {}", stays.taken_here())],
        "one picture under two regions is said once, with the reader's page number"
    );
    assert!(
        page.concerns.is_empty(),
        "it is no reason to doubt the result"
    );
    // The control: a page with nothing shared says nothing.
    assert!(aggregate(0, vec![[0.0; 4]], vec![region(Vec::new())], None)
        .notes
        .is_empty());
}

#[test]
fn a_sizing_note_needs_a_clean_verdict_and_a_note_about_what_stays_does_not() {
    let sizing = || vec!["nothing 7.4 pt or larger is readable".to_string()];
    let stays = || vec!["a picture is still in the file".to_string()];
    assert_eq!(
        notes_for(true, sizing(), stays()),
        [
            "nothing 7.4 pt or larger is readable",
            "a picture is still in the file"
        ]
    );
    assert_eq!(
        notes_for(false, sizing(), stays()),
        ["a picture is still in the file"]
    );
    assert!(notes_for(false, sizing(), Vec::new()).is_empty());
    assert_eq!(notes_for(true, sizing(), Vec::new()), sizing());
}
