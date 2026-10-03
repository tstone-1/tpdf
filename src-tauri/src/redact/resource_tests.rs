//! Where a page keeps the names of what it draws, and what that means for
//! taking a picture out: in the page, in an object of its own, in a list of
//! its own, inherited from the page tree, or shared between pages.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

use super::{remove_images, shared_draws};

/// How the pages of the fixture hold their resources.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kept {
    /// Written into each page.
    InPage,
    /// Each page has a resources object of its own.
    OwnObject,
    /// And the XObject list inside it is an object too.
    OwnList,
    /// Every page points at one resources object.
    OneObject,
    /// No page has any; the page tree's node does.
    Inherited,
    /// Each page has a resources object of its own, and all of them point at
    /// one XObject list.
    OneList,
}

struct Fixture {
    doc: Document,
    pages: Vec<ObjectId>,
    picture: ObjectId,
}

/// A document of `draws.len()` pages. Each page draws the one picture, under
/// the name `Im0`, as many times as its entry says.
fn fixture(kept: Kept, draws: &[usize]) -> Fixture {
    let mut doc = Document::with_version("1.7");
    let picture = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
        },
        vec![7],
    ));
    let names = || dictionary! { "Im0" => picture };
    let pages_id = doc.new_object_id();
    let one =
        (kept == Kept::OneObject).then(|| doc.add_object(dictionary! { "XObject" => names() }));
    let list = (kept == Kept::OneList).then(|| doc.add_object(Object::Dictionary(names())));
    let mut pages = Vec::new();
    for count in draws {
        let content = doc.add_object(Stream::new(
            dictionary! {},
            "q 10 0 0 10 0 0 cm /Im0 Do Q\n".repeat(*count).into_bytes(),
        ));
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        };
        match kept {
            Kept::InPage => page.set("Resources", dictionary! { "XObject" => names() }),
            Kept::OwnObject => {
                let resources = doc.add_object(dictionary! { "XObject" => names() });
                page.set("Resources", resources);
            }
            Kept::OwnList => {
                let list = doc.add_object(Object::Dictionary(names()));
                let resources = doc.add_object(dictionary! { "XObject" => list });
                page.set("Resources", resources);
            }
            Kept::OneObject => page.set("Resources", one.expect("made above")),
            Kept::OneList => {
                let resources =
                    doc.add_object(dictionary! { "XObject" => list.expect("made above") });
                page.set("Resources", resources);
            }
            Kept::Inherited => {}
        }
        pages.push(doc.add_object(page));
    }
    let mut node = dictionary! {
        "Type" => "Pages",
        "Kids" => pages.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
        "Count" => pages.len() as i64,
    };
    if kept == Kept::Inherited {
        node.set("Resources", dictionary! { "XObject" => names() });
    }
    doc.objects.insert(pages_id, Object::Dictionary(node));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    Fixture {
        doc,
        pages,
        picture,
    }
}

/// Adds a name to the XObject list a page draws through, wherever it is.
fn add_name(doc: &mut Document, page: ObjectId, name: &str, id: ObjectId) {
    let mut node = page;
    let resources = loop {
        let dict = doc.get_dictionary(node).unwrap();
        match dict.get(b"Resources") {
            Ok(Object::Reference(id)) => break Some(*id),
            Ok(Object::Dictionary(_)) => break None,
            _ => node = dict.get(b"Parent").unwrap().as_reference().unwrap(),
        }
    };
    let list = match resources {
        Some(id) => doc.get_dictionary(id).unwrap(),
        None => doc
            .get_dictionary(node)
            .unwrap()
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap(),
    }
    .get(b"XObject")
    .unwrap()
    .as_reference()
    .ok();
    let xobjects = match (list, resources) {
        (Some(list), _) => doc.get_dictionary_mut(list).unwrap(),
        (None, Some(id)) => doc
            .get_dictionary_mut(id)
            .unwrap()
            .get_mut(b"XObject")
            .unwrap()
            .as_dict_mut()
            .unwrap(),
        (None, None) => doc
            .get_dictionary_mut(node)
            .unwrap()
            .get_mut(b"Resources")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .get_mut(b"XObject")
            .unwrap()
            .as_dict_mut()
            .unwrap(),
    };
    xobjects.set(name, id);
}

/// How many `Do` operators a page's content still has.
fn draws_on(doc: &Document, page: ObjectId) -> usize {
    let data = doc.get_page_content(page);
    lopdf::content::Content::decode(&data)
        .unwrap()
        .operations
        .iter()
        .filter(|operation| operation.operator == "Do")
        .count()
}

/// Whether anything in the document still refers to the picture.
fn named(f: &Fixture) -> bool {
    fn refers(object: &Object, id: ObjectId) -> bool {
        match object {
            Object::Reference(other) => *other == id,
            Object::Array(items) => items.iter().any(|item| refers(item, id)),
            Object::Dictionary(dict) => dict.iter().any(|(_, value)| refers(value, id)),
            Object::Stream(stream) => stream.dict.iter().any(|(_, value)| refers(value, id)),
            _ => false,
        }
    }
    f.doc
        .objects
        .values()
        .any(|object| refers(object, f.picture))
}

#[test]
fn a_picture_is_found_and_taken_wherever_the_page_keeps_its_names() {
    // One page, drawing the picture once. Until 2026-10-03 only the first of
    // these five was found: the others answered *the page draws 0 image(s)*
    // and the whole redaction was refused.
    for kept in [
        Kept::InPage,
        Kept::OwnObject,
        Kept::OwnList,
        Kept::OneObject,
        Kept::Inherited,
        Kept::OneList,
    ] {
        let mut f = fixture(kept, &[1]);
        assert_eq!(
            shared_draws(&f.doc, f.pages[0], 1, 0).images,
            vec![None],
            "{kept:?}: drawn once, by one page"
        );
        let took = remove_images(&mut f.doc, f.pages[0], &[0], 1)
            .unwrap_or_else(|why| panic!("{kept:?}: {why}"));
        assert_eq!((took.removed, took.shows_before), (1, 1), "{kept:?}");
        assert_eq!(draws_on(&f.doc, f.pages[0]), 0, "{kept:?}");
        // Nothing names it any more, so the sweep takes its bytes.
        assert!(!named(&f), "{kept:?}: the picture is still named");
    }
}

#[test]
fn a_picture_other_pages_draw_from_the_same_list_stays() {
    // Two pages reading one list, both drawing the picture: one name, one
    // reference, two draws. Taking the name out would take it from both.
    for kept in [Kept::OneObject, Kept::Inherited, Kept::OneList] {
        let mut f = fixture(kept, &[1, 1]);
        assert_eq!(
            shared_draws(&f.doc, f.pages[0], 1, 0).images,
            vec![Some(2)],
            "{kept:?}"
        );
        let why = remove_images(&mut f.doc, f.pages[0], &[0], 1).unwrap_err();
        assert!(why.contains("is drawn 2 time(s)"), "{kept:?}: {why}");
        assert_eq!(
            draws_on(&f.doc, f.pages[0]),
            1,
            "{kept:?}: a refusal changed the page"
        );
        assert!(named(&f), "{kept:?}");
    }
    // The control: the same two shapes when the second page does not draw it.
    // The list is shared and the picture is still drawn once, so it goes, and
    // the other page is not touched.
    for kept in [Kept::OneObject, Kept::Inherited, Kept::OneList] {
        let mut f = fixture(kept, &[1, 0]);
        // The second page draws another picture through the same list, under
        // another name. That is no draw of this one.
        let other = f.doc.add_object(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1 },
            vec![9],
        ));
        let second = f.doc.get_page_contents(f.pages[1])[0];
        f.doc
            .get_object_mut(second)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(b"/Im9 Do\n".to_vec());
        add_name(&mut f.doc, f.pages[1], "Im9", other);
        assert_eq!(
            shared_draws(&f.doc, f.pages[0], 1, 0).images,
            vec![None],
            "{kept:?}"
        );
        remove_images(&mut f.doc, f.pages[0], &[0], 1)
            .unwrap_or_else(|why| panic!("{kept:?}: {why}"));
        assert_eq!(draws_on(&f.doc, f.pages[0]), 0, "{kept:?}");
        assert!(!named(&f), "{kept:?}");
    }
    // And pages that each keep their own names are counted by reference, as
    // before: two lists naming one picture are two references.
    for kept in [Kept::InPage, Kept::OwnObject, Kept::OwnList] {
        let f = fixture(kept, &[1, 1]);
        assert_eq!(
            shared_draws(&f.doc, f.pages[0], 1, 0).images,
            vec![Some(2)],
            "{kept:?}"
        );
    }
}

#[test]
fn the_nearest_list_that_names_a_picture_is_the_one_it_is_drawn_through() {
    // The page names `Im0` itself and also inherits a list naming `Im0` as
    // another picture. A reader draws the page's, so that is the one removed,
    // and the inherited name is another page's to use.
    let mut f = fixture(Kept::Inherited, &[1, 1]);
    let other = f.doc.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1 },
        vec![9],
    ));
    let own: Dictionary = dictionary! { "XObject" => dictionary! { "Im0" => other } };
    f.doc
        .get_dictionary_mut(f.pages[0])
        .unwrap()
        .set("Resources", own);
    assert_eq!(shared_draws(&f.doc, f.pages[0], 1, 0).images, vec![None]);
    remove_images(&mut f.doc, f.pages[0], &[0], 1).expect("removed");
    assert_eq!(draws_on(&f.doc, f.pages[0]), 0);
    // The page's own name is gone, and with it the last reference to `other`.
    let refers_to_other = f.doc.objects.values().any(|object| match object {
        Object::Dictionary(dict) => format!("{dict:?}").contains(&format!("{other:?}")),
        _ => false,
    });
    assert!(!refers_to_other, "the page still names the picture it drew");
    // The inherited one is untouched, for the second page.
    assert!(named(&f));
    assert_eq!(draws_on(&f.doc, f.pages[1]), 1);
}
