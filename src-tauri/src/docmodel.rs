//! The working document --- what renders --- and the journal that produced it.
//!
//! `docs/PLAN.md` §5 is the design and this is its first layer. The shape it
//! insists on, against the first draft it replaced, is three parts rather than
//! two:
//!
//! 1. **Baseline** --- the file as loaded, immutable. Here it is nothing but a
//!    page count, because nothing in this module opens a document.
//! 2. **Working document** ([`Working`]) --- a materialized view of baseline plus
//!    the commands applied so far. This is what will render, search, hit-test and
//!    report geometry.
//! 3. **Journal** --- the command log, which [`Doc`] holds, for undo and redo.
//!
//! The working document is the part the first draft did not have, and it is not
//! optional. "Annotations render as an overlay" covers annotations; deleting,
//! reordering, rotating and cropping a page change what renders *immediately*,
//! long before anything is saved, and an overlay cannot express any of them.
//!
//! ## Why there is no arithmetic on indices anywhere below
//!
//! Commands address [`PageId`], never a position. `Move { from: 3, to: 7 }` is
//! not merely discouraged here, it is unrepresentable: positions shift under
//! other commands, so the same journal would replay differently depending on
//! what preceded it.
//!
//! **Be precise about what that buys, because a test in this module cannot show
//! it.** Replay here always re-applies a whole prefix from the same baseline, and
//! a position-based journal replayed that way would be self-consistent too --- so
//! there is no failing case to write, and a test claiming to prove ids necessary
//! would be one that cannot fail. What ids are actually for is every operation
//! that changes a prefix rather than replaying it: journal compaction, the
//! rebase after save that §5 describes, and dropping a command in the middle.
//! None of those exist yet. The type is what carries the property until they do.
//!
//! ## Undo is replay, not inversion
//!
//! Undo rewinds a cursor and rebuilds the working document from the nearest
//! snapshot. The alternative --- storing an inverse for each command --- is
//! faster and was not taken, because every inverse is a second implementation
//! that has to agree with the first, and the ways they disagree are exactly the
//! cases undo is for. Resurrecting a deleted page *at its old position with its
//! own rotation and crop* is free under replay and is a written-out special case
//! under inversion.
//!
//! The cost is bounded by snapshots: a rebuild replays at most
//! [`SNAPSHOT_EVERY`] commands.
//!
//! ## The refusals are the point
//!
//! Every command states a precondition and a failure is a named [`Refusal`],
//! never a silent no-op. §5 asks for tombstones specifically so that a command
//! naming a deleted page "fails explicitly rather than silently corrupting
//! state", and that is why [`Refusal::PageDeleted`] and [`Refusal::NoSuchPage`]
//! are two variants and not one: an id that was deleted and an id that never
//! existed are different diagnoses, and collapsing them loses the only
//! distinction a caller can act on.
//!
//! ## What is deliberately not here yet
//!
//! **Nothing creates a page.** Insert, extract, split, merge and duplicate all
//! bring pages in from somewhere, which needs an id allocator, and an allocator
//! carries a property this module cannot currently get wrong: an id released by
//! an undo must never be re-issued to a different page by a later redo. `Doc`
//! has no allocator at all, so ids here are the baseline's own and that failure
//! is unreachable. When creation lands, that is the property to prove first.
//!
//! Save, save-mode classification, crash recovery and external-modification
//! handling are §5's other halves and none of them are here. This module holds
//! no file, no bytes and no `lopdf` object, and it is the better for it: it can
//! be driven directly rather than through a document.

use std::collections::{HashMap, HashSet};

/// How many commands may separate a snapshot from the next one.
///
/// The only cost of raising it is a longer replay on undo; the only cost of
/// lowering it is a clone of the working document, which is a `Vec` of ids and a
/// map of three-field structs. Neither is close to mattering at a size a reader
/// would ever produce by hand, so this is set where a rebuild stays obviously
/// cheap rather than where it was measured --- there is nothing to measure yet.
pub const SNAPSHOT_EVERY: usize = 32;

/// A page's identity, stable for the life of the working document.
///
/// Opaque on purpose. It is not a position, it is not the baseline page number,
/// and nothing outside this module should do arithmetic on it --- see the module
/// note on why commands carry these rather than indices.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct PageId(u64);

impl PageId {
    /// The raw value, for logging and for keying a map on the far side of the
    /// IPC boundary. Deliberately not a position and not usable as one.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A rectangle in PDF user space, in points, lower-left and upper-right.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub llx: f64,
    pub lly: f64,
    pub urx: f64,
    pub ury: f64,
}

impl Rect {
    /// Whether the rectangle encloses any area at all.
    ///
    /// **A `NaN` in any corner is improper**, and that falls out of the
    /// comparisons rather than being written: every comparison against `NaN` is
    /// false. It is asserted in the tests rather than left to be rediscovered,
    /// because the alternative is a crop box that renders as nothing while the
    /// model reports a crop is in force.
    pub fn is_proper(self) -> bool {
        self.urx > self.llx && self.ury > self.lly
    }
}

/// One page of the working document.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Page {
    /// Which baseline page supplies the content. Zero-based.
    ///
    /// This is the seam the render path will use: a viewport position indexes
    /// [`Working::order`], that yields a [`PageId`], and this is the page to ask
    /// the worker for.
    pub source: u32,
    /// Quarter turns clockwise **on top of the page's own `/Rotate`**, normalized
    /// to `0..=3`.
    ///
    /// Named for the composition rather than for the result, because
    /// `docs/TRAPS.md` records that PDFium's render rotation composes with
    /// `/Rotate` and wants the turned size --- a field called `rotation` here
    /// would read as the final angle and be wrong by whatever the document
    /// already said.
    pub extra_turns: u8,
    /// The visible box, or the page's own when `None`.
    pub crop: Option<Rect>,
}

/// A page operation. Every variant addresses pages by identity.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Command {
    /// Turn a page by `turns` quarter turns clockwise; negative turns the other
    /// way. Relative rather than absolute so that undo of a turn is the turn
    /// back, and so that two turns of the same page compose the way a reader
    /// pressing the key twice expects.
    Rotate { page: PageId, turns: i8 },
    /// Set or clear the visible box.
    Crop { page: PageId, to: Option<Rect> },
    /// Remove a page from the order and tombstone its id.
    Delete { page: PageId },
    /// Put `page` immediately after `after`, or at the front when `after` is
    /// `None`.
    ///
    /// Expressed against a neighbouring *id* rather than a destination index for
    /// the reason the module note gives.
    Move { page: PageId, after: Option<PageId> },
}

impl Command {
    /// The page the command acts on, for diagnostics.
    pub fn subject(self) -> PageId {
        match self {
            Command::Rotate { page, .. }
            | Command::Crop { page, .. }
            | Command::Delete { page }
            | Command::Move { page, .. } => page,
        }
    }
}

/// Why a command was not applied.
///
/// A refusal leaves the working document and the journal exactly as they were.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Refusal {
    /// No page has ever had this id.
    NoSuchPage(PageId),
    /// The id names a page that was deleted. Distinct from
    /// [`NoSuchPage`](Refusal::NoSuchPage) on purpose --- see the module note.
    PageDeleted(PageId),
    /// A page cannot be moved after itself, which has no meaning and would
    /// otherwise be a silent no-op.
    AnchorIsTarget(PageId),
    /// A document must keep at least one page. A zero-page PDF is not a
    /// document, so this is refused rather than left to the save path.
    LastPage(PageId),
    /// A crop box enclosing no area, `NaN` included.
    ///
    /// **This variant does not compare equal to itself when the rectangle holds
    /// a `NaN`**, because the derived `PartialEq` compares the floats and no
    /// comparison against `NaN` is true. That is correct for a rectangle and
    /// surprising for a refusal, so: match the variant, do not compare the
    /// value. The test below does exactly that, and it found this by failing
    /// with the two sides printing identically.
    DegenerateCrop(Rect),
}

/// Baseline plus the commands applied so far, materialized.
#[derive(Clone, PartialEq, Debug)]
pub struct Working {
    order: Vec<PageId>,
    pages: HashMap<PageId, Page>,
    /// Ids that were live and are not. Carries no state: undo rebuilds a deleted
    /// page from the journal, so a tombstone only has to make a later command
    /// naming it refusable by name.
    graves: HashSet<PageId>,
}

impl Working {
    /// The baseline: `pages` pages in file order, unturned and uncropped.
    fn baseline(pages: u32) -> Working {
        let ids: Vec<PageId> = (0..pages).map(|i| PageId(i as u64 + 1)).collect();
        let table = ids
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                (
                    id,
                    Page {
                        source: i as u32,
                        extra_turns: 0,
                        crop: None,
                    },
                )
            })
            .collect();
        Working {
            order: ids,
            pages: table,
            graves: HashSet::new(),
        }
    }

    /// The live pages, in reading order.
    pub fn order(&self) -> &[PageId] {
        &self.order
    }

    /// How many pages the reader sees.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Never true --- [`Refusal::LastPage`] keeps at least one page --- and here
    /// because clippy asks for it beside `len`, which is a fair request: a reader
    /// should not have to derive emptiness from a bound stated somewhere else.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// A live page's state, or `None` if the id is deleted or unknown.
    pub fn page(&self, id: PageId) -> Option<&Page> {
        self.pages.get(&id)
    }

    /// Whether an id names a page that once existed and was deleted.
    pub fn is_deleted(&self, id: PageId) -> bool {
        self.graves.contains(&id)
    }

    /// Refuses unless the id names a live page, naming which of the two it is.
    fn live(&self, id: PageId) -> Result<(), Refusal> {
        if self.pages.contains_key(&id) {
            Ok(())
        } else if self.graves.contains(&id) {
            Err(Refusal::PageDeleted(id))
        } else {
            Err(Refusal::NoSuchPage(id))
        }
    }

    /// Applies one command, or refuses and changes nothing.
    ///
    /// Every path checks its preconditions **before** the first mutation, which
    /// is what makes "a refusal changes nothing" true by construction rather than
    /// by each arm remembering to unwind.
    fn apply(&mut self, cmd: Command) -> Result<(), Refusal> {
        match cmd {
            Command::Rotate { page, turns } => {
                self.live(page)?;
                let p = self.pages.get_mut(&page).expect("checked live");
                p.extra_turns = (i16::from(p.extra_turns) + i16::from(turns)).rem_euclid(4) as u8;
            }
            Command::Crop { page, to } => {
                self.live(page)?;
                if let Some(r) = to {
                    if !r.is_proper() {
                        return Err(Refusal::DegenerateCrop(r));
                    }
                }
                self.pages.get_mut(&page).expect("checked live").crop = to;
            }
            Command::Delete { page } => {
                self.live(page)?;
                if self.order.len() == 1 {
                    return Err(Refusal::LastPage(page));
                }
                let at = self.position(page);
                self.order.remove(at);
                self.pages.remove(&page);
                self.graves.insert(page);
            }
            Command::Move { page, after } => {
                self.live(page)?;
                if let Some(anchor) = after {
                    if anchor == page {
                        return Err(Refusal::AnchorIsTarget(page));
                    }
                    self.live(anchor)?;
                }
                // The two statements below are in this order deliberately, and the
                // mutation harness carries the swap: reading the anchor's position
                // *before* the removal is off by one whenever the moved page sits
                // ahead of the anchor, and overshoots by one place --- which looks
                // like a drag landing one row too far rather than like a defect.
                let from = self.position(page);
                self.order.remove(from);
                let to = match after {
                    None => 0,
                    Some(anchor) => self.position(anchor) + 1,
                };
                self.order.insert(to, page);
            }
        }
        Ok(())
    }

    /// Where a live page sits in the order.
    ///
    /// Panics if it does not, which every caller has just ruled out with
    /// [`live`](Self::live). The two are kept in step by every mutation above
    /// touching both, and this is the assertion that says so.
    fn position(&self, id: PageId) -> usize {
        self.order
            .iter()
            .position(|&p| p == id)
            .expect("a live page is in the order")
    }
}

/// A document being edited: baseline, working view, journal and cursor.
#[derive(Clone, Debug)]
pub struct Doc {
    baseline: u32,
    now: Working,
    journal: Vec<Command>,
    /// How many journal entries are applied. Entries past it are the redo tail.
    cursor: usize,
    /// Working documents at selected cursor positions, so an undo replays a
    /// bounded number of commands. Keyed by cursor, never by journal index ---
    /// they are the same number and mean different things, and the key is "how
    /// many commands had been applied".
    snapshots: HashMap<usize, Working>,
}

impl Doc {
    /// Opens a document of `pages` baseline pages with an empty journal.
    pub fn open(pages: u32) -> Doc {
        Doc {
            baseline: pages,
            now: Working::baseline(pages),
            journal: Vec::new(),
            cursor: 0,
            snapshots: HashMap::new(),
        }
    }

    /// What renders.
    pub fn working(&self) -> &Working {
        &self.now
    }

    /// Whether there is anything to undo.
    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    /// Whether there is anything to redo.
    pub fn can_redo(&self) -> bool {
        self.cursor < self.journal.len()
    }

    /// How many commands are applied, and how many are in the redo tail.
    pub fn depth(&self) -> (usize, usize) {
        (self.cursor, self.journal.len() - self.cursor)
    }

    /// How many snapshots are held.
    ///
    /// An accounting observable, and here for the reason `docs/TRAPS.md` gives:
    /// a snapshot that is never taken and a snapshot that is taken and never used
    /// produce identical documents, so no assertion over [`working`](Doc::working)
    /// can tell them apart. Dropping a stale one is likewise invisible until it
    /// silently rebuilds the wrong state.
    pub fn snapshots(&self) -> usize {
        self.snapshots.len()
    }

    /// Which cursor position a rebuild to `upto` would replay from.
    ///
    /// The second half of the same accounting: it is what lets a test assert that
    /// a rebuild used a snapshot rather than replaying the whole journal, which
    /// is otherwise a claim about speed with no observable behind it.
    pub fn replay_base(&self, upto: usize) -> usize {
        self.nearest(upto)
    }

    /// Applies a command, or refuses and changes nothing.
    ///
    /// A successful apply **discards the redo tail**, which is what makes the
    /// journal a line rather than a tree.
    pub fn apply(&mut self, cmd: Command) -> Result<(), Refusal> {
        self.now.apply(cmd)?;
        self.journal.truncate(self.cursor);
        // Snapshots past the cursor describe states that no longer exist. Keeping
        // one would not merely waste a clone: the next rebuild through that
        // position would start from a document built by commands this apply just
        // discarded, and every page after it would be wrong with nothing saying so.
        self.snapshots.retain(|&at, _| at <= self.cursor);
        self.journal.push(cmd);
        self.cursor += 1;
        if self.cursor % SNAPSHOT_EVERY == 0 {
            self.snapshots.insert(self.cursor, self.now.clone());
        }
        Ok(())
    }

    /// Steps back one command. Returns whether there was one.
    pub fn undo(&mut self) -> bool {
        if !self.can_undo() {
            return false;
        }
        self.cursor -= 1;
        self.now = self.rebuild(self.cursor);
        true
    }

    /// Steps forward one command. Returns whether there was one.
    ///
    /// Applies rather than rebuilds, since the working document is already the
    /// state this command expects.
    pub fn redo(&mut self) -> bool {
        if !self.can_redo() {
            return false;
        }
        let cmd = self.journal[self.cursor];
        self.now.apply(cmd).unwrap_or_else(|why| {
            panic!("a journalled command was refused on redo: {cmd:?} -> {why:?}")
        });
        self.cursor += 1;
        true
    }

    /// The greatest snapshot position at or below `upto`, or 0 for the baseline.
    fn nearest(&self, upto: usize) -> usize {
        self.snapshots
            .keys()
            .copied()
            .filter(|&at| at <= upto)
            .max()
            .unwrap_or(0)
    }

    /// Rebuilds the working document as of `upto` commands.
    ///
    /// **A refusal here is a broken model, not a user error**, so it panics
    /// rather than skipping the command: every entry in the journal was accepted
    /// against the state its predecessors produced, and replay reproduces exactly
    /// those predecessors. Skipping instead would carry on rendering a document
    /// that is not the one the journal describes, which is the failure this whole
    /// design exists to make impossible.
    fn rebuild(&self, upto: usize) -> Working {
        let from = self.nearest(upto);
        let mut w = match self.snapshots.get(&from) {
            Some(snap) => snap.clone(),
            None => Working::baseline(self.baseline),
        };
        for &cmd in &self.journal[from..upto] {
            w.apply(cmd).unwrap_or_else(|why| {
                panic!("a journalled command was refused on replay: {cmd:?} -> {why:?}")
            });
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ids of a fresh document, in order.
    fn ids(doc: &Doc) -> Vec<u64> {
        doc.working().order().iter().map(|p| p.get()).collect()
    }

    fn rect(llx: f64, lly: f64, urx: f64, ury: f64) -> Rect {
        Rect { llx, lly, urx, ury }
    }

    #[test]
    fn a_baseline_page_maps_to_itself_in_order() {
        let doc = Doc::open(4);
        assert_eq!(ids(&doc), vec![1, 2, 3, 4]);
        for (i, &id) in doc.working().order().iter().enumerate() {
            let page = doc.working().page(id).expect("baseline pages are live");
            assert_eq!(page.source, i as u32);
            assert_eq!(page.extra_turns, 0);
            assert_eq!(page.crop, None);
        }
    }

    #[test]
    fn a_rotation_accumulates_and_wraps_at_four() {
        let mut doc = Doc::open(2);
        let a = doc.working().order()[0];
        for _ in 0..3 {
            doc.apply(Command::Rotate { page: a, turns: 1 }).unwrap();
        }
        assert_eq!(doc.working().page(a).unwrap().extra_turns, 3);
        doc.apply(Command::Rotate { page: a, turns: 1 }).unwrap();
        assert_eq!(doc.working().page(a).unwrap().extra_turns, 0);
    }

    #[test]
    fn a_negative_rotation_wraps_the_other_way() {
        let mut doc = Doc::open(2);
        let a = doc.working().order()[0];
        doc.apply(Command::Rotate { page: a, turns: -1 }).unwrap();
        assert_eq!(doc.working().page(a).unwrap().extra_turns, 3);
    }

    #[test]
    fn a_rotation_leaves_every_other_page_alone() {
        let mut doc = Doc::open(3);
        let [a, b, c] = [0, 1, 2].map(|i| doc.working().order()[i]);
        doc.apply(Command::Rotate { page: b, turns: 2 }).unwrap();
        assert_eq!(doc.working().page(a).unwrap().extra_turns, 0);
        assert_eq!(doc.working().page(b).unwrap().extra_turns, 2);
        assert_eq!(doc.working().page(c).unwrap().extra_turns, 0);
    }

    #[test]
    fn deleting_takes_the_page_out_of_the_order_and_tombstones_it() {
        let mut doc = Doc::open(3);
        let b = doc.working().order()[1];
        doc.apply(Command::Delete { page: b }).unwrap();
        assert_eq!(ids(&doc), vec![1, 3]);
        assert_eq!(doc.working().page(b), None);
        assert!(doc.working().is_deleted(b));
    }

    #[test]
    fn the_last_page_cannot_be_deleted() {
        let mut doc = Doc::open(1);
        let a = doc.working().order()[0];
        assert_eq!(
            doc.apply(Command::Delete { page: a }),
            Err(Refusal::LastPage(a))
        );
        assert_eq!(ids(&doc), vec![1]);
        assert_eq!(doc.depth(), (0, 0));
    }

    #[test]
    fn a_command_naming_a_deleted_page_is_refused_as_deleted() {
        let mut doc = Doc::open(3);
        let b = doc.working().order()[1];
        doc.apply(Command::Delete { page: b }).unwrap();
        assert_eq!(
            doc.apply(Command::Rotate { page: b, turns: 1 }),
            Err(Refusal::PageDeleted(b))
        );
    }

    #[test]
    fn a_command_naming_a_page_that_never_existed_is_refused_as_unknown() {
        let mut doc = Doc::open(3);
        let ghost = PageId(999);
        assert_eq!(
            doc.apply(Command::Rotate {
                page: ghost,
                turns: 1
            }),
            Err(Refusal::NoSuchPage(ghost))
        );
        // The distinction is the assertion: a deleted id and an id that never
        // existed both leave `page()` returning None, so only the refusal can
        // tell a caller which of the two it is holding.
        let b = doc.working().order()[1];
        doc.apply(Command::Delete { page: b }).unwrap();
        assert!(doc.working().is_deleted(b));
        assert!(!doc.working().is_deleted(ghost));
    }

    #[test]
    fn a_refused_command_changes_nothing_and_is_not_journalled() {
        let mut doc = Doc::open(3);
        let a = doc.working().order()[0];
        doc.apply(Command::Rotate { page: a, turns: 1 }).unwrap();
        let before = doc.working().clone();
        assert!(doc
            .apply(Command::Rotate {
                page: PageId(999),
                turns: 1
            })
            .is_err());
        assert_eq!(doc.working(), &before);
        assert_eq!(doc.depth(), (1, 0));
    }

    #[test]
    fn a_page_moved_after_one_that_follows_it_lands_immediately_after_it() {
        let mut doc = Doc::open(4);
        let [a, c] = [0, 2].map(|i| doc.working().order()[i]);
        // A B C D, move A after C. Reading C's position before removing A would
        // put A at index 2, i.e. B A C D --- one short, and the arrangement a
        // reader would read as the drag not taking.
        doc.apply(Command::Move {
            page: a,
            after: Some(c),
        })
        .unwrap();
        assert_eq!(ids(&doc), vec![2, 3, 1, 4]);
    }

    #[test]
    fn a_page_moved_after_one_that_precedes_it_lands_immediately_after_it() {
        let mut doc = Doc::open(4);
        let [a, d] = [0, 3].map(|i| doc.working().order()[i]);
        doc.apply(Command::Move {
            page: d,
            after: Some(a),
        })
        .unwrap();
        assert_eq!(ids(&doc), vec![1, 4, 2, 3]);
    }

    #[test]
    fn a_page_moved_with_no_anchor_goes_to_the_front() {
        let mut doc = Doc::open(3);
        let c = doc.working().order()[2];
        doc.apply(Command::Move {
            page: c,
            after: None,
        })
        .unwrap();
        assert_eq!(ids(&doc), vec![3, 1, 2]);
    }

    #[test]
    fn a_page_cannot_be_moved_after_itself() {
        let mut doc = Doc::open(3);
        let b = doc.working().order()[1];
        assert_eq!(
            doc.apply(Command::Move {
                page: b,
                after: Some(b)
            }),
            Err(Refusal::AnchorIsTarget(b))
        );
        assert_eq!(ids(&doc), vec![1, 2, 3]);
    }

    #[test]
    fn a_page_cannot_be_moved_after_a_deleted_one() {
        let mut doc = Doc::open(3);
        let [a, b] = [0, 1].map(|i| doc.working().order()[i]);
        doc.apply(Command::Delete { page: b }).unwrap();
        assert_eq!(
            doc.apply(Command::Move {
                page: a,
                after: Some(b)
            }),
            Err(Refusal::PageDeleted(b))
        );
        assert_eq!(ids(&doc), vec![1, 3]);
    }

    #[test]
    fn a_moved_page_keeps_the_state_it_had() {
        // Two properties rather than one: with only a rotation to check, a move
        // that rebuilt the page from the baseline would still pass.
        let mut doc = Doc::open(3);
        let [a, c] = [0, 2].map(|i| doc.working().order()[i]);
        doc.apply(Command::Rotate { page: a, turns: 2 }).unwrap();
        doc.apply(Command::Crop {
            page: a,
            to: Some(rect(10.0, 20.0, 30.0, 40.0)),
        })
        .unwrap();
        doc.apply(Command::Move {
            page: a,
            after: Some(c),
        })
        .unwrap();
        let page = *doc.working().page(a).unwrap();
        assert_eq!(page.extra_turns, 2);
        assert_eq!(page.crop, Some(rect(10.0, 20.0, 30.0, 40.0)));
        assert_eq!(page.source, 0);
    }

    #[test]
    fn a_crop_enclosing_no_area_is_refused() {
        let mut doc = Doc::open(2);
        let a = doc.working().order()[0];
        for bad in [
            rect(30.0, 20.0, 10.0, 40.0),
            rect(10.0, 40.0, 30.0, 20.0),
            rect(10.0, 20.0, 10.0, 40.0),
            rect(f64::NAN, 20.0, 30.0, 40.0),
            rect(10.0, 20.0, f64::NAN, 40.0),
        ] {
            // Matched rather than compared, and not as a convenience: two of
            // these carry a NaN, and `Refusal::DegenerateCrop(nan) ==
            // Refusal::DegenerateCrop(nan)` is false. Written as an equality
            // first, this failed with the left and right sides printing the same
            // text -- see the note on the variant.
            let got = doc.apply(Command::Crop {
                page: a,
                to: Some(bad),
            });
            assert!(
                matches!(got, Err(Refusal::DegenerateCrop(_))),
                "{bad:?} should be refused, got {got:?}"
            );
        }
        // A proper box on the same page still lands, so the loop above is
        // refusing these five rather than refusing every crop.
        let good = rect(10.0, 20.0, 30.0, 40.0);
        doc.apply(Command::Crop {
            page: a,
            to: Some(good),
        })
        .unwrap();
        assert_eq!(doc.working().page(a).unwrap().crop, Some(good));
    }

    #[test]
    fn a_crop_can_be_cleared() {
        let mut doc = Doc::open(2);
        let a = doc.working().order()[0];
        doc.apply(Command::Crop {
            page: a,
            to: Some(rect(1.0, 2.0, 3.0, 4.0)),
        })
        .unwrap();
        doc.apply(Command::Crop { page: a, to: None }).unwrap();
        assert_eq!(doc.working().page(a).unwrap().crop, None);
    }

    #[test]
    fn undo_then_redo_restores_the_same_document() {
        let mut doc = Doc::open(4);
        let [a, b, c] = [0, 1, 2].map(|i| doc.working().order()[i]);
        doc.apply(Command::Rotate { page: a, turns: 1 }).unwrap();
        doc.apply(Command::Delete { page: b }).unwrap();
        doc.apply(Command::Move {
            page: c,
            after: None,
        })
        .unwrap();
        let after = doc.working().clone();
        assert!(doc.undo());
        assert_ne!(doc.working(), &after);
        assert!(doc.redo());
        assert_eq!(doc.working(), &after);
    }

    #[test]
    fn undoing_a_deletion_restores_the_page_where_it_was_with_its_own_state() {
        let mut doc = Doc::open(4);
        let b = doc.working().order()[1];
        doc.apply(Command::Rotate { page: b, turns: 3 }).unwrap();
        doc.apply(Command::Crop {
            page: b,
            to: Some(rect(5.0, 6.0, 7.0, 8.0)),
        })
        .unwrap();
        let before = *doc.working().page(b).unwrap();
        doc.apply(Command::Delete { page: b }).unwrap();
        assert!(doc.undo());
        assert_eq!(ids(&doc), vec![1, 2, 3, 4]);
        assert_eq!(doc.working().page(b), Some(&before));
        assert!(!doc.working().is_deleted(b));
    }

    #[test]
    fn a_page_keeps_its_identity_across_a_deletion_and_its_undo() {
        // The property the type exists for: after the round trip, a command
        // naming the resurrected page still lands on that page and not on
        // whichever page now occupies its old position.
        let mut doc = Doc::open(4);
        let [b, c] = [1, 2].map(|i| doc.working().order()[i]);
        doc.apply(Command::Delete { page: b }).unwrap();
        assert!(doc.undo());
        doc.apply(Command::Rotate { page: b, turns: 1 }).unwrap();
        assert_eq!(doc.working().page(b).unwrap().extra_turns, 1);
        assert_eq!(doc.working().page(c).unwrap().extra_turns, 0);
    }

    #[test]
    fn undo_at_the_start_and_redo_at_the_end_are_refused_rather_than_panicking() {
        let mut doc = Doc::open(2);
        assert!(!doc.undo());
        assert!(!doc.redo());
        let a = doc.working().order()[0];
        doc.apply(Command::Rotate { page: a, turns: 1 }).unwrap();
        assert!(!doc.redo());
        assert!(doc.undo());
        assert!(!doc.undo());
        assert_eq!(doc.depth(), (0, 1));
    }

    #[test]
    fn applying_after_an_undo_discards_the_redo_tail() {
        let mut doc = Doc::open(3);
        let [a, b] = [0, 1].map(|i| doc.working().order()[i]);
        doc.apply(Command::Rotate { page: a, turns: 1 }).unwrap();
        doc.apply(Command::Rotate { page: b, turns: 1 }).unwrap();
        assert!(doc.undo());
        assert_eq!(doc.depth(), (1, 1));
        doc.apply(Command::Rotate { page: a, turns: 1 }).unwrap();
        assert_eq!(doc.depth(), (2, 0));
        assert!(!doc.can_redo());
        assert_eq!(doc.working().page(a).unwrap().extra_turns, 2);
        assert_eq!(doc.working().page(b).unwrap().extra_turns, 0);
    }

    #[test]
    fn a_rebuild_from_a_snapshot_equals_a_full_replay() {
        let mut doc = Doc::open(3);
        let a = doc.working().order()[0];
        for _ in 0..(SNAPSHOT_EVERY * 2 + 5) {
            doc.apply(Command::Rotate { page: a, turns: 1 }).unwrap();
        }
        // The control. Without it the comparison below holds by construction: if
        // no snapshot were ever taken, both sides would be a replay from the
        // baseline and the test could not fail.
        assert!(doc.snapshots() >= 2, "the test needs snapshots to exist");
        let target = SNAPSHOT_EVERY * 2 + 3;
        assert!(
            doc.replay_base(target) > 0,
            "a rebuild to {target} should start from a snapshot, not the baseline"
        );

        let from_snapshot = doc.rebuild(target);
        let full = {
            let mut w = Working::baseline(3);
            for &cmd in &doc.journal[..target] {
                w.apply(cmd).unwrap();
            }
            w
        };
        assert_eq!(from_snapshot, full);
    }

    #[test]
    fn a_rebuild_never_starts_from_a_snapshot_ahead_of_its_target() {
        // `a_journal_replays_to_the_state_it_was_applied_to` below walks a mixed
        // journal and every prefix of it, and looks like the test that would
        // cover this. It is not: it applies eight commands and SNAPSHOT_EVERY is
        // 32, so it never has a snapshot to pick the wrong one of. This is the
        // test with a failing case.
        let mut doc = Doc::open(3);
        let a = doc.working().order()[0];
        for _ in 0..(SNAPSHOT_EVERY * 2 + 5) {
            doc.apply(Command::Rotate { page: a, turns: 1 }).unwrap();
        }
        assert!(doc.snapshots() >= 2, "the test needs snapshots to exist");
        for upto in 0..=doc.depth().0 {
            let base = doc.replay_base(upto);
            assert!(base <= upto, "a rebuild to {upto} would start from {base}");
        }
        // And the state is right at every step down, including the two undos
        // that cross a snapshot boundary.
        while doc.can_undo() {
            let want = ((doc.depth().0 - 1) % 4) as u8;
            assert!(doc.undo());
            assert_eq!(doc.working().page(a).unwrap().extra_turns, want);
        }
    }

    #[test]
    fn a_stale_snapshot_is_dropped_when_the_redo_tail_is_discarded() {
        let mut doc = Doc::open(3);
        let [a, b] = [0, 1].map(|i| doc.working().order()[i]);
        for _ in 0..SNAPSHOT_EVERY {
            doc.apply(Command::Rotate { page: a, turns: 1 }).unwrap();
        }
        assert_eq!(doc.snapshots(), 1);
        // Rewind past the snapshot and take the journal somewhere else. The
        // snapshot at SNAPSHOT_EVERY now describes a state that never existed on
        // this line of history.
        for _ in 0..3 {
            assert!(doc.undo());
        }
        doc.apply(Command::Rotate { page: b, turns: 1 }).unwrap();
        assert_eq!(doc.snapshots(), 0, "the stale snapshot should be gone");

        // And the state it would have produced is not the state we are in: with
        // it retained, a rebuild through that position would resurrect the
        // discarded rotations.
        let cursor = doc.depth().0;
        assert_eq!(doc.replay_base(cursor), 0);
        assert_eq!(
            doc.working().page(a).unwrap().extra_turns,
            ((SNAPSHOT_EVERY - 3) % 4) as u8
        );
    }

    #[test]
    fn a_journal_replays_to_the_state_it_was_applied_to() {
        // The kernel property: undo-by-cursor is only sound if the derived state
        // can be rebuilt identically, so this walks a mixed journal and checks
        // every prefix against a replay from the baseline.
        let mut doc = Doc::open(6);
        let p: Vec<PageId> = doc.working().order().to_vec();
        let script = vec![
            Command::Rotate {
                page: p[0],
                turns: 1,
            },
            Command::Move {
                page: p[4],
                after: Some(p[0]),
            },
            Command::Crop {
                page: p[2],
                to: Some(rect(1.0, 1.0, 100.0, 200.0)),
            },
            Command::Delete { page: p[1] },
            Command::Move {
                page: p[5],
                after: None,
            },
            Command::Rotate {
                page: p[2],
                turns: -1,
            },
            Command::Delete { page: p[3] },
            Command::Move {
                page: p[0],
                after: Some(p[2]),
            },
        ];
        let mut states = vec![doc.working().clone()];
        for cmd in &script {
            doc.apply(*cmd).unwrap();
            states.push(doc.working().clone());
        }
        for (upto, want) in states.iter().enumerate() {
            assert_eq!(&doc.rebuild(upto), want, "replay of {upto} commands");
        }
        // And every undo down to the start lands on the state it recorded.
        for upto in (0..states.len() - 1).rev() {
            assert!(doc.undo());
            assert_eq!(doc.working(), &states[upto], "undo to {upto}");
        }
        // And every redo back up.
        for (upto, want) in states.iter().enumerate().skip(1) {
            assert!(doc.redo());
            assert_eq!(doc.working(), want, "redo to {upto}");
        }
    }
}
