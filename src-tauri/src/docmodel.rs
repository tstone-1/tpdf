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
//! ## Marks are created, and that needed the allocator
//!
//! A highlight is the first thing here that did not come out of the file, so it
//! is the first id this module has to *issue*. The property the note below used
//! to defer is now live and proved: **an id released by an undo is never
//! re-issued to a different mark by a later redo.**
//!
//! Two things carry it, and neither is a check that has to be remembered:
//!
//! * [`Doc::next_mark`] only ever counts up. Undo rewinds the *cursor*, never
//!   the allocator, so an id that has been issued is spent for the life of the
//!   document whether or not the command that used it is currently applied.
//! * A command carries the id it was issued, so **replay allocates nothing**.
//!   Redo re-applies `Annotate { id, .. }` with the id from the journal, which
//!   is why an undone mark comes back as itself rather than as a copy.
//!
//! What the ids buy is the same thing they buy for pages, one step further on:
//! [`Command::Unannotate`] names a mark that a stale frontend may have watched
//! disappear, and the answer is a refusal that says which of the two it is.
//!
//! ## What is deliberately not here yet
//!
//! **Nothing creates a page.** Insert, split, merge and duplicate all bring
//! pages in from somewhere. That now needs the allocator above rather than a new
//! one --- but a page id is also a *position* in the order and a source in the
//! baseline, so what a created page has to answer that a created mark does not
//! is where its content comes from.
//!
//! Save, save-mode classification, crash recovery and external-modification
//! handling are §5's other halves and none of them are here. This module holds
//! no file, no bytes and no `lopdf` object, and it is the better for it: it can
//! be driven directly rather than through a document.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroU64;

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

    /// Rebuilds an id from a value [`get`](Self::get) produced.
    ///
    /// The inverse of `get`, and it exists for one caller: a command arriving
    /// from the frontend, which was handed these ids in a state reply and sends
    /// one back. That round trip is the whole reason ids cross the boundary ---
    /// see `edits.rs` on why a command may not name a position.
    ///
    /// **Constructing one from an arbitrary number is safe**, which is not an
    /// accident of this being a newtype: every command checks the id against the
    /// live pages and the tombstones before it mutates anything, so a number
    /// nobody issued is [`Refusal::NoSuchPage`] rather than a page. That is what
    /// keeps this from being a hole in the opacity the type is for.
    pub fn from_raw(value: u64) -> PageId {
        PageId(value)
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

/// A page's size in points.
///
/// **Not a [`Rect`], and the difference is what it forbids.** A rectangle can
/// have any origin, and a page tpdf makes has exactly one: `[0 0 width height]`.
/// Carrying the origin would let a caller ask for a page whose media box starts
/// somewhere else, which nothing here writes and nothing here reads --- a field
/// with no reachable value is the shape `docs/TRAPS.md` records as a guard that
/// cannot fire. The writer turns this into the rectangle at the moment it writes.
#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    /// Whether the size encloses any area at all.
    ///
    /// [`Rect::is_proper`]'s twin, and the note there about `NaN` carries over
    /// word for word: every comparison against `NaN` is false, so a `NaN` in
    /// either field is improper without that being written anywhere. It is
    /// asserted in the tests rather than left to be rediscovered, because a page
    /// of `NaN` points would reach `/MediaBox` and no reader could open the file.
    pub fn is_proper(self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }
}

/// A mark's identity, stable for the life of the working document.
///
/// Opaque for the same reason [`PageId`] is, and issued rather than derived ---
/// see the module note on the allocator, which is the one property this type
/// exists to carry.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct MarkId(u64);

impl MarkId {
    /// The raw value, for logging and for keying a map across the IPC boundary.
    pub fn get(self) -> u64 {
        self.0
    }

    /// Rebuilds an id from a value [`get`](Self::get) produced.
    ///
    /// Safe for the reason [`PageId::from_raw`] is safe: every command checks the
    /// id against the live marks and the graves before it mutates anything, so a
    /// number nobody issued is [`Refusal::NoSuchMark`] rather than a mark.
    pub fn from_raw(value: u64) -> MarkId {
        MarkId(value)
    }
}

/// A redaction's identity, stable for the life of the working document.
///
/// Opaque and issued rather than derived, as [`MarkId`] is. **A separate type
/// rather than a second use of that one**, and the reason is the whole design of
/// this feature: a mark is written into the saved file as an annotation, and a
/// redaction must never be. Keeping the two identities apart is what makes
/// "write a redaction as an annotation" unexpressible instead of a per-kind
/// exclusion in `save.rs` that a later kind could be added without.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct RedactionId(u64);

impl RedactionId {
    /// The raw value, for logging and for keying a map across the IPC boundary.
    pub fn get(self) -> u64 {
        self.0
    }

    /// Rebuilds an id from a value [`get`](Self::get) produced.
    ///
    /// Safe for [`MarkId::from_raw`]'s reason: every command checks the id
    /// against the live redactions and the graves before it mutates anything, so
    /// a number nobody issued is [`Refusal::NoSuchRedaction`] rather than a
    /// redaction.
    pub fn from_raw(value: u64) -> RedactionId {
        RedactionId(value)
    }
}

/// A rectangle in **display** space: points from the displayed page's top-left
/// corner, y increasing downwards, after the page's `/Rotate` and relative to
/// its crop box.
///
/// **Not [`Rect`]**, and the two must not be confused: `Rect` is PDF user space
/// with y upwards, which is what a `/CropBox` is written in, and this is the
/// space the viewer lays glyphs out in. They differ by a flip and a turn, and a
/// value of one passed where the other is expected is a plausible rectangle in
/// the wrong place --- which is why they are separate types rather than one
/// four-field struct used twice.
///
/// The model holds display space because that is what the reader's drag produced
/// and what the overlay draws; `save.rs` maps it into the page's own space with
/// [`crate::text::from_device`] at the moment it writes, where the crop box and
/// `/Rotate` are in hand.
#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Quad {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Quad {
    /// Whether the quad covers any area at all.
    ///
    /// `NaN` in any corner is not covered, which falls out of the comparisons
    /// exactly as it does for [`Rect::is_proper`] --- and is asserted rather than
    /// left to be rediscovered, because a `/QuadPoints` entry built from one
    /// makes a whole annotation undrawable in some readers and invisible in the
    /// rest.
    pub fn covers_area(self) -> bool {
        self.right > self.left && self.bottom > self.top
    }
}

/// One point of a stroke a reader drew, in the same display space as [`Quad`].
///
/// **A separate type from [`Quad`] rather than a degenerate one**, for the reason
/// `Quad`'s own note gives about `Rect`: a value of one passed where the other is
/// expected would be plausible and wrong, and here the confusion is easy to make
/// because a point *is* expressible as a rectangle of no size --- which is
/// exactly the value [`Quad::covers_area`] exists to reject.
#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// How thick a freehand line is, in points.
///
/// **Heavier than [`crate::save::OUTLINE_WIDTH`], and the reason is what each
/// mark is for.** A box is a frame round something a reader wants to point at,
/// and a frame that competes with its contents is a worse frame. A drawn line
/// *is* the content: it is a reader's handwriting, a circle round a figure, an
/// arrow. At 1.5 pt freehand ink reads as tentative --- and unlike a box, which
/// is four straight edges, a hand-drawn line at hairline weight breaks up
/// visually wherever the pointer moved fast.
///
/// **It lives here, beside [`Stroke`], because it is part of the geometry**
/// rather than only of the drawing: an ink mark has no quads of its own, so its
/// rectangle is derived from its points padded by half this width, and that
/// derivation is what [`Stroke::bounds`] does. It sat in `save.rs` and was
/// reached for from here and from `edits.rs`, which put the pure model
/// downstream of the writer for a number the model cannot do without.
///
/// Public for [`crate::save::OUTLINE_WIDTH`]'s reason: `annot-probe` measures
/// the stroke it draws, and a second copy of the number in the probe would
/// agree with a wrong value here as readily as with a right one. `markband.ts`
/// holds the same number for the overlay.
pub const INK_WIDTH: f64 = 2.5;

/// The thinnest and thickest a [`Mark::width`] may be, in points.
///
/// **A range rather than a list, because the wire is not the table.** The nibs a
/// reader can pick are named in `marknibs.ts` and every one of them sits inside
/// this; what these two bound is the number that arrives over IPC, which is
/// whatever a caller sends. Refusing anything outside the table would put a copy
/// of the table on this side of the boundary, and the copy that is wrong is the
/// one nothing reaches.
///
/// A quarter point is a hairline at any zoom a reader works at and is still a
/// line; below it the difference between widths stops being visible and `0 w`
/// means "as thin as this device draws", which is a rendering decision rather
/// than a width. Twenty-four points is about a third of an inch --- wider than
/// any pen and wide enough that the mark is a block --- and it is the ceiling
/// rather than the answer to a question anybody asked.
pub const NIB_MIN: f64 = 0.25;
/// See [`NIB_MIN`].
pub const NIB_MAX: f64 = 24.0;

/// One continuous line a reader drew without lifting the pointer.
///
/// `/InkList` is an array of these, which is why a mark holds a `Vec<Stroke>`
/// rather than a `Vec<Point>`: lifting the pointer and drawing again is one mark
/// with two strokes, and flattening them would join the end of the first to the
/// start of the second with a line the reader never drew.
///
/// **Two points is the minimum and it is a real one.** A single point is a
/// stroke with no length, which draws nothing with a round cap and nothing at
/// all without one --- the ink equivalent of the empty quad above.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Stroke {
    pub points: Vec<Point>,
}

impl Stroke {
    /// Whether this stroke can be drawn at all.
    ///
    /// The counterpart of [`Quad::covers_area`], and the same job: a stroke that
    /// fails this is one a reader cannot see and cannot find again to remove, so
    /// writing it would be a command that silently did nothing.
    ///
    /// Length rather than point count, because a press that jitters produces
    /// several points at one place, and three copies of one coordinate is as
    /// invisible as one of them.
    pub fn is_drawable(&self) -> bool {
        let Some(first) = self.points.first() else {
            return false;
        };
        self.points.iter().any(|p| p.x != first.x || p.y != first.y)
    }

    /// The rectangle enclosing every point, grown by `pad` on each side.
    ///
    /// **This is what makes ink fit the rest of the machinery.** Every other
    /// kind answers "where is this mark" with its quads --- `/Rect`, the popup
    /// anchor, hit-testing and the mark list all ask that question --- and ink
    /// answers it with the bounds of what was drawn rather than with a shape of
    /// its own. So an ink mark carries one quad like any other, and the strokes
    /// are the *extra* field rather than a replacement for the geometry.
    ///
    /// **`pad` is half the line width, and it is not cosmetic.** A stroke
    /// straddles its path, so tight bounds clip half the ink at every edge ---
    /// the same arithmetic `outline_path` in `save.rs` is written around, and
    /// the same failure, which looks like a thinner line rather than like a bug.
    ///
    /// It also removes a degenerate case that would otherwise refuse a perfectly
    /// ordinary mark. A straight vertical line has tight bounds of **no width**,
    /// and [`Quad::covers_area`] rejects those --- so a reader ruling a margin
    /// down the side of a paragraph would have been told their mark covers no
    /// area. Padding is the honest fix rather than a special case, because the
    /// padded rectangle is the one the ink actually occupies.
    pub fn bounds(strokes: &[Stroke], pad: f32) -> Option<Quad> {
        let mut points = strokes.iter().flat_map(|s| s.points.iter());
        let first = points.next()?;
        let mut quad = Quad {
            left: first.x,
            top: first.y,
            right: first.x,
            bottom: first.y,
        };
        for point in points {
            quad.left = quad.left.min(point.x);
            quad.top = quad.top.min(point.y);
            quad.right = quad.right.max(point.x);
            quad.bottom = quad.bottom.max(point.y);
        }
        Some(Quad {
            left: quad.left - pad,
            top: quad.top - pad,
            right: quad.right + pad,
            bottom: quad.bottom + pad,
        })
    }
}

/// One version of one mark's note.
///
/// Opaque, and it never leaves this module: the frontend addresses a note by the
/// [`MarkId`] it hangs on, and this is the identity of *what it said at a point
/// in the journal*. A [`Command::Renote`] carries one of these rather than the
/// text, for exactly the reason [`Command::Annotate`] carries a [`MarkId`] ---
/// a `String` in the enum is a `Copy` bound lost and a clone per replayed
/// command.
///
/// The allocator behind it has the same property [`Doc::next_mark`] has and for
/// the same reason: it only counts up, so the text an undone `Renote` named is
/// still the text its id names when a redo re-applies it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct NoteId(u32);

/// One version of one drawing's strokes.
///
/// [`NoteId`]'s twin, for the same reason and with the same allocator: a
/// [`Command::Reink`] names *what the drawing was at a point in the journal*
/// rather than carrying the points, which keeps [`Command`] `Copy` and replay
/// allocation-free.
///
/// **It exists because an eraser makes a mark's strokes a thing that changes**,
/// and everything that changes has to be rebuildable by replay. Until the
/// eraser, a drawing's points were written once by [`Doc::annotate`] and never
/// again, so they could live in the body table with the colour and the author.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct InkId(u32);

/// One version of one mark's colour.
///
/// [`NoteId`]'s third twin, with the same allocator and the same argument:
/// a [`Command::Recolor`] names *what the mark was drawn in at a point in the
/// journal* rather than carrying three floats, so [`Command`] stays `Copy`.
///
/// Three floats would in fact fit in a `Copy` command, which is the one place
/// this differs from its twins --- and it is still an id, because the rule the
/// journal rests on is that a command names identities and the tables hold
/// bodies. A variant that carried its value would be the one place a reader had
/// to check which kind of thing they were looking at.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ColorId(u32);

/// An annotation in the file the document was opened from, as its object number
/// and generation.
///
/// **Not a model id, and that is the whole reason it is its own type.** Every
/// other id in this module names something the reader made and the model
/// numbered; this names something the *file* already numbered, which somebody
/// else wrote. It comes from `annots::Comment::object`, and a comment that has
/// none --- a direct dictionary inside a page's `/Annots` --- cannot be named
/// here at all, which is a structural limit rather than a gap.
///
/// **The model cannot check that it names anything.** It has never parsed the
/// file: the scan runs in the worker. So "no such object", "not a dictionary"
/// and "not an annotation" are `save::set_note`'s refusals and not this
/// layer's. What this layer does own is the *page*, which is why
/// [`Command::Rewrite`] carries one beside the object.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ObjectId {
    number: u32,
    generation: u16,
}

impl ObjectId {
    /// The object as `lopdf` and the plan spell it.
    pub fn new(number: u32, generation: u16) -> ObjectId {
        ObjectId { number, generation }
    }

    /// The object number.
    pub fn number(self) -> u32 {
        self.number
    }

    /// The generation.
    pub fn generation(self) -> u16 {
        self.generation
    }
}

/// One version of one foreign comment's body.
///
/// [`NoteId`]'s fourth twin, with the same allocator and the same argument: a
/// [`Command::Rewrite`] names *what the comment said at a point in the journal*
/// rather than carrying the text, so [`Command`] stays `Copy` and replay stays
/// allocation-free.
///
/// It is a separate id from [`NoteId`] rather than a reuse, because the two
/// tables hold different things: a note is a string, and this is a string **and
/// the date it was typed**. `/M` moves when a comment's text does --- every
/// viewer shows that date, and a reader whose words appear over somebody else's
/// timestamp has been told something false.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct RewriteId(u32);

/// What one version of one foreign comment says, and when it was typed.
///
/// The date is carried rather than read from a clock at save time for
/// [`Mark::made`]'s reason exactly: a model with a clock in it needs the clock
/// frozen to be tested, and a plan that writes different bytes on every attempt
/// cannot be compared against itself.
#[derive(Clone, PartialEq, Debug)]
pub struct NoteEdit {
    /// What the comment now says. Replaces `/Contents` whole.
    pub body: String,
    /// When it was typed, in PDF date form. Replaces `/M`.
    pub made: String,
}

/// A foreign comment the reader has rewritten: which page, and which version.
///
/// The page is here so that deleting a page takes its rewrites with it, the way
/// deleting a page takes its marks. Without it the model would keep an edit for
/// a comment that is not in the document any more, and the plan would carry it
/// to a writer that would either refuse a save the reader could not diagnose or
/// write it into an object about to be swept.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Rewritten {
    page: PageId,
    edit: RewriteId,
}

/// Where a mark is now: its strokes, and the rectangles it occupies.
///
/// **The two travel together so that they cannot disagree.** For a drawing the
/// rectangle is [`Stroke::bounds`] of the strokes and nothing else, derived once
/// by [`Doc::reink`]; a caller reading the strokes from here and the rectangle
/// from the body would get a rectangle that still holds an erased stroke.
///
/// **Named for ink and no longer only ink's, which is a widening rather than a
/// lie.** The eraser was the only thing that could replace a mark's geometry
/// when this was written. Dragging a mark to move it is the second, and it
/// applies to every kind --- so this now carries the geometry of whichever mark
/// [`Working::ink_of`] names, with `strokes` empty for the five kinds that have
/// none. [`Doc::displace`] is the writer.
///
/// The name stands because inside this codebase "ink" already means *how a mark
/// is laid down* rather than the substance --- `Paint` in `save.rs`, `markBand`
/// in the overlay, and `markpopup.ts` says so where it explains why a reader
/// sees "Drawing". Renaming would touch [`InkId`], [`Command::Reink`],
/// [`Doc::reink`], the table, the accessors and every test that names them, on a
/// grep that also matches [`MarkKind::Ink`] and the PDF's own `/Ink` --- the
/// mechanical-edit-keyed-on-a-name trap this repository has already paid for
/// once. A paragraph is the cheaper and the more honest fix.
#[derive(Clone, PartialEq, Debug)]
pub struct Ink {
    /// What is still drawn. Empty for every kind but [`MarkKind::Ink`].
    pub strokes: Vec<Stroke>,
    /// [`Stroke::bounds`] of them, padded as [`Doc::reink`] pads it --- or, for a
    /// mark that has no strokes, the rectangles it now occupies.
    pub quads: Vec<Quad>,
}

/// What kind of mark a reader made.
///
/// **Only what tpdf can write.** [`crate::annots::Kind`] is the reading
/// vocabulary and has seventeen variants, because a document may contain any of
/// them; this is the producing vocabulary, and a variant here is a promise that
/// the write path can turn it into an annotation Acrobat and Preview both
/// render. Growing this enum is therefore a change to `save.rs` and not only to
/// a list of names.
///
/// **Serde-visible, and the names are the wire format.** The frontend chooses
/// the kind, so this crosses the boundary in both directions --- in on
/// `NewMark`, out on `MarkView` --- and the lowercase names are what a check
/// harness and a saved session see. Renaming a variant is a protocol change.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkKind {
    /// A wash over text, `/Highlight`.
    Highlight,
    /// A line under text, `/Underline`.
    Underline,
    /// A line through text, `/StrikeOut`.
    ///
    /// The PDF name is `/StrikeOut` with that capitalisation; the serde name is
    /// `strikeout`, which is also the word the command and the menu use. Two
    /// spellings of one thing, and the only place they meet is `save.rs`'s
    /// `subtype`.
    StrikeOut,
    /// A comment anchored to a point, `/Text`.
    ///
    /// **The odd one out, and every difference follows from one fact: it does
    /// not mark text.** The other three are text-markup annotations --- they
    /// take their shape from a selection, carry `/QuadPoints`, and mean nothing
    /// without words underneath. This one is a bubble a reader drops on the
    /// page, so its single quad is the icon's own box rather than anything the
    /// document said, it writes no `/QuadPoints` at all, and it needs no
    /// selection to exist.
    ///
    /// It reuses the rest of the machinery deliberately. A comment has a page, a
    /// rectangle, a colour, an author, a date and a note that can be edited
    /// afterwards --- which is exactly [`Mark`], so removal, notes, undo, the
    /// id table and the whole state reply come free. A parallel type would have
    /// duplicated all of it to express one absent field.
    ///
    /// The serde name is `note` and the PDF name is `/Text`. That is the same
    /// two-spelling situation as `StrikeOut` above, in the other direction, and
    /// it is deliberate: to a reader this is a comment or a note, and `/Text` is
    /// a name that would suggest text on the page to anyone who has not read the
    /// specification.
    Note,
    /// A wavy line under a run of words, `/Squiggly`.
    ///
    /// **The fourth text-markup kind, and the last one PDF 32000-1 has.** It
    /// takes a selection exactly as [`MarkKind::Highlight`],
    /// [`MarkKind::Underline`] and [`MarkKind::StrikeOut`] do, carries
    /// `/QuadPoints` as they do, and is positioned by the words rather than by
    /// the reader --- so nothing about how it is made, moved, coloured or
    /// removed is new. What is new is the only thing that ever is here: what
    /// gets drawn.
    ///
    /// **It is the underline's near-twin, and that is the hazard rather than
    /// the convenience.** Both sit at the bottom of the quad, both are red by
    /// default, and every reading the checks took of an underline before this
    /// kind existed is also true of a squiggle: ink at the bottom, an empty
    /// centre, one inked side. A check that simply reused the underline's bounds
    /// would be one that cannot fail --- see the trap about a near-twin
    /// inheriting a predicate. What separates them is *height*: an underline is
    /// a rule `LINE_FRACTION` of the text tall and a squiggle occupies a band
    /// `SQUIGGLE_HEIGHT` tall (both in `save.rs`), so there is ink above the
    /// underline's rule and
    /// nothing there for an underline.
    ///
    /// The serde name is `squiggly`, the PDF name is `/Squiggly`, and the word
    /// a reader sees is **Squiggly** --- the second kind whose three spellings
    /// agree, after [`MarkKind::Ellipse`], and for the same reason: there is no
    /// better everyday word for it, and `comments.ts` already uses this one for
    /// a document's own.
    Squiggly,
    /// A rectangle a reader drew, `/Square`.
    ///
    /// **The second kind the document did not place**, and the second half of
    /// the split `Note` opened. Both take their rectangle from the reader rather
    /// than from a selection, so neither carries `/QuadPoints` --- but they part
    /// company immediately afterwards, and the two questions this file used to
    /// answer with one predicate are now genuinely two. A comment needs *no*
    /// appearance stream, because every reader synthesises its own icon; a box
    /// needs one, because no reader synthesises a rectangle, and a `/Square`
    /// with no `/AP` is an annotation Acrobat draws as nothing at all.
    ///
    /// Its ink is a **stroke**, which is the first in this enum. A filled box
    /// would cover whatever it was drawn around, and covering things is the one
    /// job a box does not have.
    ///
    /// The serde name is `square`, the PDF name is `/Square`, and the word a
    /// reader sees is **Box** --- a third spelling, and the reason is that a
    /// rectangle dragged round a figure is almost never square. `/Square` is the
    /// specification's name for the family that includes `/Circle`, not a claim
    /// about the proportions.
    Square,
    /// An ellipse a reader drew, `/Circle`.
    ///
    /// **The box's sibling, and the first kind whose shape is not its
    /// rectangle.** [`MarkKind::Square`] above says that `/Square` is the
    /// specification's name for the family that includes `/Circle` rather than a
    /// claim about proportions, and this is the other member of it. Everything
    /// that positions a mark treats the two identically: the reader drags a
    /// rectangle, `/Rect` is that rectangle, the popup hangs off it and the hit
    /// test is against it. What differs is one thing only --- what is *drawn*
    /// inside it, which is `save.rs`'s `Paint` for the file and `markband.ts`
    /// for the overlay.
    ///
    /// That is a narrower difference than it first looks and a wider one than
    /// `Viewer.armDraw`'s comment predicted. It said the next drag tool would
    /// differ "in the subtype it writes and in nothing else", and the gesture
    /// half of that is exactly right --- no drag code changed to add this. The
    /// appearance half is not: a rectangle is one `re` operator and an ellipse
    /// is four Bézier arcs, because PDF content streams have no ellipse
    /// primitive. A kind that differed *only* in its subtype would draw as a
    /// rectangle in every reader.
    ///
    /// **Its rectangle is not its ink**, which is what separates it from the
    /// box. A box's stroke runs along the edge of its `/Rect`; an ellipse's
    /// touches that edge at four points and is inside it everywhere else. So the
    /// hit test selects on a bounding box that is mostly *not* drawn --- the same
    /// bargain the box already makes with its own empty middle, which is
    /// stroked-not-filled and selectable throughout. Consistency with the box
    /// decided it: two shapes a reader drags out the same way should not answer
    /// a press by two different rules.
    ///
    /// The serde name is `ellipse`, the PDF name is `/Circle`, and the word a
    /// reader sees is **Ellipse** --- which makes this the one shape kind whose
    /// three spellings do not all differ. `/Circle` is the specification's name
    /// and is wrong for a reader in exactly the way `/Square` is: one that is
    /// actually circular is the rare case.
    Ellipse,
    /// A box of the reader's own words, `/FreeText`.
    ///
    /// **The first kind whose note is not metadata but the mark itself.** Every
    /// other kind has a note *about* something drawn: take the note away and the
    /// highlight, the box, the drawing are all still there. Take a text box's
    /// away and there is nothing left --- the words are what is on the page.
    ///
    /// That is one property with three consequences, and they are the whole of
    /// why this kind is more than a new subtype:
    ///
    /// - **Editing the note changes the appearance.** [`Command::Renote`]
    ///   already rebuilds [`Working`] and `save.rs` already builds its plan from
    ///   the model on every save, so this needs no new machinery --- but it is
    ///   the first kind for which that mattered, and a design that had cached an
    ///   appearance per mark at creation would break here.
    /// - **The writer has to lay text out**, which needs the width of every
    ///   glyph. `textbox.rs` is that, and it is the only place in this
    ///   repository that measures text.
    /// - **What a reader types can be unwritable.** Helvetica with
    ///   `/WinAnsiEncoding` covers Latin-1 and nothing else, so a pasted line of
    ///   Greek is refused rather than written as substituted glyphs --- see
    ///   `textbox::encodable`.
    ///
    /// It is placed by a drag, exactly as [`MarkKind::Square`] and
    /// [`MarkKind::Ellipse`] are, and carries no `/QuadPoints` for their reason:
    /// its rectangle is the reader's, not a run of words.
    ///
    /// The serde name is `textbox`, the PDF name is `/FreeText`, and the word a
    /// reader sees is **Text box**. Three spellings again, and `/FreeText` is
    /// the one that had to go: "free" there means unattached to a text
    /// selection, which is a distinction only the specification makes.
    TextBox,
    /// A line a reader drew freehand, `/Ink`.
    ///
    /// **The first kind whose shape is not a rectangle**, and the reason it is a
    /// field on [`Mark`] rather than a type of its own. Ink has a page, a
    /// colour, an author, a date, a note that can be edited afterwards, and a
    /// rectangle it occupies --- which is every other kind --- plus the strokes.
    /// That is one extra field, and the argument [`MarkKind::Note`] makes above
    /// about not duplicating the machinery to express one *absent* field applies
    /// unchanged to one present one.
    ///
    /// Its quad is [`Stroke::bounds`], so removal, the popup anchor,
    /// hit-testing, the mark list and `/Rect` all keep asking the one question
    /// they already ask. What the strokes decide is only what is *drawn*, in
    /// `save.rs` for the file and `markband.ts` for the overlay.
    ///
    /// The serde name is `ink`, the PDF name is `/Ink`, and the word a reader
    /// sees is **Draw** --- a third spelling for [`MarkKind::Square`]'s reason,
    /// with the extra one that "ink" inside this codebase already names
    /// `save.rs`'s `Paint`, which is how a mark is laid down rather than which
    /// mark it is.
    Ink,
    /// A standard stamp a reader placed, `/Stamp`.
    ///
    /// **The last markup kind, and the first whose content is a closed set.**
    /// Every other kind's content comes from the reader: a selection, a
    /// rectangle, a note they typed, strokes they drew. A stamp says one of a
    /// fixed list of words, and which one is the whole of the mark ---
    /// [`StampName`] is that list.
    ///
    /// **It needs an appearance stream, and that was measured rather than
    /// assumed.** [`MarkKind::Note`] carries none because every reader
    /// synthesises a `/Text` icon; a stamp looks like the same case and is not.
    /// Rendering a `/Stamp` with `/Name /Approved` and no `/AP` through PDFium
    /// draws **0** non-white pixels, against **336** for a `/Text` with no `/AP`
    /// on the same page through the same code --- so the control is what makes
    /// the zero mean "nothing was drawn" rather than "nothing was rendered". It
    /// is on [`MarkKind::Square`]'s side of the line, not the comment's.
    ///
    /// `/Name` is written anyway. It is not what draws the stamp here, and it is
    /// what a reader that *would* synthesise one draws from, which is why the
    /// list is the specification's own rather than words we chose.
    ///
    /// It is placed by a drag, as [`MarkKind::Square`], [`MarkKind::Ellipse`]
    /// and [`MarkKind::TextBox`] are, and carries no `/QuadPoints` for their
    /// reason.
    ///
    /// The serde name is `stamp`, the PDF name is `/Stamp`, and the word a
    /// reader sees is **Stamp** --- the third kind whose three spellings agree,
    /// after [`MarkKind::Ellipse`] and [`MarkKind::Squiggly`], and for their
    /// reason: there is no better everyday word.
    Stamp,
}

/// Which standard stamp a [`MarkKind::Stamp`] is.
///
/// **A closed set, and the specification's own.** PDF 32000-1 Table 181 lists
/// fourteen standard `/Name` values for a stamp annotation; these four are the
/// ones whose meaning is unambiguous in everyday English, and the rest are a
/// table entry away rather than a design change. Choosing words of our own
/// instead would have made `/Name` unwritable --- a reader that synthesises a
/// stamp appearance draws from that list and from nothing else.
///
/// **The word drawn and the `/Name` written are the same choice**, which is why
/// this is one enum and not a name beside a string. A stamp whose appearance
/// said `DRAFT` and whose `/Name` said `/Approved` would read differently in two
/// readers and be right in neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StampName {
    Approved,
    Confidential,
    Draft,
    Final,
}

impl StampName {
    /// The `/Name` value, which is the specification's spelling.
    #[must_use]
    pub fn pdf_name(self) -> &'static [u8] {
        match self {
            Self::Approved => b"Approved",
            Self::Confidential => b"Confidential",
            Self::Draft => b"Draft",
            Self::Final => b"Final",
        }
    }

    /// The word to draw, which is upper case because a stamp is.
    ///
    /// Latin-1 throughout, which is not a coincidence to rely on quietly:
    /// `textbox.rs` writes Helvetica with `/WinAnsiEncoding` and refuses
    /// anything outside it, so a name added here in another script would be
    /// refused by the writer rather than drawn wrong.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Approved => "APPROVED",
            Self::Confidential => "CONFIDENTIAL",
            Self::Draft => "DRAFT",
            Self::Final => "FINAL",
        }
    }
}

/// One mark a reader made, with everything a writer needs to put it in a file.
///
/// Not `Copy`, which is why it lives in a table on [`Doc`] keyed by [`MarkId`]
/// rather than inside the command: keeping [`Command`] `Copy` is what lets
/// replay stay a `for &cmd in ...` loop over the journal.
///
/// **The note is not here**, and that is the one field a reader can change after
/// the fact. Everything in this struct is fixed at the moment the mark is made,
/// which is what lets [`Doc`] keep one body per id and never touch it again; a
/// mutable field would have to be rebuilt by undo, and undo rebuilds
/// [`Working`]. So the note lives there --- see [`NoteId`].
#[derive(Clone, PartialEq, Debug)]
pub struct Mark {
    pub kind: MarkKind,
    /// The page the mark is on. Held here as well as in [`Working`] so that a
    /// writer holding only the table knows where each mark goes.
    pub page: PageId,
    /// The covered rectangles, in display space, in reading order.
    ///
    /// For [`MarkKind::Ink`] this is one rectangle, [`Stroke::bounds`] of the
    /// strokes below, so that everything asking *where* a mark is gets the same
    /// answer for every kind.
    pub quads: Vec<Quad>,
    /// The lines drawn freehand, in display space. Empty for every kind but
    /// [`MarkKind::Ink`], and non-empty for that one.
    ///
    /// **The biconditional is enforced rather than typed**, and that is a
    /// deliberate trade this file otherwise argues against. Putting the strokes
    /// inside the `MarkKind::Ink` variant would carry it in the type --- and
    /// would cost [`MarkKind`] its `Copy`, which [`Command`] is built on, and
    /// would make every `match` on a kind a `match` on a shape. So it is a
    /// field, and [`Doc::annotate`] refuses both halves: strokes on a kind that
    /// is not ink, and ink with nothing drawn. Both refusals have a test,
    /// because a rule with no failing case is a comment.
    pub strokes: Vec<Stroke>,
    /// Which standard stamp this is. `Some` exactly for [`MarkKind::Stamp`].
    ///
    /// **The same arrangement as [`Mark::strokes`] above, and for its reason**:
    /// putting the name inside the `MarkKind::Stamp` variant would carry the
    /// biconditional in the type and would cost [`MarkKind`] its `Copy`, which
    /// [`Command`] is built on. So it is a field, [`Doc::annotate`] refuses both
    /// halves, and both refusals have a test.
    ///
    /// Unlike the note, it is fixed at the moment the mark is made. A reader who
    /// wants a different word places a different stamp --- which is what a stamp
    /// is, and it keeps this struct's "everything here is fixed at creation"
    /// property intact.
    pub stamp: Option<StampName>,
    /// The comment this one answers, when it is a reply. `None` for a mark that
    /// answers nothing, which is every mark a reader places on the page.
    ///
    /// **An [`ObjectId`] rather than a [`MarkId`], and the asymmetry is the
    /// point.** A reply's parent is an annotation *out of the file* --- the
    /// model has never read it and has no id for it --- so the only name the two
    /// sides agree on is the one the file itself uses, which is exactly what
    /// [`Command::Rewrite`] says about the comment it edits.
    ///
    /// **One-directional where [`Mark::stamp`] is a biconditional.** A reply
    /// must be a [`MarkKind::Note`], because `/IRT` on a highlight is not a
    /// thread, it is a highlight with a stray key. But a `Note` with no parent
    /// is an ordinary sticky note and is the commoner thing by far, so
    /// [`Doc::annotate`] refuses one half and not the other --- and the test for
    /// it is named for that, because a rule with no failing case is a comment.
    ///
    /// Fixed at creation like everything else here. A reader who wants to answer
    /// a different comment writes a different reply; moving one between threads
    /// is not an operation any reader has asked for and would need `/IRT` to be
    /// mutable, which would put it in [`Working`] with the note.
    pub reply_to: Option<ObjectId>,
    /// Red, green and blue in 0..=1, as `/C` takes them.
    pub color: [f32; 3],
    /// How thick this mark's ink is, in the page's own points.
    ///
    /// [`INK_WIDTH`] is what a caller sends when the reader has chosen nothing,
    /// and every kind carries the number rather than ink alone --- `save.rs`
    /// writes a `w` operator for all of them, so a field only ink could use
    /// would make the writer ask which kind it had before reading a value it
    /// needs either way.
    ///
    /// **Points rather than anything on screen, and there is no choice in it.**
    /// The appearance stream's `w` is in the form's space, which is the page's
    /// with no matrix, so this number *is* what a foreign reader draws. A width
    /// taken from the view would make a line drawn at 400% four times thinner in
    /// the file than the same line drawn at 100%, and neither the file nor the
    /// reader would say so. The eraser's nib is view pixels for the opposite
    /// reason and `markband.ts` argues it there; the two are called the same
    /// word and are not the same quantity.
    ///
    /// **Fixed at creation, like [`Mark::stamp`] and unlike [`Mark::color`]**,
    /// and the reason is geometry rather than taste. A colour changes nothing
    /// about where a mark is, so [`Command::Recolor`] can replace one by naming
    /// an id. Ink's rectangle is [`Stroke::bounds`] of its strokes padded by
    /// *half this number*, so changing it after the fact would have to rebuild
    /// [`Mark::quads`] on every replay --- and a `/Rect` that did not follow
    /// would be a mark whose ink hangs outside its own box. That is a command
    /// with a body table and a derivation, which is a separate piece of work;
    /// what is here is the pen a reader picks up before they draw.
    ///
    /// Brought into `NIB_MIN..=NIB_MAX` at the wire boundary --- see
    /// `edits.rs`'s `nib`, which exists because `1e40` is valid JSON and
    /// `format!("{} w", f64::INFINITY)` is `inf` in the middle of a content
    /// stream.
    pub width: f64,
    /// `/T`. Empty when the reader has no name set.
    pub author: String,
    /// `/M`, already in PDF date form.
    ///
    /// Supplied by the caller rather than read from a clock here. This module
    /// has no clock on purpose --- a test that has to freeze time to assert a
    /// journal is a test about the clock.
    pub made: String,
}

/// A region a reader has marked for removal.
///
/// **Not a [`Mark`], and the difference is a lifecycle rather than a shape.** A
/// mark is a thing the reader adds to the document: it is drawn, it is saved as
/// an annotation, and it is still there when the file is reopened. A redaction
/// is an *instruction* about the document: it is drawn only while it is pending,
/// it is never written to a file, and applying it destroys content and consumes
/// the instruction. Two lifecycles is two types --- see [`RedactionId`] for the
/// property that buys.
///
/// It is deliberately smaller than a mark, and every absent field is absent for
/// a reason rather than for now. There is no kind, because a redaction has one.
/// There is no colour, because what a pending redaction is drawn in is the
/// overlay's decision and nothing about it survives the apply. There is no note
/// and no author, because nothing here is written into a file for anyone to
/// read. If overlay text (`/OverlayText`, the word a reader can leave in the
/// hole) is ever built, it is a field then and a command of its own.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Redaction {
    /// The page the region is on. Held here as well as in [`Working`] for
    /// [`Mark::page`]'s reason: a reader of the table alone knows where it goes.
    pub page: PageId,
    /// The region, in display space.
    ///
    /// **One rectangle rather than a list**, which is the other place this
    /// parts company with a mark. A text-markup mark takes its shape from a
    /// selection and needs a quad per line; a redaction is a region a reader
    /// dragged out, and covering three lines with one rectangle is what they
    /// asked for. A selection that should redact several lines is several
    /// redactions, which is also what the review list has to show them.
    pub area: Quad,
}

/// What supplies a page's content.
///
/// **The seam the render path uses**: a viewport position indexes
/// [`Working::order`], that yields a [`PageId`], and this says what to draw for
/// it. It was a bare `u32` naming a baseline page until 2026-08-30, and widening
/// it is the whole of what inserting a page costs everywhere except the writer
/// --- every consumer that asked a worker for a tile, a page's text or its
/// objects has to say what it does when no file supplies the page.
///
/// **Named `PageSource` rather than `Source`**, which is taken:
/// [`crate::docgraph::Source`] says where a *document's bytes* come from. Two
/// types of one name in one crate is the collision `docs/TRAPS.md` records as
/// the worse kind, because it compiles wherever the wrong one happens to have a
/// method of the right shape.
#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageSource {
    /// Page `n` of the file this document was opened from. Zero-based.
    Baseline(u32),
    /// A page tpdf made, at this size. No file supplies it.
    ///
    /// **The size is here rather than beside it**, because a page has a size
    /// exactly when no baseline page supplies one --- a baseline page's size is
    /// the file's answer and nothing here may contradict it. Two fields would
    /// make that a biconditional somebody has to check; one enum makes it the
    /// type's answer, which is the distinction [`Refusal::ShapeMismatch`] exists
    /// because [`Mark`] could not draw.
    Blank(Size),
}

/// One page of the working document.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Page {
    /// What supplies the content --- see [`PageSource`].
    pub source: PageSource,
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
    /// Put a new blank page immediately after `after`, or at the front when
    /// `after` is `None`.
    ///
    /// Expressed against a neighbouring *id* rather than a destination index for
    /// [`Command::Move`]'s reason, and it is that command's shape with one field
    /// more: the page being placed does not exist yet, so `page` is the id this
    /// command **issues** rather than one it names.
    ///
    /// **The size travels in the command**, unlike every other body in this
    /// enum, which lives in a [`Doc`] table. There is no version of a page's
    /// size for a table to be keyed by --- nothing edits it --- and a [`Size`]
    /// is two `f64`s, so carrying it here costs the journal sixteen bytes and
    /// keeps the enum `Copy`. A table would buy the indirection and nothing else.
    ///
    /// **Replay re-issues nothing.** The id is in the command, so replaying it
    /// from a snapshot rebuilds the same page rather than a different one
    /// wearing a new number --- the property the module note states for marks,
    /// and it holds here for the same reason.
    Insert {
        page: PageId,
        after: Option<PageId>,
        size: Size,
    },
    /// Put a mark on a page.
    ///
    /// The mark's body is in [`Doc`]'s table under `mark`; only the identity is
    /// journalled, which is what keeps this enum `Copy` and replay allocation-free.
    ///
    /// `note` is what the mark says when this command is applied, which is
    /// almost always nothing: a reader highlights a line and types on it
    /// afterwards, if at all.
    Annotate {
        mark: MarkId,
        page: PageId,
        note: NoteId,
    },
    /// Take a mark off the page it is on.
    Unannotate { mark: MarkId },
    /// Replace what a mark says.
    ///
    /// A whole note rather than an edit to one: the reader types in a box and
    /// commits it, so the command is *what it now says*, and undo is the
    /// previous `Renote` (or the `Annotate`) being replayed instead. An
    /// insert-at-offset command would make the journal a text editor's, which is
    /// a different thing to get right and buys nothing a reader can see.
    Renote { mark: MarkId, note: NoteId },
    /// Replace what a drawing is made of.
    ///
    /// [`Command::Renote`]'s twin, and the argument there carries over word for
    /// word: a whole stroke list rather than an edit to one, because the reader
    /// sweeps an eraser and lets go, so the command is *what the drawing now
    /// is*. Undo is the previous `Reink` --- or the [`Command::Annotate`] ---
    /// being replayed instead.
    ///
    /// **Erasing every stroke is not this command.** A mark that draws nothing
    /// must not exist, so [`Doc::reink`] refuses an empty list and the caller
    /// issues [`Command::Unannotate`]. Which of the two a gesture produces is a
    /// decision about the product, and it is made one layer up in `edits.rs`
    /// rather than here.
    Reink { mark: MarkId, ink: InkId },
    /// Replace what a mark is drawn in.
    ///
    /// The third of [`Command::Renote`]'s family, and it takes that entry's
    /// argument once more: a whole colour rather than a channel, because the
    /// reader picks a swatch and undo is the previous pick being replayed.
    ///
    /// **Every kind takes one**, unlike [`Command::Reink`], which is ink's
    /// alone. A colour is the one property all six share --- `/C` is written for
    /// each of them --- so this is the only member of the family whose subject
    /// is not narrowed by a kind check.
    Recolor { mark: MarkId, color: ColorId },
    /// Replace what a comment **out of the file** says.
    ///
    /// [`Command::Renote`]'s counterpart for an annotation the model did not
    /// make, and it takes that entry's argument once more: a whole body rather
    /// than an edit to one, because the reader types in a box and commits it.
    ///
    /// **Three fields where `Renote` has two**, and the extra one is the point.
    /// A mark's page is in the table this document owns; a foreign comment's is
    /// not, because the model has never read the file. So the page comes in with
    /// the command, and it is what makes a rewrite die with its page rather than
    /// outliving it.
    ///
    /// Undoing back past the first `Rewrite` for an object leaves the model with
    /// nothing to say about it, and the file's own text is what a save then
    /// writes. There is no "revert this comment" command and none is needed: the
    /// model never learned what the file said, so putting the original text back
    /// is undo's job and not a command's.
    Rewrite {
        object: ObjectId,
        page: PageId,
        edit: RewriteId,
    },
    /// Take a comment **out of the file** off the page it is on.
    ///
    /// [`Command::Unannotate`]'s counterpart for an annotation the model did not
    /// make, and it takes [`Command::Rewrite`]'s extra field for that entry's
    /// reason: the page comes in with the command, because the model has never
    /// read the file and has no table to look it up in. That is what makes a
    /// discard die with its page rather than outliving it.
    ///
    /// **It issues no body, which is what separates it from every command
    /// above.** A rewrite says what the comment now reads; this says the comment
    /// is not there. There is nothing to put in a table, so there is no
    /// `DiscardId` --- and [`Doc::trim`] has nothing to remove for this variant,
    /// which is why it is one of the arms listed there rather than left to a
    /// catch-all.
    ///
    /// **A discarded comment may also have been rewritten, and the discard
    /// wins.** Both are recorded, because undo replays the journal and a
    /// `Rewrite` that vanished when a later `Discard` arrived could not come
    /// back; what the writer does with an object in both sets is `save.rs`'s
    /// decision and is written there.
    ///
    /// ⚠ **This cannot be an append**, and that is the whole of what makes it
    /// bigger than it looks. An incremental save only *adds* objects, so a
    /// deletion has nothing it can write --- see [`crate::edits::Plan`]'s
    /// `is_appendable`, which gained a clause for this and would otherwise
    /// classify *highlight one line and delete one comment* as an append,
    /// write the highlight, drop the deletion and report success.
    Discard { object: ObjectId, page: PageId },
    /// Mark a region of a page for removal.
    ///
    /// [`Command::Annotate`]'s counterpart, and journalled the same way: the
    /// body is in [`Doc`]'s table under `redaction` and only the identity is
    /// here, which is what keeps this enum `Copy`.
    ///
    /// **It carries no note and no colour**, which is where the resemblance
    /// stops. Those two are what a mark says and what it is drawn in, and a
    /// redaction is neither drawn into the file nor read by anyone --- see
    /// [`Redaction`].
    Redact {
        redaction: RedactionId,
        page: PageId,
    },
    /// Take a pending redaction back off the page.
    ///
    /// **The reader's only way out**, and the reason marking is a journal
    /// command rather than a list somebody edits: a redaction that could not be
    /// undone would make the review step (`docs/PLAN.md` §6) a formality. Apply
    /// is the destructive step; this is not it.
    Unredact { redaction: RedactionId },
}

impl Command {
    /// The page the command acts on, for diagnostics.
    ///
    /// `None` for the two variants that name a mark and not a page --- and
    /// deliberately does not go looking one up. The page a mark sits on is in
    /// the table, and a diagnostic that has to consult another structure to
    /// answer is one that can be asked after the answer has gone.
    pub fn subject(self) -> Option<PageId> {
        match self {
            Command::Rotate { page, .. }
            | Command::Crop { page, .. }
            | Command::Delete { page }
            | Command::Move { page, .. }
            | Command::Insert { page, .. }
            | Command::Annotate { page, .. }
            | Command::Rewrite { page, .. }
            | Command::Discard { page, .. }
            | Command::Redact { page, .. } => Some(page),
            Command::Unannotate { .. }
            | Command::Renote { .. }
            | Command::Reink { .. }
            | Command::Recolor { .. }
            | Command::Unredact { .. } => None,
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
    /// A page of no area, `NaN` included.
    ///
    /// [`DegenerateCrop`](Refusal::DegenerateCrop)'s twin, and its note about
    /// `NaN` applies here too: **this variant does not compare equal to itself**
    /// when the size holds one, so match the variant rather than the value.
    ///
    /// Its own variant rather than a second meaning for the crop's, because the
    /// two say different things to a reader --- one is a box they dragged over a
    /// page that exists, the other is a page they asked tpdf to make. That is
    /// the trap about one predicate answering two questions, refused before it
    /// could be made.
    DegeneratePage(Size),
    /// A crop box on a page tpdf made.
    ///
    /// The box cannot be measured: a reader drags a rectangle in display space
    /// and `crop.ts` asks PDFium to turn it into a box in the page's own space,
    /// naming a page of the file. There is no page of the file to name, so
    /// accepting a crop here would mean holding one nothing could have produced.
    /// **Clearing a crop is not refused**, and nothing is missing: a made page
    /// has none to clear, so the refusal would be about an operation with no
    /// effect.
    ///
    /// ⚠ **This and [`RedactionOnMadePage`](Refusal::RedactionOnMadePage) were
    /// one variant, `MadePage`, whose doc argued at length that it should be
    /// "named for the fact rather than for the reason" because its callers
    /// refuse for different reasons.** The argument was right about naming and
    /// wrong about the consequence: one variant becomes one string in
    /// [`crate::edits::describe`], and the string it had was *"tpdf cannot mark
    /// a page it made yet"* --- which was a sentence about marking, shown to a
    /// reader who had dragged a redaction. A refusal a reader sees is not one
    /// fact, it is one fact plus what they were trying to do, and only the
    /// second of those tells them what to do next.
    CropOnMadePage(PageId),
    /// A region marked for removal on a page tpdf made.
    ///
    /// A blank page has no content, so a region on one removes nothing. That is
    /// [`EmptyRedaction`](Refusal::EmptyRedaction)'s argument --- a row in the
    /// review list certifying the removal of nothing --- reaching the same
    /// conclusion by a different route.
    RedactionOnMadePage(PageId),
    /// No mark has ever had this id.
    NoSuchMark(MarkId),
    /// The id names a mark that was taken off the page. Distinct from
    /// [`NoSuchMark`](Refusal::NoSuchMark) for the reason
    /// [`PageDeleted`](Refusal::PageDeleted) is distinct from
    /// [`NoSuchPage`](Refusal::NoSuchPage): a reader who removed a highlight and
    /// a frontend addressing one that never existed need different answers.
    MarkRemoved(MarkId),
    /// A mark covering nothing: no quads at all, or every quad degenerate.
    ///
    /// Refused here rather than at the write path, because a mark that covers
    /// nothing is invisible in the viewer *and* in the file --- so accepting one
    /// puts the reader in front of a document that claims an unsaved change and
    /// shows no sign of it.
    EmptyMark,
    /// Strokes on a mark that is not ink, or ink with nothing drawn.
    ///
    /// **One variant for both halves, because they are one rule**: the strokes
    /// field is non-empty exactly when the kind is [`MarkKind::Ink`]. Splitting
    /// it in two would suggest a caller could sensibly hit one and not the
    /// other, and neither is reachable from the frontend --- both mean the wire
    /// and the model disagree about what a mark is, which is a defect on the
    /// sending side rather than something a reader did.
    ///
    /// It carries the kind so a diagnostic can say which way round it went.
    ShapeMismatch(MarkKind),
    /// A stamp name on a mark that is not a stamp, or a stamp without one.
    ///
    /// **Its own variant rather than a second meaning for
    /// [`ShapeMismatch`](Refusal::ShapeMismatch)**, which is documented as one
    /// variant for one rule about one field. Two fields with two biconditionals
    /// are two rules, and a caller told only "shape mismatch" would have to
    /// guess which. That is the trap about one predicate answering two
    /// questions, refused here before it could be made.
    StampMismatch(MarkKind),
    /// A parent comment named on a mark that is not a comment.
    ///
    /// Its own variant for [`StampMismatch`](Refusal::StampMismatch)'s reason,
    /// and the third rule about a third field. **Only one half is a refusal**,
    /// unlike the two above: a [`MarkKind::Note`] with no parent is an ordinary
    /// sticky note, which is what most of them are, so there is nothing to
    /// refuse in that direction. It carries the kind the caller sent, which is
    /// the only thing a diagnostic can usefully name.
    ReplyMismatch(MarkKind),
    /// A comment out of the file that one of the reader's own replies answers.
    ///
    /// **The one refusal deleting a comment needs that no other command in its
    /// family does**, and it exists because replying to a foreign comment landed
    /// first. A reply is a [`MarkKind::Note`] carrying `/IRT` --- the object
    /// number of the comment it answers --- and that object number is the only
    /// name the file and this model agree on. Take the comment away and the
    /// reply's `/IRT` points at nothing, which is a malformed thread in every
    /// reader that draws one.
    ///
    /// **Refused rather than taking the reply with it**, which was the other
    /// option and is worse: a reader who deletes somebody else's comment has not
    /// asked to lose what they themselves wrote, and a silent second deletion is
    /// the shape nobody notices until the file is reopened. So the reader is
    /// told, and removing their reply first is one press.
    ///
    /// **And refused here rather than at the save**, which is where the same
    /// invariant is checked for a reply naming an object the file does not have.
    /// That check is about the *file* and can only be made when one is being
    /// written; this one is about two things the model holds, so it can be made
    /// at the moment the reader asks and answered with something they can act
    /// on.
    ///
    /// It carries the object so a diagnostic can name it.
    ReplyAnswersIt(ObjectId),
    /// No redaction has ever had this id.
    NoSuchRedaction(RedactionId),
    /// The id names a redaction that was taken back off the page --- including
    /// one that went with a deleted page. Distinct from
    /// [`NoSuchRedaction`](Refusal::NoSuchRedaction) for the reason
    /// [`MarkRemoved`](Refusal::MarkRemoved) is distinct from
    /// [`NoSuchMark`](Refusal::NoSuchMark).
    RedactionRemoved(RedactionId),
    /// A redaction covering nothing: a degenerate rectangle, `NaN` included.
    ///
    /// Refused here rather than at the apply path, for [`EmptyMark`](Refusal::EmptyMark)'s
    /// reason and one that is sharper: a region covering no area removes no
    /// content, so accepting one puts a row in the review list that certifies
    /// the removal of nothing.
    EmptyRedaction,
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
    /// The marks on each page, in the order they were made.
    ///
    /// A page with no marks has no entry rather than an empty vector, so that
    /// two working documents that have never been annotated compare equal --- and
    /// they must, because `PartialEq` here is what a snapshot is checked against.
    marks: HashMap<PageId, Vec<MarkId>>,
    /// Marks that were on a page and are not, for the same reason `graves` holds
    /// pages: a second [`Command::Unannotate`] naming one has to be told which
    /// of the two answers it is getting.
    ///
    /// **A mark that went with its deleted page is in here too**, and that was
    /// not the first design. The first left it out on the reasoning that the
    /// page is the better diagnosis --- and the result was that removing a
    /// highlight on a deleted page answered [`Refusal::NoSuchMark`], which says
    /// the mark never existed. A wrong diagnosis is worse than a coarse one, and
    /// a caller that wants to know about the page is asking about a page, where
    /// [`Refusal::PageDeleted`] is waiting for it.
    mark_graves: HashSet<MarkId>,
    /// What each live mark says, as the id of the text rather than the text.
    ///
    /// Here rather than on the mark's body because it is the one thing about a
    /// mark that changes, and everything that changes has to be rebuildable by
    /// replay --- which rebuilds this struct and nothing else.
    ///
    /// **Its keys are exactly the live marks**, empty note or not: `Annotate`
    /// puts one in, `Unannotate` and a page's deletion take it out. So an absent
    /// entry means the mark is not on a page, never that it has nothing to say
    /// --- which is the opposite of the "absent rather than empty" rule `marks`
    /// above states, and worth saying because the two maps sit next to each
    /// other.
    notes: HashMap<MarkId, NoteId>,
    /// Which version of a drawing's strokes is current.
    ///
    /// **Unlike `notes` above, an absent entry is the common case and means
    /// something**: the strokes the mark was made with, still in the body table.
    /// Only a mark an eraser has touched has an entry here. That asymmetry is
    /// deliberate --- [`Command::Annotate`] would otherwise have to carry an
    /// [`InkId`] for five kinds that have no strokes at all.
    inks: HashMap<MarkId, InkId>,
    /// Which version of a mark's colour is current.
    ///
    /// `inks` above, for the property every kind has: an absent entry is the
    /// common case and means the colour the mark was made with, still in the
    /// body table. Only a mark somebody has recoloured has an entry here.
    colors: HashMap<MarkId, ColorId>,
    /// The pending redactions on each page, in the order they were made.
    ///
    /// `marks` above in every structural respect --- absent rather than empty,
    /// for the reason stated there --- and deliberately a second map rather than
    /// a second kind in the first one. What separates them is not how they are
    /// stored but where they go: `marks` is what `save.rs` writes, and nothing
    /// in this map can reach it.
    redactions: HashMap<PageId, Vec<RedactionId>>,
    /// Redactions that were on a page and are not, for `mark_graves`' reason and
    /// with its subtlety: one that went with a deleted page is in here too, so
    /// naming it answers [`Refusal::RedactionRemoved`] rather than claiming it
    /// never existed.
    redaction_graves: HashSet<RedactionId>,
    /// Which version of each rewritten foreign comment is current, and which
    /// page it is on.
    ///
    /// Keyed by the object rather than by the page, unlike `marks`, because that
    /// is the identity the reader edits by and the one the writer needs --- and
    /// a comment has exactly one current body, where a page has a list of marks.
    /// The page rides along so that a deletion can take its rewrites with it in
    /// one pass.
    ///
    /// **There is no `rewrite_graves`, and nothing is missing.** A tombstone
    /// exists so that naming a removed thing answers truthfully, and the only
    /// way a rewrite goes is with its page --- where naming it again is refused
    /// by [`Refusal::PageDeleted`], which is the better diagnosis anyway.
    rewrites: BTreeMap<ObjectId, Rewritten>,
    /// The comments **out of the file** a reader has deleted, and the page each
    /// was on.
    ///
    /// [`Working::rewrites`]' twin and deliberately a second map rather than a
    /// state inside `Rewritten`: a comment can be rewritten and then discarded,
    /// so the two are not alternatives, and folding them would make the commoner
    /// operation carry a field about the rarer one.
    ///
    /// The page rides along for `rewrites`' reason exactly --- deleting a page
    /// takes its discards with it, so a comment on a page that has gone does not
    /// outlive it and come back if the page is restored by undo carrying a
    /// deletion nobody re-issued.
    ///
    /// A `BTreeMap` rather than a `HashMap` for `rewrites`' reason: the writer
    /// walks it, and an order that varied per process would make an ordering
    /// assertion fail about one run in three.
    discards: BTreeMap<ObjectId, PageId>,
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
                        source: PageSource::Baseline(i as u32),
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
            marks: HashMap::new(),
            mark_graves: HashSet::new(),
            notes: HashMap::new(),
            inks: HashMap::new(),
            colors: HashMap::new(),
            redactions: HashMap::new(),
            redaction_graves: HashSet::new(),
            rewrites: BTreeMap::new(),
            discards: BTreeMap::new(),
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

    /// The marks on a page, in the order they were made.
    ///
    /// Empty for a page nobody has annotated and for a page that does not exist,
    /// which is deliberate: every caller of this is drawing, and a page that is
    /// not there has nothing to draw either way. A caller that needs the two
    /// distinguished is asking about a *page*, and [`page`](Self::page) answers
    /// that.
    pub fn marks_on(&self, page: PageId) -> &[MarkId] {
        self.marks.get(&page).map_or(&[], |list| list.as_slice())
    }

    /// Which page a live mark is on, if any.
    ///
    /// A linear walk of the pages that have marks. That is the right shape at
    /// this size --- a reader makes tens of highlights, not thousands --- and the
    /// alternative is a second index that has to be kept in step with the first
    /// by every mutation, which is the class of defect this model is arranged to
    /// avoid.
    pub fn page_of(&self, mark: MarkId) -> Option<PageId> {
        self.marks
            .iter()
            .find(|(_, list)| list.contains(&mark))
            .map(|(page, _)| *page)
    }

    /// Every live mark, with the page it sits on, in page order.
    ///
    /// Page order rather than creation order, because both consumers --- the
    /// overlay and the writer --- work a page at a time.
    pub fn all_marks(&self) -> Vec<(PageId, MarkId)> {
        self.order
            .iter()
            .flat_map(|page| self.marks_on(*page).iter().map(|mark| (*page, *mark)))
            .collect()
    }

    /// The pending redactions on one page, in the order they were made.
    pub fn redactions_on(&self, page: PageId) -> &[RedactionId] {
        self.redactions
            .get(&page)
            .map_or(&[], |list| list.as_slice())
    }

    /// Which page a pending redaction is on, if any.
    ///
    /// [`page_of`](Self::page_of)'s twin, and a linear walk for its reason.
    pub fn page_of_redaction(&self, redaction: RedactionId) -> Option<PageId> {
        self.redactions
            .iter()
            .find(|(_, list)| list.contains(&redaction))
            .map(|(page, _)| *page)
    }

    /// Every pending redaction, with the page it sits on, in page order.
    ///
    /// Page order for [`all_marks`](Self::all_marks)'s reason, and here it is
    /// also the order the review list reads in --- which is the order a reader
    /// checks the document in, rather than the order they happened to drag.
    pub fn all_redactions(&self) -> Vec<(PageId, RedactionId)> {
        self.order
            .iter()
            .flat_map(|page| self.redactions_on(*page).iter().map(|r| (*page, *r)))
            .collect()
    }

    /// Refuses unless the id names a redaction on a page, naming which of the
    /// two it is, and answers with the page it found.
    ///
    /// [`live_mark`](Self::live_mark)'s twin, one implementation for its reason.
    fn live_redaction(&self, redaction: RedactionId) -> Result<PageId, Refusal> {
        self.page_of_redaction(redaction)
            .ok_or(if self.redaction_graves.contains(&redaction) {
                Refusal::RedactionRemoved(redaction)
            } else {
                Refusal::NoSuchRedaction(redaction)
            })
    }

    /// Tombstones a mark and drops every table keyed by its id.
    ///
    /// One implementation for the two arms that end a mark --- deleting the
    /// page it is on, and removing the mark itself --- because they are the
    /// same cleanup and were not the same code: `Unannotate` dropped the note,
    /// the ink and the colour, and `Delete` dropped only the note, leaving two
    /// entries keyed by an id nothing can reach. Nothing read them, so no
    /// behaviour differed and nothing could go red; what drifts in that state
    /// is the next table somebody adds, which one arm will clear and the other
    /// will not.
    ///
    /// Safe on either path because undo **replays** rather than inverts: a
    /// table cleared here is rebuilt by the journal, not recovered from what
    /// was left behind.
    fn forget_mark(&mut self, mark: MarkId) {
        self.mark_graves.insert(mark);
        self.notes.remove(&mark);
        self.inks.remove(&mark);
        self.colors.remove(&mark);
    }

    /// Refuses unless the id names a mark on a page, naming which of the two it
    /// is, and answers with the page it found.
    ///
    /// One implementation for three callers --- [`Command::Unannotate`],
    /// [`Command::Renote`] and [`Doc::renote`]'s pre-check --- because two
    /// copies of "which refusal is this" is precisely the pair that drifts:
    /// they would agree on every ordinary id and disagree about the one case
    /// the distinction exists for.
    fn live_mark(&self, mark: MarkId) -> Result<PageId, Refusal> {
        self.page_of(mark)
            .ok_or(if self.mark_graves.contains(&mark) {
                Refusal::MarkRemoved(mark)
            } else {
                Refusal::NoSuchMark(mark)
            })
    }

    /// What a live mark says, as an id into [`Doc`]'s table.
    pub fn ink_of(&self, mark: MarkId) -> Option<InkId> {
        self.inks.get(&mark).copied()
    }

    /// Which version of a mark's colour is current.
    pub fn color_of(&self, mark: MarkId) -> Option<ColorId> {
        self.colors.get(&mark).copied()
    }

    /// Which version of a mark's note is current.
    pub fn note_of(&self, mark: MarkId) -> Option<NoteId> {
        self.notes.get(&mark).copied()
    }

    /// Every rewritten foreign comment, with the page it is on, in page order,
    /// and by object number within a page.
    ///
    /// **Both orderings are by construction rather than by a sort**, which is
    /// the whole reason `rewrites` is a `BTreeMap`: the outer walk is the
    /// reading order, exactly as [`all_marks`](Self::all_marks) walks it, and the
    /// inner one is the map's own. A `HashMap` here would iterate differently
    /// between runs, so the same document would put its note edits into a plan
    /// in a different order each time and two saves of it would produce
    /// different bytes with nothing wrong --- and a test for the ordering would
    /// be a test that fails one run in six rather than every time.
    ///
    /// Quadratic in pages times rewrites, which is what [`all_marks`] avoids by
    /// holding a list per page. It is not held that way here because the object
    /// is the identity a reader edits by, and both factors are small: a document
    /// has hundreds of pages and a reader edits a handful of comments.
    pub fn all_rewrites(&self) -> Vec<(PageId, ObjectId, RewriteId)> {
        self.order
            .iter()
            .flat_map(|page| {
                self.rewrites
                    .iter()
                    .filter(move |(_, held)| held.page == *page)
                    .map(move |(object, held)| (*page, *object, held.edit))
            })
            .collect()
    }

    /// Which version of a foreign comment's body is current, if any.
    pub fn rewrite_of(&self, object: ObjectId) -> Option<RewriteId> {
        self.rewrites.get(&object).map(|held| held.edit)
    }

    /// Every foreign comment a reader has deleted, with the page it was on, in
    /// page order and by object number within a page.
    ///
    /// [`all_rewrites`](Self::all_rewrites)'s twin, and both orderings are by
    /// construction for that entry's reason --- an order that varied per run
    /// would put deletions into a plan differently each time, so two saves of one
    /// document would produce different bytes with nothing wrong.
    pub fn all_discards(&self) -> Vec<(PageId, ObjectId)> {
        self.order
            .iter()
            .flat_map(|page| {
                self.discards
                    .iter()
                    .filter(move |(_, on)| *on == page)
                    .map(move |(object, _)| (*page, *object))
            })
            .collect()
    }

    /// Whether a reader has deleted this foreign comment.
    pub fn is_discarded(&self, object: ObjectId) -> bool {
        self.discards.contains_key(&object)
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
                // **Forced rather than chosen**, unlike the refusal that used
                // to sit beside it in `annotate`. A crop box is in the page's own
                // unrotated space and the reader drags a rectangle in display
                // space; `crop.ts` asks PDFium to convert between them, naming a
                // page of the *file*. A page tpdf made is in no file, so the
                // frontend cannot produce the box in the first place --- and a
                // model that accepted one anyway would hold a crop nothing could
                // have measured.
                if let Some(page_now) = self.pages.get(&page) {
                    if matches!(page_now.source, PageSource::Blank(_)) && to.is_some() {
                        return Err(Refusal::CropOnMadePage(page));
                    }
                }
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
                // The marks go with the page, and their ids are tombstoned so
                // that naming one afterwards is answered truthfully. Undo brings
                // page and marks back together, because it replays rather than
                // inverts.
                for mark in self.marks.remove(&page).unwrap_or_default() {
                    self.forget_mark(mark);
                }
                // The pending redactions go with it for the same reason and
                // with the same consequence: naming one afterwards answers
                // `RedactionRemoved` rather than claiming it never existed.
                // Leaving them behind would be worse here than for a mark ---
                // an apply would carry an instruction about a page nobody can
                // see, and the review list would show a row with no page.
                for redaction in self.redactions.remove(&page).unwrap_or_default() {
                    self.redaction_graves.insert(redaction);
                }
                // And the rewrites, for the marks' reason with one difference:
                // there is no tombstone, because naming the object again after
                // the page has gone is answered by `PageDeleted` from the page
                // check above. A walk rather than a lookup, since this map is
                // keyed by the object --- see the field.
                self.rewrites.retain(|_, held| held.page != page);
                // The same, for the same reason: a discard names a page it can
                // outlive, and one that did would tell the writer to delete an
                // object off a page that is not in the plan.
                self.discards.retain(|_, on| *on != page);
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
            Command::Insert { page, after, size } => {
                if !size.is_proper() {
                    return Err(Refusal::DegeneratePage(size));
                }
                if let Some(anchor) = after {
                    self.live(anchor)?;
                }
                // `Command::Annotate`'s assertion, for its reason: the id is
                // issued once and a journalled `Insert` is replayed exactly
                // where it was accepted, so an id arriving here twice is a
                // broken model rather than something a reader did. Both halves,
                // because a page can be absent from `pages` two ways and only
                // one of them is a fresh id.
                debug_assert!(
                    !self.pages.contains_key(&page) && !self.graves.contains(&page),
                    "a page was inserted twice: {page:?}"
                );
                // After the anchor's check rather than before it, so a refused
                // insert leaves the order alone --- the property the doc comment
                // on this function states and every other arm keeps.
                let at = match after {
                    None => 0,
                    Some(anchor) => self.position(anchor) + 1,
                };
                self.order.insert(at, page);
                self.pages.insert(
                    page,
                    Page {
                        source: PageSource::Blank(size),
                        extra_turns: 0,
                        crop: None,
                    },
                );
            }
            Command::Annotate { mark, page, note } => {
                self.live(page)?;
                // Not a check on `mark_graves`: an id is issued once and a
                // journalled `Annotate` is replayed exactly where it was
                // accepted, so an id arriving here twice is a broken model
                // rather than a user error. It is asserted rather than assumed.
                debug_assert!(
                    self.page_of(mark).is_none(),
                    "a mark was annotated onto two pages: {mark:?}"
                );
                self.marks.entry(page).or_default().push(mark);
                self.mark_graves.remove(&mark);
                self.notes.insert(mark, note);
            }
            Command::Unannotate { mark } => {
                let page = self.live_mark(mark)?;
                let list = self.marks.get_mut(&page).expect("page_of found it here");
                list.retain(|&held| held != mark);
                // An empty entry is removed rather than left, so the map holds
                // exactly the pages that have marks --- which is what
                // `page_of`'s walk and anything else iterating it reads.
                //
                // **This used to claim the document then compares equal to one
                // that was never annotated, and that is false**, measured
                // 2026-08-26 while writing the same rule for redactions: the
                // tombstone in `mark_graves` stays, so the two `Working`s
                // differ whatever this line does. The sentence went on to say a
                // snapshot comparison rests on it, and nothing does --- a
                // snapshot is cloned and replayed through this same `apply`,
                // never compared against a differently-built document. The rule
                // is worth keeping and the reason given for it was checkable
                // and wrong.
                if list.is_empty() {
                    self.marks.remove(&page);
                }
                self.forget_mark(mark);
            }
            Command::Renote { mark, note } => {
                self.live_mark(mark)?;
                self.notes.insert(mark, note);
            }
            Command::Reink { mark, ink } => {
                self.live_mark(mark)?;
                self.inks.insert(mark, ink);
            }
            Command::Recolor { mark, color } => {
                self.live_mark(mark)?;
                self.colors.insert(mark, color);
            }
            Command::Rewrite { object, page, edit } => {
                // The page, and nothing about the object: this layer cannot ask
                // whether the file holds it. See [`ObjectId`].
                self.live(page)?;
                self.rewrites.insert(object, Rewritten { page, edit });
            }
            Command::Discard { object, page } => {
                // The page, and nothing about the object, for the reason the
                // arm above gives: this layer has never read the file and
                // cannot ask whether it holds the object. The writer can, and
                // refuses one it cannot find.
                self.live(page)?;
                // **The rewrite is left in place.** A comment can be edited and
                // then deleted, and taking the edit out here would mean an undo
                // of the deletion brought the comment back reading whatever the
                // *file* said rather than what the reader had typed. Both are
                // recorded; the writer decides what an object in both sets gets,
                // and `save.rs` is where that is written down.
                self.discards.insert(object, page);
            }
            Command::Redact { redaction, page } => {
                self.live(page)?;
                // `Command::Annotate`'s assertion, for its reason: an id is
                // issued once and a journalled `Redact` is replayed exactly
                // where it was accepted, so an id arriving here twice is a
                // broken model rather than something a reader did.
                debug_assert!(
                    self.page_of_redaction(redaction).is_none(),
                    "a redaction was put on two pages: {redaction:?}"
                );
                self.redactions.entry(page).or_default().push(redaction);
                self.redaction_graves.remove(&redaction);
            }
            Command::Unredact { redaction } => {
                let page = self.live_redaction(redaction)?;
                let list = self
                    .redactions
                    .get_mut(&page)
                    .expect("page_of_redaction found it here");
                list.retain(|&held| held != redaction);
                // Emptied rather than left empty, for `Unannotate`'s reason:
                // the map holds exactly the pages that have pending redactions.
                // Read that arm's note before writing a test that asserts more
                // than that --- the equality it used to claim is false.
                if list.is_empty() {
                    self.redactions.remove(&page);
                }
                self.redaction_graves.insert(redaction);
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

/// Which gesture a journalled command belonged to.
///
/// **Minted by the frontend, one per gesture**, and the backend only ever asks
/// whether two *adjacent* entries carry the same one. Nothing is looked up by
/// it and no table is keyed by it, so a webview that sent nonsense could group
/// commands it issued back to back --- which it could equally have achieved by
/// not issuing them. That is the whole of the trust this places in the number,
/// and it is why the id is not minted here: the backend cannot see where a
/// gesture ends.
///
/// Non-zero because zero is the wire's way of saying *this command belongs to
/// no gesture*, which is what every command but the eraser's sends.
pub type SweepId = NonZeroU64;

/// One journalled command, and the gesture it belonged to.
///
/// **A struct rather than two vectors in lockstep.** The command and its sweep
/// are one fact about one moment; kept apart they would be two lengths that
/// must agree, which `docs/TRAPS.md` has more than one entry about. `Command`
/// is `Copy`, so this is too, and replay stays a `for` loop over values.
#[derive(Clone, Copy, Debug)]
struct Entry {
    cmd: Command,
    /// `None` for every command that is not part of a gesture, which is all of
    /// them but the eraser's. Two entries group only when both are `Some` and
    /// equal, so a `None` never joins anything --- including another `None`.
    sweep: Option<SweepId>,
}

/// A document being edited: baseline, working view, journal and cursor.
#[derive(Clone, Debug)]
pub struct Doc {
    baseline: u32,
    now: Working,
    journal: Vec<Entry>,
    /// How many journal entries are applied. Entries past it are the redo tail.
    cursor: usize,
    /// Working documents at selected cursor positions, so an undo replays a
    /// bounded number of commands. Keyed by cursor, never by journal index ---
    /// they are the same number and mean different things, and the key is "how
    /// many commands had been applied".
    snapshots: HashMap<usize, Working>,
    /// What each mark is, keyed by the id its command carries.
    ///
    /// Outside [`Working`] on purpose. A mark's body never changes --- editing
    /// one would be a command of its own --- so a snapshot has no reason to clone
    /// it, and keeping it here is what lets [`Command`] stay `Copy`.
    marks: HashMap<MarkId, Mark>,
    /// The next id to issue.
    ///
    /// **Only ever counts up.** Undo moves the cursor and never this, which is
    /// the whole of the property the module note states: an id spent by a
    /// command that has since been undone is still spent, so redo restores the
    /// mark it named rather than a different one wearing its number.
    next_mark: u64,
    /// What each note says, keyed by the id its command carries.
    ///
    /// A second table for the same reason as `marks`, with one difference worth
    /// stating: a mark has one body and many notes over its life, so this is
    /// keyed by the *version* rather than by the mark. Which version is current
    /// is [`Working`]'s answer, and that is the whole of what makes a note
    /// undoable.
    notes: HashMap<NoteId, String>,
    /// The next note id to issue. Only ever counts up, as [`next_mark`](Doc::next_mark) does.
    next_note: u32,
    /// What each version of each drawing is made of, keyed by the id its command
    /// carries. `notes`' twin, and keyed by the version for the same reason.
    inks: HashMap<InkId, Ink>,
    /// The next ink id to issue. Only ever counts up.
    next_ink: u32,
    /// What each version of each mark's colour is, keyed by the id its command
    /// carries. `notes`' third twin, and keyed by the version for its reason.
    colors: HashMap<ColorId, [f32; 3]>,
    /// The next colour id to issue. Only ever counts up.
    next_color: u32,
    /// What each pending redaction is, keyed by the id its command carries.
    ///
    /// `marks`' twin and outside [`Working`] for its reason: a redaction's body
    /// never changes --- moving or resizing one would be a command of its own,
    /// and there is not one yet --- so a snapshot has no reason to clone it.
    redactions: HashMap<RedactionId, Redaction>,
    /// The next redaction id to issue. Only ever counts up, as
    /// [`next_mark`](Doc::next_mark) does and for its reason.
    next_redaction: u64,
    /// The next page id to issue, for a page tpdf makes.
    ///
    /// **Starts past the baseline**, because [`Working::baseline`] hands the
    /// file's own pages `1..=pages` before any command runs, and an id issued
    /// twice would put two pages under one name. Only ever counts up, as
    /// [`next_mark`](Doc::next_mark) does and for its reason: an id spent by an
    /// insert that has since been undone is still spent, so redo brings back the
    /// page it named rather than a different one.
    next_page: u64,
    /// What each version of each foreign comment's body is, keyed by the id its
    /// command carries. `notes`' fourth twin, keyed by the version for its
    /// reason --- and holding a date beside the text, which is what makes it a
    /// table of its own rather than a reuse of `notes`. See [`RewriteId`].
    rewrites: HashMap<RewriteId, NoteEdit>,
    /// The next rewrite id to issue. Only ever counts up.
    next_rewrite: u32,
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
            marks: HashMap::new(),
            next_mark: 1,
            notes: HashMap::new(),
            next_note: 1,
            inks: HashMap::new(),
            next_ink: 1,
            colors: HashMap::new(),
            next_color: 1,
            redactions: HashMap::new(),
            next_redaction: 1,
            next_page: u64::from(pages) + 1,
            rewrites: HashMap::new(),
            next_rewrite: 1,
        }
    }

    /// What renders.
    pub fn working(&self) -> &Working {
        &self.now
    }

    /// How many pages the file this was opened from had.
    ///
    /// Not [`Working::len`], and the difference is the whole of what a save has
    /// to check: the working document is what the reader kept, and this is what
    /// the file had. A plan of three pages against a baseline of five is a
    /// deletion; a *baseline* that disagrees with the file on disk is the file
    /// having changed underneath the reader.
    pub fn baseline(&self) -> u32 {
        self.baseline
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

    /// What a mark is, for a mark this document has issued.
    ///
    /// Answers for an id whose command has been undone as well as for a live
    /// one, and that is not an oversight: the body outlives the command, which
    /// is what lets redo restore it. A caller asking *which marks are on the
    /// page* asks [`Working::marks_on`], and this only ever fills in the detail.
    pub fn mark(&self, id: MarkId) -> Option<&Mark> {
        self.marks.get(&id)
    }

    /// How many mark bodies are held.
    ///
    /// An accounting observable, here for the reason [`snapshots`](Doc::snapshots)
    /// is: a body kept after the command that used it was discarded, and a body
    /// correctly dropped, produce identical documents. Without this, the pruning
    /// below could stop happening and no assertion over
    /// [`working`](Doc::working) would notice.
    pub fn mark_bodies(&self) -> usize {
        self.marks.len()
    }

    /// What a pending redaction is, for one this document has issued.
    ///
    /// Answers for an undone id as well as a live one, for [`mark`](Doc::mark)'s
    /// reason: the body outlives the command, which is what lets redo restore it.
    pub fn redaction(&self, id: RedactionId) -> Option<&Redaction> {
        self.redactions.get(&id)
    }

    /// How many redaction bodies are held.
    ///
    /// [`mark_bodies`](Doc::mark_bodies)' twin, and an accounting observable for
    /// its reason: a body kept after its command was discarded produces a
    /// document identical to one where it was dropped, so nothing about the
    /// working state can see the leak.
    pub fn redaction_bodies(&self) -> usize {
        self.redactions.len()
    }

    /// Puts a new blank page after `after`, issuing its id.
    ///
    /// The third entry point that allocates, and it cannot be a caller-built
    /// [`Command::Insert`] for [`annotate`](Doc::annotate)'s reason: an id a
    /// caller chose is an id two pages can share.
    ///
    /// **`None` puts it at the front**, which is [`Command::Move`]'s convention
    /// and is why the anchor is an id rather than a position --- a reader who
    /// asks for a page after page 3 means the page they are looking at, and an
    /// index would name whatever moved into that slot while the request was in
    /// flight.
    ///
    /// # Errors
    ///
    /// [`Refusal::DegeneratePage`] for a size enclosing no area, and whatever
    /// [`Command::Insert`] refuses --- the anchor not existing, or having been
    /// deleted. **The id is issued after those checks**, so a refused insert
    /// spends nothing. Both are checked twice, here and in the apply: this
    /// method must not spend an id on a command that will be refused, and the
    /// apply must refuse the same on replay.
    pub fn insert(&mut self, after: Option<PageId>, size: Size) -> Result<PageId, Refusal> {
        if !size.is_proper() {
            return Err(Refusal::DegeneratePage(size));
        }
        if let Some(anchor) = after {
            self.now.live(anchor)?;
        }

        let id = PageId(self.next_page);
        self.next_page += 1;
        // A refusal cannot reach here: the size was checked a few lines ago and
        // the anchor against the same working document. It is still routed
        // through `apply` rather than mutating `now` directly, so that the
        // journal, the cursor, the snapshot rule and the redo-tail discard are
        // all the ones every other command gets.
        self.apply(Command::Insert {
            page: id,
            after,
            size,
        })?;
        Ok(id)
    }

    /// Marks a region of a page for removal, issuing its id.
    ///
    /// [`annotate`](Doc::annotate)'s counterpart, and the second entry point
    /// that allocates. A caller cannot build [`Command::Redact`] itself for that
    /// method's reason: an id a caller chose is an id two redactions can share.
    ///
    /// **Marking is not removing.** Nothing here touches a byte of the document
    /// --- this puts a row in the review list and an outline on the page, and
    /// `docs/PLAN.md` §6 step 3 is where content is destroyed.
    ///
    /// # Errors
    ///
    /// [`Refusal::EmptyRedaction`] when the region covers no area, and whatever
    /// [`Command::Redact`] refuses --- the page not existing, or having been
    /// deleted. **The id is issued after those checks**, so a refused redaction
    /// spends nothing.
    pub fn redact(&mut self, redaction: Redaction) -> Result<RedactionId, Refusal> {
        if !redaction.area.covers_area() {
            return Err(Refusal::EmptyRedaction);
        }
        self.now.live(redaction.page)?;
        // **For a reason that does not expire**, unlike the same refusal in
        // `annotate`: a page tpdf made has no content, so a region on one
        // removes nothing --- which is `Refusal::EmptyRedaction`'s own argument
        // arriving by a different route. After the liveness check, so a deleted
        // page gets the better diagnosis, and before the id is issued.
        if let Some(page) = self.now.page(redaction.page) {
            if let PageSource::Blank(_) = page.source {
                return Err(Refusal::RedactionOnMadePage(redaction.page));
            }
        }

        let id = RedactionId(self.next_redaction);
        let page = redaction.page;
        self.redactions.insert(id, redaction);
        self.next_redaction += 1;
        // A refusal cannot reach here --- the page was checked live a line ago
        // against the same working document and the id is fresh --- and it is
        // routed through `apply` anyway, so that the journal, the cursor, the
        // snapshot rule and the redo-tail discard are the ones every other
        // command gets. `Doc::annotate` says the same and for the same reason.
        self.apply(Command::Redact {
            redaction: id,
            page,
        })?;
        Ok(id)
    }

    /// Takes a pending redaction back off its page.
    ///
    /// A thin wrapper over [`Command::Unredact`], here so that a caller does not
    /// have to know that the command is the whole of it. Undoable like anything
    /// else: this is the reader changing their mind before step 3, not after.
    ///
    /// # Errors
    ///
    /// [`Refusal::NoSuchRedaction`] for an id nobody issued, and
    /// [`Refusal::RedactionRemoved`] for one already taken off --- including one
    /// that went with a deleted page.
    pub fn unredact(&mut self, redaction: RedactionId) -> Result<(), Refusal> {
        self.apply(Command::Unredact { redaction })
    }

    /// The highest id issued so far.
    ///
    /// The second half of that accounting, and the one that says the allocator
    /// was not rewound: after an undo this is unchanged, which is exactly what
    /// makes the next mark a new one.
    pub fn marks_issued(&self) -> u64 {
        self.next_mark - 1
    }

    /// Puts a mark on a page, issuing its id.
    ///
    /// The one entry point that allocates. Everything else replays, which is why
    /// this is a method rather than a [`Command`] a caller can build: an id that
    /// a caller chose is an id two marks can share.
    ///
    /// # Errors
    ///
    /// [`Refusal::EmptyMark`] when the mark covers no area, and whatever
    /// [`Command::Annotate`] refuses --- the page not existing, or having been
    /// deleted. **The id is issued after those checks**, so a refused mark spends
    /// nothing; a document where every attempt failed has issued no ids at all.
    pub fn annotate(&mut self, mark: Mark, note: String) -> Result<MarkId, Refusal> {
        // The biconditional [`Mark::strokes`] states, and the reason it is
        // checked rather than typed is written there. Before the emptiness
        // check below, because a mark whose shape and kind disagree has no
        // meaningful emptiness to report.
        if mark.strokes.is_empty() != (mark.kind != MarkKind::Ink) {
            return Err(Refusal::ShapeMismatch(mark.kind));
        }
        // The same shape for [`Mark::stamp`], and beside the one above rather
        // than folded into it: they are two rules about two fields, and a mark
        // can break either without breaking the other.
        if mark.stamp.is_some() != (mark.kind == MarkKind::Stamp) {
            return Err(Refusal::StampMismatch(mark.kind));
        }
        // The third of the family, and the only one that is not a
        // biconditional --- see [`Mark::reply_to`] on why a comment with no
        // parent needs no refusal while a highlight with one does.
        if mark.reply_to.is_some() && mark.kind != MarkKind::Note {
            return Err(Refusal::ReplyMismatch(mark.kind));
        }
        // **Two questions, asked of the kind that can tell them apart.** For
        // every other kind "covers nothing" is a quad with no area; for ink it
        // is a stroke with no length, and its quad *always* covers area because
        // `Stroke::bounds` pads it. Asking `covers_area` alone would therefore
        // accept a mark of one repeated point --- invisible, unfindable, and an
        // unsaved change the reader cannot see. This is the trap about one
        // predicate answering two questions, caught while adding the second.
        let empty = if mark.kind == MarkKind::Ink {
            !mark.strokes.iter().any(Stroke::is_drawable)
        } else {
            !mark.quads.iter().any(|quad| quad.covers_area())
        };
        if empty {
            return Err(Refusal::EmptyMark);
        }
        self.now.live(mark.page)?;
        // **A page tpdf made takes a mark like any other**, and until
        // 2026-08-30 this is where it did not. The refusal here was never about
        // marks: `PlannedMark` addressed a page object by its *baseline number*,
        // which a page no file supplies does not have, so the honest place to
        // say no was the moment the reader drew rather than the save. The plan
        // addresses a page by its position now, which both kinds have.
        //
        // The two refusals that remain --- `Refusal::CropOnMadePage` and
        // `Refusal::RedactionOnMadePage` --- do not expire, and both say
        // something about the page rather than about tpdf.

        let id = MarkId(self.next_mark);
        let page = mark.page;
        self.marks.insert(id, mark);
        self.next_mark += 1;
        let note = self.issue_note(note);
        // A refusal cannot reach here: the page was checked live a line ago
        // against the same working document, and the id is fresh. It is still
        // routed through `apply` rather than mutating `now` directly, so that
        // the journal, the cursor, the snapshot rule and the redo-tail discard
        // are all the ones every other command gets.
        self.apply(Command::Annotate {
            mark: id,
            page,
            note,
        })?;
        Ok(id)
    }

    /// Replaces what a mark says.
    ///
    /// Takes the whole note, for the reason [`Command::Renote`] gives. Setting
    /// it to what it already says is still a command: it lands in the journal,
    /// makes the document dirty and costs an undo. That is the honest reading of
    /// what the caller asked for, and the alternative --- comparing against the
    /// current text and dropping a no-op --- puts a rule in the model about what
    /// a reader *meant*, which the frontend is better placed to decide and does.
    ///
    /// # Errors
    ///
    /// [`Refusal::NoSuchMark`] for an id nobody issued, and
    /// [`Refusal::MarkRemoved`] for a mark that was taken off the page ---
    /// including one that went with a deleted page. Checked before the id is
    /// issued, so a refused note spends nothing.
    pub fn renote(&mut self, mark: MarkId, note: String) -> Result<(), Refusal> {
        self.now.live_mark(mark)?;
        let note = self.issue_note(note);
        self.apply(Command::Renote { mark, note })
    }

    /// Replaces what a comment out of the file says.
    ///
    /// Takes the whole body for [`Command::Renote`]'s reason, and the page
    /// beside the object for [`Command::Rewrite`]'s: this model has never read
    /// the file, so the page is the only thing about the comment it can check
    /// and the only thing that lets a deletion take the edit with it.
    ///
    /// `made` is the timestamp in PDF date form, passed in rather than taken
    /// from a clock here for [`Doc::annotate`]'s reason.
    ///
    /// **Nothing here asks whether the object is really an annotation, or is
    /// there at all.** Those are `save::set_note`'s refusals, and they
    /// are refusals about the *file* --- the case a reader can act on, because
    /// it means the document changed under them.
    ///
    /// # Errors
    ///
    /// The page does not exist, or was deleted.
    pub fn rewrite(
        &mut self,
        object: ObjectId,
        page: PageId,
        body: String,
        made: String,
    ) -> Result<(), Refusal> {
        // The page **before** the id is issued, which is `renote`'s arrangement
        // with `live_mark` and matters for the same reason `redact`'s does: a
        // refusal must leave the tables exactly as it found them, or a reader
        // who names a deleted page grows `rewrites` by one body nothing will
        // ever reach. `Working::apply` checks the page again, and that is not a
        // second copy of the rule -- it is the same call, reached on the replay
        // path where this pre-check does not run.
        self.now.live(page)?;
        let edit = self.issue_rewrite(NoteEdit { body, made });
        self.apply(Command::Rewrite { object, page, edit })
    }

    /// Whether any live mark is a reply to this foreign comment.
    ///
    /// **Over the live marks rather than over every mark ever made**, which is
    /// what [`Working::all_marks`] gives: a reply the reader removed is not a
    /// reply any more, and refusing a deletion because of one would tell them to
    /// remove something that is already gone.
    ///
    /// Linear in the live marks, which is the same cost `all_marks` already
    /// pays and is asked once per deletion --- an operation a reader performs by
    /// hand, one comment at a time.
    fn answered_by_a_reply(&self, object: ObjectId) -> bool {
        self.now
            .all_marks()
            .into_iter()
            .filter_map(|(_, id)| self.mark(id))
            .any(|mark| mark.reply_to == Some(object))
    }

    /// Takes a comment out of the file off the page it is on.
    ///
    /// [`rewrite`](Self::rewrite)'s counterpart and it takes that entry's page
    /// argument for the same reason: this model has never read the file, so the
    /// page is the only thing about the comment it can check and the only thing
    /// that lets a page deletion take the discard with it.
    ///
    /// **Nothing here asks whether the object is an annotation or is there at
    /// all**, which is `rewrite`'s note word for word: those are the writer's
    /// refusals, and they are refusals about the *file* --- the case a reader can
    /// act on, because it means the document changed under them.
    ///
    /// **Discarding a comment already discarded is still a command**, which is
    /// [`renote`](Self::renote)'s decision applied to a removal: whether a reader
    /// *meant* a no-op is a question about a gesture, and the layer holding the
    /// gesture drops it. The frontend does, by not offering Remove on a row that
    /// is already gone.
    ///
    /// # Errors
    ///
    /// The page does not exist, or was deleted.
    pub fn discard(&mut self, object: ObjectId, page: PageId) -> Result<(), Refusal> {
        // The page before anything else, for `rewrite`'s reason --- though there
        // is no body to spend here, which is what makes this the cheapest
        // command in the file.
        self.now.live(page)?;
        // **Before the command, and it is the reason this method is not two
        // lines.** A reply the reader wrote carries this object in `/IRT`, and
        // an `/IRT` naming an object the file no longer has is a malformed
        // thread. See [`Refusal::ReplyAnswersIt`] for why it is refused rather
        // than taking the reply along.
        if self.answered_by_a_reply(object) {
            return Err(Refusal::ReplyAnswersIt(object));
        }
        self.apply(Command::Discard { object, page })
    }

    /// Replaces what a drawing is made of --- the eraser's one command.
    ///
    /// **Refuses an empty result rather than removing the mark**, because a mark
    /// that draws nothing must not exist and this layer does not get to decide
    /// what a gesture meant. Sweeping an eraser over the last stroke of a
    /// drawing is [`Command::Unannotate`], issued by the caller; see
    /// [`Command::Reink`].
    ///
    /// The rectangle is derived here, from [`Stroke::bounds`], so that it is
    /// derived in one place for both the drawing that is made and the drawing
    /// that is erased --- `edits.rs` calls the same function when a mark is
    /// first sent, and two derivations would be free to disagree about the
    /// padding.
    ///
    /// # Errors
    ///
    /// The id names no mark or one already removed; the mark is not ink;
    /// nothing drawable survives.
    pub fn reink(&mut self, mark: MarkId, strokes: Vec<Stroke>) -> Result<(), Refusal> {
        self.reink_in(mark, strokes, None)
    }

    /// [`reink`](Doc::reink)'s twin, for a re-inking that belongs to a gesture.
    ///
    /// One sweep of the eraser re-inks every drawing it crossed, and the reader
    /// pressing undo is putting back the sweep rather than one of the drawings.
    ///
    /// # Errors
    ///
    /// As [`reink`](Doc::reink).
    pub fn reink_in(
        &mut self,
        mark: MarkId,
        strokes: Vec<Stroke>,
        sweep: Option<SweepId>,
    ) -> Result<(), Refusal> {
        self.now.live_mark(mark)?;
        let kind = self.mark(mark).map_or(MarkKind::Highlight, |m| m.kind);
        if kind != MarkKind::Ink {
            return Err(Refusal::ShapeMismatch(kind));
        }
        if !strokes.iter().any(Stroke::is_drawable) {
            return Err(Refusal::EmptyMark);
        }
        // **Half the mark's own width, not half [`INK_WIDTH`].** The pad is
        // what makes the rectangle hold the ink rather than the path, so a
        // drawing made with a broad nib and then erased down to two strokes
        // would otherwise come back with a box too small for the line still in
        // it. The kind is read a line above from the same lookup, so this costs
        // nothing but saying it.
        let width = self.mark(mark).map_or(INK_WIDTH, |m| m.width);
        let quads = Stroke::bounds(&strokes, (width / 2.0) as f32)
            .into_iter()
            .collect();
        let ink = self.issue_ink(Ink { strokes, quads });
        self.apply_in(Command::Reink { mark, ink }, sweep)
    }

    /// Moves a mark by `(dx, dy)` in the page's display space.
    ///
    /// **A delta, not a new geometry, and that is what makes it a move.** The
    /// caller holds the gesture and could compute the rectangles itself --- it is
    /// what [`Doc::annotate`] takes --- but then "move this mark" and "put this
    /// mark somewhere else entirely" would be one command, and a defect in the
    /// arithmetic on the far side of the boundary could silently resize a mark
    /// or reshape a drawing. One offset applied here to everything the mark owns
    /// cannot do either: every rectangle keeps its size and every stroke keeps
    /// its shape, by construction rather than by a check.
    ///
    /// **Clamping is the caller's, deliberately.** Keeping a mark on its page
    /// needs the page's size in points, which this model does not hold --- pages
    /// are ids, turns and crops here, and the size comes from the renderer. The
    /// frontend clamps the delta against the laid-out page before sending it, in
    /// the same place and for the same reason `iconQuad` and `boxQuad` clamp the
    /// geometry they build.
    ///
    /// **Every kind, and no shape check** --- [`Doc::recolor`]'s posture rather
    /// than [`Doc::reink`]'s. Geometry is geometry: a highlight has a rectangle
    /// like everything else, and whether dragging one *means* anything to a
    /// reader is a question about the product that the layer holding the gesture
    /// answers. Today the viewer offers the drag on the kinds a reader places
    /// and not on the ones made out of words.
    ///
    /// It journals as [`Command::Reink`], which is the eraser's command and is
    /// now also this one: both replace what a mark is laid down as, both undo by
    /// replaying whichever version came before, and a second variant would be a
    /// second precedence rule for [`Doc::quads_of`] to get right. See [`Ink`] on
    /// why the name stands.
    ///
    /// A zero offset is still a command, for the reason [`renote`](Doc::renote)
    /// gives: whether a reader *meant* a no-op is a question about a gesture, and
    /// the layer holding the gesture drops it.
    ///
    /// # Errors
    ///
    /// The id names no mark, or one already removed.
    pub fn displace(&mut self, mark: MarkId, dx: f32, dy: f32) -> Result<(), Refusal> {
        self.now.live_mark(mark)?;
        let quads = self
            .quads_of(mark)
            .iter()
            .map(|q| Quad {
                left: q.left + dx,
                top: q.top + dy,
                right: q.right + dx,
                bottom: q.bottom + dy,
            })
            .collect();
        let strokes = self
            .strokes_of(mark)
            .iter()
            .map(|stroke| Stroke {
                points: stroke
                    .points
                    .iter()
                    .map(|p| Point {
                        x: p.x + dx,
                        y: p.y + dy,
                    })
                    .collect(),
            })
            .collect();
        let ink = self.issue_ink(Ink { strokes, quads });
        self.apply(Command::Reink { mark, ink })
    }

    /// Replaces what a mark is drawn in --- the swatch row's one command.
    ///
    /// **Every kind, and no shape check.** [`reink`](Doc::reink) is ink's alone
    /// and refuses anything else; a colour is written as `/C` for all six, so
    /// the only thing that can go wrong here is the id.
    ///
    /// Recolouring a mark to the colour it already is is still a command, for
    /// the reason [`renote`](Doc::renote) gives at length: whether a reader
    /// *meant* a no-op is a question about a gesture, and the layer holding the
    /// gesture is the one that gets to drop it.
    ///
    /// **Not clamped here.** [`Mark::color`] promises `0..=1` and the wire
    /// boundary in `edits.rs` is what makes that true, for a `NewMark` and for
    /// this; a second clamp would be a second copy of the rule, and the copy
    /// that is wrong is the one nothing reaches.
    ///
    /// # Errors
    ///
    /// [`Refusal::NoSuchMark`] for an id nobody issued, and
    /// [`Refusal::MarkRemoved`] for a mark taken off the page --- checked before
    /// the id is issued, so a refused colour spends nothing.
    pub fn recolor(&mut self, mark: MarkId, color: [f32; 3]) -> Result<(), Refusal> {
        self.now.live_mark(mark)?;
        let color = self.issue_color(color);
        self.apply(Command::Recolor { mark, color })
    }

    /// Records a colour and returns the id that names it.
    fn issue_color(&mut self, color: [f32; 3]) -> ColorId {
        let id = ColorId(self.next_color);
        self.colors.insert(id, color);
        self.next_color += 1;
        id
    }

    /// What a mark is drawn in now, after any recolouring.
    ///
    /// **The one accessor every reader of a mark's colour has to go through**,
    /// for [`quads_of`](Doc::quads_of)'s reason exactly: a caller taking
    /// [`Mark::color`] straight from the body would draw the overlay, and write
    /// the file, in the colour the mark was made in rather than the one it is.
    ///
    /// Falls back to the body, which is the answer for every mark nobody has
    /// recoloured. Black for an id this document never issued, which no caller
    /// reaches --- both of them are walking marks the model just gave them.
    pub fn color_of(&self, mark: MarkId) -> [f32; 3] {
        self.now
            .color_of(mark)
            .and_then(|color| self.colors.get(&color))
            .copied()
            .unwrap_or_else(|| self.mark(mark).map_or([0.0; 3], |m| m.color))
    }

    /// How many colour versions are held.
    ///
    /// The accounting observable for colours, and it exists for the reason
    /// [`note_bodies`](Doc::note_bodies) does --- with the difference that its
    /// twin's version of this had no test reading it and leaked for a week.
    pub fn color_bodies(&self) -> usize {
        self.colors.len()
    }

    /// Records a version of a drawing and returns the id that names it.
    fn issue_ink(&mut self, ink: Ink) -> InkId {
        let id = InkId(self.next_ink);
        self.inks.insert(id, ink);
        self.next_ink += 1;
        id
    }

    /// What a drawing is made of now, after any erasing.
    ///
    /// Reads the *working* document, so an undo restores what it answers. Falls
    /// back to the strokes the mark was made with, which is the answer for every
    /// mark an eraser has never touched --- and for every kind that has no
    /// strokes at all, where it is empty.
    pub fn strokes_of(&self, mark: MarkId) -> &[Stroke] {
        self.now
            .ink_of(mark)
            .and_then(|ink| self.inks.get(&ink))
            .map_or_else(
                || self.mark(mark).map_or(&[][..], |m| &m.strokes[..]),
                |ink| &ink.strokes[..],
            )
    }

    /// The rectangles a mark occupies now.
    ///
    /// **The one accessor every reader of a mark's geometry has to go through**,
    /// because erasing a stroke moves the rectangle: a caller taking
    /// [`Mark::quads`] straight from the body would place the popup, hit-test
    /// and write a `/Rect` around a stroke that is no longer drawn.
    pub fn quads_of(&self, mark: MarkId) -> &[Quad] {
        self.now
            .ink_of(mark)
            .and_then(|ink| self.inks.get(&ink))
            .map_or_else(
                || self.mark(mark).map_or(&[][..], |m| &m.quads[..]),
                |ink| &ink.quads[..],
            )
    }

    /// How many drawing versions are held.
    ///
    /// The accounting observable for strokes, and it exists for the reason
    /// [`note_bodies`](Doc::note_bodies) does: a version kept after the command
    /// naming it was discarded, and one correctly dropped, produce identical
    /// documents.
    pub fn ink_bodies(&self) -> usize {
        self.inks.len()
    }

    /// Records a note's text and returns the id that names it.
    fn issue_note(&mut self, note: String) -> NoteId {
        let id = NoteId(self.next_note);
        self.notes.insert(id, note);
        self.next_note += 1;
        id
    }

    /// What a mark says, empty for one nobody has typed on.
    ///
    /// Reads the *working* document, so it answers what an undo has restored
    /// rather than what was last typed. Empty for a mark that is not on a page,
    /// which is the same posture [`Working::marks_on`] takes: every caller of
    /// this is writing a mark out or drawing it, and a mark that is not there
    /// has no note to show either way.
    pub fn note_of(&self, mark: MarkId) -> &str {
        self.now
            .note_of(mark)
            .and_then(|note| self.notes.get(&note))
            .map_or("", String::as_str)
    }

    /// How many note versions are held.
    ///
    /// The accounting observable for notes, and it exists for the reason
    /// [`mark_bodies`](Doc::mark_bodies) does: a version kept after the command
    /// naming it was discarded, and one correctly dropped, produce identical
    /// documents.
    pub fn note_bodies(&self) -> usize {
        self.notes.len()
    }

    /// Records a version of a foreign comment's body and returns its id.
    fn issue_rewrite(&mut self, edit: NoteEdit) -> RewriteId {
        let id = RewriteId(self.next_rewrite);
        self.rewrites.insert(id, edit);
        self.next_rewrite += 1;
        id
    }

    /// What a foreign comment now says, or `None` for one nobody has edited.
    ///
    /// Reads the *working* document, so it answers what an undo has restored.
    /// `None` rather than an empty body is the answer that matters: it is the
    /// difference between "leave this comment alone" and "write an empty
    /// `/Contents` over it", and a reader who selects a comment's text and
    /// deletes it means the second.
    pub fn rewrite_of(&self, object: ObjectId) -> Option<&NoteEdit> {
        self.now
            .rewrite_of(object)
            .and_then(|edit| self.rewrites.get(&edit))
    }

    /// How many rewrite versions are held.
    ///
    /// The accounting observable for rewrites, and it exists for
    /// [`note_bodies`](Doc::note_bodies)' reason --- with that entry's warning
    /// attached: the colour table's version of this sat unread for a week, so
    /// there is a test that reads it.
    pub fn rewrite_bodies(&self) -> usize {
        self.rewrites.len()
    }

    /// Applies a command, or refuses and changes nothing.
    ///
    /// A successful apply **discards the redo tail**, which is what makes the
    /// journal a line rather than a tree.
    pub fn apply(&mut self, cmd: Command) -> Result<(), Refusal> {
        self.apply_in(cmd, None)
    }

    /// Applies a command as part of a gesture, or refuses and changes nothing.
    ///
    /// [`apply`](Doc::apply)'s twin, and the only way a command joins a group.
    /// Undo and redo cross a run of adjacent entries carrying the same
    /// [`SweepId`] in one step, which is what makes one press of undo put back
    /// a whole sweep of the eraser rather than the last drawing it touched.
    pub fn apply_in(&mut self, cmd: Command, sweep: Option<SweepId>) -> Result<(), Refusal> {
        self.now.apply(cmd)?;
        // Bodies belonging to the discarded tail go with it. Without this, a
        // reader who annotates and undoes in a loop grows the table forever ---
        // and the ids are never re-issued, so nothing else would ever notice.
        for discarded in &self.journal[self.cursor..] {
            match discarded.cmd {
                Command::Annotate { mark, note, .. } => {
                    self.marks.remove(&mark);
                    self.notes.remove(&note);
                }
                Command::Renote { note, .. } => {
                    self.notes.remove(&note);
                }
                Command::Reink { ink, .. } => {
                    self.inks.remove(&ink);
                }
                Command::Recolor { color, .. } => {
                    self.colors.remove(&color);
                }
                Command::Rewrite { edit, .. } => {
                    self.rewrites.remove(&edit);
                }
                Command::Redact { redaction, .. } => {
                    self.redactions.remove(&redaction);
                }
                // **Listed rather than left to a catch-all**, which is what
                // this arm used to be. Every command that issues a body needs a
                // line here, and `_ => {}` is a rule nobody is reminded of ---
                // the trap index calls a catch-all the thing that makes
                // forgetting the quiet outcome. These five issue nothing, so
                // spelling them out costs a line each and makes the next
                // command that does issue something a compile error.
                Command::Rotate { .. }
                | Command::Crop { .. }
                | Command::Delete { .. }
                | Command::Move { .. }
                // Here rather than above with the six that issue a body, and
                // the size is why it looks like it belongs there: it is carried
                // *in* the command rather than in a table keyed by an id, so a
                // discarded `Insert` leaves nothing behind to remove. See the
                // variant.
                | Command::Insert { .. }
                | Command::Unannotate { .. }
                // With the removals rather than with the six above, and the
                // object number is why it looks like it belongs there: it names
                // something the *file* numbered, so there is no table of ours
                // keyed by it and a discarded `Discard` leaves nothing behind.
                | Command::Discard { .. }
                | Command::Unredact { .. } => {}
            }
        }
        self.journal.truncate(self.cursor);
        // Snapshots past the cursor describe states that no longer exist. Keeping
        // one would not merely waste a clone: the next rebuild through that
        // position would start from a document built by commands this apply just
        // discarded, and every page after it would be wrong with nothing saying so.
        self.snapshots.retain(|&at, _| at <= self.cursor);
        self.journal.push(Entry { cmd, sweep });
        self.cursor += 1;
        if self.cursor % SNAPSHOT_EVERY == 0 {
            self.snapshots.insert(self.cursor, self.now.clone());
        }
        Ok(())
    }

    /// Steps back one command, or one whole gesture. Returns whether there was one.
    ///
    /// **A gesture is one press**, which is the whole reason [`Entry`] carries a
    /// sweep: one drag of the eraser across five drawings is five commands, and
    /// a reader who wants them back wants them all back. Only *adjacent* entries
    /// group, and only when both name the same gesture, so nothing a reader did
    /// between two sweeps is swallowed by either.
    pub fn undo(&mut self) -> bool {
        if !self.can_undo() {
            return false;
        }
        self.cursor -= 1;
        while let Some(with) = self.grouped(self.cursor) {
            if self.cursor == 0 || self.grouped(self.cursor - 1) != Some(with) {
                break;
            }
            self.cursor -= 1;
        }
        self.now = self.rebuild(self.cursor);
        true
    }

    /// The gesture the entry at `at` belongs to, if it belongs to one.
    ///
    /// Indexing rather than `get` because every caller has already established
    /// that the entry exists --- undo by stepping onto it, redo by applying it,
    /// and both of them again for the neighbour they are about to compare
    /// against.
    ///
    /// **Both operands of that comparison come through here**, which is the
    /// point: *a command outside a gesture stands alone* would otherwise be
    /// enforced twice, once by the `while let` and once by an `Option`
    /// equality that reads the field directly. Two mechanisms for one rule
    /// make each other unfalsifiable, and `docs/TRAPS.md` has the entry --- a
    /// mutation of this function survived until both sides read it.
    fn grouped(&self, at: usize) -> Option<SweepId> {
        self.journal[at].sweep
    }

    /// Steps forward one command, or one whole gesture. Returns whether there was one.
    ///
    /// Applies rather than rebuilds, since the working document is already the
    /// state this command expects.
    ///
    /// **The mirror of [`undo`](Doc::undo), and it has to be.** A gesture that
    /// goes back in one press and returns in five would leave the two counts
    /// disagreeing about how many presses the reader is from where they were.
    pub fn redo(&mut self) -> bool {
        if !self.can_redo() {
            return false;
        }
        self.step();
        while let Some(with) = self.grouped(self.cursor - 1) {
            if !self.can_redo() || self.grouped(self.cursor) != Some(with) {
                break;
            }
            self.step();
        }
        true
    }

    /// Re-applies the entry at the cursor and advances past it.
    ///
    /// **A refusal here is a broken model**, for [`rebuild`](Doc::rebuild)'s
    /// reason: every entry was accepted against the state its predecessors
    /// produced, and redo has just reproduced those predecessors.
    fn step(&mut self) {
        let cmd = self.journal[self.cursor].cmd;
        self.now.apply(cmd).unwrap_or_else(|why| {
            panic!("a journalled command was refused on redo: {cmd:?} -> {why:?}")
        });
        self.cursor += 1;
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
        for &Entry { cmd, .. } in &self.journal[from..upto] {
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

    /// A4 in points, which is what a blank page is in most of these.
    const A4: Size = Size {
        width: 595.0,
        height: 842.0,
    };

    /// What a page shows, for the assertions about an insert.
    fn source_of(doc: &Doc, page: PageId) -> PageSource {
        doc.working().page(page).expect("a live page").source
    }

    #[test]
    fn an_inserted_page_lands_behind_its_anchor_and_shows_nothing() {
        let mut doc = Doc::open(3);
        let second = doc.working().order()[1];
        let made = doc.insert(Some(second), A4).expect("insert");

        assert_eq!(
            ids(&doc),
            vec![1, 2, made.get(), 3],
            "the new page sits behind the one it was anchored to"
        );
        assert_eq!(source_of(&doc, made), PageSource::Blank(A4));
        assert_eq!(
            doc.working().page(made).expect("live").extra_turns,
            0,
            "a page tpdf makes starts unturned"
        );
        assert_eq!(doc.working().page(made).expect("live").crop, None);
        // The page it was placed behind is untouched, which is the half a test
        // that only counts pages cannot see.
        assert_eq!(source_of(&doc, second), PageSource::Baseline(1));
    }

    #[test]
    fn an_insert_with_no_anchor_goes_to_the_front() {
        let mut doc = Doc::open(2);
        let made = doc.insert(None, A4).expect("insert");
        assert_eq!(ids(&doc), vec![made.get(), 1, 2]);
    }

    /// The id is past the baseline's, which is what stops two pages sharing one.
    #[test]
    fn a_made_page_is_never_given_a_baseline_page_s_id() {
        let mut doc = Doc::open(3);
        let made = doc.insert(None, A4).expect("insert");
        assert_eq!(made.get(), 4, "one past the three the file has");
        let second = doc.insert(None, A4).expect("insert");
        assert_eq!(second.get(), 5);
    }

    #[test]
    fn a_page_enclosing_no_area_is_refused_and_spends_no_id() {
        let mut doc = Doc::open(2);
        for size in [
            Size {
                width: 0.0,
                height: 800.0,
            },
            Size {
                width: 600.0,
                height: 0.0,
            },
            Size {
                width: -600.0,
                height: 800.0,
            },
            Size {
                width: f64::NAN,
                height: 800.0,
            },
        ] {
            // The variant, never the value: a `Size` holding a `NaN` does not
            // compare equal to itself, which `Refusal::DegeneratePage` states
            // and which is what made the first version of this assertion fail
            // with both sides printing identically.
            assert!(
                matches!(doc.insert(None, size), Err(Refusal::DegeneratePage(_))),
                "{size:?}"
            );
        }
        assert_eq!(ids(&doc), vec![1, 2], "nothing was inserted");
        // The id the four refusals would have spent is still the next one.
        let made = doc.insert(None, A4).expect("insert");
        assert_eq!(made.get(), 3);
    }

    /// The apply refuses it too, which is not the same check twice.
    ///
    /// `Doc::insert` refuses before it spends an id; the apply refuses because
    /// `Command::Insert` is public and reaches it through `Doc::apply` without
    /// passing that method at all. Two guards, two callers, and a test that only
    /// went through `insert` could not tell the second from a missing one --- the
    /// trap about a guard whose neighbour refuses the same input first.
    #[test]
    fn a_page_of_no_area_is_refused_by_the_apply_as_well_as_by_the_entry_point() {
        let mut doc = Doc::open(2);
        assert!(matches!(
            doc.apply(Command::Insert {
                page: PageId::from_raw(99),
                after: None,
                size: Size {
                    width: 0.0,
                    height: 800.0,
                },
            }),
            Err(Refusal::DegeneratePage(_))
        ));
        assert_eq!(ids(&doc), vec![1, 2]);
        // The control: the same command with a proper size is applied, so this
        // is about the size rather than about `Doc::apply` refusing an insert.
        doc.apply(Command::Insert {
            page: PageId::from_raw(99),
            after: None,
            size: A4,
        })
        .expect("a proper size through the same door");
        assert_eq!(ids(&doc), vec![99, 1, 2]);
    }

    #[test]
    fn an_insert_behind_a_page_that_is_not_there_is_refused() {
        let mut doc = Doc::open(2);
        let second = doc.working().order()[1];
        doc.apply(Command::Delete { page: second }).expect("delete");

        assert!(matches!(
            doc.insert(Some(second), A4),
            Err(Refusal::PageDeleted(_))
        ));
        assert!(matches!(
            doc.insert(Some(PageId::from_raw(9_999)), A4),
            Err(Refusal::NoSuchPage(_))
        ));
        assert_eq!(ids(&doc), vec![1], "neither refusal changed the order");
    }

    /// Undo takes it back, and redo brings back *the same page*.
    ///
    /// The identity is the point rather than the count: ids only ever count up,
    /// so a redo that issued a fresh one would put a different page in the same
    /// slot and every mark and reference naming the first would be orphaned.
    #[test]
    fn undoing_an_insert_removes_it_and_redo_brings_the_same_page_back() {
        let mut doc = Doc::open(2);
        let made = doc.insert(None, A4).expect("insert");
        assert_eq!(ids(&doc), vec![made.get(), 1, 2]);

        assert!(doc.undo());
        assert_eq!(ids(&doc), vec![1, 2]);
        assert!(
            doc.working().page(made).is_none(),
            "the page is gone, not merely out of the order"
        );

        assert!(doc.redo());
        assert_eq!(ids(&doc), vec![made.get(), 1, 2]);
        assert_eq!(source_of(&doc, made), PageSource::Blank(A4));
    }

    /// Replay past a snapshot rebuilds the same page rather than a new one.
    ///
    /// Undo replays from a snapshot rather than inverting, so an `Insert` is
    /// *re-applied* --- and the id it carries is what makes that reproduce the
    /// page it made the first time. A command that issued a fresh id on replay
    /// would pass every count-based check here and rename the page.
    #[test]
    fn an_insert_survives_a_rebuild_through_a_snapshot() {
        let mut doc = Doc::open(2);
        let made = doc.insert(None, A4).expect("insert");
        let first = doc.working().order()[1];
        for _ in 0..(SNAPSHOT_EVERY * 2 + 5) {
            doc.apply(Command::Rotate {
                page: first,
                turns: 1,
            })
            .expect("rotate");
        }
        for _ in 0..(SNAPSHOT_EVERY * 2 + 5) {
            assert!(doc.undo());
        }
        assert_eq!(ids(&doc), vec![made.get(), 1, 2], "back to just the insert");
        assert_eq!(source_of(&doc, made), PageSource::Blank(A4));
    }

    #[test]
    fn a_made_page_turns_and_deletes_like_any_other() {
        let mut doc = Doc::open(2);
        let made = doc.insert(None, A4).expect("insert");
        doc.apply(Command::Rotate {
            page: made,
            turns: 1,
        })
        .expect("turn a made page");
        let page = *doc.working().page(made).expect("live");
        assert_eq!(page.extra_turns, 1);
        assert_eq!(page.source, PageSource::Blank(A4), "still the same page");

        doc.apply(Command::Move {
            page: made,
            after: Some(doc.working().order()[2]),
        })
        .expect("move a made page");
        assert_eq!(ids(&doc), vec![1, 2, made.get()]);

        doc.apply(Command::Delete { page: made }).expect("delete");
        assert_eq!(ids(&doc), vec![1, 2]);
    }

    /// A crop on a made page is refused, and clearing one is not.
    ///
    /// The asymmetry is the point and it is stated on `Refusal::CropOnMadePage`: a box
    /// cannot be measured for a page no file supplies, and clearing a crop that
    /// does not exist is an operation with no effect rather than an error. The
    /// control is the same box on a page out of the file.
    #[test]
    fn a_page_tpdf_made_takes_no_crop_and_clearing_one_is_not_an_error() {
        let mut doc = Doc::open(2);
        let made = doc.insert(None, A4).expect("insert");
        let ordinary = doc.working().order()[1];

        assert!(matches!(
            doc.apply(Command::Crop {
                page: made,
                to: Some(rect(0.0, 0.0, 100.0, 200.0)),
            }),
            Err(Refusal::CropOnMadePage(_))
        ));
        doc.apply(Command::Crop {
            page: ordinary,
            to: Some(rect(0.0, 0.0, 100.0, 200.0)),
        })
        .expect("the same box on a page out of the file");

        doc.apply(Command::Crop {
            page: made,
            to: None,
        })
        .expect("clearing a crop a made page never had");
        assert_eq!(doc.working().page(made).expect("live").crop, None);
    }

    /// A region marked for removal on a made page is refused, and a mark is not.
    ///
    /// The two halves used to be one rule and are now the difference between a
    /// refusal that expired and one that does not. The control beside the
    /// redaction is what says it is about *this* page rather than about
    /// redacting at all; the mark's control is the mark landing on the made page
    /// and being **on** it afterwards, because "did not return an error" is
    /// satisfied by a model that accepted it onto nothing.
    #[test]
    fn a_page_tpdf_made_takes_a_mark_and_no_redaction() {
        let mut doc = Doc::open(2);
        let made = doc.insert(None, A4).expect("insert");
        let ordinary = doc.working().order()[1];

        let mark = doc
            .annotate(mark_on(made), String::new())
            .expect("a mark on a page tpdf made");
        assert_eq!(doc.working().marks_on(made), vec![mark]);
        assert!(doc.working().marks_on(ordinary).is_empty());

        assert!(matches!(
            doc.redact(Redaction {
                page: made,
                area: Quad {
                    left: 10.0,
                    top: 10.0,
                    right: 90.0,
                    bottom: 40.0,
                },
            }),
            Err(Refusal::RedactionOnMadePage(_))
        ));
        doc.redact(Redaction {
            page: ordinary,
            area: Quad {
                left: 10.0,
                top: 10.0,
                right: 90.0,
                bottom: 40.0,
            },
        })
        .expect("the same region on a page out of the file");
    }

    /// A highlight over one ordinary-looking line, on `page`.
    fn mark_on(page: PageId) -> Mark {
        Mark {
            kind: MarkKind::Highlight,
            stamp: None,
            reply_to: None,
            page,
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
            made: "D:20260818T120000Z".to_string(),
        }
    }

    #[test]
    fn a_baseline_page_maps_to_itself_in_order() {
        let doc = Doc::open(4);
        assert_eq!(ids(&doc), vec![1, 2, 3, 4]);
        for (i, &id) in doc.working().order().iter().enumerate() {
            let page = doc.working().page(id).expect("baseline pages are live");
            assert_eq!(page.source, PageSource::Baseline(i as u32));
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
        assert_eq!(page.source, PageSource::Baseline(0));
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
            for &Entry { cmd, .. } in &doc.journal[..target] {
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

    // --- Marks ---------------------------------------------------------------

    #[test]
    fn a_mark_lands_on_the_page_it_names() {
        let mut doc = Doc::open(3);
        let second = doc.working().order()[1];
        let id = doc
            .annotate(mark_on(second), String::new())
            .expect("annotate");

        assert_eq!(doc.working().marks_on(second), [id]);
        assert_eq!(doc.working().page_of(id), Some(second));
        // The other pages are untouched, which a `marks_on` that ignored its
        // argument would also have to satisfy.
        assert!(doc.working().marks_on(doc.working().order()[0]).is_empty());
        assert_eq!(doc.mark(id).expect("body").kind, MarkKind::Highlight);
    }

    #[test]
    fn an_id_spent_by_an_undone_mark_is_never_issued_again() {
        // The property `docmodel` deferred until something created an id, stated
        // as the module note states it: undo rewinds the cursor and never the
        // allocator, so the second mark is a second mark.
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        let first = doc.annotate(mark_on(page), String::new()).expect("first");
        assert!(doc.undo());
        let second = doc.annotate(mark_on(page), String::new()).expect("second");

        assert_ne!(
            first, second,
            "an undone mark's id was handed to a different mark"
        );
        assert_eq!(doc.marks_issued(), 2);
        // And the first is gone rather than merely unreachable: its command was
        // in the redo tail that the second annotate discarded, so keeping its
        // body would be a leak no behaviour could see.
        assert!(doc.mark(first).is_none());
        assert_eq!(doc.mark_bodies(), 1);
    }

    #[test]
    fn redo_restores_the_mark_it_undid_rather_than_a_copy() {
        // The other half of the same property, and the reason the id is carried
        // in the command rather than allocated on apply: replay must not spend
        // anything.
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        let body = mark_on(page);
        let id = doc
            .annotate(body.clone(), "the one I made".to_string())
            .expect("annotate");

        assert!(doc.undo());
        assert!(doc.working().marks_on(page).is_empty());
        assert!(doc.redo());

        assert_eq!(doc.working().marks_on(page), [id]);
        assert_eq!(doc.mark(id), Some(&body));
        // Including what it said, which the body does not carry: a redo that
        // restored the mark and not its note would satisfy every line above.
        assert_eq!(doc.note_of(id), "the one I made");
        assert_eq!(
            doc.marks_issued(),
            1,
            "replay issued an id, so a later undo would rename the mark"
        );
    }

    /// A drawing of two strokes, on `page`.
    fn ink_on(page: PageId) -> Mark {
        let strokes = vec![Stroke {
            points: vec![Point { x: 72.0, y: 90.0 }, Point { x: 300.0, y: 108.0 }],
        }];
        Mark {
            kind: MarkKind::Ink,
            stamp: None,
            reply_to: None,
            page,
            quads: Stroke::bounds(&strokes, 1.25).into_iter().collect(),
            strokes,
            color: [0.85, 0.15, 0.15],
            width: INK_WIDTH,
            author: "a reader".to_string(),
            made: "D:20260820T120000Z".to_string(),
        }
    }

    /// A drawing of three strokes, well apart, on `page`.
    ///
    /// Three rather than two because the eraser tests need a *middle* one: with
    /// two, "the survivors are the ones not named" and "the survivor is the last
    /// one" are the same assertion.
    fn drawing_on(page: PageId) -> Mark {
        let strokes = vec![
            Stroke {
                points: vec![Point { x: 72.0, y: 90.0 }, Point { x: 300.0, y: 90.0 }],
            },
            Stroke {
                points: vec![Point { x: 72.0, y: 150.0 }, Point { x: 300.0, y: 150.0 }],
            },
            Stroke {
                points: vec![Point { x: 72.0, y: 210.0 }, Point { x: 300.0, y: 210.0 }],
            },
        ];
        Mark {
            kind: MarkKind::Ink,
            stamp: None,
            reply_to: None,
            page,
            quads: Stroke::bounds(&strokes, 1.25).into_iter().collect(),
            strokes,
            color: [0.85, 0.15, 0.15],
            width: INK_WIDTH,
            author: "a reader".to_string(),
            made: "D:20260820T120000Z".to_string(),
        }
    }

    #[test]
    fn erasing_a_stroke_leaves_the_others_and_shrinks_the_rectangle() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let before = doc.quads_of(id)[0];
        assert_eq!(doc.strokes_of(id).len(), 3);

        let keep = vec![doc.strokes_of(id)[0].clone(), doc.strokes_of(id)[1].clone()];
        doc.reink(id, keep).expect("erased");

        assert_eq!(doc.strokes_of(id).len(), 2, "the third stroke is gone");
        let after = doc.quads_of(id)[0];
        // The rectangle is the whole point of `quads_of` existing: the body
        // still holds the three-stroke bounds, and a reader taking those would
        // hit-test and place the popup around a stroke nobody can see.
        assert!(
            after.bottom < before.bottom,
            "the rectangle still reaches the erased stroke: {after:?} against {before:?}"
        );
        assert_eq!(
            doc.mark(id).expect("body").quads[0],
            before,
            "the body is unchanged, which is why every reader has to go through the accessor"
        );
    }

    #[test]
    fn an_undone_erasure_puts_the_stroke_back() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let whole = doc.strokes_of(id).to_vec();
        let box_ = doc.quads_of(id)[0];

        doc.reink(id, vec![doc.strokes_of(id)[0].clone()])
            .expect("erased");
        assert_eq!(doc.strokes_of(id).len(), 1);

        assert!(doc.undo(), "an erasure is one command and undoes");
        assert_eq!(doc.strokes_of(id), &whole[..], "every stroke is back");
        assert_eq!(doc.quads_of(id)[0], box_, "and so is the rectangle");

        assert!(doc.redo(), "and it redoes");
        assert_eq!(doc.strokes_of(id).len(), 1);
    }

    #[test]
    fn a_drawing_erased_to_nothing_is_refused_here() {
        // The model's half of the rule; `Edits::erase` is the layer that turns
        // the refusal into a removal, because only it knows the gesture meant
        // "get rid of it". See `Command::Reink`.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        assert_eq!(doc.reink(id, Vec::new()), Err(Refusal::EmptyMark));
        // A stroke of one point draws nothing either, and is the case a caller
        // filtering on `is_empty` would let through.
        let dot = Stroke {
            points: vec![Point { x: 72.0, y: 90.0 }],
        };
        assert_eq!(doc.reink(id, vec![dot]), Err(Refusal::EmptyMark));
        assert_eq!(doc.strokes_of(id).len(), 3, "and nothing was erased");
    }

    #[test]
    fn only_a_drawing_can_be_erased() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc.annotate(mark_on(page), String::new()).expect("marked");
        let stroke = drawing_on(page).strokes[0].clone();
        assert_eq!(
            doc.reink(id, vec![stroke]),
            Err(Refusal::ShapeMismatch(MarkKind::Highlight)),
            "a highlight has no strokes to rub out"
        );
    }

    #[test]
    fn erasing_a_mark_that_is_not_there_is_refused_before_an_id_is_spent() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let stroke = doc.strokes_of(id)[0].clone();
        doc.apply(Command::Unannotate { mark: id })
            .expect("removed");

        let held = doc.ink_bodies();
        assert_eq!(doc.reink(id, vec![stroke]), Err(Refusal::MarkRemoved(id)));
        assert_eq!(
            doc.ink_bodies(),
            held,
            "a refused erasure spends no version, the way a refused mark spends no id"
        );
    }

    #[test]
    fn moving_a_mark_carries_its_rectangle_and_its_strokes_together() {
        // **A drawing is the fixture on purpose.** Every other kind has quads and
        // nothing else, so a `displace` that moved the rectangle and left the
        // strokes where they were would pass for all five of them --- and the
        // saved file would then draw the line in the old place inside a `/Rect`
        // in the new one. Ink is the only kind that can see it.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let quads: Vec<Quad> = doc.quads_of(id).to_vec();
        let strokes: Vec<Stroke> = doc.strokes_of(id).to_vec();

        doc.displace(id, 40.0, -15.0).expect("moved");

        for (was, now) in quads.iter().zip(doc.quads_of(id)) {
            assert_eq!(now.left, was.left + 40.0);
            assert_eq!(now.right, was.right + 40.0);
            assert_eq!(now.top, was.top - 15.0);
            assert_eq!(now.bottom, was.bottom - 15.0);
        }
        for (was, now) in strokes.iter().zip(doc.strokes_of(id)) {
            for (before, after) in was.points.iter().zip(&now.points) {
                assert_eq!(after.x, before.x + 40.0);
                assert_eq!(after.y, before.y - 15.0);
            }
        }
    }

    #[test]
    fn moving_a_mark_changes_where_it_is_and_nothing_else_about_it() {
        // The property that makes this a *move* rather than a new geometry: one
        // offset applied to everything the mark owns cannot resize a rectangle or
        // reshape a drawing, which is why `Doc::displace` takes two numbers and
        // not a list of quads. Asserted rather than argued, because the arithmetic
        // is three lines and a sign error in one of them is a resize.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let sizes: Vec<(f32, f32)> = doc
            .quads_of(id)
            .iter()
            .map(|q| (q.right - q.left, q.bottom - q.top))
            .collect();
        let kind = doc.mark(id).expect("body").kind;

        doc.displace(id, -7.5, 22.0).expect("moved");

        let after: Vec<(f32, f32)> = doc
            .quads_of(id)
            .iter()
            .map(|q| (q.right - q.left, q.bottom - q.top))
            .collect();
        assert_eq!(after, sizes, "a move does not resize");
        assert_eq!(doc.mark(id).expect("body").kind, kind);
        assert_eq!(doc.strokes_of(id).len(), 3, "and takes no stroke away");
    }

    #[test]
    fn every_kind_can_be_moved_including_the_one_that_cannot_be_erased() {
        // `recolor`'s posture rather than `reink`'s, and stated here because the
        // two neighbours disagree: geometry is geometry, and which kinds a reader
        // is *offered* the drag on is `markband.ts`'s rule, one layer up where the
        // gesture is. A model that refused a highlight here would make that rule
        // unchangeable without a second commit in another language.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc.annotate(mark_on(page), String::new()).expect("marked");
        assert_eq!(
            doc.reink(id, vec![drawing_on(page).strokes[0].clone()]),
            Err(Refusal::ShapeMismatch(MarkKind::Highlight)),
            "the neighbour refuses it"
        );
        assert_eq!(doc.displace(id, 5.0, 5.0), Ok(()), "and this one does not");
        assert_eq!(doc.quads_of(id)[0].left, 77.0);
    }

    #[test]
    fn moving_a_mark_that_is_not_there_is_refused_before_a_version_is_spent() {
        // `reink`'s neighbour test, and the same accounting observable: a refused
        // move must leave no `Ink` body behind, because a version kept after the
        // command naming it was discarded and one correctly dropped produce
        // identical documents --- so nothing a reader can see would ever say.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc.annotate(mark_on(page), String::new()).expect("marked");
        doc.apply(Command::Unannotate { mark: id })
            .expect("removed");

        let held = doc.ink_bodies();
        assert_eq!(doc.displace(id, 5.0, 5.0), Err(Refusal::MarkRemoved(id)));
        assert_eq!(doc.ink_bodies(), held);
    }

    #[test]
    fn undoing_a_move_puts_the_mark_back_where_it_was() {
        // The whole reason this journals as a command rather than editing the
        // body in place. Two moves and two undos, because one of each cannot tell
        // "restores the previous version" from "restores the original body" ---
        // and the second is what a `Reink` that replayed the `Annotate` would do.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc.annotate(mark_on(page), String::new()).expect("marked");
        let home = doc.quads_of(id)[0].left;

        doc.displace(id, 10.0, 0.0).expect("moved");
        doc.displace(id, 25.0, 0.0).expect("moved again");
        assert_eq!(doc.quads_of(id)[0].left, home + 35.0);

        doc.undo();
        assert_eq!(doc.quads_of(id)[0].left, home + 10.0, "back one move");
        doc.undo();
        assert_eq!(
            doc.quads_of(id)[0].left,
            home,
            "and back to where it started"
        );
        doc.redo();
        assert_eq!(doc.quads_of(id)[0].left, home + 10.0, "and forward again");
    }

    #[test]
    fn a_drawing_nobody_has_erased_answers_out_of_its_body() {
        // The fallback arm of both accessors, which every existing drawing takes
        // and which no other test here reaches on purpose.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let body = doc.mark(id).expect("body");
        assert_eq!(doc.strokes_of(id), &body.strokes[..]);
        assert_eq!(doc.quads_of(id), &body.quads[..]);
        assert_eq!(doc.ink_bodies(), 0, "and no version was recorded");
    }

    #[test]
    fn a_removed_drawing_forgets_which_version_it_was_on() {
        // `Unannotate` drops the entry, as it does for the note. Without that, a
        // mark removed and restored by undo would come back at whatever version
        // an erasure had left it on rather than at the one the journal says.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        doc.reink(id, vec![doc.strokes_of(id)[0].clone()])
            .expect("erased");
        doc.apply(Command::Unannotate { mark: id })
            .expect("removed");
        assert_eq!(
            doc.working().ink_of(id),
            None,
            "the removed mark still names a version"
        );
    }

    const GREEN: [f32; 3] = [0.35, 0.8, 0.35];

    #[test]
    fn a_mark_nobody_has_recoloured_answers_out_of_its_body() {
        // The fallback arm of `color_of`, which every existing mark takes and
        // which no other test here reaches on purpose.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc.annotate(mark_on(page), String::new()).expect("marked");
        assert_eq!(doc.color_of(id), doc.mark(id).expect("body").color);
        assert_eq!(doc.color_bodies(), 0, "and no version was recorded");
    }

    #[test]
    fn recolouring_changes_what_a_mark_is_drawn_in_and_undo_puts_it_back() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc.annotate(mark_on(page), String::new()).expect("marked");
        let made_in = doc.color_of(id);
        assert_ne!(made_in, GREEN, "the fixture must not already be green");

        doc.recolor(id, GREEN).expect("recoloured");
        assert_eq!(doc.color_of(id), GREEN);
        // The body is untouched, which is the whole of why this is undoable:
        // `Mark` is written once and `Working` is what replay rebuilds.
        assert_eq!(doc.mark(id).expect("body").color, made_in);

        assert!(doc.undo());
        assert_eq!(doc.color_of(id), made_in, "undo left the new colour on");
        assert!(doc.redo());
        assert_eq!(doc.color_of(id), GREEN);
    }

    #[test]
    fn every_kind_can_be_recoloured_including_the_one_that_cannot_be_erased() {
        // The property that separates this command from `Reink`: a colour is
        // `/C` and every kind has one, so there is no shape check to fail.
        // Ink is the discriminating case --- it is the kind `reink` accepts ---
        // so a highlight beside it is what says the rule is about colour rather
        // than about strokes.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let wash = doc.annotate(mark_on(page), String::new()).expect("marked");
        let drawn = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");

        doc.recolor(wash, GREEN).expect("a highlight takes it");
        doc.recolor(drawn, GREEN).expect("a drawing takes it too");
        assert_eq!(doc.color_of(wash), GREEN);
        assert_eq!(doc.color_of(drawn), GREEN);
        // And the kind that a *stroke* command refuses still refuses it, so the
        // absence of a check here is this command's and not a hole in that one.
        assert_eq!(
            doc.reink(wash, vec![doc.strokes_of(drawn)[0].clone()]),
            Err(Refusal::ShapeMismatch(MarkKind::Highlight))
        );
    }

    #[test]
    fn recolouring_a_mark_that_is_not_there_is_refused_before_an_id_is_spent() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc.annotate(mark_on(page), String::new()).expect("marked");
        doc.apply(Command::Unannotate { mark: id })
            .expect("removed");

        let held = doc.color_bodies();
        assert_eq!(doc.recolor(id, GREEN), Err(Refusal::MarkRemoved(id)));
        assert_eq!(
            doc.color_bodies(),
            held,
            "a refused colour spends no version, the way a refused mark spends no id"
        );
        assert_eq!(
            doc.recolor(MarkId(9999), GREEN),
            Err(Refusal::NoSuchMark(MarkId(9999)))
        );
    }

    #[test]
    fn a_removed_mark_forgets_which_colour_it_was_on() {
        // `Unannotate` drops the entry, as it does for the note and the strokes.
        // Without that, a mark removed and restored by undo would come back in
        // whatever colour a recolour had left it rather than the journal's.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc.annotate(mark_on(page), String::new()).expect("marked");
        doc.recolor(id, GREEN).expect("recoloured");
        doc.apply(Command::Unannotate { mark: id })
            .expect("removed");
        assert_eq!(
            doc.working().color_of(id),
            None,
            "the removed mark still names a version"
        );
    }

    #[test]
    fn a_colour_in_the_discarded_redo_tail_goes_with_it() {
        // The leak `color_bodies` exists to see --- written at the same time as
        // the table rather than a week later, which is the whole reason the
        // drawing's version of this test found a real one.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc.annotate(mark_on(page), String::new()).expect("marked");
        doc.recolor(id, GREEN).expect("kept");
        doc.recolor(id, [0.3, 0.6, 0.95]).expect("discarded");
        assert_eq!(doc.color_bodies(), 2, "two versions were recorded");

        assert!(doc.undo());
        doc.apply(Command::Rotate { page, turns: 1 })
            .expect("this discards the tail");

        assert_eq!(doc.color_bodies(), 1, "the discarded version was kept");
        assert_eq!(doc.color_of(id), GREEN);
    }

    #[test]
    fn a_kind_and_a_shape_that_disagree_are_refused_both_ways_round() {
        // The biconditional [`Mark::strokes`] states. Neither half is reachable
        // from the window --- the viewer sends strokes for ink and quads for
        // everything else --- so this is the only place the rule can fail, and
        // a rule with no failing case is a comment.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];

        let mut ink_without = ink_on(page);
        ink_without.strokes.clear();
        assert_eq!(
            doc.annotate(ink_without, String::new()),
            Err(Refusal::ShapeMismatch(MarkKind::Ink))
        );

        let mut highlight_with = mark_on(page);
        highlight_with.strokes = ink_on(page).strokes;
        assert_eq!(
            doc.annotate(highlight_with, String::new()),
            Err(Refusal::ShapeMismatch(MarkKind::Highlight))
        );

        // The control: after both refusals the model still takes a real
        // drawing. Without it, a model that refused every ink mark would pass
        // the first assertion and read as the rule working.
        assert!(doc.annotate(ink_on(page), String::new()).is_ok());
        assert_eq!(doc.marks_issued(), 1, "a refused mark spent an id");
    }

    #[test]
    fn a_kind_and_a_stamp_name_that_disagree_are_refused_both_ways_round() {
        // [`Mark::stamp`]'s biconditional, and its own test rather than a third
        // case in the one above: they are two rules about two fields, and a
        // mark can break either without breaking the other. A shared test would
        // pass while one of them was unchecked.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];

        let mut stamp_without = mark_on(page);
        stamp_without.kind = MarkKind::Stamp;
        assert_eq!(
            doc.annotate(stamp_without, String::new()),
            Err(Refusal::StampMismatch(MarkKind::Stamp))
        );

        let mut highlight_with = mark_on(page);
        highlight_with.stamp = Some(StampName::Draft);
        assert_eq!(
            doc.annotate(highlight_with, String::new()),
            Err(Refusal::StampMismatch(MarkKind::Highlight))
        );

        // The control, for the reason the test above gives: a model that
        // refused every stamp would pass the first assertion and read as the
        // rule working.
        let mut real = mark_on(page);
        real.kind = MarkKind::Stamp;
        real.stamp = Some(StampName::Approved);
        assert!(doc.annotate(real, String::new()).is_ok());
        assert_eq!(doc.marks_issued(), 1, "a refused mark spent an id");
    }

    /// A parent on a mark that cannot answer one is refused, and a comment
    /// answering one is not.
    #[test]
    fn only_a_comment_may_answer_a_comment() {
        // [`Mark::reply_to`]'s rule, and its own test for the reason the stamp
        // one above gives. **One direction only**, which is the thing to be
        // careful about: a comment with no parent is an ordinary sticky note and
        // is what almost every comment is, so there is nothing to refuse there
        // --- and a test asserting both halves would be asserting something the
        // model deliberately does not do.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let parent = ObjectId::new(12, 0);

        let mut highlight_answering = mark_on(page);
        highlight_answering.reply_to = Some(parent);
        assert_eq!(
            doc.annotate(highlight_answering, String::new()),
            Err(Refusal::ReplyMismatch(MarkKind::Highlight))
        );

        // The control the entry above is named for: a model that refused every
        // parent would pass the assertion above and read as the rule working.
        let mut reply = mark_on(page);
        reply.kind = MarkKind::Note;
        reply.reply_to = Some(parent);
        let id = doc
            .annotate(reply, "I answer it.".to_string())
            .expect("reply");
        assert_eq!(
            doc.mark(id).expect("body").reply_to,
            Some(parent),
            "the parent did not survive into the mark's body"
        );

        // The other control, and the half the rule must *not* refuse: an
        // ordinary comment, with nothing to answer.
        let mut alone = mark_on(page);
        alone.kind = MarkKind::Note;
        assert!(doc.annotate(alone, String::new()).is_ok());

        assert_eq!(doc.marks_issued(), 2, "a refused mark spent an id");
    }

    #[test]
    fn ink_that_never_moved_is_refused_though_its_rectangle_covers_area() {
        // **Not the same refusal as an empty quad, and this is what a padded
        // rectangle would let through.** `Stroke::bounds` grows the quad by half
        // a line width, so a stroke standing still still covers area --- which
        // is exactly why `annotate` asks `is_drawable` for ink rather than
        // `covers_area`. Asserted here rather than only in the harness, because
        // the reading that makes it interesting is the second one: the quad
        // this mark carries is a perfectly good rectangle.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];

        let still = vec![Stroke {
            points: vec![Point { x: 50.0, y: 50.0 }; 3],
        }];
        let quads: Vec<Quad> = Stroke::bounds(&still, 1.25).into_iter().collect();
        assert!(
            quads.iter().all(|quad| quad.covers_area()),
            "the padded rectangle covers area, which is the whole point"
        );

        let mut mark = ink_on(page);
        mark.strokes = still;
        mark.quads = quads;
        assert_eq!(doc.annotate(mark, String::new()), Err(Refusal::EmptyMark));
        assert_eq!(doc.marks_issued(), 0);
    }

    #[test]
    fn a_straight_stroke_is_accepted_because_its_bounds_are_padded() {
        // The control for the padding, and the reason it exists: a reader ruling
        // a straight line down a margin produces bounds of no width, which
        // `covers_area` rejects. Without the pad this is refused and the reader
        // is told their drawing covers no area.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];

        let vertical = vec![Stroke {
            points: vec![Point { x: 50.0, y: 50.0 }, Point { x: 50.0, y: 300.0 }],
        }];
        let tight = Stroke::bounds(&vertical, 0.0).expect("bounds");
        assert!(
            !tight.covers_area(),
            "unpadded bounds of a vertical line have no width"
        );

        let mut mark = ink_on(page);
        mark.quads = Stroke::bounds(&vertical, 1.25).into_iter().collect();
        mark.strokes = vertical;
        assert!(doc.annotate(mark, String::new()).is_ok());
    }

    #[test]
    fn a_mark_covering_nothing_is_refused_and_spends_no_id() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];

        let mut empty = mark_on(page);
        empty.quads.clear();
        assert_eq!(doc.annotate(empty, String::new()), Err(Refusal::EmptyMark));

        // A quad with no area, which is what a click rather than a drag
        // produces, and what a selection collapsed to a caret produces.
        let mut degenerate = mark_on(page);
        degenerate.quads = vec![Quad {
            left: 100.0,
            top: 100.0,
            right: 100.0,
            bottom: 140.0,
        }];
        assert_eq!(
            doc.annotate(degenerate, String::new()),
            Err(Refusal::EmptyMark)
        );

        // A `NaN` corner, which no comparison accepts -- the same reason
        // `Rect::is_proper` refuses one, and worth its own case because it
        // arrives from arithmetic rather than from a click.
        let mut nan = mark_on(page);
        nan.quads = vec![Quad {
            left: f32::NAN,
            top: 100.0,
            right: 300.0,
            bottom: 140.0,
        }];
        assert_eq!(doc.annotate(nan, String::new()), Err(Refusal::EmptyMark));

        assert_eq!(doc.marks_issued(), 0, "a refused mark spent an id");
        assert_eq!(doc.mark_bodies(), 0);
        assert!(!doc.can_undo(), "a refusal reached the journal");
    }

    #[test]
    fn one_quad_with_area_is_enough() {
        // The control for the three refusals above, and it is not a formality:
        // a selection that runs off the end of a line yields a real rectangle
        // followed by an empty one, so an `all` where the code says `any` would
        // refuse ordinary highlights and pass every test in the case above.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let mut mixed = mark_on(page);
        mixed.quads.push(Quad {
            left: 300.0,
            top: 90.0,
            right: 300.0,
            bottom: 108.0,
        });
        assert!(doc.annotate(mixed, String::new()).is_ok());
    }

    #[test]
    fn a_mark_on_a_page_that_is_not_there_is_refused_by_the_page() {
        let mut doc = Doc::open(2);
        let gone = doc.working().order()[0];
        doc.apply(Command::Delete { page: gone }).expect("delete");

        assert_eq!(
            doc.annotate(mark_on(gone), String::new()),
            Err(Refusal::PageDeleted(gone))
        );
        let never = PageId::from_raw(99);
        assert_eq!(
            doc.annotate(mark_on(never), String::new()),
            Err(Refusal::NoSuchPage(never))
        );
    }

    #[test]
    fn deleting_a_page_takes_its_marks_and_undo_brings_both_back() {
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(mark_on(page), String::new())
            .expect("annotate");

        doc.apply(Command::Delete { page }).expect("delete");
        assert!(doc.working().page_of(id).is_none());
        assert!(doc.working().all_marks().is_empty());

        // Naming it now says it was removed, not that it never existed. The
        // first version of this model left the id out of the tombstones and
        // answered `NoSuchMark`, which is a wrong diagnosis rather than a coarse
        // one.
        assert_eq!(
            doc.apply(Command::Unannotate { mark: id }),
            Err(Refusal::MarkRemoved(id))
        );

        assert!(doc.undo());
        assert_eq!(doc.working().marks_on(page), [id]);
    }

    #[test]
    fn removing_a_mark_twice_says_so() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(mark_on(page), String::new())
            .expect("annotate");

        doc.apply(Command::Unannotate { mark: id }).expect("remove");
        assert!(doc.working().marks_on(page).is_empty());
        assert_eq!(
            doc.apply(Command::Unannotate { mark: id }),
            Err(Refusal::MarkRemoved(id))
        );
        assert_eq!(
            doc.apply(Command::Unannotate {
                mark: MarkId::from_raw(4242)
            }),
            Err(Refusal::NoSuchMark(MarkId::from_raw(4242)))
        );
    }

    #[test]
    fn a_document_annotated_and_cleared_compares_equal_to_one_that_never_was() {
        // Not a nicety about `PartialEq`: a snapshot is a clone of `Working` and
        // a rebuild is checked against one. An empty vector left behind under a
        // page key makes two identical documents unequal, which turns every
        // assertion built on a snapshot into one that passes for the wrong
        // reason -- or fails for none.
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(mark_on(page), String::new())
            .expect("annotate");
        doc.apply(Command::Unannotate { mark: id }).expect("remove");

        let mut untouched = Doc::open(2);
        // Something in the journal, so the two differ in history and not in
        // state -- otherwise this compares two documents nothing happened to.
        untouched
            .apply(Command::Rotate {
                page: untouched.working().order()[0],
                turns: 4,
            })
            .expect("a full turn is a no-op");
        assert_eq!(doc.working().marks, untouched.working().marks);
    }

    #[test]
    fn marks_come_back_in_page_order_after_the_pages_move() {
        // `all_marks` walks the order rather than the map, which is what makes
        // it answer in reading order -- and a `HashMap` iteration would pass a
        // one-page test forever.
        let mut doc = Doc::open(3);
        let [a, b, c] = [
            doc.working().order()[0],
            doc.working().order()[1],
            doc.working().order()[2],
        ];
        let on_a = doc.annotate(mark_on(a), String::new()).expect("a");
        let on_c = doc.annotate(mark_on(c), String::new()).expect("c");
        assert_eq!(doc.working().all_marks(), vec![(a, on_a), (c, on_c)]);

        // Put c first. The marks must follow their pages.
        doc.apply(Command::Move {
            page: c,
            after: None,
        })
        .expect("move");
        assert_eq!(doc.working().order(), [c, a, b]);
        assert_eq!(doc.working().all_marks(), vec![(c, on_c), (a, on_a)]);
    }

    #[test]
    fn a_mark_survives_the_snapshot_boundary() {
        // Undo past a snapshot rebuilds from the clone rather than from the
        // baseline, so a `Working` that did not carry its marks would lose them
        // here and nowhere else. `SNAPSHOT_EVERY` commands of padding puts the
        // mark on the far side of one.
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(mark_on(page), "what it said".to_string())
            .expect("annotate");
        for _ in 0..SNAPSHOT_EVERY {
            doc.apply(Command::Rotate { page, turns: 1 }).expect("turn");
        }
        assert!(
            doc.snapshots() > 0,
            "no snapshot was taken, so this tests nothing"
        );
        assert!(
            doc.replay_base(doc.depth().0) > 0,
            "the rebuild would not use one"
        );

        assert!(doc.undo());
        assert_eq!(
            doc.working().marks_on(page),
            [id],
            "the mark did not survive a rebuild from a snapshot"
        );
        // And what it said, which is the other half of `Working` and the half a
        // snapshot could plausibly be built without: the mark is in the map that
        // was already there, the note is in the one this increment added.
        assert_eq!(doc.note_of(id), "what it said");
    }

    // --- Notes ---------------------------------------------------------------

    #[test]
    fn a_mark_says_what_it_was_last_told_to_say() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(mark_on(page), String::new())
            .expect("annotate");
        assert_eq!(doc.note_of(id), "", "a fresh mark said something");

        doc.renote(id, "check this".to_string()).expect("note it");
        assert_eq!(doc.note_of(id), "check this");
        doc.renote(id, "checked".to_string())
            .expect("note it again");
        assert_eq!(doc.note_of(id), "checked");
    }

    #[test]
    fn undo_takes_a_note_back_to_what_it_said_before() {
        // The property the whole `NoteId` arrangement exists for. A note held on
        // the mark's body would satisfy every assertion in the test above and
        // fail every one in this one, because undo rebuilds the working document
        // and touches no body.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(mark_on(page), String::new())
            .expect("annotate");
        doc.renote(id, "first".to_string()).expect("first");
        doc.renote(id, "second".to_string()).expect("second");

        assert!(doc.undo());
        assert_eq!(doc.note_of(id), "first");
        assert!(doc.undo());
        assert_eq!(doc.note_of(id), "", "the note before the first was empty");
        assert!(doc.redo());
        assert_eq!(doc.note_of(id), "first");
        assert!(doc.redo());
        assert_eq!(doc.note_of(id), "second");
        // The mark itself never moved, which is what says the undos above were
        // about the note rather than about the highlight.
        assert_eq!(doc.working().marks_on(page), [id]);
    }

    #[test]
    fn a_note_names_a_mark_and_is_refused_by_name() {
        let mut doc = Doc::open(2);
        let [page, other] = [doc.working().order()[0], doc.working().order()[1]];
        let id = doc
            .annotate(mark_on(page), String::new())
            .expect("annotate");
        let never = MarkId::from_raw(4242);
        assert_eq!(
            doc.renote(never, "hello".to_string()),
            Err(Refusal::NoSuchMark(never))
        );

        doc.apply(Command::Unannotate { mark: id }).expect("remove");
        assert_eq!(
            doc.renote(id, "hello".to_string()),
            Err(Refusal::MarkRemoved(id)),
            "a removed mark answered as one that never existed"
        );

        // And a mark that went with its page answers the same way, which is the
        // distinction `mark_graves` was widened to keep -- see its own note.
        let on_other = doc.annotate(mark_on(other), String::new()).expect("second");
        doc.apply(Command::Delete { page: other }).expect("delete");
        assert_eq!(
            doc.renote(on_other, "hello".to_string()),
            Err(Refusal::MarkRemoved(on_other))
        );
    }

    #[test]
    fn a_mark_that_is_removed_says_nothing() {
        // The keys of the note map are meant to be exactly the live marks, and a
        // leftover is invisible everywhere else: the mark is gone from the page,
        // out of every list, and out of anything written. This is the only
        // observable it has.
        //
        // The obvious stronger assertion -- that the working document now equals
        // one nobody annotated -- is *false* and was written here first: the
        // mark's tombstone stays on purpose, so that naming the mark again is
        // answered truthfully rather than as an id nobody issued.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(mark_on(page), String::new())
            .expect("annotate");
        doc.renote(id, "for now".to_string()).expect("note it");

        doc.apply(Command::Unannotate { mark: id })
            .expect("remove it");
        assert_eq!(doc.note_of(id), "");

        // And the note comes back with the mark, which is what says the line
        // above removed an entry rather than the text behind it.
        assert!(doc.undo());
        assert_eq!(doc.note_of(id), "for now");
    }

    #[test]
    fn a_refused_note_spends_nothing() {
        // The same accounting `annotate` states for ids: a refusal must leave the
        // document exactly as it was, and an id issued before the check is a
        // version of a note nobody can ever read.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(mark_on(page), String::new())
            .expect("annotate");
        let (before, held) = (doc.depth(), doc.note_bodies());

        assert!(doc.renote(MarkId::from_raw(99), "x".to_string()).is_err());

        assert_eq!(doc.depth(), before, "a refused note reached the journal");
        assert_eq!(doc.note_bodies(), held, "a refused note kept its text");
        assert_eq!(doc.note_of(id), "");
    }

    #[test]
    fn a_drawing_in_the_discarded_redo_tail_goes_with_it() {
        // The note's test below, for the eraser's table --- and it was written
        // second and went red, which is why it exists rather than being assumed
        // from the symmetry. `ink_bodies` was added with the eraser as the
        // accounting observable for exactly this and nothing read it, so a
        // reader erasing and undoing in a loop grew the table forever with no
        // assertion over the working document able to see it.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let strokes = doc.strokes_of(id).to_vec();
        doc.reink(id, strokes.clone()).expect("kept");
        doc.reink(id, strokes).expect("discarded");
        assert_eq!(doc.ink_bodies(), 2, "two versions were recorded");

        assert!(doc.undo());
        doc.apply(Command::Rotate { page, turns: 1 })
            .expect("this discards the tail");

        assert_eq!(doc.ink_bodies(), 1, "the discarded version was kept");
    }

    #[test]
    fn a_note_in_the_discarded_redo_tail_goes_with_it() {
        // The leak `note_bodies` exists to see. Two notes typed, one undone, and
        // then a command that discards the tail: the undone version is reachable
        // by nothing afterwards, and no assertion over the working document
        // could tell it was still held.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(mark_on(page), String::new())
            .expect("annotate");
        doc.renote(id, "kept".to_string()).expect("kept");
        doc.renote(id, "discarded".to_string()).expect("discarded");
        assert_eq!(doc.note_bodies(), 3, "the empty note counts too");

        assert!(doc.undo());
        doc.apply(Command::Rotate { page, turns: 1 })
            .expect("this discards the tail");

        assert_eq!(doc.note_bodies(), 2, "the discarded version was kept");
        assert_eq!(doc.note_of(id), "kept");
    }

    #[test]
    fn a_marks_note_goes_with_it_and_comes_back_with_it() {
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(mark_on(page), String::new())
            .expect("annotate");
        doc.renote(id, "still here".to_string()).expect("note it");

        doc.apply(Command::Delete { page })
            .expect("delete the page");
        assert_eq!(
            doc.note_of(id),
            "",
            "a mark that is on no page still had something to say"
        );

        assert!(doc.undo());
        assert_eq!(
            doc.note_of(id),
            "still here",
            "the page came back without what was written on it"
        );
    }

    /// A region to mark for removal, on `page`.
    fn region_on(page: PageId) -> Redaction {
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

    #[test]
    fn a_region_marked_for_removal_lands_on_the_page_it_names() {
        let mut doc = Doc::open(3);
        let second = doc.working().order()[1];
        let id = doc.redact(region_on(second)).expect("redact");

        assert_eq!(doc.working().redactions_on(second), [id]);
        assert_eq!(doc.working().page_of_redaction(id), Some(second));
        // The control a `redactions_on` ignoring its argument would fail.
        assert!(doc
            .working()
            .redactions_on(doc.working().order()[0])
            .is_empty());
        assert_eq!(doc.redaction(id).expect("body").page, second);
    }

    #[test]
    fn a_pending_redaction_is_not_a_mark_and_a_mark_is_not_a_pending_redaction() {
        // **The property the whole two-type arrangement exists for**, asserted
        // rather than left to the type system to be trusted about. The two
        // populations are separate at every accessor, so nothing that walks the
        // marks --- which is what `save.rs` does --- can reach a redaction.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let mark = doc.annotate(mark_on(page), String::new()).expect("mark");
        let region = doc.redact(region_on(page)).expect("redact");

        assert_eq!(doc.working().marks_on(page), [mark]);
        assert_eq!(doc.working().redactions_on(page), [region]);
        assert_eq!(doc.working().all_marks(), vec![(page, mark)]);
        assert_eq!(doc.working().all_redactions(), vec![(page, region)]);
        // The two id spaces count independently, so the raw numbers collide ---
        // which is exactly why they are different types and why nothing may
        // look one up in the other table.
        assert_eq!(mark.get(), region.get());
    }

    #[test]
    fn a_region_covering_nothing_is_refused_rather_than_listed() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let mut flat = region_on(page);
        flat.area.bottom = flat.area.top;

        assert_eq!(doc.redact(flat), Err(Refusal::EmptyRedaction));
        // Nothing was spent: the id is issued after the check, so the next
        // acceptable region gets the first number.
        assert_eq!(doc.redaction_bodies(), 0);
        assert!(!doc.can_undo());
        assert_eq!(doc.redact(region_on(page)).expect("redact").get(), 1);
    }

    #[test]
    fn a_region_with_a_corner_that_is_not_a_number_covers_nothing() {
        // `Quad::covers_area` excludes NaN, and this is the redaction path
        // asserting it rather than inheriting the claim: a region whose corner
        // is NaN compares false against every object on the page, so accepting
        // one would put a row in the review list that removes nothing.
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let mut bad = region_on(page);
        bad.area.right = f32::NAN;

        assert_eq!(doc.redact(bad), Err(Refusal::EmptyRedaction));
    }

    #[test]
    fn undo_takes_a_pending_redaction_off_and_redo_puts_the_same_one_back() {
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        let id = doc.redact(region_on(page)).expect("redact");

        assert!(doc.undo());
        assert!(doc.working().redactions_on(page).is_empty());
        assert!(doc.redo());
        // The same id, not a fresh one wearing its number --- the property the
        // allocator carries and the reason a redaction is journalled at all.
        assert_eq!(doc.working().redactions_on(page), [id]);
    }

    #[test]
    fn removing_a_pending_redaction_twice_is_told_which_answer_it_is_getting() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc.redact(region_on(page)).expect("redact");

        assert_eq!(doc.unredact(id), Ok(()));
        assert!(doc.working().redactions_on(page).is_empty());
        assert_eq!(doc.unredact(id), Err(Refusal::RedactionRemoved(id)));
        // And an id nobody issued is the other diagnosis, which is the whole
        // reason the two variants are separate.
        assert_eq!(
            doc.unredact(RedactionId::from_raw(999)),
            Err(Refusal::NoSuchRedaction(RedactionId::from_raw(999)))
        );
    }

    #[test]
    fn deleting_a_page_takes_its_pending_redactions_with_it() {
        let mut doc = Doc::open(2);
        let first = doc.working().order()[0];
        let id = doc.redact(region_on(first)).expect("redact");

        doc.apply(Command::Delete { page: first }).expect("delete");
        assert!(doc.working().page_of_redaction(id).is_none());
        // The diagnosis a reader gets afterwards is "removed", not "never
        // existed" --- the same wrong-diagnosis trap `mark_graves` records, and
        // sharper here: a redaction left behind would be an instruction about a
        // page nobody can see.
        assert_eq!(doc.unredact(id), Err(Refusal::RedactionRemoved(id)));
        // And undo brings page and instruction back together, because replay
        // rebuilds rather than inverts.
        assert!(doc.undo());
        assert_eq!(doc.working().redactions_on(first), [id]);
    }

    #[test]
    fn a_pending_redaction_survives_a_rebuild_across_a_snapshot_boundary() {
        // **The test with a failing case for `Redact`'s replay.** Undo rewinds
        // by rebuilding from the nearest snapshot forward, so a command whose
        // arm is right in `apply` and absent from replay would be correct on
        // every short journal and lose the redaction on a long one --- and a
        // redaction silently missing from the review list is the one thing this
        // subsystem must never do. SNAPSHOT_EVERY is 32, so the journal has to
        // be longer than that for a snapshot to exist at all.
        let mut doc = Doc::open(3);
        let page = doc.working().order()[0];
        let id = doc.redact(region_on(page)).expect("redact");
        for _ in 0..(SNAPSHOT_EVERY * 2 + 5) {
            doc.apply(Command::Rotate { page, turns: 1 }).expect("turn");
        }
        assert!(doc.snapshots() >= 2, "the test needs snapshots to exist");

        // Every step back, including the two that cross a snapshot boundary.
        while doc.depth().0 > 1 {
            assert!(doc.undo());
            assert_eq!(
                doc.working().redactions_on(page),
                [id],
                "the redaction went missing at cursor {}",
                doc.depth().0
            );
        }
        assert!(doc.undo());
        assert!(doc.working().redactions_on(page).is_empty());
    }

    #[test]
    fn a_discarded_redo_tail_drops_the_redaction_bodies_it_named() {
        // The accounting observable, and the reason there is one: a body kept
        // after its command was discarded produces a document identical to one
        // where it was dropped, so nothing about the working state can see it.
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        doc.redact(region_on(page)).expect("redact");
        assert_eq!(doc.redaction_bodies(), 1);

        assert!(doc.undo());
        doc.apply(Command::Rotate { page, turns: 1 }).expect("turn");
        assert_eq!(doc.redaction_bodies(), 0);
    }

    #[test]
    fn pending_redactions_are_listed_in_page_order_rather_than_in_drag_order() {
        // Page order is what the review list reads in --- a reader checks the
        // document, not the sequence they happened to drag --- and a move is
        // what tells the two apart.
        let mut doc = Doc::open(3);
        let [first, second, third] = [0, 1, 2].map(|i| doc.working().order()[i]);
        let on_third = doc.redact(region_on(third)).expect("redact");
        let on_first = doc.redact(region_on(first)).expect("redact");

        assert_eq!(
            doc.working().all_redactions(),
            vec![(first, on_first), (third, on_third)]
        );
        doc.apply(Command::Move {
            page: third,
            after: None,
        })
        .expect("move");
        assert_eq!(
            doc.working().all_redactions(),
            vec![(third, on_third), (first, on_first)]
        );
        assert!(doc.working().redactions_on(second).is_empty());
    }

    #[test]
    fn a_region_on_a_page_that_is_gone_says_which_of_the_two_it_is() {
        let mut doc = Doc::open(2);
        let first = doc.working().order()[0];
        doc.apply(Command::Delete { page: first }).expect("delete");

        assert_eq!(
            doc.redact(region_on(first)),
            Err(Refusal::PageDeleted(first))
        );
        let ghost = PageId::from_raw(9_999);
        assert_eq!(
            doc.redact(region_on(ghost)),
            Err(Refusal::NoSuchPage(ghost))
        );
        // Neither refusal spent an id, so the next acceptable region is the
        // first one.
        assert_eq!(doc.redaction_bodies(), 0);
    }

    /// A comment out of the file, for the rewrite tests.
    fn object(number: u32) -> ObjectId {
        ObjectId::new(number, 0)
    }

    /// Rewriting a foreign comment is undone by replaying without it.
    ///
    /// The property the journal exists for, asked of the one command whose
    /// subject the model did not make. What makes it worth a test of its own is
    /// that "before" is *not a value the model holds*: a mark's note reverts to
    /// the previous `Renote` or to the `Annotate` that made it, and there is no
    /// `Annotate` here --- the comment came out of the file, which this model has
    /// never read. So undoing the first rewrite has to leave the model with
    /// **nothing to say**, which is what makes a save write the file's own text.
    #[test]
    fn undoing_the_first_rewrite_leaves_the_model_with_nothing_to_say() {
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        doc.rewrite(object(7), page, "mine".into(), "D:2026".into())
            .expect("rewrite");
        doc.rewrite(object(7), page, "mine, again".into(), "D:2027".into())
            .expect("rewrite");
        assert_eq!(
            doc.rewrite_of(object(7)).map(|edit| edit.body.as_str()),
            Some("mine, again")
        );

        assert!(doc.undo());
        assert_eq!(
            doc.rewrite_of(object(7)).map(|edit| edit.body.as_str()),
            Some("mine")
        );
        assert!(doc.undo());
        assert_eq!(doc.rewrite_of(object(7)), None);

        // And forward again, to the same two bodies: the ids were never
        // re-issued, so redo restores what each command named.
        assert!(doc.redo());
        assert_eq!(
            doc.rewrite_of(object(7)).map(|edit| edit.body.as_str()),
            Some("mine")
        );
        assert!(doc.redo());
        assert_eq!(
            doc.rewrite_of(object(7)).map(|edit| edit.made.as_str()),
            Some("D:2027")
        );
    }

    /// Deleting a page takes the rewrites on it, and leaves the others.
    ///
    /// The reason [`Command::Rewrite`] carries a page at all. Without it the
    /// model would hand a save an edit for a comment the file being written does
    /// not contain --- and the two-page fixture is what makes the assertion say
    /// that rather than "the map was emptied".
    #[test]
    fn deleting_a_page_takes_the_rewrites_on_it_and_no_others() {
        let mut doc = Doc::open(2);
        let [first, second] = [0, 1].map(|i| doc.working().order()[i]);
        doc.rewrite(object(7), first, "on the first".into(), "D:2026".into())
            .expect("rewrite");
        doc.rewrite(object(9), second, "on the second".into(), "D:2026".into())
            .expect("rewrite");

        doc.apply(Command::Delete { page: first }).expect("delete");
        assert_eq!(doc.rewrite_of(object(7)), None);
        assert_eq!(
            doc.rewrite_of(object(9)).map(|edit| edit.body.as_str()),
            Some("on the second")
        );

        // Undo brings page and edit back together, because the journal replays
        // rather than inverts --- the same property `Delete` has for marks.
        assert!(doc.undo());
        assert_eq!(
            doc.rewrite_of(object(7)).map(|edit| edit.body.as_str()),
            Some("on the first")
        );
    }

    /// Deleting a foreign comment is undone by replaying without it.
    ///
    /// [`Command::Rewrite`]'s twin and the same property: the journal replays
    /// rather than inverts, so undo is the model rebuilt without this command
    /// and the comment comes back because nothing ever removed it from the file.
    #[test]
    fn discarding_a_foreign_comment_is_undone_by_replaying_without_it() {
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        doc.discard(object(7), page).expect("discard");
        assert!(doc.working().is_discarded(object(7)));

        assert!(doc.undo());
        assert!(
            !doc.working().is_discarded(object(7)),
            "undo left the comment deleted"
        );
        assert!(doc.redo());
        assert!(doc.working().is_discarded(object(7)));
    }

    /// Deleting a page takes the discards on it, and leaves the others.
    ///
    /// [`Command::Rewrite`]'s page argument exists for this and so does this
    /// one's: without it the model would hand a save a deletion for a comment the
    /// file being written does not contain. The two-page fixture is what makes
    /// the assertion say that rather than "the map was emptied".
    #[test]
    fn deleting_a_page_takes_the_discards_on_it_and_no_others() {
        let mut doc = Doc::open(2);
        let [first, second] = [0, 1].map(|i| doc.working().order()[i]);
        doc.discard(object(7), first).expect("discard");
        doc.discard(object(9), second).expect("discard");

        doc.apply(Command::Delete { page: first }).expect("delete");
        assert!(!doc.working().is_discarded(object(7)));
        assert!(
            doc.working().is_discarded(object(9)),
            "the other page's discard went with it"
        );

        assert!(doc.undo());
        assert!(
            doc.working().is_discarded(object(7)),
            "undo brings page and discard back together"
        );
    }

    /// A comment can be rewritten and then deleted, and both are recorded.
    ///
    /// The reason `discards` is a second map rather than a state inside
    /// `Rewritten`. Undoing the deletion has to leave the comment reading what
    /// the reader typed, not what the file said --- and it can only do that if
    /// the rewrite was still there while the deletion stood.
    #[test]
    fn a_comment_can_be_rewritten_and_then_discarded_and_undo_keeps_the_edit() {
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        doc.rewrite(object(7), page, "mine".into(), "D:2026".into())
            .expect("rewrite");
        doc.discard(object(7), page).expect("discard");

        assert!(doc.working().is_discarded(object(7)));
        assert_eq!(
            doc.rewrite_of(object(7)).map(|edit| edit.body.as_str()),
            Some("mine"),
            "the edit was thrown away when the comment was deleted"
        );

        assert!(doc.undo());
        assert!(!doc.working().is_discarded(object(7)));
        assert_eq!(
            doc.rewrite_of(object(7)).map(|edit| edit.body.as_str()),
            Some("mine"),
            "undoing the deletion brought back the file's text, not the reader's"
        );
    }

    /// A comment one of the reader's own replies answers cannot be deleted.
    ///
    /// **The one refusal this command needs that no other in its family does**,
    /// and it exists because replying landed first: `/IRT` names the comment by
    /// object, so taking the comment away leaves the reply pointing at nothing.
    ///
    /// The control is the whole of it. Without a comment that *is* deletable in
    /// the same document, every assertion here is satisfied by a method that
    /// refuses everything --- and the two objects differ in exactly one thing,
    /// which is whether a reply names them.
    #[test]
    fn a_comment_a_reply_answers_cannot_be_discarded() {
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        doc.annotate(
            Mark {
                kind: MarkKind::Note,
                reply_to: Some(object(7)),
                ..mark_on(page)
            },
            "answering".into(),
        )
        .expect("a reply");

        assert_eq!(
            doc.discard(object(7), page),
            Err(Refusal::ReplyAnswersIt(object(7)))
        );
        assert!(
            !doc.working().is_discarded(object(7)),
            "a refusal changed the model"
        );

        // The control: the same document, the same page, an object no reply
        // names.
        doc.discard(object(9), page).expect("nothing answers 9");
        assert!(doc.working().is_discarded(object(9)));
    }

    /// Removing the reply makes the comment deletable again.
    ///
    /// The refusal's own advice, executed: it tells a reader to delete their
    /// reply first, and a refusal that stayed after they did would be telling
    /// them to do something that does not work. **Over the live marks, which is
    /// what makes this pass** --- a scan over every mark ever made would still
    /// find the removed reply and refuse for ever.
    #[test]
    fn removing_the_reply_makes_the_comment_deletable_again() {
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        let reply = doc
            .annotate(
                Mark {
                    kind: MarkKind::Note,
                    reply_to: Some(object(7)),
                    ..mark_on(page)
                },
                "answering".into(),
            )
            .expect("a reply");
        assert!(
            doc.discard(object(7), page).is_err(),
            "refused while it is there"
        );

        doc.apply(Command::Unannotate { mark: reply })
            .expect("remove the reply");
        doc.discard(object(7), page).expect("deletable now");
        assert!(doc.working().is_discarded(object(7)));
    }

    /// A rewrite on a page that is gone says which of the two it is, and spends
    /// nothing.
    ///
    /// The page is the *only* thing this layer can refuse a rewrite for --- the
    /// object is a name out of a file the model has never parsed --- so the
    /// diagnosis has to be the page's. The body count is the second half: a
    /// refusal that had issued an id first would leave a version nothing can
    /// ever reach, and no document would look any different.
    #[test]
    fn a_rewrite_on_a_page_that_is_gone_says_which_of_the_two_it_is() {
        let mut doc = Doc::open(2);
        let first = doc.working().order()[0];
        doc.apply(Command::Delete { page: first }).expect("delete");

        assert_eq!(
            doc.rewrite(object(7), first, "mine".into(), "D:2026".into()),
            Err(Refusal::PageDeleted(first))
        );
        let ghost = PageId::from_raw(9_999);
        assert_eq!(
            doc.rewrite(object(7), ghost, "mine".into(), "D:2026".into()),
            Err(Refusal::NoSuchPage(ghost))
        );
        assert_eq!(doc.rewrite_bodies(), 0);
    }

    /// Rewrites are reported in reading order, and by object within a page.
    ///
    /// Both orderings are by construction --- the outer walk is the page order,
    /// the inner one the `BTreeMap`'s --- so this fails every run rather than
    /// one in six. The objects are inserted **out of order and off their pages'
    /// order**, which is what makes the assertion about the walk rather than
    /// about insertion.
    #[test]
    fn rewrites_come_back_in_reading_order_and_by_object() {
        let mut doc = Doc::open(3);
        let [first, second, third] = [0, 1, 2].map(|i| doc.working().order()[i]);
        for (number, page) in [(9, third), (4, first), (2, third), (7, second)] {
            doc.rewrite(object(number), page, "mine".into(), "D:2026".into())
                .expect("rewrite");
        }

        let seen: Vec<(PageId, u32)> = doc
            .working()
            .all_rewrites()
            .into_iter()
            .map(|(page, object, _)| (page, object.number()))
            .collect();
        assert_eq!(seen, vec![(first, 4), (second, 7), (third, 2), (third, 9)]);

        // And it is the *reading* order, not the file's: moving the last page to
        // the front moves its two rewrites with it.
        doc.apply(Command::Move {
            page: third,
            after: None,
        })
        .expect("move");
        let moved: Vec<(PageId, u32)> = doc
            .working()
            .all_rewrites()
            .into_iter()
            .map(|(page, object, _)| (page, object.number()))
            .collect();
        assert_eq!(moved, vec![(third, 2), (third, 9), (first, 4), (second, 7)]);
    }

    /// A discarded redo tail takes its rewrite bodies with it.
    ///
    /// The accounting observable, read for the reason the trap index gives: a
    /// body kept after the command naming it was thrown away, and one correctly
    /// dropped, produce identical documents. Nothing else in this file can see
    /// the difference, and the colour table's version of this sat unread for a
    /// week.
    #[test]
    fn a_discarded_rewrite_takes_its_body_with_it() {
        let mut doc = Doc::open(2);
        let page = doc.working().order()[0];
        doc.rewrite(object(7), page, "first".into(), "D:2026".into())
            .expect("rewrite");
        doc.rewrite(object(7), page, "second".into(), "D:2026".into())
            .expect("rewrite");
        assert_eq!(doc.rewrite_bodies(), 2);

        assert!(doc.undo());
        // A command applied here discards the tail, and the body it named goes
        // with it.
        doc.apply(Command::Rotate { page, turns: 1 })
            .expect("rotate");
        assert_eq!(doc.rewrite_bodies(), 1);
        assert_eq!(
            doc.rewrite_of(object(7)).map(|edit| edit.body.as_str()),
            Some("first")
        );
    }

    /// A gesture id for a test, which only has to be distinct from its neighbours.
    fn sweep(n: u64) -> Option<SweepId> {
        SweepId::new(n)
    }

    #[test]
    fn one_press_of_undo_puts_back_a_sweep_that_crossed_several_drawings() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let first = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let second = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");

        // One release of the pointer, two drawings, two commands.
        let keep = vec![doc.strokes_of(first)[0].clone()];
        doc.reink_in(first, keep, sweep(1)).expect("erased");
        let keep = vec![doc.strokes_of(second)[0].clone()];
        doc.reink_in(second, keep, sweep(1)).expect("erased");
        assert_eq!(doc.strokes_of(first).len(), 1);
        assert_eq!(doc.strokes_of(second).len(), 1);

        assert!(doc.undo(), "there was a sweep to undo");
        assert_eq!(
            (doc.strokes_of(first).len(), doc.strokes_of(second).len()),
            (3, 3),
            "one press put back only part of the gesture"
        );
        // And the drawings themselves are still there, so the undo stopped at
        // the sweep rather than running back over everything before it.
        assert!(doc.mark(first).is_some());
        assert!(doc.mark(second).is_some());
    }

    #[test]
    fn two_sweeps_are_two_presses() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let first = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let second = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");

        let keep = vec![doc.strokes_of(first)[0].clone()];
        doc.reink_in(first, keep, sweep(1)).expect("erased");
        let keep = vec![doc.strokes_of(second)[0].clone()];
        doc.reink_in(second, keep, sweep(2)).expect("erased");

        assert!(doc.undo());
        assert_eq!(
            (doc.strokes_of(first).len(), doc.strokes_of(second).len()),
            (1, 3),
            "one press crossed both gestures"
        );
        assert!(doc.undo());
        assert_eq!(doc.strokes_of(first).len(), 3);
    }

    #[test]
    fn a_command_belonging_to_no_gesture_joins_neither_neighbour() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");

        // Two ungrouped erasures. `None` never groups --- including with another
        // `None`, which is the case an equality on `Option` would get wrong.
        let keep = vec![doc.strokes_of(id)[0].clone(), doc.strokes_of(id)[1].clone()];
        doc.reink_in(id, keep, None).expect("erased");
        let keep = vec![doc.strokes_of(id)[0].clone()];
        doc.reink_in(id, keep, None).expect("erased");
        assert_eq!(doc.strokes_of(id).len(), 1);

        assert!(doc.undo());
        assert_eq!(
            doc.strokes_of(id).len(),
            2,
            "two ungrouped commands went back in one press"
        );
    }

    #[test]
    fn a_gesture_does_not_swallow_what_a_reader_did_before_it() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let id = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        doc.apply(Command::Rotate { page, turns: 1 }).expect("turn");

        let keep = vec![doc.strokes_of(id)[0].clone(), doc.strokes_of(id)[1].clone()];
        doc.reink_in(id, keep, sweep(9)).expect("erased");
        let keep = vec![doc.strokes_of(id)[0].clone()];
        doc.reink_in(id, keep, sweep(9)).expect("erased");

        assert!(doc.undo());
        assert_eq!(doc.strokes_of(id).len(), 3, "the sweep went back whole");
        assert_eq!(
            doc.working().page(page).expect("live").extra_turns,
            1,
            "the undo ran past the sweep and took the turn with it"
        );
    }

    #[test]
    fn redo_returns_a_whole_sweep_in_one_press() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let first = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let second = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let keep = vec![doc.strokes_of(first)[0].clone()];
        doc.reink_in(first, keep, sweep(4)).expect("erased");
        let keep = vec![doc.strokes_of(second)[0].clone()];
        doc.reink_in(second, keep, sweep(4)).expect("erased");

        assert!(doc.undo());
        assert!(doc.redo(), "there was a sweep to redo");
        assert_eq!(
            (doc.strokes_of(first).len(), doc.strokes_of(second).len()),
            (1, 1),
            "one press brought back only part of the gesture"
        );
        assert!(!doc.can_redo(), "the whole tail was the sweep");
    }

    #[test]
    fn a_sweep_that_takes_a_whole_mark_and_part_of_a_drawing_is_one_press() {
        let mut doc = Doc::open(1);
        let page = doc.working().order()[0];
        let drawn = doc
            .annotate(drawing_on(page), String::new())
            .expect("drawn");
        let whole = doc.annotate(mark_on(page), String::new()).expect("marked");

        // The two halves of one release: `reink` for the drawing it clipped,
        // `Unannotate` for the highlight it crossed. Different commands, one
        // gesture.
        let keep = vec![doc.strokes_of(drawn)[0].clone()];
        doc.reink_in(drawn, keep, sweep(3)).expect("erased");
        doc.apply_in(Command::Unannotate { mark: whole }, sweep(3))
            .expect("removed");
        assert!(!doc.working().marks_on(page).contains(&whole));

        assert!(doc.undo());
        assert_eq!(doc.strokes_of(drawn).len(), 3);
        assert!(
            doc.working().marks_on(page).contains(&whole),
            "the highlight the same gesture took is still gone"
        );
    }
}
