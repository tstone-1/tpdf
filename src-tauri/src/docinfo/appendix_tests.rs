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

/// What `read_appendix` says of `built` with one revision appended: `edit` is
/// given the document as it stood and the revision to fill.
fn appended(built: &Built, edit: impl FnOnce(&Document, &mut Document)) -> Appendix {
    let prev = Document::load_mem(&built.bytes).expect("the document parses");
    let before = prev.clone();
    let mut incremental = IncrementalDocument::create_from(built.bytes.clone(), prev);
    edit(&before, &mut incremental.new_document);
    let mut whole = Vec::new();
    incremental.save_to(&mut whole).expect("saved");
    let appendix = read_appendix(&whole, built.bytes.len(), None);
    assert!(!appendix.unread, "the appendix reads");
    appendix
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
