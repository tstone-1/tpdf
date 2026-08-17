//! The open documents' edit models, and the shapes the frontend sees.
//!
//! [`docmodel`](crate::docmodel) is deliberately a pure module: it holds no
//! file, no bytes and no `lopdf` object, which is what lets its tests drive it
//! directly. This is the layer that gives it a home --- one [`Doc`] per open
//! document, keyed by the same `doc` handle every other command takes --- and
//! the layer that translates between the two vocabularies at the boundary.
//!
//! **The two vocabularies, and why the translation is here.** The frontend
//! addresses pages by *position*: slot 0 is the first page in the scroller, and
//! every array it holds --- sizes, boxes, tile epochs --- is indexed that way. The
//! model addresses pages by *identity*, for the reason `docmodel`'s own note
//! gives: a command that names a position is wrong the moment an earlier command
//! moves one. So a state reply carries both --- the order, and each page's id ---
//! and a command carries the id.
//!
//! That is not ceremony over an identity mapping. It is the one property that
//! makes a stale frontend safe: a reader who presses rotate on a page that a
//! command in flight has just deleted gets [`Refusal::PageDeleted`], which is a
//! diagnosis, where a position would have silently rotated whichever page moved
//! into that slot.
//!
//! **Slot `i` is no longer baseline page `i`, as of the increment that wired
//! [`Command::Delete`].** Every consumer that addresses a page --- the tile
//! request, `page_text`, `search_page`, the outline's destinations, links,
//! comments, the thumbnails, the accessibility tree --- goes through the
//! translation in `src/lib/pages.ts`, which is built from the `pages` of a state
//! reply and is the frontend's only copy of it.
//!
//! The translation is the frontend's rather than this layer's, and that is a
//! decision rather than an accident: the frontend has to hold the order anyway in
//! order to lay the document out, so a second translation here would be a second
//! reader of the same rule, able to disagree with the first about which page is
//! where. What crosses the boundary is one answer.
//!
//! What is still **not** here: nothing creates a page (`docmodel`'s note has the
//! id-allocator property that would need proving first), and nothing moves one.
//! `Command::Move` is written and tested in the model and is wired to nothing ---
//! `save.rs` refuses a plan whose pages are out of document order rather than
//! writing them in the order the file happens to have.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;

use crate::docmodel::{Command, Doc, PageId, Refusal};

/// One page as the frontend sees it.
///
/// Field names are the Rust identifiers --- there is no `rename_all` here, for
/// the same reason `render.rs` has none: `ipc.ts` mirrors these by hand and a
/// rename that only one side hears about type-checks green on both.
#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub struct PageView {
    /// The model's identity for this page, opaque to the frontend.
    ///
    /// Sent back verbatim in a command. Deliberately not a position and not the
    /// baseline page number, even where those three currently coincide.
    pub id: u64,
    /// Which baseline page supplies the content.
    ///
    /// Equal to the slot today. It is sent anyway because it is what a tile
    /// request will have to carry once a page can move, and because a reader of
    /// the reply should not have to know that the two are the same to read it.
    pub source: u32,
    /// Quarter turns clockwise **on top of the page's own `/Rotate`**, 0 to 3.
    ///
    /// Named for the composition, not the result --- the page may already say
    /// `/Rotate 90`, and PDFium composes the render rotation with it.
    pub turns: u8,
}

/// The whole edit state of one document, as one reply.
///
/// Sent whole rather than as a delta on every edit. A delta is smaller and was
/// not taken: the frontend would then hold a state built by applying its own
/// copy of the rules, which is the second implementation `docmodel`'s undo note
/// rejects for exactly the same reason. This way the frontend holds a cache of
/// an answer, and the only way for it to be wrong is to be stale.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct EditState {
    /// The live pages, in reading order.
    pub pages: Vec<PageView>,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Whether anything differs from the file on disk.
    ///
    /// Read from the journal cursor rather than by comparing the working
    /// document against the baseline: a rotate-and-rotate-back leaves an
    /// identical document and *is* an unsaved change to a reader who did it, and
    /// more to the point a comparison would report "clean" for a journal that a
    /// save has not written.
    pub dirty: bool,
}

/// Every open document's edit model.
///
/// A `Mutex` rather than a `RwLock`: every operation here mutates except
/// [`state`](Edits::state), the lock is held for a `HashMap` lookup and a page
/// walk, and nothing is on the tile path --- the frontend composes turns from a
/// state reply it already has, so a render does not come through here.
#[derive(Default)]
pub struct Edits {
    docs: Mutex<HashMap<u32, Doc>>,
}

impl Edits {
    /// Starts a model for a freshly opened document.
    ///
    /// Replaces any model already under that handle. That is not defensive: the
    /// render service reuses document numbers, so an id can legitimately name a
    /// different file than it did, and keeping the old journal would apply one
    /// document's edits to another.
    pub fn open(&self, doc: u32, pages: u32) {
        self.docs
            .lock()
            .expect("edits lock")
            .insert(doc, Doc::open(pages));
    }

    /// Drops a document's model. Silent if there is none.
    pub fn close(&self, doc: u32) {
        self.docs.lock().expect("edits lock").remove(&doc);
    }

    /// How many documents have a model, for tests and diagnostics.
    pub fn len(&self) -> usize {
        self.docs.lock().expect("edits lock").len()
    }

    /// Whether no document has a model.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The current state of one document.
    ///
    /// # Errors
    ///
    /// The handle names no open document.
    pub fn state(&self, doc: u32) -> Result<EditState, String> {
        let docs = self.docs.lock().expect("edits lock");
        let model = docs.get(&doc).ok_or_else(|| unknown(doc))?;
        Ok(snapshot(model))
    }

    /// Turns one page, addressed by identity.
    ///
    /// # Errors
    ///
    /// The handle names no open document, or the model refuses --- an id that
    /// never existed and an id that was deleted are different diagnoses and are
    /// reported as such.
    pub fn rotate(&self, doc: u32, page: u64, turns: i8) -> Result<EditState, String> {
        self.command(
            doc,
            Command::Rotate {
                page: PageId::from_raw(page),
                turns,
            },
        )
    }

    /// Removes one page from the working document, addressed by identity.
    ///
    /// The page is gone from the reply's `pages`, and its id is a tombstone: a
    /// later command naming it is [`Refusal::PageDeleted`] rather than
    /// [`Refusal::NoSuchPage`], which is the distinction a stale frontend needs.
    /// Undo puts it back, at its old position and with its own rotation, because
    /// undo is replay --- see `docmodel`'s note on why that is not an inverse.
    ///
    /// # Errors
    ///
    /// The handle names no open document; the id names no page or a deleted one;
    /// or it is the last page, since a document with no pages is not a document.
    pub fn delete(&self, doc: u32, page: u64) -> Result<EditState, String> {
        self.command(
            doc,
            Command::Delete {
                page: PageId::from_raw(page),
            },
        )
    }

    /// Applies a command and returns the state it produced.
    fn command(&self, doc: u32, cmd: Command) -> Result<EditState, String> {
        let mut docs = self.docs.lock().expect("edits lock");
        let model = docs.get_mut(&doc).ok_or_else(|| unknown(doc))?;
        model.apply(cmd).map_err(describe)?;
        Ok(snapshot(model))
    }

    /// Steps back one command.
    ///
    /// Returns the state either way. A reader pressing undo with nothing to undo
    /// has made no error, so this is not a refusal --- and the reply still says
    /// `can_undo: false`, which is what the frontend needs in order to stop
    /// offering it.
    ///
    /// # Errors
    ///
    /// The handle names no open document.
    pub fn undo(&self, doc: u32) -> Result<EditState, String> {
        let mut docs = self.docs.lock().expect("edits lock");
        let model = docs.get_mut(&doc).ok_or_else(|| unknown(doc))?;
        model.undo();
        Ok(snapshot(model))
    }

    /// Steps forward one command. Returns the state either way, as [`undo`](Edits::undo) does.
    ///
    /// # Errors
    ///
    /// The handle names no open document.
    pub fn redo(&self, doc: u32) -> Result<EditState, String> {
        let mut docs = self.docs.lock().expect("edits lock");
        let model = docs.get_mut(&doc).ok_or_else(|| unknown(doc))?;
        model.redo();
        Ok(snapshot(model))
    }

    /// What to write, or print, for one document.
    ///
    /// Carries the same pages [`state`](Edits::state) reports, so a saved file
    /// and a rendered page cannot disagree about what the reader was looking at
    /// --- two readings of one answer rather than two derivations of one rule ---
    /// and the baseline beside them, which the frontend has no use for and a
    /// writer cannot do without.
    ///
    /// # Errors
    ///
    /// The handle names no open document.
    pub fn plan(&self, doc: u32) -> Result<Plan, String> {
        let docs = self.docs.lock().expect("edits lock");
        let model = docs.get(&doc).ok_or_else(|| unknown(doc))?;
        Ok(Plan {
            baseline: model.baseline(),
            pages: snapshot(model).pages,
        })
    }
}

/// The working document as something that writes a file needs it.
///
/// Not sent to the frontend. It is what [`save::write_copy`](crate::save::write_copy)
/// and the print path are handed, and it exists rather than a bare `Vec<PageView>`
/// because both of them have to answer a question the page list cannot: *how many
/// pages did the file have*. Without the baseline a save cannot tell three pages
/// kept out of five from a five-page document that lost two under it.
#[derive(Clone, PartialEq, Debug)]
pub struct Plan {
    /// How many pages the file this document was opened from had.
    pub baseline: u32,
    /// The kept pages, in reading order.
    pub pages: Vec<PageView>,
}

impl Plan {
    /// Whether this describes the file exactly as it is on disk.
    ///
    /// Every baseline page present, in order, unturned. It is what lets the print
    /// path hand the file over byte for byte rather than rewriting it to produce
    /// the same document --- a rewrite drops encryption silently and reflows
    /// structure, so "nothing was edited" is worth recognising rather than
    /// approximating with `dirty`, which is `true` after a turn and a turn back.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.pages.len() == self.baseline as usize
            && self
                .pages
                .iter()
                .enumerate()
                .all(|(at, page)| page.source as usize == at && page.turns % 4 == 0)
    }
}

/// The message a refusal becomes on the wire.
///
/// Spelled out rather than `Debug`-formatted. `Debug` would carry the `PageId`'s
/// raw number into a reader-facing string, and the two refusals that matter here
/// differ in their *diagnosis*, which is the part worth wording.
fn describe(why: Refusal) -> String {
    match why {
        Refusal::NoSuchPage(_) => "no such page".into(),
        Refusal::PageDeleted(_) => "that page has been deleted".into(),
        Refusal::AnchorIsTarget(_) => "a page cannot be moved after itself".into(),
        Refusal::LastPage(_) => "a document must keep at least one page".into(),
        Refusal::DegenerateCrop(_) => "that crop encloses no area".into(),
    }
}

fn unknown(doc: u32) -> String {
    format!("no open document {doc}")
}

/// Reads a model into the reply shape.
fn snapshot(model: &Doc) -> EditState {
    let working = model.working();
    let pages = working
        .order()
        .iter()
        .map(|&id| {
            let page = working.page(id).expect("a page in the order is live");
            PageView {
                id: id.get(),
                source: page.source,
                turns: page.extra_turns,
            }
        })
        .collect();
    let (applied, _) = model.depth();
    EditState {
        pages,
        can_undo: model.can_undo(),
        can_redo: model.can_redo(),
        dirty: applied > 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A three-page document with a model, and its handle.
    fn opened() -> Edits {
        let edits = Edits::default();
        edits.open(7, 3);
        edits
    }

    #[test]
    fn a_freshly_opened_document_is_the_file_on_disk() {
        let state = opened().state(7).expect("open");
        assert_eq!(state.pages.len(), 3);
        assert!(state.pages.iter().all(|page| page.turns == 0));
        assert!(!state.dirty, "nothing has been applied");
        assert!(!state.can_undo);
        assert!(!state.can_redo);
    }

    #[test]
    fn every_page_reports_its_own_identity_and_source() {
        let state = opened().state(7).expect("open");
        let ids: Vec<u64> = state.pages.iter().map(|page| page.id).collect();
        let sources: Vec<u32> = state.pages.iter().map(|page| page.source).collect();
        assert_eq!(sources, vec![0, 1, 2], "slot is baseline page, for now");
        assert_eq!(ids.len(), 3);
        // The ids are distinct, which is the only thing about them this layer
        // may assert --- their values are `docmodel`'s business, and a test that
        // pinned them would fail the first time an allocator changed.
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 3, "ids are distinct: {ids:?}");
    }

    #[test]
    fn a_turn_lands_on_the_page_it_named_and_nowhere_else() {
        let edits = opened();
        let middle = edits.state(7).expect("open").pages[1].id;
        let after = edits.rotate(7, middle, 1).expect("rotate");
        assert_eq!(
            after.pages.iter().map(|p| p.turns).collect::<Vec<_>>(),
            vec![0, 1, 0]
        );
        assert!(after.dirty);
        assert!(after.can_undo);
        assert!(!after.can_redo, "a fresh command has no redo tail");
    }

    #[test]
    fn turns_compose_and_wrap_the_way_a_reader_pressing_twice_expects() {
        let edits = opened();
        let first = edits.state(7).expect("open").pages[0].id;
        for expected in [1, 2, 3, 0] {
            let after = edits.rotate(7, first, 1).expect("rotate");
            assert_eq!(after.pages[0].turns, expected);
        }
    }

    #[test]
    fn turning_the_other_way_is_the_turn_back() {
        let edits = opened();
        let first = edits.state(7).expect("open").pages[0].id;
        edits.rotate(7, first, 1).expect("rotate");
        let after = edits.rotate(7, first, -1).expect("rotate back");
        assert_eq!(after.pages[0].turns, 0);
        assert!(
            after.dirty,
            "two commands were applied; the document is not the one on disk merely because \
             it looks like it"
        );
    }

    #[test]
    fn undo_puts_the_page_back_and_redo_brings_it_forward() {
        let edits = opened();
        let first = edits.state(7).expect("open").pages[0].id;
        edits.rotate(7, first, 1).expect("rotate");

        let undone = edits.undo(7).expect("undo");
        assert_eq!(undone.pages[0].turns, 0);
        assert!(!undone.dirty, "the journal cursor is back at the baseline");
        assert!(!undone.can_undo);
        assert!(undone.can_redo);

        let redone = edits.redo(7).expect("redo");
        assert_eq!(redone.pages[0].turns, 1);
        assert!(redone.dirty);
        assert!(redone.can_undo);
        assert!(!redone.can_redo);
    }

    #[test]
    fn undo_with_nothing_to_undo_is_not_an_error() {
        let edits = opened();
        let state = edits.undo(7).expect("undo is not a refusal");
        assert!(!state.can_undo);
        assert_eq!(state.pages.len(), 3);
    }

    #[test]
    fn redo_with_nothing_to_redo_is_not_an_error() {
        let edits = opened();
        let state = edits.redo(7).expect("redo is not a refusal");
        assert!(!state.can_redo);
    }

    #[test]
    fn a_command_after_an_undo_discards_the_redo_tail() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        edits.rotate(7, pages[0].id, 1).expect("rotate");
        edits.undo(7).expect("undo");
        let after = edits.rotate(7, pages[2].id, 2).expect("rotate another");
        assert!(!after.can_redo, "the tail is gone");
        assert_eq!(
            after.pages.iter().map(|p| p.turns).collect::<Vec<_>>(),
            vec![0, 0, 2]
        );
    }

    #[test]
    fn a_deleted_page_leaves_the_order_and_the_ones_after_it_move_up() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        let after = edits.delete(7, pages[0].id).expect("delete");

        assert_eq!(after.pages.len(), 2);
        assert_eq!(
            after.pages.iter().map(|p| p.source).collect::<Vec<_>>(),
            vec![1, 2],
            "slot 0 is now baseline page 1 --- the equality between the two is what \
             this command breaks, and every consumer that assumed it has to translate"
        );
        assert_eq!(
            after.pages.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![pages[1].id, pages[2].id],
            "and the survivors kept their identities, which is what makes a command \
             in flight still land on the page it named"
        );
        assert!(after.dirty);
        assert!(after.can_undo);
    }

    #[test]
    fn undo_puts_a_deleted_page_back_where_it_was() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        // Turned first, so the restored page has something of its own to lose.
        edits.rotate(7, pages[1].id, 1).expect("rotate");
        edits.delete(7, pages[1].id).expect("delete");

        let back = edits.undo(7).expect("undo");
        assert_eq!(back.pages.len(), 3);
        assert_eq!(back.pages[1].id, pages[1].id, "at its old position");
        assert_eq!(
            back.pages[1].turns, 1,
            "with its own rotation. Undo is replay from the baseline, so this is \
             free --- an inverse would have to store it"
        );
    }

    #[test]
    fn a_second_command_naming_a_deleted_page_says_it_was_deleted() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        edits.delete(7, pages[0].id).expect("delete");

        let why = edits.rotate(7, pages[0].id, 1).expect_err("gone");
        assert_eq!(
            why, "that page has been deleted",
            "distinct from 'no such page', which is what a frontend one state \
             behind needs in order to tell a stale command from a wrong one"
        );
        assert_eq!(
            edits.delete(7, pages[0].id).expect_err("still gone"),
            "that page has been deleted"
        );
    }

    #[test]
    fn the_last_page_cannot_be_deleted() {
        let edits = Edits::default();
        edits.open(3, 1);
        let only = edits.state(3).expect("open").pages[0].id;
        let why = edits.delete(3, only).expect_err("must refuse");
        assert_eq!(why, "a document must keep at least one page");
        assert_eq!(
            edits.state(3).expect("state").pages.len(),
            1,
            "a refusal changes nothing"
        );
        assert!(
            !edits.state(3).expect("state").dirty,
            "and does not enter the journal"
        );
    }

    #[test]
    fn a_plan_after_a_deletion_keeps_the_baseline_it_was_opened_with() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        edits.delete(7, pages[1].id).expect("delete");

        let plan = edits.plan(7).expect("plan");
        assert_eq!(plan.baseline, 3, "the file still has three pages");
        assert_eq!(
            plan.pages.iter().map(|p| p.source).collect::<Vec<_>>(),
            vec![0, 2],
            "and the plan names the two it kept, by the page of the file they are"
        );
        assert!(
            !plan.is_identity(),
            "a save cannot hand this file over unchanged"
        );
    }

    #[test]
    fn only_an_unedited_document_is_the_file_on_disk() {
        let edits = opened();
        assert!(
            edits.plan(7).expect("plan").is_identity(),
            "nothing has been done to it"
        );

        let first = edits.state(7).expect("open").pages[0].id;
        edits.rotate(7, first, 1).expect("rotate");
        assert!(!edits.plan(7).expect("plan").is_identity());

        // Turned back. The journal is two commands deep and the document is the
        // one on disk again --- which is the case `dirty` deliberately reports
        // the other way, and the reason this is not spelled `!dirty`.
        edits.rotate(7, first, -1).expect("rotate back");
        let plan = edits.plan(7).expect("plan");
        assert!(plan.is_identity(), "every page present, in order, unturned");
        assert!(
            edits.state(7).expect("state").dirty,
            "the control: the reader has unsaved commands, and this still describes \
             the file exactly"
        );
    }

    #[test]
    fn an_id_no_document_ever_had_is_refused_by_name() {
        let edits = opened();
        let why = edits.rotate(7, 9_999, 1).expect_err("unknown id");
        assert_eq!(why, "no such page");
    }

    #[test]
    fn a_command_for_a_document_that_is_not_open_says_so() {
        let edits = opened();
        let why = edits.rotate(8, 1, 1).expect_err("unknown document");
        assert!(why.contains('8'), "the message names the handle: {why}");
        assert!(edits.state(8).is_err());
    }

    #[test]
    fn closing_a_document_drops_its_journal() {
        let edits = opened();
        let first = edits.state(7).expect("open").pages[0].id;
        edits.rotate(7, first, 1).expect("rotate");
        edits.close(7);
        assert!(edits.is_empty());
        assert!(
            edits.state(7).is_err(),
            "the model is gone, not merely reset"
        );
    }

    #[test]
    fn reopening_under_a_reused_handle_does_not_inherit_the_previous_journal() {
        let edits = opened();
        let first = edits.state(7).expect("open").pages[0].id;
        edits.rotate(7, first, 2).expect("rotate");
        // The render service reuses document numbers, so this is a real sequence
        // and not a contrived one.
        edits.open(7, 5);
        let state = edits.state(7).expect("reopened");
        assert_eq!(state.pages.len(), 5, "the new document's page count");
        assert!(state.pages.iter().all(|page| page.turns == 0));
        assert!(!state.dirty);
        assert!(!state.can_undo, "the previous document's journal is gone");
    }

    #[test]
    fn two_documents_keep_their_own_journals() {
        let edits = opened();
        edits.open(8, 2);
        let a = edits.state(7).expect("a").pages[0].id;
        edits.rotate(7, a, 1).expect("rotate a");

        let b = edits.state(8).expect("b");
        assert!(!b.dirty, "the other document was not touched");
        assert!(b.pages.iter().all(|page| page.turns == 0));
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn the_save_plan_is_the_state_the_reader_is_looking_at() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        edits.rotate(7, pages[1].id, 3).expect("rotate");
        let plan = edits.plan(7).expect("plan");
        assert_eq!(plan.pages, edits.state(7).expect("state").pages);
        assert_eq!(plan.pages[1].turns, 3);
        assert_eq!(plan.baseline, 3, "the file's pages, not the working ones");
    }
}
