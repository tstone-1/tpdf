//! Pages taken from another file: [`Doc::import`] and [`Command::Import`].
//!
//! A module of its own for `docmodel_text_tests.rs`'s reason: the model's own
//! test module is five thousand lines, and these share one fixture --- a
//! [`SourceFile`] nothing ever opens, because the model never opens one.

use super::*;
use crate::fingerprint::Fingerprint;

/// A file of `pages` pages. Its path names nothing and its digest describes
/// no bytes: the model records both and reads neither.
fn file(pages: u32) -> SourceFile {
    file_with(pages, 7)
}

/// [`file`] with a chosen digest, so two records of one path can differ.
fn file_with(pages: u32, digest: u8) -> SourceFile {
    SourceFile {
        path: "/nonexistent/other.pdf".into(),
        fingerprint: Fingerprint {
            len: 1000,
            modified_ns: None,
            digest: [digest; 32],
        },
        pages,
    }
}

fn ids(doc: &Doc) -> Vec<u64> {
    doc.working().order().iter().map(|p| p.get()).collect()
}

fn source_of(doc: &Doc, page: PageId) -> PageSource {
    doc.working().page(page).expect("a live page").source
}

/// The pages land behind the anchor, in the order asked for rather than the
/// file's, and each says which page of which file it shows.
///
/// **Out of file order on purpose**: a selection of `[0, 1]` is what a correct
/// placement and one that sorted the list both produce.
#[test]
fn an_import_places_its_pages_behind_the_anchor_in_the_order_asked() {
    let mut doc = Doc::open(3);
    let second = doc.working().order()[1];
    let placed = doc
        .import(Some(second), file(5), vec![4, 0, 2])
        .expect("import");

    assert_eq!(placed.len(), 3);
    assert_eq!(
        ids(&doc),
        vec![1, 2, placed[0].get(), placed[1].get(), placed[2].get(), 3]
    );
    let shown: Vec<u32> = placed
        .iter()
        .map(|&id| match source_of(&doc, id) {
            PageSource::Imported { page, .. } => page,
            other => panic!("an imported page answered {other:?}"),
        })
        .collect();
    assert_eq!(shown, vec![4, 0, 2], "the reader's order, not the file's");
    assert_eq!(
        source_of(&doc, second),
        PageSource::Baseline(1),
        "the anchor is untouched"
    );
    assert_eq!(
        placed.iter().map(|p| p.get()).collect::<Vec<_>>(),
        vec![4, 5, 6],
        "ids past the baseline's three, from the same allocator an insert uses"
    );
}

#[test]
fn an_import_with_no_anchor_goes_to_the_front() {
    let mut doc = Doc::open(2);
    let placed = doc.import(None, file(1), vec![0]).expect("import");
    assert_eq!(ids(&doc), vec![placed[0].get(), 1, 2]);
}

/// One press of undo takes every page away, and one press of redo brings the
/// same pages back --- the same ids, which is what a mark or a later move
/// naming one of them needs.
#[test]
fn one_undo_takes_away_a_whole_import_and_redo_brings_the_same_pages_back() {
    let mut doc = Doc::open(2);
    let placed = doc.import(None, file(4), vec![1, 3]).expect("import");
    let with = ids(&doc);
    assert_eq!(doc.depth(), (1, 0), "one command for two pages");

    assert!(doc.undo());
    assert_eq!(ids(&doc), vec![1, 2], "both pages went with one press");
    for id in &placed {
        assert!(doc.working().page(*id).is_none());
    }

    assert!(doc.redo());
    assert_eq!(ids(&doc), with, "and came back as themselves");
    assert!(matches!(
        source_of(&doc, placed[1]),
        PageSource::Imported { page: 3, .. }
    ));
}

/// Every refusal comes before any id is spent: the next page anything issues
/// is the first past the baseline, and no body is held.
#[test]
fn a_refused_import_spends_no_id_and_holds_no_body() {
    let mut doc = Doc::open(2);
    let first = doc.working().order()[0];
    doc.apply(Command::Delete { page: first }).expect("delete");

    assert_eq!(doc.import(None, file(3), vec![]), Err(Refusal::EmptyImport));
    assert_eq!(
        doc.import(None, file(3), vec![0, 3]),
        Err(Refusal::NoSuchSourcePage { page: 3, pages: 3 })
    );
    assert_eq!(
        doc.import(None, file(3), vec![1, 0, 1]),
        Err(Refusal::ImportedTwice(1))
    );
    assert_eq!(
        doc.import(Some(first), file(3), vec![0]),
        Err(Refusal::PageDeleted(first))
    );

    assert_eq!(doc.source_bodies(), 0);
    assert_eq!(doc.selection_bodies(), 0);
    assert_eq!(doc.depth(), (1, 0), "only the deletion is journalled");
    let made = doc
        .insert(
            None,
            Size {
                width: 10.0,
                height: 10.0,
            },
        )
        .expect("insert");
    assert_eq!(made.get(), 3, "no page id was spent by any refusal");
}

/// The apply refuses what the entry point refuses, for a command a caller
/// built itself --- `Doc::apply` is public.
#[test]
fn an_import_command_naming_no_selection_is_refused_by_the_apply() {
    let mut doc = Doc::open(1);
    let forged = SelectionId(9);
    assert_eq!(
        doc.apply(Command::Import {
            selection: forged,
            after: None
        }),
        Err(Refusal::NoSuchSelection(forged))
    );
    assert_eq!(doc.depth(), (0, 0));
}

/// Replay from a snapshot rebuilds the imported pages, not new ones: the
/// import sits below the snapshot and the undo target above it.
#[test]
fn an_import_survives_a_rebuild_through_a_snapshot() {
    let mut doc = Doc::open(1);
    let placed = doc.import(None, file(2), vec![1, 0]).expect("import");
    let base = doc.working().order()[2];
    for _ in 0..SNAPSHOT_EVERY {
        doc.apply(Command::Rotate {
            page: base,
            turns: 1,
        })
        .expect("turn");
    }
    let before = doc.working().clone();
    assert!(doc.snapshots() >= 1, "the premise: a snapshot exists");
    assert!(doc.undo());
    assert!(
        doc.replay_base(doc.depth().0) > 0,
        "the premise: the rebuild started from the snapshot, above the import"
    );
    assert!(doc.redo());
    assert_eq!(doc.working(), &before);
    assert!(matches!(
        source_of(&doc, placed[0]),
        PageSource::Imported { page: 1, .. }
    ));
}

/// Every page operation that works on a page of the opened file works on one
/// of another file.
#[test]
fn an_imported_page_turns_crops_moves_takes_a_mark_and_deletes() {
    let mut doc = Doc::open(2);
    let placed = doc.import(None, file(1), vec![0]).expect("import")[0];
    let last = doc.working().order()[2];

    doc.apply(Command::Rotate {
        page: placed,
        turns: 1,
    })
    .expect("turn");
    let box_pt = Rect {
        llx: 10.0,
        lly: 10.0,
        urx: 200.0,
        ury: 300.0,
    };
    doc.apply(Command::Crop {
        page: placed,
        to: Some(box_pt),
    })
    .expect("crop");
    doc.apply(Command::Move {
        page: placed,
        after: Some(last),
    })
    .expect("move");
    let mark = doc
        .annotate(
            Mark {
                kind: MarkKind::Highlight,
                stamp: None,
                image: None,
                reply_to: None,
                page: placed,
                quads: vec![Quad {
                    left: 72.0,
                    top: 90.0,
                    right: 300.0,
                    bottom: 108.0,
                }],
                strokes: Vec::new(),
                color: [1.0, 0.9, 0.2],
                width: INK_WIDTH,
                author: "a reader".to_string(),
                made: "D:20260919T120000Z".to_string(),
            },
            String::new(),
        )
        .expect("mark");

    let page = doc.working().page(placed).expect("live");
    assert_eq!(page.extra_turns, 1);
    assert_eq!(page.crop, Some(box_pt));
    assert_eq!(doc.working().order().last(), Some(&placed));
    assert_eq!(doc.working().page_of(mark), Some(placed));

    doc.apply(Command::Delete { page: placed }).expect("delete");
    assert!(doc.working().is_deleted(placed));
    assert_eq!(doc.working().page_of(mark), None, "the mark went with it");
}

/// A text replacement is validated against the opened document and addressed
/// by its page number there, which an imported page does not have.
#[test]
fn text_on_an_imported_page_is_refused() {
    let mut doc = Doc::open(1);
    let placed = doc.import(None, file(1), vec![0]).expect("import")[0];
    let change = crate::textedit::Change {
        layout: None,
        page: 0,
        revision: vec![1; 32],
        operator: 0,
        original: "SYNTHETIC ORIGINAL".into(),
        replacement: "SYNTHETIC EDIT".into(),
    };
    assert_eq!(
        doc.replace_text(placed, change),
        Err(Refusal::TextOnImportedPage(placed))
    );
    assert_eq!(doc.depth(), (1, 0));
}

/// A redaction anywhere is refused while any page came from another file,
/// and the refusal lifts when the last such page goes.
#[test]
fn a_redaction_is_refused_while_any_page_came_from_another_file() {
    let mut doc = Doc::open(2);
    let own = doc.working().order()[0];
    let placed = doc.import(None, file(1), vec![0]).expect("import")[0];
    let region = Redaction {
        page: own,
        area: Quad {
            left: 72.0,
            top: 90.0,
            right: 300.0,
            bottom: 108.0,
        },
    };
    assert_eq!(
        doc.redact(region),
        Err(Refusal::RedactionBesideImportedPages),
        "on a page of the opened file too"
    );

    doc.apply(Command::Delete { page: placed }).expect("delete");
    doc.redact(region)
        .expect("with no imported page left, the region is accepted");
}

#[test]
fn an_import_is_refused_while_regions_are_marked_for_removal() {
    let mut doc = Doc::open(1);
    let own = doc.working().order()[0];
    doc.redact(Redaction {
        page: own,
        area: Quad {
            left: 72.0,
            top: 90.0,
            right: 300.0,
            bottom: 108.0,
        },
    })
    .expect("mark a region");
    assert_eq!(
        doc.import(None, file(1), vec![0]),
        Err(Refusal::ImportBesideRedactions)
    );
    assert_eq!(doc.source_bodies(), 0);
}

/// A comment of the opened file cannot be on a page of another one, through
/// either entry point or through a command a caller built.
#[test]
fn a_foreign_comment_cannot_be_said_to_be_on_an_imported_page() {
    let mut doc = Doc::open(1);
    let placed = doc.import(None, file(1), vec![0]).expect("import")[0];
    let object = ObjectId::new(12, 0);

    assert_eq!(
        doc.rewrite(
            object,
            placed,
            "SYNTHETIC".into(),
            "D:20260919000000Z".into()
        ),
        Err(Refusal::ForeignCommentOnImportedPage(placed))
    );
    assert_eq!(doc.rewrite_bodies(), 0, "refused before a body was issued");
    assert_eq!(
        doc.discard(object, placed),
        Err(Refusal::ForeignCommentOnImportedPage(placed))
    );
    assert_eq!(
        doc.apply(Command::Discard {
            object,
            page: placed
        }),
        Err(Refusal::ForeignCommentOnImportedPage(placed))
    );
    let own = doc.working().order()[1];
    doc.discard(object, own)
        .expect("the control: the opened file's page takes it");
}

/// One file imported from twice is read once at save; the same path with
/// other bytes is another file.
#[test]
fn the_same_file_is_recorded_once_and_a_changed_one_again() {
    let mut doc = Doc::open(1);
    let a = doc.import(None, file(2), vec![0]).expect("first")[0];
    let b = doc.import(None, file(2), vec![0]).expect("again")[0];
    let c = doc.import(None, file_with(2, 9), vec![0]).expect("changed")[0];

    let source = |id| match source_of(&doc, id) {
        PageSource::Imported { source, .. } => source,
        other => panic!("{other:?}"),
    };
    assert_eq!(source(a), source(b));
    assert_ne!(source(a), source(c));
    assert_eq!(doc.source_bodies(), 2);
    assert_eq!(doc.selection_bodies(), 3);
    assert_ne!(a, b, "the same page twice, in two imports, is two pages");
}

/// A discarded redo tail drops the selection, and the file record with it
/// once no selection names it.
#[test]
fn a_discarded_import_takes_its_selection_and_its_file_record_with_it() {
    let mut doc = Doc::open(1);
    doc.import(None, file(2), vec![0]).expect("kept");
    doc.import(None, file_with(2, 9), vec![1]).expect("undone");
    assert!(doc.undo());
    assert_eq!(
        (doc.source_bodies(), doc.selection_bodies()),
        (2, 2),
        "the undone import's bodies stay while redo can reach them"
    );

    let own = doc.working().order()[1];
    doc.apply(Command::Rotate {
        page: own,
        turns: 1,
    })
    .expect("a new command discards the redo tail");
    assert_eq!((doc.source_bodies(), doc.selection_bodies()), (1, 1));
}

/// Undoing a command after an import rebuilds the working document by
/// replaying the import from the baseline, which is the one path that reads
/// the selection out of the table rather than out of the command that just
/// ran.
#[test]
fn undoing_a_later_command_replays_the_import_from_its_body() {
    let mut doc = Doc::open(1);
    let placed = doc.import(None, file(3), vec![2, 1]).expect("import");
    let with = doc.working().clone();
    doc.apply(Command::Rotate {
        page: placed[0],
        turns: 1,
    })
    .expect("turn");
    assert_eq!(
        doc.replay_base(1),
        0,
        "the premise: no snapshot to start from"
    );
    assert!(doc.undo());
    assert_eq!(doc.working(), &with);
}
