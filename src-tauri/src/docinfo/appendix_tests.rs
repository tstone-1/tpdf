//! Which pages an append touched: `read_appendix`'s page count, on documents
//! built here with one revision appended by `lopdf`.
//!
//! Driven through `read_appendix` directly, with the end of the first
//! revision as the end of a signed range. Nothing here needs a signature: the
//! count is a comparison of two parses, and what a signature adds to it is the
//! number `end`.

use lopdf::{dictionary, Dictionary, Document, IncrementalDocument, Object, ObjectId, Stream};

use super::{read_appendix, Appendix, PageListing};

/// A two-page document, and the objects a test rewrites.
struct Built {
    bytes: Vec<u8>,
    catalog: ObjectId,
    /// The page tree's root, which both pages inherit from.
    pages: ObjectId,
    /// The two branches under it, each holding one of the pages.
    branch: [ObjectId; 2],
    /// The two pages, the first with a link to the second.
    page: [ObjectId; 2],
    /// Each page's own content stream.
    content: [ObjectId; 2],
    /// A form both pages draw, through resources each page states itself.
    shared: ObjectId,
    /// The font in the resources both pages inherit from the tree's root.
    inherited_font: ObjectId,
    /// A text field's dictionary; its widget is on the first page.
    field: ObjectId,
}

fn built() -> Built {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let branch = [doc.new_object_id(), doc.new_object_id()];
    let shared = doc.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()] },
        b"0 0 10 10 re f".to_vec(),
    ));
    let inherited_font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let inherited = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => inherited_font },
    });
    let content = [
        doc.add_object(Stream::new(dictionary! {}, b"/Fm0 Do".to_vec())),
        doc.add_object(Stream::new(dictionary! {}, b"/Fm0 Do".to_vec())),
    ];
    let second = doc.new_object_id();
    let first = doc.new_object_id();
    let field = doc.new_object_id();
    let widget = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "Parent" => field, "P" => first,
        "Rect" => vec![10.into(), 10.into(), 90.into(), 30.into()],
    });
    doc.objects.insert(
        field,
        Object::Dictionary(dictionary! {
            "FT" => "Tx", "T" => Object::string_literal("Name"),
            "V" => Object::string_literal("as signed"),
            "Kids" => vec![widget.into()],
        }),
    );
    let link = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "P" => first,
        "Rect" => vec![10.into(), 40.into(), 90.into(), 60.into()],
        "Dest" => vec![second.into(), "Fit".into()],
    });
    let own = || dictionary! { "XObject" => dictionary! { "Fm0" => shared } };
    doc.objects.insert(
        first,
        Object::Dictionary(dictionary! {
            "Type" => "Page", "Parent" => branch[0], "Contents" => content[0],
            "Resources" => own(), "Annots" => vec![widget.into(), link.into()],
        }),
    );
    doc.objects.insert(
        second,
        Object::Dictionary(dictionary! {
            "Type" => "Page", "Parent" => branch[1], "Contents" => content[1],
            "Resources" => own(),
        }),
    );
    for (node, kid) in branch.into_iter().zip([first, second]) {
        doc.objects.insert(
            node,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Parent" => pages, "Kids" => vec![kid.into()], "Count" => 1,
            }),
        );
    }
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![branch[0].into(), branch[1].into()], "Count" => 2,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
            "Resources" => inherited,
        }),
    );
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages,
        "AcroForm" => dictionary! { "Fields" => vec![field.into()] },
    });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    Built {
        bytes,
        catalog,
        pages,
        branch,
        page: [first, second],
        content,
        shared,
        inherited_font,
        field,
    }
}

/// `built` with one revision appended: `edit` is given the document as it
/// stood and the revision to fill.
fn revised(built: &Built, edit: impl FnOnce(&Document, &mut Document)) -> Vec<u8> {
    let prev = Document::load_mem(&built.bytes).expect("the document parses");
    let before = prev.clone();
    let mut incremental = IncrementalDocument::create_from(built.bytes.clone(), prev);
    edit(&before, &mut incremental.new_document);
    let mut whole = Vec::new();
    incremental.save_to(&mut whole).expect("saved");
    whole
}

/// What `read_appendix` says of `whole`, with `built` as the signed part.
fn read(built: &Built, whole: &[u8]) -> Appendix {
    assert_eq!(&whole[..built.bytes.len()], built.bytes, "an append");
    let appendix = read_appendix(whole, built.bytes.len(), None);
    assert!(!appendix.unread, "the appendix reads");
    appendix
}

/// What `read_appendix` says of `built` with one revision appended by `edit`.
fn appended(built: &Built, edit: impl FnOnce(&Document, &mut Document)) -> Appendix {
    read(built, &revised(built, edit))
}

/// `bytes` with one more revision that is a cross-reference stream and
/// nothing else, marking each of `freed` as a free object.
///
/// Written by hand, because `lopdf` writes no free entry: the stream's own
/// object, an entry of type 0 for each freed object and one of type 1 for the
/// stream itself, and `/Prev` naming the section before it.
fn with_freed(bytes: &[u8], catalog: ObjectId, freed: &[ObjectId]) -> Vec<u8> {
    let listed: Vec<(ObjectId, Option<usize>)> = freed.iter().map(|id| (*id, None)).collect();
    with_section(bytes, catalog, &listed)
}

/// Where the last cross-reference section of `bytes` starts.
fn last_section(bytes: &[u8]) -> usize {
    let last = |needle: &[u8]| {
        bytes
            .windows(needle.len())
            .rposition(|window| window == needle)
            .expect("the marker")
    };
    let at = last(b"startxref") + b"startxref".len();
    String::from_utf8_lossy(&bytes[at..last(b"%%EOF")])
        .trim()
        .parse()
        .expect("where the last section starts")
}

/// The same revision with each listed object either free (`None`) or said to
/// be at an offset.
fn with_section(bytes: &[u8], catalog: ObjectId, listed: &[(ObjectId, Option<usize>)]) -> Vec<u8> {
    let prev = last_section(bytes);
    let own = Document::load_mem(bytes).expect("parses").max_id + 1;
    let mut listed: Vec<(u32, Option<usize>)> = listed.iter().map(|(id, at)| (id.0, *at)).collect();
    listed.sort_unstable();

    let mut out = bytes.to_vec();
    out.push(b'\n');
    let start = out.len();
    // `/W [1 4 2]`: the type, then an offset (or the next free object), then
    // a generation.
    let mut entries = Vec::new();
    let mut index = String::new();
    for (id, at) in &listed {
        match at {
            None => entries.extend_from_slice(&[0, 0, 0, 0, 0, 0xff, 0xff]),
            Some(at) => {
                entries.push(1);
                entries.extend_from_slice(&u32::try_from(*at).expect("small").to_be_bytes());
                entries.extend_from_slice(&[0, 0]);
            }
        }
        index.push_str(&format!("{id} 1 "));
    }
    entries.push(1);
    entries.extend_from_slice(&u32::try_from(start).expect("small").to_be_bytes());
    entries.extend_from_slice(&[0, 0]);
    index.push_str(&format!("{own} 1"));
    out.extend_from_slice(
        format!(
            "{own} 0 obj\n<< /Type /XRef /Size {} /W [1 4 2] /Index [{index}] \
             /Root {} {} R /Prev {prev} /Length {} >>\nstream\n",
            own + 1,
            catalog.0,
            catalog.1,
            entries.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&entries);
    out.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{start}\n%%EOF\n").as_bytes());
    out
}

/// `bytes` with one more revision that is a classic cross-reference table and
/// its trailer, marking `freed` free. `more` is added to the trailer.
fn with_freed_in_a_table(bytes: &[u8], catalog: ObjectId, freed: ObjectId, more: &str) -> Vec<u8> {
    let prev = last_section(bytes);
    let size = Document::load_mem(bytes).expect("parses").max_id + 1;
    let mut out = bytes.to_vec();
    out.push(b'\n');
    let start = out.len();
    out.extend_from_slice(
        format!(
            "xref\n{} 1\n0000000000 00001 f \ntrailer\n<< /Size {size} /Root {} {} R \
             /Prev {prev} {more} >>\nstartxref\n{start}\n%%EOF\n",
            freed.0, catalog.0, catalog.1
        )
        .as_bytes(),
    );
    out
}

/// The revision a second signer appends, as far as this reading goes: a
/// signature field whose widget is added to the first page's annotations.
fn signs_the_first_page(built: &Built, before: &Document, new: &mut Document) {
    let value = new.add_object(dictionary! {
        "Type" => "Sig", "Filter" => "Adobe.PPKLite", "SubFilter" => "adbe.pkcs7.detached",
    });
    let widget = new.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Sig",
        "T" => Object::string_literal("Synthetic second signer"),
        "V" => value, "P" => built.page[0],
        "Rect" => vec![0.into(), 0.into(), 0.into(), 0.into()],
    });
    let mut page = dict_of(before, built.page[0]);
    let mut annots = page
        .get(b"Annots")
        .and_then(Object::as_array)
        .expect("the first page's annotations")
        .clone();
    annots.push(widget.into());
    page.set("Annots", annots);
    new.set_object(built.page[0], page);
}

/// What `--strict` makes of an appendix after an intact signature.
fn strict(appendix: Appendix) -> crate::cli::verify::After {
    crate::cli::verify::after_last_signature(&[signature(
        crate::integrity::Verdict::Intact,
        900,
        Some(appendix),
    )])
}

/// The dictionary of `id` as it stood, to write again with one thing changed.
fn dict_of(document: &Document, id: ObjectId) -> Dictionary {
    document
        .get_object(id)
        .and_then(Object::as_dict)
        .expect("a dictionary")
        .clone()
}

/// The append this count exists for: a page's content stream written again.
/// The page object is not among what was replaced, and the page is what
/// changed.
#[test]
fn a_content_stream_replaced_after_signing_touches_its_page_and_no_other() {
    let built = built();
    let appendix = appended(&built, |_, new| {
        new.set_object(
            built.content[0],
            Stream::new(dictionary! {}, b"0 0 200 200 re f".to_vec()),
        );
    });
    assert_eq!(appendix.replaced, 1, "{appendix:?}");
    assert_eq!(appendix.kinds, ["stream"]);
    assert_eq!(appendix.pages_touched, 1, "{appendix:?}");
    assert_eq!(appendix.pages_listing, Vec::<PageListing>::new());
}

/// The same append with nothing about the stream's dictionary changed: the
/// new content is as long as the old one. `lopdf` prints a stream without its
/// bytes, so a comparison of what it prints called this stream unchanged, the
/// page untouched, and `verify --strict` passed a page drawn differently
/// after it was signed.
#[test]
fn a_content_stream_replaced_with_the_same_length_touches_its_page() {
    use crate::cli::verify::After;
    let built = built();
    // What the fixture holds, so that "the same length" is a fact about it.
    let was = Document::load_mem(&built.bytes).expect("parses");
    let old = &was
        .get_object(built.content[0])
        .and_then(Object::as_stream)
        .expect("a stream")
        .content;
    let new_content = b"/Fm9 Do".to_vec();
    assert_eq!(old.len(), new_content.len());
    assert_ne!(old, &new_content);

    let appendix = appended(&built, |_, new| {
        new.set_object(
            built.content[0],
            Stream::new(dictionary! {}, new_content.clone()),
        );
    });
    assert_eq!(appendix.replaced, 1, "{appendix:?}");
    assert_eq!(appendix.pages_touched, 1, "{appendix:?}");
    assert_eq!(strict(appendix), After::Pages(1));

    // The control: the same stream written again byte for byte is not a
    // replacement, so the comparison is of the content and not of the fact
    // that a stream was written.
    let again = appended(&built, |_, new| {
        new.set_object(built.content[0], Stream::new(dictionary! {}, old.clone()));
    });
    assert_eq!((again.replaced, again.pages_touched), (0, 0), "{again:?}");
    assert_eq!(strict(again), After::Unchanged);
}

/// And beside a signature field added in the same revision. The page object
/// is written again to list the field, which is the one rewrite reported as
/// "the page's content is unchanged"; a content stream replaced at its old
/// length must take that away.
#[test]
fn a_same_length_replacement_beside_a_new_signature_field_is_not_a_listing() {
    use crate::cli::verify::After;
    let built = built();
    // The control: the field alone is a listing, and passes.
    let signed = appended(&built, |before, new| {
        signs_the_first_page(&built, before, new)
    });
    assert_eq!(
        signed.pages_listing,
        [PageListing {
            page: 1,
            timestamp: false
        }],
        "{signed:?}"
    );
    assert_eq!(signed.pages_touched, 1);
    assert_eq!(strict(signed), After::Unchanged);

    let appendix = appended(&built, |before, new| {
        signs_the_first_page(&built, before, new);
        new.set_object(
            built.content[0],
            Stream::new(dictionary! {}, b"/Fm9 Do".to_vec()),
        );
    });
    assert_eq!(appendix.pages_listing, Vec::<PageListing>::new());
    assert_eq!(appendix.pages_touched, 1, "{appendix:?}");
    assert_eq!(strict(appendix), After::Pages(1));
}

/// An object freed after signing: one cross-reference stream appended, which
/// marks the first page's content stream free and writes no other object.
/// A reader then finds nothing where the page's content was. No object was
/// added or replaced for that, so the page is found from what was removed,
/// in the signed document, where the page still reaches it.
#[test]
fn an_object_freed_after_signing_touches_the_page_that_drew_it() {
    let built = built();
    let freed = read(
        &built,
        &with_freed(&built.bytes, built.catalog, &[built.content[0]]),
    );
    assert_eq!(freed.removed, 1, "{freed:?}");
    assert_eq!(freed.pages_touched, 1, "{freed:?}");
    // The cross-reference stream is the one object that arrived.
    assert_eq!((freed.added, freed.replaced), (1, 0), "{freed:?}");
    assert_eq!(freed.kinds, ["stream"]);

    // Control: the same revision with no object freed touches nothing, so
    // what is counted is the freeing and not a cross-reference stream.
    let alone = read(&built, &with_freed(&built.bytes, built.catalog, &[]));
    assert_eq!(
        (alone.added, alone.removed, alone.pages_touched),
        (1, 0, 0),
        "{alone:?}"
    );

    // Control: the stream emptied and not freed is a replacement, and one
    // page rewritten, as it was before removal was counted.
    let emptied = appended(&built, |_, new| {
        new.set_object(built.content[0], Stream::new(dictionary! {}, Vec::new()));
    });
    assert_eq!(
        (emptied.replaced, emptied.removed, emptied.pages_touched),
        (1, 0, 1),
        "{emptied:?}"
    );
}

/// The same in a classic table. PDFium 8066 still draws the page then and
/// poppler reports the object as not found, so which a recipient sees is
/// their reader's choice and the report counts it.
#[test]
fn an_object_freed_in_a_classic_table_is_counted_too() {
    let built = built();
    let freed = read(
        &built,
        &with_freed_in_a_table(&built.bytes, built.catalog, built.content[0], ""),
    );
    assert_eq!(
        (
            freed.added,
            freed.replaced,
            freed.removed,
            freed.pages_touched
        ),
        (0, 0, 1, 1),
        "{freed:?}"
    );
}

/// A section this cannot follow is an appendix that could not be read, and
/// never one in which nothing was freed. Here the trailer names its own
/// cross-reference stream by a reference, which `lopdf` passes over and this
/// does not resolve.
#[test]
fn a_section_that_cannot_be_followed_makes_the_appendix_unread() {
    let built = built();
    let stated = |more: &str| {
        let whole = with_freed_in_a_table(&built.bytes, built.catalog, built.content[0], more);
        read_appendix(&whole, built.bytes.len(), None)
    };
    let appendix = stated("/XRefStm 1 0 R");
    assert!(appendix.unread, "{appendix:?}");
    assert_eq!(strict(appendix), crate::cli::verify::After::Unread);
    // The control: an entry that states nothing about a section is read.
    assert!(!stated("/Synthetic 1 0 R").unread);
}

/// An object the whole document no longer has at all: its entry is written
/// again to point at another object, and a parser that takes an object's
/// number from the file finds the first page's content stream nowhere.
#[test]
fn an_object_the_whole_document_no_longer_has_is_removed() {
    let built = built();
    let needle = format!("\n{} 0 obj", built.content[1].0);
    let other = built
        .bytes
        .windows(needle.len())
        .position(|window| window == needle.as_bytes())
        .expect("the second page's content stream")
        + 1;
    let moved = read(
        &built,
        &with_section(
            &built.bytes,
            built.catalog,
            &[(built.content[0], Some(other))],
        ),
    );
    assert_eq!((moved.removed, moved.pages_touched), (1, 1), "{moved:?}");
}

/// What two pages drew is both pages' when it goes, and a page object that
/// is itself removed is one page and not two: it is found among what was
/// removed and is not counted again for no longer being where it was.
#[test]
fn a_freed_object_is_followed_over_the_signed_document() {
    let built = built();
    let shared = read(
        &built,
        &with_freed(&built.bytes, built.catalog, &[built.shared]),
    );
    assert_eq!((shared.removed, shared.pages_touched), (1, 2), "{shared:?}");

    let page = read(
        &built,
        &with_freed(&built.bytes, built.catalog, &[built.page[1]]),
    );
    assert_eq!((page.removed, page.pages_touched), (1, 1), "{page:?}");

    // An annotation is its page's too: the link on the first page.
    let was = Document::load_mem(&built.bytes).expect("parses");
    let link = was
        .objects
        .iter()
        .find_map(|(id, object)| {
            let dict = object.as_dict().ok()?;
            (dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Link".as_slice()))
                .then_some(*id)
        })
        .expect("the link");
    let linked = read(&built, &with_freed(&built.bytes, built.catalog, &[link]));
    assert_eq!((linked.removed, linked.pages_touched), (1, 1), "{linked:?}");
}

/// A page rewritten to list a signature field, in a document that then loses
/// a font the page inherits: the page is not one whose rewrite "did one
/// thing", whatever its own object says.
#[test]
fn a_page_that_lost_what_it_drew_is_not_reported_as_only_listing_a_field() {
    use crate::cli::verify::After;
    let built = built();
    let signed = revised(&built, |before, new| {
        signs_the_first_page(&built, before, new)
    });
    // The control is the listing itself, read from the same bytes.
    assert_eq!(read(&built, &signed).pages_listing.len(), 1);

    let appendix = read(
        &built,
        &with_freed(&signed, built.catalog, &[built.inherited_font]),
    );
    assert_eq!(appendix.removed, 1, "{appendix:?}");
    assert_eq!(appendix.pages_listing, Vec::<PageListing>::new());
    assert_eq!(appendix.pages_touched, 2, "{appendix:?}");
    assert_eq!(strict(appendix), After::Pages(2));
}

/// `--strict` on the freed content stream: a page no signature covers, where
/// the same revision freeing nothing passes.
#[test]
fn strict_fails_a_signature_followed_by_a_freed_content_stream() {
    use crate::cli::verify::After;
    let built = built();
    let freed = read(
        &built,
        &with_freed(&built.bytes, built.catalog, &[built.content[0]]),
    );
    assert_eq!(strict(freed), After::Pages(1));
    let alone = read(&built, &with_freed(&built.bytes, built.catalog, &[]));
    assert_eq!(strict(alone), After::Unchanged);
}

/// The control for every count here: validation data, which is what most
/// appends after a signature are, reaches no page.
#[test]
fn validation_data_touches_no_page() {
    let built = built();
    let appendix = appended(&built, |before, new| {
        let certificate = new.add_object(Stream::new(dictionary! {}, vec![0x30, 0x00]));
        let dss = new.add_object(dictionary! { "Certs" => vec![certificate.into()] });
        let mut catalog = dict_of(before, built.catalog);
        catalog.set("DSS", dss);
        new.set_object(built.catalog, catalog);
    });
    assert_eq!(appendix.catalog_gained, ["DSS"]);
    assert_eq!(appendix.pages_touched, 0, "{appendix:?}");
}

/// What two pages share is both pages': a form each draws through its own
/// resources.
#[test]
fn an_object_two_pages_draw_touches_both() {
    let built = built();
    let appendix = appended(&built, |_, new| {
        new.set_object(
            built.shared,
            Stream::new(
                dictionary! { "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()] },
                b"0 0 5 5 re f".to_vec(),
            ),
        );
    });
    assert_eq!(appendix.pages_touched, 2, "{appendix:?}");
}

/// A field's value is what its widget shows, so a value written again touches
/// the page the widget is on --- and not the other one.
#[test]
fn a_field_value_changed_touches_the_page_its_widget_is_on() {
    let built = built();
    let appendix = appended(&built, |before, new| {
        let mut field = dict_of(before, built.field);
        field.set("V", Object::string_literal("changed afterwards"));
        new.set_object(built.field, field);
    });
    assert_eq!(appendix.pages_touched, 1, "{appendix:?}");
}

/// A page that names another --- here by a link's destination --- does not
/// draw from it. The second page is written again, the first links to it, and
/// one page is touched: without the rule that a path ends at a page, both
/// would be.
#[test]
fn a_page_that_links_to_a_rewritten_page_is_not_touched_by_that() {
    let built = built();
    let appendix = appended(&built, |before, new| {
        let mut second = dict_of(before, built.page[1]);
        second.set("Rotate", 90);
        new.set_object(built.page[1], second);
    });
    assert_eq!(appendix.pages_touched, 1, "{appendix:?}");
    // Not a listing: nothing was added to its annotations.
    assert_eq!(appendix.pages_listing, Vec::<PageListing>::new());

    // The same when the second page is reached rather than written: its
    // content stream changes, the walk back arrives at the page, and stops
    // there instead of going on to the link that names it.
    let appendix = appended(&built, |_, new| {
        new.set_object(
            built.content[1],
            Stream::new(dictionary! {}, b"0 0 200 200 re f".to_vec()),
        );
    });
    assert_eq!(appendix.pages_touched, 1, "{appendix:?}");
}

/// What a page takes from the tree above it is the page's too. The font is
/// in resources neither page states: both inherit them from the root, two
/// nodes up.
#[test]
fn what_a_page_inherits_touches_every_page_below_it() {
    let built = built();
    let font = appended(&built, |_, new| {
        new.set_object(
            built.inherited_font,
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier" },
        );
    });
    assert_eq!(font.pages_touched, 2, "{font:?}");
    // And the node itself, written again with something a page inherits
    // stated directly in it.
    let node = appended(&built, |before, new| {
        let mut pages = dict_of(before, built.pages);
        pages.set("Rotate", 180);
        new.set_object(built.pages, pages);
    });
    assert_eq!(node.pages_touched, 2, "{node:?}");
}

/// A branch of the tree written again touches the pages under it, and not the
/// pages of the branch beside it: a node hands down, and never across.
#[test]
fn a_branch_of_the_tree_rewritten_touches_only_the_pages_under_it() {
    let built = built();
    let appendix = appended(&built, |before, new| {
        let mut branch = dict_of(before, built.branch[1]);
        branch.set("Rotate", 90);
        new.set_object(built.branch[1], branch);
    });
    assert_eq!(appendix.pages_touched, 1, "{appendix:?}");
}

/// A page taken out of the document with no page object written: the tree's
/// root loses a branch. The page left is under a rewritten node; the one
/// removed is in no tree to be found under, and is counted because it is no
/// longer where it was.
#[test]
fn a_page_taken_out_of_the_document_is_counted() {
    let built = built();
    let appendix = appended(&built, |before, new| {
        let mut pages = dict_of(before, built.pages);
        pages.set("Kids", vec![built.branch[0].into()]);
        pages.set("Count", 1);
        new.set_object(built.pages, pages);
    });
    assert_eq!(appendix.pages_touched, 2, "{appendix:?}");

    // The same from the catalog, with no node of the tree written at all:
    // it names a tree that holds the second page alone.
    let appendix = appended(&built, |before, new| {
        let alone = new.add_object(dictionary! {
            "Type" => "Pages", "Kids" => vec![built.page[1].into()], "Count" => 1,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        });
        let mut catalog = dict_of(before, built.catalog);
        catalog.set("Pages", alone);
        new.set_object(built.catalog, catalog);
    });
    assert_eq!(appendix.pages_touched, 2, "{appendix:?}");
}

// ------------------------------------------ what `verify --strict` makes of it

/// A signed field with this verdict, this many bytes after its range, and
/// this appendix.
fn signature(
    verdict: crate::integrity::Verdict,
    appended_bytes: u64,
    appendix: Option<Appendix>,
) -> super::Signature {
    super::Signature {
        signed: true,
        integrity: Some(crate::integrity::Integrity {
            verdict,
            ..Default::default()
        }),
        appended_bytes,
        appendix,
        ..Default::default()
    }
}

/// The rule, on appendices stated by hand: what follows the last intact
/// signature passes when it touches no page, or touches pages only to list a
/// signature or timestamp field, and otherwise says how many pages, or that
/// it could not be read.
#[test]
fn what_follows_the_last_intact_signature_is_judged_by_the_pages_it_touches() {
    use crate::cli::verify::{after_last_signature, After};
    use crate::integrity::Verdict::{Altered, Intact};
    let touching = |pages_touched: usize, listed: usize| {
        Some(Appendix {
            pages_touched,
            pages_listing: vec![PageListing::default(); listed],
            ..Appendix::default()
        })
    };
    let unread = Some(Appendix {
        unread: true,
        ..Appendix::default()
    });
    for (what, signatures, want) in [
        (
            "nothing follows",
            vec![signature(Intact, 0, None)],
            After::Unchanged,
        ),
        (
            "validation data follows",
            vec![signature(Intact, 900, touching(0, 0))],
            After::Unchanged,
        ),
        (
            "a field was listed on a page",
            vec![signature(Intact, 900, touching(1, 1))],
            After::Unchanged,
        ),
        (
            "a page was rewritten",
            vec![signature(Intact, 900, touching(1, 0))],
            After::Pages(1),
        ),
        (
            "a field was listed on one page and two more were rewritten",
            vec![signature(Intact, 900, touching(3, 1))],
            After::Pages(2),
        ),
        (
            "what follows could not be read",
            vec![signature(Intact, 900, unread)],
            After::Unread,
        ),
        // A later signature that covers the rewriting answers for it.
        (
            "a second signature covers what followed the first",
            vec![
                signature(Intact, 900, touching(1, 0)),
                signature(Intact, 0, None),
            ],
            After::Unchanged,
        ),
        // One that is not intact answers for nothing, however far its range
        // says it reaches.
        (
            "the signature after the rewriting is not intact",
            vec![
                signature(Intact, 900, touching(1, 0)),
                signature(Altered, 0, None),
            ],
            After::Pages(1),
        ),
        // The order the fields come in is the document's, not the order they
        // were signed in.
        (
            "the last signature is listed first",
            vec![
                signature(Intact, 0, None),
                signature(Intact, 900, touching(1, 0)),
            ],
            After::Unchanged,
        ),
    ] {
        assert_eq!(after_last_signature(&signatures), want, "{what}");
    }
}

/// Each answer that is not `Unchanged` fails `--strict` and is said under the
/// document's signatures; `Unchanged` passes and says nothing.
#[test]
fn what_follows_the_last_signature_decides_strict_and_is_said() {
    use crate::cli::report::File;
    use crate::cli::verify::{after_text, signature_report, verified_after, After};
    let holds = super::Signature {
        trust: Some(crate::trust::Trust {
            standing: crate::trust::Standing::Trusted,
            ..Default::default()
        }),
        ..signature(crate::integrity::Verdict::Intact, 900, None)
    };
    let file = || File {
        path: "signed.pdf".into(),
        error: None,
        signatures: vec![signature_report(&holds)],
    };
    for (after, passes, said) in [
        (After::Unchanged, true, None),
        (
            After::Pages(1),
            false,
            Some("  After the last signature: 1 page was rewritten, which no signature covers"),
        ),
        (
            After::Pages(3),
            false,
            Some("  After the last signature: 3 pages were rewritten, which no signature covers"),
        ),
        (
            After::Unread,
            false,
            Some(
                "  After the last signature: something was appended that tpdf could not read, \
                 and no signature covers it",
            ),
        ),
    ] {
        assert_eq!(
            verified_after(vec![file()], &[after]).strict_passed,
            passes,
            "{after:?}"
        );
        assert_eq!(after_text(after).as_deref(), said, "{after:?}");
    }
    // Each document by its own answer: the second of two is the one that
    // fails, and a document with no answer has nothing known to follow.
    let both = |after: &[After]| verified_after(vec![file(), file()], after).strict_passed;
    assert!(both(&[After::Unchanged, After::Unchanged]));
    assert!(!both(&[After::Unchanged, After::Pages(1)]));
    assert!(both(&[After::Unchanged]));
}
