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

/// A replacement addressed in the other file, on `page` of it.
fn other_change(page: u32, replacement: &str) -> crate::textedit::Change {
    crate::textedit::Change {
        layout: None,
        page,
        revision: vec![1; 32],
        operator: 0,
        original: "SYNTHETIC ORIGINAL".into(),
        replacement: replacement.into(),
    }
}

/// A text replacement on an imported page is journalled against the **file the
/// page came from**, by its page number there --- and a page of the opened
/// file with the same number is untouched by it.
///
/// The second half is what makes this more than a smoke test: both edits are
/// page 0, operator 0, so a journal that recorded only the page number would
/// have one of them overwrite the other and every count would still agree.
#[test]
fn a_replacement_on_an_imported_page_names_the_file_it_came_from() {
    let mut doc = Doc::open(1);
    let own = doc.working().order()[0];
    let placed = doc.import(None, file(2), vec![0]).expect("import")[0];
    doc.replace_text(placed, other_change(0, "FROM THE OTHER FILE"))
        .expect("the other file's page is editable");
    doc.replace_text(
        own,
        crate::textedit::Change {
            replacement: "FROM THE OPENED FILE".into(),
            ..other_change(0, "")
        },
    )
    .expect("the opened file's page is editable");

    let mut edits = doc.text_changes();
    edits.sort_by_key(|edit| edit.change.replacement.clone());
    assert_eq!(
        edits
            .iter()
            .map(|edit| (edit.source, edit.change.replacement.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (None, "FROM THE OPENED FILE"),
            (Some(1), "FROM THE OTHER FILE"),
        ],
        "one entry per page, each naming the document its page number belongs to"
    );
}

/// Undo takes the imported page's replacement back, and redo brings it back
/// still naming the file it came from.
#[test]
fn undo_and_redo_keep_an_imported_replacement_with_its_file() {
    let mut doc = Doc::open(1);
    let placed = doc.import(None, file(2), vec![1]).expect("import")[0];
    doc.replace_text(placed, other_change(1, "EDITED")).unwrap();
    assert!(doc.undo());
    assert!(doc.text_changes().is_empty());
    assert!(doc.redo());
    assert_eq!(
        doc.text_changes()
            .iter()
            .map(|edit| (edit.source, edit.change.page))
            .collect::<Vec<_>>(),
        vec![(Some(1), 1)]
    );
    // And the import itself undone takes the replacement with it: the page is
    // gone, so nothing is addressed at all.
    assert!(doc.undo());
    assert!(doc.undo());
    assert!(doc.text_changes().is_empty());
}

/// A replacement whose page number is not this page's is refused, even when
/// the file is right --- the stale-address case, on the other file's side.
#[test]
fn a_replacement_addressed_at_another_page_of_the_same_file_is_refused() {
    let mut doc = Doc::open(1);
    let placed = doc.import(None, file(3), vec![2]).expect("import")[0];
    assert_eq!(
        doc.replace_text(placed, other_change(0, "WRONG PAGE")),
        Err(Refusal::TextEdit("text no longer belongs to this page"))
    );
    assert!(doc.text_changes().is_empty());
}

/// One page of one file in two positions cannot be edited: the writer edits
/// the file and imports it once per position, so the edit would appear in
/// both.
#[test]
fn text_on_a_page_this_document_shows_twice_is_refused() {
    let mut doc = Doc::open(1);
    let first = doc.import(None, file(2), vec![0]).expect("first")[0];
    let second = doc.import(None, file(2), vec![0]).expect("second")[0];
    assert_eq!(
        doc.replace_text(first, other_change(0, "EDITED")),
        Err(Refusal::TextOnRepeatedImport(first))
    );
    assert_eq!(
        doc.replace_text(second, other_change(0, "EDITED")),
        Err(Refusal::TextOnRepeatedImport(second))
    );
    assert!(doc.text_changes().is_empty());
    // Deleting one of the two is what the refusal tells the reader to do, and
    // it is enough: the other is then the only position showing that page.
    doc.apply(Command::Delete { page: second }).expect("delete");
    doc.replace_text(first, other_change(0, "EDITED"))
        .expect("the one that is left is editable");
    // A *different* page of the same file was never in question.
    let other = doc.import(None, file(2), vec![1]).expect("third")[0];
    doc.replace_text(other, other_change(1, "ALSO EDITED"))
        .expect("a page placed once is editable");
}

/// Inserting a page that already carries a replacement is refused, which is
/// the same rule met from the other side --- and it names the page.
#[test]
fn inserting_a_page_that_already_has_edited_text_is_refused() {
    let mut doc = Doc::open(1);
    let placed = doc.import(None, file(2), vec![0]).expect("import")[0];
    doc.replace_text(placed, other_change(0, "EDITED")).unwrap();
    let depth = doc.depth();
    assert_eq!(
        doc.import(None, file(2), vec![1, 0]),
        Err(Refusal::ImportOfEditedPage(0))
    );
    assert_eq!(doc.depth(), depth, "a refused import spends nothing");
    // The page that is not edited may still be inserted.
    doc.import(None, file(2), vec![1])
        .expect("an unedited page of the same file");
    // And a different file's page 0 is a different page: its own record, its
    // own id, nothing to collide with.
    doc.import(None, file_with(2, 9), vec![0])
        .expect("another file's page 0");
}

/// A region of the reader's own page while an inserted page is live.
///
/// ⚠ **This test asserted the opposite until 2026-09-20**, under the name
/// `a_redaction_is_refused_while_any_page_came_from_another_file`, and its own
/// message said what was wrong with it: *"on a page of the opened file too"*.
/// The refusal it pinned was document-wide, and three of the four steps it
/// named were routing --- see [`Refusal::RedactionOnImportedPage`], which
/// records each of them and which one is still true.
///
/// The accept here is the narrowing; the two refusals below are what is left.
#[test]
fn a_region_on_the_reader_s_own_page_is_accepted_beside_an_inserted_one() {
    let mut doc = Doc::open(2);
    let own = doc.working().order()[0];
    let placed = doc.import(None, file(2), vec![0]).expect("import")[0];
    let mine = doc.redact(region_on(own)).expect("a region on my own page");
    assert_eq!(doc.working().redactions_on(own), &[mine]);

    // The inserted page is not refused for being beside a marked region
    // either, which is the same rule from the other side --- and the variant
    // that used to say so is gone with it.
    doc.import(None, file(2), vec![1])
        .expect("a second page inserted while a region is marked");
    assert_eq!(doc.source_bodies(), 1, "one file, imported from twice");

    // And the marked page is still the only marked one, which is what says the
    // accept above did not quietly mark something else.
    assert!(doc.working().redactions_on(placed).is_empty());
}

/// A region on a page that came from another file, by either route to it.
///
/// **Per page, which is `Refusal::CropOnMadePage`'s shape**: the sentence names
/// what is true of this page rather than of the document around it.
#[test]
fn a_region_on_a_page_from_another_file_is_refused() {
    let mut doc = Doc::open(2);
    let placed = doc.import(None, file(1), vec![0]).expect("import")[0];
    let depth = doc.depth();
    assert_eq!(
        doc.redact(region_on(placed)),
        Err(Refusal::RedactionOnImportedPage(placed))
    );
    assert_eq!(doc.depth(), depth, "a refused redaction spends nothing");
    assert!(doc.working().redactions_on(placed).is_empty());

    // The refusal a reader is shown names the page and the way out, and does
    // not name the document --- the sentence is the narrowing as much as the
    // variant is.
    let said = crate::edits::describe(Refusal::RedactionOnImportedPage(placed));
    assert!(
        said.contains("that page came from another document"),
        "{said:?}"
    );
}

/// The repeated-import hazard, answered by the refusal above rather than by one
/// of its own.
///
/// **A removal reaches the file, not the position.** `save::apply_redactions`
/// edits the page *object*, so a region on a page the document shows twice
/// would strike both positions --- which is exactly the shape
/// [`Refusal::TextOnRepeatedImport`] exists for on the text side. There is no
/// counterpart here and there does not need to be: neither position of an
/// imported page can carry a region at all, so the hazard has no way in.
///
/// Asserted on **both** placements, because a check that refused only the
/// second would pass an assertion about the first.
#[test]
fn a_page_inserted_twice_can_be_redacted_at_neither_position() {
    let mut doc = Doc::open(2);
    let own = doc.working().order()[0];
    let first = doc.import(None, file(1), vec![0]).expect("import")[0];
    let second = doc.import(None, file(1), vec![0]).expect("again")[0];
    assert_ne!(
        first, second,
        "two positions, two ids, one page of one file"
    );

    for placed in [first, second] {
        assert_eq!(
            doc.redact(region_on(placed)),
            Err(Refusal::RedactionOnImportedPage(placed))
        );
    }
    // And the document is still redactable where it is the reader's own.
    doc.redact(region_on(own)).expect("my own page");
}

/// The one rectangle these three tests mark, so none of them can pass by
/// marking a different one.
fn region_on(page: crate::docmodel::PageId) -> Redaction {
    Redaction {
        page,
        area: Quad {
            left: 72.0,
            top: 90.0,
            right: 300.0,
            bottom: 108.0,
        },
    }
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
