//! Shadow text around what a removal takes out of a Form XObject, and around a
//! picture taken off a page.
//!
//! `/ActualText`, `/Alt` and `/E` on a span are an alternate rendering of what
//! the span draws. The page's own removals cleared them; the three removals
//! inside a form, and the page's picture removal, did not, so the words stayed
//! in the form's stream beside the glyphs that went.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

use super::{remove_form_images, remove_form_shows, remove_images, take_form_paths};

/// A page drawing one Form XObject whose content is `body`.
///
/// The form's own resources hold a picture `Im0` and `properties` as
/// `/Properties`; `struct_parents` is the form's `/StructParents`, and when it
/// is set the document's parent tree gives that key one element, which carries
/// `/Alt` and is the answer [`element`] returns.
struct Fixture {
    doc: Document,
    page: ObjectId,
    form: ObjectId,
    element: Option<ObjectId>,
}

fn picture(doc: &mut Document) -> ObjectId {
    doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
        },
        vec![0],
    ))
}

fn fixture(body: &str, properties: Dictionary, struct_parents: Option<i64>) -> Fixture {
    let mut doc = Document::with_version("1.7");
    let image = picture(&mut doc);
    let mut form = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
        "Resources" => dictionary! {
            "XObject" => dictionary! { "Im0" => image },
            "Properties" => properties,
        },
    };
    if let Some(key) = struct_parents {
        form.set("StructParents", key);
    }
    let form = doc.add_object(Stream::new(form, body.as_bytes().to_vec()));
    let content = doc.add_object(Stream::new(dictionary! {}, b"/Fm0 Do\n".to_vec()));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "Contents" => content,
        "Resources" => dictionary! { "XObject" => dictionary! { "Fm0" => form } },
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
        }),
    );
    let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages };
    let mut element = None;
    if let Some(key) = struct_parents {
        let root = doc.new_object_id();
        let id = doc.add_object(dictionary! {
            "Type" => "StructElem",
            "S" => "Figure",
            "P" => root,
            "Alt" => Object::string_literal("A PORTRAIT OF THE SIGNER"),
        });
        let tree = doc.add_object(dictionary! {
            "Nums" => vec![Object::Integer(key), Object::Array(vec![id.into()])],
        });
        doc.objects.insert(
            root,
            Object::Dictionary(dictionary! { "Type" => "StructTreeRoot", "ParentTree" => tree }),
        );
        catalog.set("StructTreeRoot", root);
        element = Some(id);
    }
    let catalog = doc.add_object(catalog);
    doc.trailer.set("Root", catalog);
    Fixture {
        doc,
        page,
        form,
        element,
    }
}

/// The form's content as written, decompressed.
fn form_body(doc: &Document, form: ObjectId) -> String {
    let stream = doc
        .get_object(form)
        .and_then(Object::as_stream)
        .expect("the form");
    let body = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    String::from_utf8_lossy(&body).into_owned()
}

/// A text removal inside a form takes the span's `/ActualText` with it.
#[test]
fn text_taken_out_of_a_form_takes_the_spans_actual_text() {
    let mut f = fixture(
        "/Span << /ActualText (SECRET WORDS) >> BDC\nBT (SECRET WORDS) Tj ET\nEMC\n",
        Dictionary::new(),
        None,
    );

    let removed = remove_form_shows(&mut f.doc, f.page, &[(0, 1)], 0, &[0]).expect("removed");

    assert_eq!(removed.carriers, 1);
    assert!(
        !form_body(&f.doc, f.form).contains("SECRET"),
        "{}",
        form_body(&f.doc, f.form)
    );
}

/// A picture taken out of a form takes the span's `/Alt` with it.
#[test]
fn a_picture_taken_out_of_a_form_takes_the_spans_alt() {
    let mut f = fixture(
        "/Figure << /Alt (A PORTRAIT OF THE SIGNER) >> BDC\nq 10 0 0 10 0 0 cm /Im0 Do Q\nEMC\n",
        Dictionary::new(),
        None,
    );

    let removed = remove_form_images(&mut f.doc, f.page, &[(0, 1)], 0, &[0]).expect("removed");

    assert_eq!(removed.carriers, 1);
    assert!(!form_body(&f.doc, f.form).contains("PORTRAIT"));
}

/// A drawing taken out of a form takes the span's `/E` with it.
#[test]
fn a_drawing_taken_out_of_a_form_takes_the_spans_expansion() {
    let mut f = fixture(
        "/Span << /E (A HANDWRITTEN SIGNATURE) >> BDC\n0 0 10 10 re f\nEMC\n",
        Dictionary::new(),
        None,
    );

    let took = take_form_paths(&mut f.doc, f.page, &[(0, 1)], 0, &[0], &[]).expect("removed");

    assert_eq!(took.removed.carriers, 1);
    assert!(!form_body(&f.doc, f.form).contains("HANDWRITTEN"));
}

/// A named property list in the form's own resources that carries shadow text
/// is refused, as a shared one on the page is: editing it would change every
/// place that names it.
#[test]
fn a_named_list_in_the_forms_resources_carrying_shadow_text_is_refused() {
    let mut f = fixture(
        "/Span /P0 BDC\nBT (SECRET WORDS) Tj ET\nEMC\n",
        dictionary! { "P0" => dictionary! { "ActualText" => Object::string_literal("SECRET WORDS") } },
        None,
    );
    let before = form_body(&f.doc, f.form);

    let why = remove_form_shows(&mut f.doc, f.page, &[(0, 1)], 0, &[0]).expect_err("refused");

    assert!(why.contains("/P0"), "{why}");
    assert!(why.contains("ActualText"), "{why}");
    assert_eq!(form_body(&f.doc, f.form), before, "nothing was changed");
}

/// The control for the refusal above: a named list in the form's resources
/// carrying no shadow text is not refused, and its `/MCID`, numbered under the
/// form's own `/StructParents`, leads to the element whose `/Alt` goes.
#[test]
fn a_named_lists_mcid_leads_to_the_forms_structure_element() {
    let mut f = fixture(
        "/Figure /P0 BDC\nq 10 0 0 10 0 0 cm /Im0 Do Q\nEMC\n",
        dictionary! { "P0" => dictionary! { "MCID" => 0 } },
        Some(7),
    );
    let element = f.element.expect("the element");

    let removed = remove_form_images(&mut f.doc, f.page, &[(0, 1)], 0, &[0]).expect("removed");

    assert_eq!(removed.struct_carriers, 1);
    assert!(
        !f.doc
            .get_dictionary(element)
            .expect("the element")
            .has(b"Alt"),
        "the element's copy went"
    );
}

/// An inline `/MCID` inside a form is looked up under the form's
/// `/StructParents`, not the page's.
#[test]
fn an_inline_mcid_inside_a_form_reaches_the_forms_structure_element() {
    let mut f = fixture(
        "/Span << /MCID 0 >> BDC\nBT (SECRET WORDS) Tj ET\nEMC\n",
        Dictionary::new(),
        Some(3),
    );
    let element = f.element.expect("the element");

    let removed = remove_form_shows(&mut f.doc, f.page, &[(0, 1)], 0, &[0]).expect("removed");

    assert_eq!(removed.struct_carriers, 1);
    assert!(!f
        .doc
        .get_dictionary(element)
        .expect("the element")
        .has(b"Alt"));
}

/// A picture taken off a page takes the page span's `/Alt` with it.
#[test]
fn a_picture_taken_off_a_page_takes_the_spans_alt() {
    let mut f = fixture("", Dictionary::new(), None);
    let image = picture(&mut f.doc);
    let content = f.doc.add_object(Stream::new(
        dictionary! {},
        b"/Figure << /Alt (A PORTRAIT OF THE SIGNER) >> BDC\nq 10 0 0 10 0 0 cm /Im1 Do Q\nEMC\n"
            .to_vec(),
    ));
    let page = f
        .doc
        .get_object_mut(f.page)
        .and_then(Object::as_dict_mut)
        .expect("the page");
    page.set("Contents", content);
    page.set(
        "Resources",
        dictionary! { "XObject" => dictionary! { "Im1" => image } },
    );

    let removed = remove_images(&mut f.doc, f.page, &[0], 1).expect("removed");

    assert_eq!(removed.carriers, 1);
    let written = f.doc.get_page_content(f.page);
    assert!(!String::from_utf8_lossy(&written).contains("PORTRAIT"));
}
