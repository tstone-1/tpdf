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
//! **A plan can now be out of document order**, as of the increment that wired
//! [`Command::Move`]. `save.rs` and `print.rs` rebuild the page tree for one
//! rather than refusing it, and both check first: rebuilding it costs every page
//! its ancestry, so a document nobody rearranged must not go through that path.
//!
//! What is still **not** here: nothing creates a page --- `docmodel`'s note has
//! the id-allocator property that would need proving first.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::docmodel::{
    Command, Doc, Mark, MarkId, MarkKind, PageId, Point, Quad, Rect, Redaction, RedactionId,
    Refusal, StampName, Stroke,
};
use crate::fingerprint::Fingerprint;

/// One open document: the edit model, and what its file looked like at open.
///
/// One value rather than two maps, and that is the whole reason it exists. The
/// fingerprint answers the same question `Doc`'s baseline does --- *what was
/// this file when the reader started editing it* --- so a document that has one
/// and not the other is a state nothing should be able to reach. Two maps keyed
/// the same way can drift; a struct cannot.
struct Open {
    model: Doc,
    /// What the file looked like at open, once the hash has been computed.
    ///
    /// **Behind a `OnceLock` because hashing is not free on the documents this
    /// project exists for.** Measured on this machine: 452 ms cold and 156 ms
    /// warm for the 337 MB scan fixture, against 3.8 ms for a 3 MB drawing and
    /// 0.1 ms for a small text page. Priority 1 is a cold start under 300 ms, so
    /// putting a full read in front of every open would spend more than the whole
    /// budget on exactly the files a reader most needs opened promptly.
    ///
    /// So it is computed on a thread and the open returns without it. Everything
    /// that needs it waits --- `Edits::plan`, which is only reached by a save or a
    /// print, both of which are about to read the whole file anyway. The inner
    /// `Option` is `None` when the fingerprint could not be taken at all; the
    /// `OnceLock` being unset means it is still being computed, and those are
    /// different states that must not be collapsed.
    opened_as: Arc<OnceLock<Option<Fingerprint>>>,
}

/// One page as the frontend sees it.
///
/// Field names are the Rust identifiers --- there is no `rename_all` here, for
/// the same reason `render.rs` has none: `ipc.ts` mirrors these by hand and a
/// rename that only one side hears about type-checks green on both.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
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
    /// The page's visible box as the reader has set it, or `None` for the
    /// file's own.
    ///
    /// `[llx, lly, urx, ury]` in the page's own space, y upwards --- what
    /// `/CropBox` uses, and the one place in this reply that is *not* in display
    /// space. It has to be: it is the number that decides what display space
    /// even is, so expressing it in that space would be circular.
    ///
    /// **Absolute, never a delta.** A second crop replaces the first, so
    /// clearing one is `None` rather than an inverse to compute, and a reader
    /// who crops twice gets the second answer rather than the composition of two
    /// --- which is what `docmodel::Command::Crop` means by taking an
    /// `Option<Rect>` rather than an adjustment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<[f64; 4]>,
}

/// One mark as the frontend sees it.
///
/// Field names are the Rust identifiers, as [`PageView`]'s are and for the same
/// reason.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct MarkView {
    /// The model's identity for this mark, sent back verbatim to remove it.
    pub id: u64,
    /// What kind of mark it is, so the reader can be told which one they are
    /// about to remove. The overlay does not draw from it --- PDFium paints
    /// every mark inside the tile --- so this is a label, not geometry.
    pub kind: MarkKind,
    /// The page it is on, by [`PageView::id`] --- never a position.
    pub page: u64,
    /// Four numbers per quad: left, top, right, bottom, in display-space points
    /// from the page's top-left corner.
    ///
    /// Flat rather than a struct per quad, which is what `text.rs` does with
    /// character boxes and for the same reason: this crosses to the webview as
    /// JSON and the overlay wants to iterate rather than to name fields.
    pub quads: Vec<f32>,
    /// One entry per stroke, each `x y x y ...` in the same display space.
    ///
    /// **The overlay needs these or it cannot draw ink at all.** Every other
    /// kind takes its shape from the quads above, so this list is empty for
    /// them --- and for ink the quad is only the rectangle the drawing occupies,
    /// which is a box round the ink rather than the ink.
    pub strokes: Vec<Vec<f32>>,
    /// Which standard stamp this is, for `MarkKind::Stamp` and nothing else.
    ///
    /// **The overlay needs it or it cannot draw a stamp at all**, which is
    /// [`MarkView::strokes`]'s situation exactly: the quads say where the stamp
    /// is and this says what it says. A stamp with no name would be an empty
    /// border, and an empty border is a box.
    pub stamp: Option<StampName>,
    /// Red, green and blue in 0..=1.
    pub color: [f32; 3],
    /// What the reader typed, which may be empty.
    ///
    /// **Attacker-controlled the moment a saved file is reopened**, so it is
    /// treated exactly as `annots.rs` treats a body: it reaches the DOM as text
    /// and nothing here may carry a URL. See `docs/THREAT-MODEL.md` T8.
    pub note: String,
    /// The note broken into the lines a text box will be drawn in, empty for
    /// every other kind.
    ///
    /// **Computed here rather than in the overlay, and that is the whole point
    /// of the field.** The webview can measure text --- `ctx.measureText` --- but
    /// it would be measuring whatever font the system resolved, and the file is
    /// set in Helvetica by `textbox.rs`'s own metrics. Two measurements of two
    /// fonts break lines in different places, so a reader would see three lines
    /// on screen and get four in the saved file, with no way to tell which was
    /// right.
    ///
    /// So there is one layout, in one language, and the overlay draws the lines
    /// it is handed. It is the same argument `OUTLINE_WIDTH` makes for being one
    /// number, applied to an algorithm instead of a constant.
    ///
    /// Attacker-controlled exactly as [`MarkView::note`] is, and it reaches the
    /// DOM the same way: as text, through a canvas, never as markup.
    pub lines: Vec<String>,
}

/// One region a reader has marked for removal, as the backend reports it.
///
/// **Deliberately not a [`MarkView`] with another kind**, and the reason is the
/// one [`crate::docmodel::RedactionId`] states: a mark is written into the saved
/// file and a redaction must never be. Keeping them in two lists means the
/// writer's input --- [`Plan::marks`], built from [`EditState::marks`] --- cannot
/// carry one by accident, rather than not carrying one because somebody
/// remembered to filter.
///
/// It has no colour, no note and no author for [`crate::docmodel::Redaction`]'s
/// reason: nothing here is drawn into a file for anyone to read.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct RedactionView {
    /// The model's identity for this redaction, sent back verbatim to remove it.
    pub id: u64,
    /// The page it is on, by [`PageView::id`] --- never a position.
    pub page: u64,
    /// Four numbers --- left, top, right, bottom --- in display-space points
    /// from the page's top-left corner.
    ///
    /// One rectangle rather than a list, which is where this parts company with
    /// [`MarkView::quads`]. A redaction is a region a reader dragged out; see
    /// [`crate::docmodel::Redaction::area`].
    pub area: [f32; 4],
}

/// What the frontend asks for when a reader makes a mark.
///
/// A struct rather than a parameter list, and not only because clippy counts to
/// seven: **`made` is deliberately not in here.** The timestamp is the
/// application's, taken from its own clock at the moment the command arrives, so
/// a caller cannot choose what a mark claims about when it was made.
#[derive(Clone, Debug, Deserialize)]
pub struct NewMark {
    /// Which kind of mark this is.
    ///
    /// Chosen by the frontend, unlike `made`: a reader picks Highlight or
    /// Underline and there is nothing for the application to decide. The set is
    /// closed by the type, so an unknown name is a deserialisation error at the
    /// boundary rather than a mark written as something else.
    pub kind: MarkKind,
    /// The page, by the identity a state reply gave it.
    pub page: u64,
    /// Four numbers per rectangle --- left, top, right, bottom --- in display
    /// space.
    ///
    /// **Empty for ink**, whose rectangle nobody sends: it is
    /// [`Stroke::bounds`] of the strokes, computed here so that the frontend and
    /// the model cannot disagree about where a drawing is. A sender that
    /// supplied one anyway would be describing the same fact twice, and the
    /// copy that is wrong is the one that never gets looked at.
    pub quads: Vec<f32>,
    /// One entry per stroke, each `x y x y ...` in display space.
    ///
    /// **Flat pairs rather than a list of `{x, y}` objects**, matching `quads`
    /// above and for the same reason: a freehand line is hundreds of points, and
    /// a JSON object per point is an order of magnitude more bytes over the IPC
    /// boundary for a shape that is entirely positional.
    ///
    /// Defaulted so that every existing sender --- which is every mark that is
    /// not ink --- keeps working unchanged, and so that a missing field is an
    /// empty list rather than a deserialisation failure the reader would see as
    /// "the highlight did nothing".
    #[serde(default)]
    pub strokes: Vec<Vec<f32>>,
    /// Which standard stamp this is, for [`MarkKind::Stamp`] and nothing else.
    ///
    /// Defaulted for [`NewMark::strokes`]'s reason: every sender that predates
    /// stamps keeps working, and a missing field is `None` rather than a
    /// deserialisation failure a reader would see as the command doing nothing.
    /// The model refuses the two ways this can disagree with `kind`.
    #[serde(default)]
    pub stamp: Option<StampName>,
    /// Red, green and blue in 0..=1.
    pub color: [f32; 3],
    pub author: String,
    pub note: String,
}

/// One colour channel from the wire, brought into the range `Mark` promises.
///
/// **`Mark::color` is documented as "in 0..=1" and nothing made it so**, which
/// mattered more than it sounds: JSON has no `NaN` or `Infinity` literals, so
/// serde refuses those, but `1e40` is perfectly good JSON and becomes
/// `f32::INFINITY` on the way into an `f32`. `save.rs` writes each channel with
/// `format!`, and `format!("{}", f32::INFINITY)` is `inf` --- three letters in
/// the middle of a content stream, which is a syntax error. tpdf would have
/// written a file no reader can parse and signed its name to it.
///
/// Clamped rather than refused. A colour a fraction outside the range is what a
/// slider produces and is not an error; every PDF reader clamps `/C` anyway, so
/// refusing would be stricter than the format. A non-finite value has no
/// clamped meaning at all --- `f32::NAN.clamp(0.0, 1.0)` is `NaN` --- so it
/// becomes zero, which is the only total answer.
fn channel(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
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
    /// Every mark the reader has made, in page order.
    ///
    /// Whole rather than per page, for the reason the note above this struct
    /// gives: the frontend holds a cache of one answer, and a per-page reply
    /// would have it stitching several together with rules of its own.
    pub marks: Vec<MarkView>,
    /// Every region the reader has marked for removal, in page order.
    ///
    /// **A second list rather than more entries in `marks`**, and the whole of
    /// the reason is where each one goes: `marks` is what a save writes and this
    /// is what an apply destroys. See [`RedactionView`].
    ///
    /// Whole rather than per page, for `marks`' reason.
    pub redactions: Vec<RedactionView>,
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
    docs: Mutex<HashMap<u32, Open>>,
}

impl Edits {
    /// Starts a model for a freshly opened document.
    ///
    /// Replaces any model already under that handle. That is not defensive: the
    /// render service reuses document numbers, so an id can legitimately name a
    /// different file than it did, and keeping the old journal would apply one
    /// document's edits to another.
    pub fn open(&self, doc: u32, pages: u32, source: Option<PathBuf>) {
        let opened_as: Arc<OnceLock<Option<Fingerprint>>> = Arc::new(OnceLock::new());
        match source {
            Some(path) => {
                let cell = Arc::clone(&opened_as);
                // Detached on purpose: nothing joins it, and the only reader
                // blocks on the cell rather than on the thread. A handle would
                // have to be stored, kept alive across a close, and reasoned
                // about when a document is dropped mid-hash.
                std::thread::spawn(move || {
                    let taken = match Fingerprint::of(&path) {
                        Ok(print) => Some(print),
                        Err(why) => {
                            // Through `diag` rather than `eprintln!`, which is
                            // the difference between a line the reader can send
                            // us and one that goes nowhere: a process started
                            // from Explorer or the Dock has no stderr, and this
                            // is the line that explains a refusal the reader
                            // *does* see --- Save is off for this document for
                            // the rest of the session and nothing else says why.
                            // The sink is a `OnceLock`, so this is safe from a
                            // detached thread.
                            crate::diag::note(&format!(
                                "[WARN] {} could not be fingerprinted, so Save is refused for it: {why}",
                                path.display()
                            ));
                            None
                        }
                    };
                    // A failed `set` means the document was closed and reopened
                    // under the same handle while this ran. The new open has its
                    // own cell, so there is nothing to correct.
                    let _ = cell.set(taken);
                });
            }
            // No path to fingerprint: settle immediately rather than leaving a
            // reader of the cell waiting for a thread that was never started.
            None => {
                let _ = opened_as.set(None);
            }
        }
        self.docs.lock().expect("edits lock").insert(
            doc,
            Open {
                model: Doc::open(pages),
                opened_as,
            },
        );
    }

    /// What the file looked like when this document was opened.
    ///
    /// `None` for a document with no model, and `Some(None)` for one whose
    /// fingerprint could not be taken --- which a save must treat as a refusal
    /// rather than as permission. The two are different facts and collapsing
    /// them is how "could not look" becomes "looked, and it was fine".
    #[must_use]
    pub fn opened_as(&self, doc: u32) -> Option<Option<Fingerprint>> {
        let pending = self.pending(doc)?;
        // Waited on with no lock held --- see `pending`.
        Some(pending.wait().clone())
    }

    /// The cell holding a document's fingerprint, without waiting for it.
    ///
    /// Separated so that every caller waits **outside** the `docs` mutex. Waiting
    /// inside it would hold the lock for as long as the hash takes --- 452 ms on
    /// the 337 MB fixture --- and block every other edit command on the file
    /// being read, which is the shape of a hang rather than of a slow save.
    fn pending(&self, doc: u32) -> Option<Arc<OnceLock<Option<Fingerprint>>>> {
        self.docs
            .lock()
            .expect("edits lock")
            .get(&doc)
            .map(|open| Arc::clone(&open.opened_as))
    }

    /// Drops a document's model. Silent if there is none.
    pub fn close(&self, doc: u32) {
        self.docs.lock().expect("edits lock").remove(&doc);
    }

    /// Drops every document's model, returning how many there were.
    ///
    /// For a webview that has just started and therefore holds no document id.
    /// See `lib.rs`'s `release_documents`, which is the only caller and which
    /// carries the argument for why that is sound.
    pub fn release_all(&self) -> usize {
        let mut docs = self.docs.lock().expect("edits lock");
        let held = docs.len();
        docs.clear();
        held
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
        let open = docs.get(&doc).ok_or_else(|| unknown(doc))?;
        let model = &open.model;
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

    /// Sets or clears one page's visible box, addressed by identity.
    ///
    /// `to` is `[llx, lly, urx, ury]` in the page's own space, y upwards, or
    /// `None` to put the file's own box back. Absolute rather than relative for
    /// the reason [`PageView::crop`] gives.
    ///
    /// **Nothing is written to the file here**, and the separation is worth
    /// stating because two different mechanisms carry this number. What the
    /// reader *sees* comes from PDFium being handed the box on every request
    /// (`RawDocument::page_cropped`); what they *save* comes from `save.rs`
    /// writing `/CropBox` out of the plan. Neither reads the other, so a check
    /// that the two agree is a real check rather than a tautology.
    ///
    /// # Errors
    ///
    /// The handle names no open document; the id names no page or a deleted one;
    /// or the rectangle encloses no area, which includes any corner that is not
    /// a number --- see `docmodel::Rect::is_proper`.
    pub fn crop(&self, doc: u32, page: u64, to: Option<[f64; 4]>) -> Result<EditState, String> {
        self.command(
            doc,
            Command::Crop {
                page: PageId::from_raw(page),
                to: to.map(|r| Rect {
                    llx: r[0],
                    lly: r[1],
                    urx: r[2],
                    ury: r[3],
                }),
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

    /// Moves one page so that it sits immediately after `after`, or first when
    /// `after` is `None`.
    ///
    /// **A neighbour rather than a destination index**, which is the same
    /// argument as addressing the page by identity: an index names a position in
    /// an order that a command in flight may already have changed, and the page
    /// would land next to whatever moved into it. The frontend does the
    /// arithmetic, because it is the side that holds the order --- see
    /// `pages.ts`.
    ///
    /// # Errors
    ///
    /// The handle names no open document; either id names no page or a deleted
    /// one; or the anchor is the page itself, which describes no move.
    pub fn move_page(&self, doc: u32, page: u64, after: Option<u64>) -> Result<EditState, String> {
        self.command(
            doc,
            Command::Move {
                page: PageId::from_raw(page),
                after: after.map(PageId::from_raw),
            },
        )
    }

    /// Puts a highlight on a page, over the rectangles the reader dragged across.
    ///
    /// `quads` are flat --- four numbers per rectangle, `left, top, right,
    /// bottom` --- in the display space the frontend already holds every glyph
    /// box in. **They are stored as given and mapped into the page's own space
    /// only at the moment of writing**, by `save.rs`, which is where the crop
    /// box and `/Rotate` that define the mapping are in hand. Two consequences
    /// worth stating: the overlay draws exactly what the model holds, with no
    /// conversion between the reader's drag and what they see; and a mark made
    /// on a page whose geometry cannot be read is refused by the writer rather
    /// than silently placed.
    ///
    /// `made` is the timestamp in PDF date form. It is passed in rather than
    /// taken from a clock here for the reason `docmodel` gives: a model with a
    /// clock in it needs the clock frozen to be tested.
    ///
    /// # Errors
    ///
    /// The handle names no open document; `quads` is not a multiple of four; a
    /// stroke has an odd number of numbers; any coordinate is not finite; the
    /// note is longer than [`crate::textbox::MAX_NOTE_CHARS`]; the mark covers
    /// no area; or the page does not exist or was deleted.
    pub fn annotate(&self, doc: u32, want: NewMark, made: String) -> Result<EditState, String> {
        if want.quads.len() % 4 != 0 {
            return Err(format!(
                "a mark is four numbers per rectangle, and this has {}",
                want.quads.len()
            ));
        }
        // Same shape as the quads above and the same reason for checking it
        // here: a ragged list is a sender defect, and chunking it silently would
        // drop the last point of a stroke rather than say so.
        if let Some(ragged) = want.strokes.iter().find(|s| s.len() % 2 != 0) {
            return Err(format!(
                "a stroke is two numbers per point, and one of these has {}",
                ragged.len()
            ));
        }
        // The third door, and it was the one left open. `channel` clamps a
        // colour and [`displace`](Edits::displace) refuses a non-finite offset
        // --- its doc comment is where the reasoning is written out --- while a
        // mark's own geometry, which is the number that actually reaches
        // `/Rect`, `/QuadPoints` and `/InkList`, was taken as sent. `1e40` is
        // valid JSON and is `f32::INFINITY` by the time it is here;
        // `Quad::covers_area` excludes `NaN` and not infinities, so the model
        // accepts it; and `save.rs` writes it with `format!`, which spells it
        // `inf` in the middle of a content stream. On an append the read-back
        // parse fails and the write is cut back, so the reader loses the save
        // and keeps the file; on a rewrite `verify_before_commit` compares
        // fingerprints and cannot see it, so the malformed file is renamed over
        // the document. Refused rather than clamped for `displace`'s reason: a
        // mark silently moved somewhere else is not the mark the reader drew.
        if let Some(bad) = want
            .quads
            .iter()
            .chain(want.strokes.iter().flatten())
            .find(|v| !v.is_finite())
        {
            return Err(format!("a mark cannot have a corner at {bad}"));
        }
        too_long(&want.note)?;
        let strokes: Vec<Stroke> = want
            .strokes
            .iter()
            .map(|flat| Stroke {
                points: flat
                    .chunks_exact(2)
                    .map(|p| Point { x: p[0], y: p[1] })
                    .collect(),
            })
            .collect();
        let mut quads: Vec<Quad> = want
            .quads
            .chunks_exact(4)
            .map(|q| Quad {
                left: q[0],
                top: q[1],
                right: q[2],
                bottom: q[3],
            })
            .collect();
        // **Ink's rectangle is derived, not sent**, for the reason `NewMark::quads`
        // gives. `INK_WIDTH / 2.0` because a stroke straddles its path, so the
        // rectangle the ink occupies is wider than the points it runs through ---
        // and tight bounds would additionally refuse a straight vertical line as
        // covering no area. See `Stroke::bounds`.
        if want.kind == MarkKind::Ink {
            quads = Stroke::bounds(&strokes, (crate::docmodel::INK_WIDTH / 2.0) as f32)
                .into_iter()
                .collect();
        }

        let mut docs = self.docs.lock().expect("edits lock");
        let model = &mut docs.get_mut(&doc).ok_or_else(|| unknown(doc))?.model;
        model
            .annotate(
                Mark {
                    kind: want.kind,
                    page: PageId::from_raw(want.page),
                    quads,
                    strokes,
                    stamp: want.stamp,
                    color: want.color.map(channel),
                    author: want.author,
                    made,
                },
                want.note,
            )
            .map_err(describe)?;
        Ok(snapshot(model))
    }

    /// Takes one mark off the page it is on, addressed by identity.
    ///
    /// # Errors
    ///
    /// The handle names no open document; the id names no mark, or one that has
    /// already been removed --- two diagnoses, for the reason
    /// [`delete`](Edits::delete) keeps two.
    pub fn unannotate(&self, doc: u32, mark: u64) -> Result<EditState, String> {
        self.command(
            doc,
            Command::Unannotate {
                mark: MarkId::from_raw(mark),
            },
        )
    }

    /// Marks a region of a page for removal.
    ///
    /// **Nothing is destroyed here.** This is `docs/PLAN.md` §6 step 1: the
    /// region joins the review list and the overlay draws it, and the document
    /// is untouched until step 3 applies it. Undo takes it back off, which is
    /// what makes step 2 a review rather than a formality.
    ///
    /// Not routed through [`command`](Edits::command) --- like
    /// [`annotate`](Edits::annotate), it allocates an id, and an id a caller
    /// chose is an id two redactions can share.
    ///
    /// # Errors
    ///
    /// The handle names no open document; the rectangle holds a corner that is
    /// not finite; or the model refuses --- the page not existing, having been
    /// deleted, or the region covering no area.
    pub fn redact(&self, doc: u32, page: u64, area: [f32; 4]) -> Result<EditState, String> {
        // [`annotate`](Edits::annotate)'s third door, and the same reasoning
        // applies with one difference in where it bites: a mark's infinite
        // corner is written into a content stream as `inf`, and a redaction's
        // would be compared against every object on the page, where it makes
        // one region cover the document. Refused rather than clamped, so a
        // reader is never told a redaction covers what they did not drag.
        if let Some(bad) = area.iter().find(|v| !v.is_finite()) {
            return Err(format!("a redaction cannot have a corner at {bad}"));
        }
        let mut docs = self.docs.lock().expect("edits lock");
        let model = &mut docs.get_mut(&doc).ok_or_else(|| unknown(doc))?.model;
        model
            .redact(Redaction {
                page: PageId::from_raw(page),
                area: Quad {
                    left: area[0],
                    top: area[1],
                    right: area[2],
                    bottom: area[3],
                },
            })
            .map_err(describe)?;
        Ok(snapshot(model))
    }

    /// Takes a pending redaction back off its page, addressed by identity.
    ///
    /// # Errors
    ///
    /// The handle names no open document, or the model refuses --- an id that
    /// never existed and one already removed are different diagnoses and are
    /// reported as such.
    pub fn unredact(&self, doc: u32, redaction: u64) -> Result<EditState, String> {
        self.command(
            doc,
            Command::Unredact {
                redaction: RedactionId::from_raw(redaction),
            },
        )
    }

    /// Rubs strokes out of a drawing, addressed by identity and by position.
    ///
    /// **Positions rather than points**, because the frontend is deciding *which*
    /// strokes the eraser touched and the backend owns *what they are*: sending
    /// the survivors back would let a stale or wrong frontend rewrite a drawing's
    /// geometry through a command whose name says it only removes.
    ///
    /// **The whole gesture is one call and one command.** A reader sweeps an
    /// eraser across four strokes and lets go; that is one thing they did, so it
    /// is one undo. The frontend hides the doomed strokes while the drag is live
    /// and sends the list on release, exactly as drawing accumulates and commits
    /// on Enter.
    ///
    /// **Erasing everything removes the mark**, rather than leaving a drawing of
    /// nothing behind: `Doc::reink` refuses an empty result and this is the
    /// layer that knows the gesture meant "get rid of it". A reader who rubs out
    /// the last stroke of a drawing and presses undo gets the whole drawing
    /// back, because `Unannotate` is one command too.
    ///
    /// # Errors
    ///
    /// The handle names no open document; the id names no mark, or one already
    /// removed; the mark is not a drawing; a position names no stroke.
    pub fn erase(&self, doc: u32, mark: u64, remove: Vec<usize>) -> Result<EditState, String> {
        let mut docs = self.docs.lock().expect("edits lock");
        let model = &mut docs.get_mut(&doc).ok_or_else(|| unknown(doc))?.model;
        let id = MarkId::from_raw(mark);
        let held = model.strokes_of(id).len();
        // Refused rather than ignored: a position past the end means the sender
        // is looking at a drawing this is not, and quietly erasing the strokes
        // it *did* name would act on half a stale gesture.
        if let Some(past) = remove.iter().find(|&&at| at >= held) {
            return Err(format!(
                "stroke {past} of a drawing that has {held}, so this gesture was                  aimed at something else"
            ));
        }
        let keep: Vec<Stroke> = model
            .strokes_of(id)
            .iter()
            .enumerate()
            .filter(|(at, _)| !remove.contains(at))
            .map(|(_, stroke)| stroke.clone())
            .collect();
        if keep.iter().any(Stroke::is_drawable) {
            model.reink(id, keep).map_err(describe)?;
        } else {
            model
                .apply(Command::Unannotate { mark: id })
                .map_err(describe)?;
        }
        Ok(snapshot(model))
    }

    /// Replaces what one mark says, addressed by identity.
    ///
    /// Not routed through [`command`](Edits::command) like the other four, and
    /// the reason is the same one [`annotate`](Edits::annotate) has: the command
    /// carries an id that only the model may issue --- here the note's --- so
    /// what crosses this boundary is the text, and the [`Command`] is built on
    /// the far side of the lock.
    ///
    /// **A text box refuses text Helvetica cannot write**, which no other kind
    /// does, because for every other kind the note is metadata: an unwritable
    /// character in a highlight's note goes into `/Contents` as UTF-16 and is
    /// read back perfectly. A text box's note is drawn on the page in a
    /// `/WinAnsiEncoding` font, and what a font cannot encode it draws as
    /// something else.
    ///
    /// Refusing is the unfriendly answer and the honest one. The alternative is
    /// to write what can be encoded and substitute the rest, which looks correct
    /// in tpdf --- the overlay draws with a system font that has the glyphs --- and
    /// is wrong in every reader that opens the file, including this one after a
    /// reopen. A reader told "no" can paste it into a comment instead; a reader
    /// not told loses the words silently.
    ///
    /// # Errors
    ///
    /// The handle names no open document; the id names no mark, or one that has
    /// already been removed; the note is longer than
    /// [`crate::textbox::MAX_NOTE_CHARS`]; the mark is a text box and the note
    /// holds a character `/WinAnsiEncoding` has no byte for.
    pub fn renote(&self, doc: u32, mark: u64, note: String) -> Result<EditState, String> {
        // Before the lock, because it needs nothing from the model and the lock
        // is what a long note makes expensive to hold.
        too_long(&note)?;
        let mut docs = self.docs.lock().expect("edits lock");
        let model = &mut docs.get_mut(&doc).ok_or_else(|| unknown(doc))?.model;
        let id = MarkId::from_raw(mark);
        // Read before the write, and only for the kind it applies to. A mark
        // that does not exist falls through to `model.renote`, which is the one
        // place that answers "that mark has already been removed" -- deciding it
        // here as well would be a second copy of that message.
        if model.mark(id).is_some_and(|m| m.kind == MarkKind::TextBox)
            && !crate::textbox::encodable(&note)
        {
            return Err(
                "a text box is written in Helvetica, which cannot draw every character in that text"
                    .to_string(),
            );
        }
        model.renote(id, note).map_err(describe)?;
        Ok(snapshot(model))
    }

    /// Replaces what one mark is drawn in, addressed by identity.
    ///
    /// [`renote`](Edits::renote)'s shape exactly, and not routed through
    /// [`command`](Edits::command) for its reason: the [`Command`] carries an id
    /// only the model may issue, so what crosses this boundary is the colour.
    ///
    /// **Clamped here**, by the same [`channel`] a new mark's colour goes
    /// through --- this is the second door into `/C` and it has to be the same
    /// door. Without it a sender could put `1e40` in a content stream through a
    /// command whose name says it only changes an appearance.
    ///
    /// # Errors
    ///
    /// The handle names no open document; the id names no mark, or one that has
    /// already been removed.
    pub fn recolor(&self, doc: u32, mark: u64, color: [f32; 3]) -> Result<EditState, String> {
        let mut docs = self.docs.lock().expect("edits lock");
        let model = &mut docs.get_mut(&doc).ok_or_else(|| unknown(doc))?.model;
        model
            .recolor(MarkId::from_raw(mark), color.map(channel))
            .map_err(describe)?;
        Ok(snapshot(model))
    }

    /// Moves one mark by an offset, addressed by identity.
    ///
    /// [`recolor`](Edits::recolor)'s shape, and its caveat too: what crosses the
    /// boundary is the offset, because the [`Command`] carries an id only the
    /// model may issue.
    ///
    /// **Refused rather than substituted when the offset is not finite**, which
    /// is where this parts company with [`channel`] above. JSON has no `NaN`
    /// literal, so serde refuses that --- but `1e40` is perfectly good JSON and
    /// arrives as `f32::INFINITY`, and `save.rs` writes a rectangle with
    /// `format!`, which spells that `inf`: three letters in the middle of a
    /// `/Rect`. A colour has a total answer for the nonsense case, since zero is
    /// a colour; an offset does not, because "do not move" is indistinguishable
    /// to the reader from a drag that silently failed. No pointer can produce
    /// one, so a sender that does is broken and is told so.
    ///
    /// The clamp that keeps a mark on its page is not here, for the reason
    /// [`Doc::displace`] gives: the page's size in points is the renderer's
    /// answer and neither this layer nor the model holds it.
    ///
    /// # Errors
    ///
    /// The handle names no open document; the offset is not finite; the id names
    /// no mark, or one that has already been removed.
    pub fn displace(&self, doc: u32, mark: u64, dx: f32, dy: f32) -> Result<EditState, String> {
        if !dx.is_finite() || !dy.is_finite() {
            return Err(format!("a mark cannot be moved by ({dx}, {dy})"));
        }
        let mut docs = self.docs.lock().expect("edits lock");
        let model = &mut docs.get_mut(&doc).ok_or_else(|| unknown(doc))?.model;
        model
            .displace(MarkId::from_raw(mark), dx, dy)
            .map_err(describe)?;
        Ok(snapshot(model))
    }

    /// Applies a command and returns the state it produced.
    fn command(&self, doc: u32, cmd: Command) -> Result<EditState, String> {
        let mut docs = self.docs.lock().expect("edits lock");
        let model = &mut docs.get_mut(&doc).ok_or_else(|| unknown(doc))?.model;
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
        let model = &mut docs.get_mut(&doc).ok_or_else(|| unknown(doc))?.model;
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
        let model = &mut docs.get_mut(&doc).ok_or_else(|| unknown(doc))?.model;
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
        // Resolved before the lock is taken. Hashing a large document takes
        // hundreds of milliseconds, and holding `docs` across that would block
        // every other edit command on the file being read --- see `Open::opened_as`
        // for the measurement and for why it is not on the open path either.
        let opened_as = self.opened_as(doc).ok_or_else(|| unknown(doc))?;
        let docs = self.docs.lock().expect("edits lock");
        let open = docs.get(&doc).ok_or_else(|| unknown(doc))?;
        let model = &open.model;
        let pages = snapshot(model).pages;
        Ok(Plan {
            baseline: model.baseline(),
            opened_as: opened_as.clone(),
            // Before `pages`, which the line below moves. Field order in a
            // struct literal is evaluation order, so this reads the borrow while
            // there is still one to read.
            marks: planned_marks(model, &pages),
            pages,
            // Empty, always. See the field.
            redactions: Vec::new(),
        })
    }

    /// The pending regions, grouped by the **baseline page** they are on.
    ///
    /// What the redact command asks a worker about: the worker addresses a page
    /// by its position in the file it has mapped, and the model addresses one by
    /// identity. This is the one translation between the two, and it is here
    /// rather than in the command because it needs the model's page order --- the
    /// same reason [`Plan::pages`] is built here.
    ///
    /// Ordered by baseline page, and within a page by the order the model
    /// reports its regions, so the ordinals a worker answers with can be zipped
    /// back onto the regions that produced them. A region on a page the reader
    /// has deleted is not here at all: `Working::all_redactions` walks the live
    /// page order, and a redaction on a page nobody is keeping removes nothing
    /// from a file that will not contain it.
    ///
    /// # Errors
    ///
    /// The handle names no open document.
    pub fn redaction_targets(&self, doc: u32) -> Result<Vec<RedactionTarget>, String> {
        let docs = self.docs.lock().expect("edits lock");
        let open = docs.get(&doc).ok_or_else(|| unknown(doc))?;
        let model = &open.model;
        let state = snapshot(model);
        let mut out: Vec<RedactionTarget> = Vec::new();
        for region in &state.redactions {
            // The page's position in the *file*, which is what a worker means by
            // a page number. A region whose page is not in the kept list is
            // skipped rather than defaulted to page 0 --- see the trap about a
            // failure path that acts hardest where it knows least.
            let Some(source) = state
                .pages
                .iter()
                .find(|page| page.id == region.page)
                .map(|page| page.source)
            else {
                continue;
            };
            match out.iter_mut().find(|target| target.source == source) {
                Some(target) => target.regions.push(region.area),
                None => out.push(RedactionTarget {
                    source,
                    regions: vec![region.area],
                }),
            }
        }
        out.sort_by_key(|target| target.source);
        Ok(out)
    }

    /// A plan naming only the pages at `slots`, for extracting a subset.
    ///
    /// **This changes nothing.** Extract writes a second file out of the pages a
    /// reader picked; the working document, the journal and undo are untouched,
    /// which is why it is a plan rather than a command. That also settles what
    /// undo does after an extract --- whatever it was going to do before one.
    ///
    /// The slots index the *current* order, so a reader who moved a page and
    /// then extracted "pages 1 to 3" gets the three pages they can see. Taking
    /// positions rather than ids is right here for the same reason
    /// [`Command::Move`](crate::docmodel::Command::Move) takes an id and is not:
    /// this is a selection a reader typed in the vocabulary they typed it in,
    /// resolved in the same lock that reads the order, so there is no window in
    /// which it can go stale.
    ///
    /// The baseline is carried over unchanged, because it describes the *file*
    /// and not the selection. Handing `write_copy` a baseline of three for a
    /// three-page extract from a ten-page document would be a lie of exactly the
    /// shape its external-modification check exists to catch.
    ///
    /// # Errors
    ///
    /// The handle names no open document; the selection is empty; or a slot is
    /// past the end of the current order. An empty selection is refused here
    /// rather than left to `write_copy` --- it refuses an empty plan too, but a
    /// message about a plan is not a message about what the reader typed.
    pub fn plan_subset(&self, doc: u32, slots: &[u32]) -> Result<Plan, String> {
        // Resolved before the lock is taken. Hashing a large document takes
        // hundreds of milliseconds, and holding `docs` across that would block
        // every other edit command on the file being read --- see `Open::opened_as`
        // for the measurement and for why it is not on the open path either.
        let opened_as = self.opened_as(doc).ok_or_else(|| unknown(doc))?;
        let docs = self.docs.lock().expect("edits lock");
        let open = docs.get(&doc).ok_or_else(|| unknown(doc))?;
        let model = &open.model;
        let all = snapshot(model).pages;

        if slots.is_empty() {
            return Err("no pages were named".into());
        }
        let mut pages = Vec::with_capacity(slots.len());
        // Walked in the order given rather than sorted here, and the duplicate
        // check is a `contains` over what has been taken. The frontend already
        // sorts and deduplicates, and doing it again would make this agree with
        // its caller by construction --- so instead a slot that arrives twice,
        // or out of order, is a defect this can still report.
        let mut taken: Vec<u32> = Vec::with_capacity(slots.len());
        for &slot in slots {
            let page = all
                .get(slot as usize)
                .ok_or_else(|| format!("this document has {} pages", all.len()))?;
            if taken.contains(&slot) {
                return Err(format!("page {} was named twice", slot + 1));
            }
            taken.push(slot);
            pages.push(*page);
        }
        if taken.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("the pages are not in document order".into());
        }

        let marks = planned_marks(model, &pages);
        Ok(Plan {
            baseline: model.baseline(),
            opened_as: opened_as.clone(),
            pages,
            marks,
            redactions: Vec::new(),
        })
    }
}

/// The marks that belong to `pages`, in page order, as the writer needs them.
///
/// Driven by `pages` rather than by the model's own order, which is what makes
/// it correct for a subset: a mark whose page was not taken is simply never
/// reached. Doing it the other way --- walking every mark and asking whether its
/// page is in the list --- would give the same answer today and would need its
/// own filter the moment a subset can repeat a page.
fn planned_marks(model: &Doc, pages: &[PageView]) -> Vec<PlannedMark> {
    let working = model.working();
    pages
        .iter()
        .flat_map(|view| {
            let id = PageId::from_raw(view.id);
            working.marks_on(id).iter().map(move |mark| {
                let body = model.mark(*mark).expect("a live mark has a body");
                PlannedMark {
                    kind: body.kind,
                    source: view.source,
                    // `snapshot`'s reason, and the consequence here is the
                    // one that reaches a file: `/Rect` and `/InkList` are
                    // written from these.
                    quads: model.quads_of(*mark).to_vec(),
                    strokes: model.strokes_of(*mark).to_vec(),
                    stamp: body.stamp,
                    color: model.color_of(*mark),
                    author: body.author.clone(),
                    note: model.note_of(*mark).to_string(),
                    made: body.made.clone(),
                }
            })
        })
        .collect()
}

/// The working document as something that writes a file needs it.
///
/// Not sent to the frontend. It is what [`save::write_copy`](crate::save::write_copy)
/// and the print path are handed, and it exists rather than a bare `Vec<PageView>`
/// because both of them have to answer a question the page list cannot: *how many
/// pages did the file have*. Without the baseline a save cannot tell three pages
/// kept out of five from a five-page document that lost two under it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Plan {
    /// How many pages the file this document was opened from had.
    pub baseline: u32,
    /// What that file looked like, so a writer can tell it has not been replaced.
    ///
    /// Beside `baseline` because it is the same kind of fact and has the same
    /// lifetime: both describe the file the plan was made against, and a plan
    /// carrying one without the other could check the shape of a document while
    /// missing that every byte of it changed. `None` when the fingerprint could
    /// not be taken --- see `fingerprint.rs` on why that is refused for a save in
    /// place and tolerated for a copy.
    ///
    /// **Never crosses the worker boundary**, and the `skip` is the mechanism
    /// rather than a note asking anyone to remember. A `Plan` is sent to the
    /// worker so it can build an update section (`save::append_update`), and a
    /// fingerprint is a fact about a *path* --- something the worker has no
    /// access to and no business asserting. Skipping it means a deserialised
    /// plan always carries `None`, so a worker cannot be handed one and cannot
    /// send one back. `worker_proto.rs` states the same property for `Request`
    /// generally: a request names nothing the worker could act on.
    #[serde(skip)]
    pub opened_as: Option<Fingerprint>,
    /// The kept pages, in reading order.
    pub pages: Vec<PageView>,
    /// The marks to write, on the pages that are kept.
    ///
    /// A mark whose page is not in `pages` is **absent rather than carried with
    /// a dangling reference**, which is what makes an extract of pages 1--3
    /// write the highlights on those three pages and nothing else.
    pub marks: Vec<PlannedMark>,
    /// The regions to remove, by baseline page.
    ///
    /// **Always empty out of the model**, and that is the safety property rather
    /// than an omission: the ordinals here address show operators in a content
    /// stream, and nothing in the model has parsed one. They are filled in by
    /// the one command that redacts, out of answers a worker computed against
    /// the file's own objects --- so an ordinary save, copy, extract or print
    /// carries none and destroys nothing.
    ///
    /// `#[serde(default)]` so a plan written before this existed still parses as
    /// the un-redacted one it meant. It crosses the worker boundary with the
    /// rest of the plan for an append, where it is always empty: a plan carrying
    /// a redaction is never an append, which [`Plan::only_adds_marks`] is what
    /// enforces.
    #[serde(default)]
    pub redactions: Vec<PlannedRedaction>,
}

/// One baseline page and the regions marked on it.
///
/// A named pair rather than a tuple, because a `Vec<(u32, Vec<[f32; 4]>)>` is a
/// type nobody can read twice --- which is also what clippy says about it. The
/// two halves are addressed differently on purpose: `source` is a page of the
/// **file**, which is what a worker means by a page number, and `regions` are in
/// the file's display space, which is what the model holds them in.
#[derive(Clone, PartialEq, Debug)]
pub struct RedactionTarget {
    /// The page's position in the baseline file, zero-based.
    pub source: u32,
    /// Every marked region on it, in the order the model reports them.
    ///
    /// The order matters: a worker answers with one plan per region in the order
    /// it was asked, so this is what the answers are zipped back onto.
    pub regions: Vec<[f32; 4]>,
}

/// One page's worth of removal, as the writer needs it.
///
/// [`PlannedMark`]'s counterpart for the other direction, and it names its page
/// the same way and for the same reason: a writer walking the page tree has
/// baseline positions, and a `PageId` means nothing to `lopdf`.
// `PartialEq` without `Eq`: `areas` is floats, and `RegionPlan` gave up the same
// derive for the same reason. `Eq` on a type holding an `f32` is a promise about
// reflexivity that a `NaN` breaks.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PlannedRedaction {
    /// Which baseline page this removes from, zero-based.
    pub source: u32,
    /// Ordinals among that page's **text objects**, as PDFium enumerated them.
    ///
    /// Every region on the page, merged: two regions covering one line of text
    /// name the same operator, and removing it twice is removing it once.
    pub shows: Vec<usize>,
    /// How many text objects PDFium found on the page.
    ///
    /// Carried because `redact::remove_shows` refuses when it disagrees with
    /// what `lopdf` finds in the content stream. Nothing connects the two lists
    /// but order, and a mis-addressed removal deletes the wrong words while
    /// reporting success --- see `redact.rs`, which is where that refusal lives.
    pub text_objects: usize,
    /// The regions themselves, in the page's own absolute space.
    ///
    /// `shows` says which drawing instructions go; this says *where*, which is
    /// what the annotation carrier needs --- an annotation is not a page object
    /// and has no ordinal among them, so the only way to ask whether one is over
    /// a region is to compare rectangles. `redact::RegionPlan::area` is where
    /// these come from, and the reason they are carried rather than recomputed
    /// is written there.
    pub areas: Vec<[f32; 4]>,
    /// The words the plan reported it would remove, one string per region.
    ///
    /// **From PDFium, through the font's own encoding**, which is why it is
    /// carried rather than derived where it is used. The operands
    /// `redact::remove_shows` deletes are font-encoded bytes: on a base-14
    /// document they happen to read as text and on a Type0 one they are CIDs,
    /// so a writer that read them would be right on the easy fixture and wrong
    /// on the document that matters. `redact::RegionPlan::taking` is where these
    /// come from.
    ///
    /// What needs them is the outline: a bookmark's title is the heading it
    /// points at, so the only way to ask whether an entry names something that
    /// went is to compare its title against what went. See
    /// `redact::covered_outline`.
    pub taking: Vec<String>,
}

/// One mark as the writer needs it.
///
/// Distinct from [`MarkView`], which is the same mark as the *frontend* needs
/// it, and the difference is the one that matters here: this names the page by
/// its position in the **baseline file**, because that is what a writer walking
/// the page tree has. A `MarkView`'s page id means nothing to `lopdf`.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PlannedMark {
    pub kind: MarkKind,
    /// Which baseline page this goes on, zero-based.
    pub source: u32,
    /// Display space, as the model holds it. Mapped into the page's own space by
    /// the writer --- see [`Edits::annotate`].
    pub quads: Vec<Quad>,
    /// The strokes, for ink, in the same display space. Empty for every other
    /// kind --- the biconditional is [`Mark::strokes`]'s and is
    /// enforced by the model before a mark can reach a plan.
    pub strokes: Vec<Stroke>,
    /// Which standard stamp this is, for `MarkKind::Stamp` and nothing else.
    ///
    /// Carried to the writer for [`PlannedMark::strokes`]'s reason: it is what
    /// gets drawn, and the model has already refused the two ways it can
    /// disagree with the kind.
    pub stamp: Option<StampName>,
    pub color: [f32; 3],
    pub author: String,
    pub note: String,
    pub made: String,
}

impl Plan {
    /// Whether this describes the file exactly as it is on disk.
    ///
    /// Every baseline page present, in order, unturned and uncropped, with no
    /// mark on any of them. It is what lets the print path hand the file over
    /// byte for byte rather than rewriting it to produce the same document --- a
    /// rewrite drops encryption silently and reflows structure, so "nothing was
    /// edited" is worth recognising rather than approximating with `dirty`,
    /// which is `true` after a turn and a turn back.
    ///
    /// **Every field of [`PageView`] that a reader can change has to appear
    /// here, and the crop did not until 2026-08-22.** A cropped document
    /// answered `true`: the marks were empty, every page was present in order,
    /// and nothing asked about the box. So printing a page a reader had cropped
    /// handed the printer the *uncropped* file, and the panel showed it that way
    /// too. Measured, not deduced --- a plan carrying `crop: Some(...)` reported
    /// `is_identity = true` and `select` answered `Pages::All`.
    ///
    /// The shape of the mistake is worth more than the missing clause: this
    /// predicate is a **list of the ways a document can differ from its file**,
    /// and a list like that is wrong the moment a new way is added, silently and
    /// in the reassuring direction. `a_plan_that_differs_in_any_one_field_is_not
    /// _the_file` walks the fields rather than naming them, so the next one has
    /// to be classified rather than forgotten.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        // A mark is a change the file does not have, so a plan carrying one is
        // never the file. Written first because it is the cheap half and because
        // leaving it out would let the print path hand over the original bytes
        // for a document the reader has highlighted --- which prints, correctly
        // and confusingly, without the highlights.
        self.marks.is_empty() && self.redactions.is_empty() && self.pages_are_the_file()
    }

    /// Whether the only thing this adds to the file is marks.
    ///
    /// **The predicate that decides whether a save can be an append**, and it is
    /// deliberately narrow: every page present, in order, unturned and
    /// uncropped, and at least one mark. Adding an annotation touches the page's
    /// `/Annots` and nothing else, which is the edit `docs/PLAN.md` §5 measured
    /// through four independent parsers --- PDFium, QPDF, poppler and
    /// CoreGraphics. A deletion, a move, a turn or a crop rewrites the page tree
    /// or the page dictionary in ways that spike never put to them, so they
    /// take the rewrite. Where the evidence stops, so does this.
    ///
    /// It shares [`Plan::is_identity`]'s page walk rather than repeating it,
    /// because the two questions differ in exactly one clause --- and two copies
    /// of a page walk is how one of them comes to disagree about the crop, which
    /// is a thing that has already happened here once.
    #[must_use]
    pub fn only_adds_marks(&self) -> bool {
        !self.marks.is_empty() && self.redactions.is_empty() && self.pages_are_the_file()
    }

    /// Whether the pages are the file's, in the file's order and shape.
    ///
    /// The half [`Plan::is_identity`] and [`Plan::only_adds_marks`] share. It
    /// says nothing about marks, which is all that separates them.
    fn pages_are_the_file(&self) -> bool {
        self.pages.len() == self.baseline as usize
            && self.pages.iter().enumerate().all(|(at, page)| {
                // **Destructured so that a new field cannot be forgotten.** The
                // crop was, for as long as pages could be cropped, and the reason
                // is structural rather than careless: this predicate is a list of
                // the ways a document can differ from its file, and a list like
                // that is wrong the moment somebody adds a way --- silently, and
                // in the direction that reads as "nothing was edited".
                //
                // Written this way round, a tenth field on `PageView` is
                // `error[E0027]` here until whoever added it says which half it
                // is in. `id` is the only one that is deliberately ignored: it is
                // the model's name for the page and says nothing about whether
                // the page differs from the file.
                let PageView {
                    id: _,
                    source,
                    turns,
                    crop,
                } = page;
                *source as usize == at && turns % 4 == 0 && crop.is_none()
            })
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
        Refusal::NoSuchMark(_) => "no such mark".into(),
        Refusal::MarkRemoved(_) => "that mark has already been removed".into(),
        Refusal::EmptyMark => "that mark covers nothing".into(),
        // Not worded for a reader, because no reader can cause it: it means the
        // wire and the model disagree about what a mark is --- strokes on a kind
        // that is not ink, or ink with none. Naming the kind is what makes the
        // report actionable for whoever sent it.
        Refusal::ShapeMismatch(kind) => {
            format!("a {kind:?} mark cannot carry the strokes it was sent with")
        }
        // The stamp half of the same situation, and worded so the two cannot be
        // confused in a report: a stamp needs a name and nothing else may have
        // one.
        Refusal::StampMismatch(kind) => {
            format!("a {kind:?} mark cannot carry the stamp name it was sent with")
        }
        // The three redaction refusals, worded for a reader rather than for a
        // sender: unlike the two above, every one of them is reachable from a
        // panel row that has gone stale under a reader's own undo.
        Refusal::NoSuchRedaction(_) => "no such redaction".into(),
        Refusal::RedactionRemoved(_) => "that redaction has already been removed".into(),
        Refusal::EmptyRedaction => "that region covers nothing".into(),
    }
}

/// Refuses a note past [`crate::textbox::MAX_NOTE_CHARS`].
///
/// One function rather than the check written at each of the two doors a note
/// arrives through, so that the bound and its wording cannot differ between
/// them --- `docs/TRAPS.md` records a distinction kept in two copies drifting
/// until a mutation of one survived.
///
/// Counted in characters rather than bytes, because that is what the reader
/// typed and what [`crate::textbox::wrap`] walks; a byte bound would refuse a
/// German note a third shorter than an English one.
///
/// # Errors
///
/// The note is longer than the bound.
fn too_long(note: &str) -> Result<(), String> {
    let chars = note.chars().count();
    if chars > crate::textbox::MAX_NOTE_CHARS {
        return Err(format!(
            "that note is {chars} characters, and a note holds at most {}",
            crate::textbox::MAX_NOTE_CHARS
        ));
    }
    Ok(())
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
                crop: page.crop.map(|r| [r.llx, r.lly, r.urx, r.ury]),
            }
        })
        .collect();
    let marks = working
        .all_marks()
        .into_iter()
        .map(|(page, id)| {
            let mark = model.mark(id).expect("a live mark has a body");
            MarkView {
                id: id.get(),
                kind: mark.kind,
                // Off the body rather than through an accessor, unlike the
                // geometry below: a stamp's name is fixed at the moment it is
                // made, so there is no "what it is now" for the model to answer.
                stamp: mark.stamp,
                page: page.get(),
                // **Through the model's accessors, not off the body**, because
                // an eraser moves both: `Doc::quads_of` and `Doc::strokes_of`
                // answer what is drawn *now*, and the body still holds what was
                // drawn first. Taking `mark.quads` here would put the popup and
                // the hit test around a stroke that has gone.
                quads: model
                    .quads_of(id)
                    .iter()
                    .flat_map(|q| [q.left, q.top, q.right, q.bottom])
                    .collect(),
                strokes: model
                    .strokes_of(id)
                    .iter()
                    .map(|stroke| stroke.points.iter().flat_map(|p| [p.x, p.y]).collect())
                    .collect(),
                // The accessor for the reason above it: `Doc::color_of` answers
                // what the mark is drawn in *now*, and the body still holds what
                // it was made in. Taking `mark.color` here would leave the
                // overlay painting a recoloured mark in its first colour.
                color: model.color_of(id),
                note: model.note_of(id).to_string(),
                // Wrapped to the mark's own rectangle, so the overlay's line
                // breaks are the file's. Empty for every kind that is not a text
                // box --- there is nothing to lay out, and sending the note
                // twice for a highlight would be a second copy of a string the
                // frontend already has.
                lines: if mark.kind == MarkKind::TextBox {
                    let width = model.quads_of(id).first().map_or(0.0, |q| {
                        f64::from(q.right - q.left) - crate::textbox::INSET * 2.0
                    });
                    crate::textbox::wrap(model.note_of(id), crate::textbox::SIZE, width.max(1.0))
                } else {
                    Vec::new()
                },
            }
        })
        .collect();

    let redactions = working
        .all_redactions()
        .into_iter()
        .map(|(page, id)| {
            let redaction = model.redaction(id).expect("a live redaction has a body");
            RedactionView {
                id: id.get(),
                page: page.get(),
                // Off the body, unlike a mark's geometry, and the difference is
                // real rather than an inconsistency: a mark's quads are read
                // through an accessor because an eraser can move them, and
                // nothing moves a redaction. The day one does, this becomes an
                // accessor and the comment on `MarkView::quads` explains why.
                area: [
                    redaction.area.left,
                    redaction.area.top,
                    redaction.area.right,
                    redaction.area.bottom,
                ],
            }
        })
        .collect();

    let (applied, _) = model.depth();
    EditState {
        pages,
        marks,
        redactions,
        can_undo: model.can_undo(),
        can_redo: model.can_redo(),
        dirty: applied > 0,
    }
}

#[cfg(test)]
mod tests {

    /// A plan waits for the fingerprint the open started, rather than racing it.
    ///
    /// The open deliberately returns before the hash is done --- it costs 452 ms
    /// on the 337 MB fixture and the cold-start budget is 300 ms --- so the whole
    /// design rests on the wait being somewhere. If `plan` did not wait, a save
    /// issued promptly after an open would see no fingerprint and refuse with
    /// "could not record", which reads as a broken file rather than as a race.
    #[test]
    fn a_plan_carries_the_fingerprint_the_open_started() {
        let dir = std::env::temp_dir().join(format!("tpdf-edits-fp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        let file = dir.join("source.bin");
        std::fs::write(&file, b"some bytes").expect("write");

        let edits = Edits::default();
        edits.open(1, 2, Some(file.clone()));
        let plan = edits.plan(1).expect("plan");
        assert!(
            plan.opened_as.is_some(),
            "the plan must carry what the file was, not race the thread that read it"
        );
        assert_eq!(plan.opened_as.expect("some").len, 10);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A document opened with no path settles immediately rather than hanging.
    ///
    /// The control for the test above, and it is the one that would fail as a
    /// **hang** rather than as a red test: nothing starts a thread here, so a cell
    /// nobody sets would leave every later `plan` waiting for ever. `docs/TRAPS.md`
    /// has that shape twice --- a check whose failure mode is a wait cannot fail.
    #[test]
    fn a_document_with_no_path_plans_with_no_fingerprint_and_does_not_wait() {
        let edits = Edits::default();
        edits.open(1, 2, None);
        let plan = edits.plan(1).expect("plan");
        assert!(plan.opened_as.is_none());
    }
    use super::*;

    /// A three-page document with a model, and its handle.
    fn opened() -> Edits {
        let edits = Edits::default();
        edits.open(7, 3, None);
        edits
    }

    #[test]
    fn a_subset_plan_names_the_pages_asked_for_and_keeps_the_file_s_baseline() {
        let edits = opened();
        let plan = edits.plan_subset(7, &[0, 2]).expect("subset");
        assert_eq!(
            plan.pages.iter().map(|p| p.source).collect::<Vec<_>>(),
            vec![0, 2]
        );
        // The baseline describes the FILE, not the selection. A plan of two
        // pages with a baseline of two would tell `write_copy` that the source
        // has two pages, which is the external-modification lie its check
        // exists to catch.
        assert_eq!(plan.baseline, 3);
    }

    #[test]
    fn a_subset_plan_carries_the_turns_the_reader_applied() {
        // Extract writes what the reader is looking at, so a page turned in the
        // working document comes out turned. Nothing else in this file would
        // notice if the subset dropped the turns: every other assertion here is
        // about which pages, not how they sit.
        let edits = opened();
        let middle = edits.state(7).expect("open").pages[1].id;
        edits.rotate(7, middle, 1).expect("rotate");
        let plan = edits.plan_subset(7, &[1]).expect("subset");
        assert_eq!(plan.pages.len(), 1);
        assert_eq!(plan.pages[0].turns, 1);
    }

    #[test]
    fn a_subset_plan_reads_the_current_order_rather_than_the_file_s() {
        // A reader who moved a page and then extracted "the first two" means
        // the two they can see. This is what makes slots the right vocabulary
        // here, and it fails loudly if the subset ever indexes the baseline.
        let edits = opened();
        let last = edits.state(7).expect("open").pages[2].id;
        edits.move_page(7, last, None).expect("move to the front");
        let plan = edits.plan_subset(7, &[0]).expect("subset");
        assert_eq!(plan.pages[0].source, 2, "the moved page is now slot 0");
    }

    #[test]
    fn extracting_changes_nothing_about_the_document() {
        // The property that makes this a plan rather than a command: no journal
        // entry, nothing to undo, and not dirty.
        let edits = opened();
        let before = edits.state(7).expect("open");
        edits.plan_subset(7, &[0, 1]).expect("subset");
        let after = edits.state(7).expect("open");
        assert_eq!(before, after);
        assert!(!after.dirty);
        assert!(!after.can_undo);
    }

    #[test]
    fn an_empty_selection_is_refused_here_rather_than_by_the_writer() {
        let edits = opened();
        assert!(edits.plan_subset(7, &[]).is_err());
    }

    #[test]
    fn a_slot_past_the_end_is_refused_and_the_message_says_how_many_there_are() {
        let edits = opened();
        let why = edits.plan_subset(7, &[3]).expect_err("out of range");
        assert!(why.contains('3'), "names the count: {why}");
    }

    #[test]
    fn a_slot_named_twice_is_refused_rather_than_written_twice() {
        // The frontend deduplicates, so this can only arrive from a defect --
        // and a page written twice produces a valid PDF that nobody asked for,
        // which nothing downstream could report.
        let edits = opened();
        assert!(edits.plan_subset(7, &[0, 0]).is_err());
    }

    #[test]
    fn slots_out_of_order_are_refused_rather_than_silently_reordering() {
        // Extract produces a subset. Reordering is what Move is for, and one
        // operation quietly doing both would make `5,1` mean something no
        // reader could predict.
        let edits = opened();
        assert!(edits.plan_subset(7, &[2, 0]).is_err());
    }

    #[test]
    fn a_subset_of_every_page_in_order_is_the_document_itself() {
        // The identity check is what lets the print path hand a file over byte
        // for byte, so a full-document extract must still be recognised as one.
        let edits = opened();
        let plan = edits.plan_subset(7, &[0, 1, 2]).expect("subset");
        assert!(plan.is_identity());
    }

    #[test]
    fn a_subset_that_drops_a_page_is_not_the_document() {
        let edits = opened();
        let plan = edits.plan_subset(7, &[0, 1]).expect("subset");
        assert!(!plan.is_identity(), "two of three pages is not the file");
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
        edits.open(3, 1, None);
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
    fn a_moved_page_lands_behind_the_page_it_named_and_keeps_its_identity() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        // The first page to the end: behind the last one.
        let after = edits
            .move_page(7, pages[0].id, Some(pages[2].id))
            .expect("move");

        assert_eq!(
            after.pages.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![pages[1].id, pages[2].id, pages[0].id]
        );
        assert_eq!(
            after.pages.iter().map(|p| p.source).collect::<Vec<_>>(),
            vec![1, 2, 0],
            "and the sources say which page of the FILE is now in each slot --- \
             the equality between the two is what a move breaks without changing \
             the page count, which is how it differs from a deletion"
        );
        assert!(after.dirty);
        assert!(after.can_undo);
    }

    #[test]
    fn a_move_to_the_front_names_no_anchor() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        let after = edits.move_page(7, pages[2].id, None).expect("move");
        assert_eq!(
            after.pages.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![pages[2].id, pages[0].id, pages[1].id]
        );
    }

    #[test]
    fn a_moved_page_keeps_the_turn_it_had() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        edits.rotate(7, pages[0].id, 1).expect("rotate");
        let after = edits
            .move_page(7, pages[0].id, Some(pages[2].id))
            .expect("move");
        assert_eq!(
            after.pages.iter().map(|p| p.turns).collect::<Vec<_>>(),
            vec![0, 0, 1],
            "the turn travelled with the page rather than staying in slot 0"
        );
    }

    #[test]
    fn undo_puts_a_moved_page_back_where_it_came_from() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        edits
            .move_page(7, pages[0].id, Some(pages[2].id))
            .expect("move");
        let back = edits.undo(7).expect("undo");
        assert_eq!(
            back.pages.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![pages[0].id, pages[1].id, pages[2].id]
        );
        assert!(!back.dirty);
    }

    #[test]
    fn a_page_cannot_be_moved_behind_itself() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        let why = edits
            .move_page(7, pages[1].id, Some(pages[1].id))
            .expect_err("must refuse");
        assert_eq!(why, "a page cannot be moved after itself");
        assert!(
            !edits.state(7).expect("state").dirty,
            "a refusal does not enter the journal"
        );
    }

    #[test]
    fn a_move_naming_a_deleted_anchor_says_the_anchor_was_deleted() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        edits.delete(7, pages[2].id).expect("delete");
        let why = edits
            .move_page(7, pages[0].id, Some(pages[2].id))
            .expect_err("gone");
        assert_eq!(
            why, "that page has been deleted",
            "the anchor is checked as carefully as the subject --- a frontend one \
             state behind is as likely to name a stale neighbour as a stale page"
        );
        assert_eq!(
            edits.state(7).expect("state").pages.len(),
            2,
            "and the move did not half-happen"
        );
    }

    #[test]
    fn a_plan_after_a_move_is_out_of_document_order() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        edits
            .move_page(7, pages[0].id, Some(pages[2].id))
            .expect("move");

        let plan = edits.plan(7).expect("plan");
        assert_eq!(plan.baseline, 3);
        assert_eq!(
            plan.pages.iter().map(|p| p.source).collect::<Vec<_>>(),
            vec![1, 2, 0],
            "which is what `save.rs` and `print.rs` rebuild a page tree for"
        );
        assert!(
            !plan.is_identity(),
            "every page is present and unturned, and this is still not the file \
             on disk --- a length-and-turns check would have said it was"
        );
    }

    #[test]
    fn a_plan_after_a_moved_page_lands_back_where_it_started_is_the_file_again() {
        let edits = opened();
        let pages = edits.state(7).expect("open").pages;
        edits
            .move_page(7, pages[0].id, Some(pages[2].id))
            .expect("move");
        edits.move_page(7, pages[0].id, None).expect("move back");

        let plan = edits.plan(7).expect("plan");
        assert!(
            plan.is_identity(),
            "the control for the check above: `is_identity` is reading the order \
             rather than counting the commands"
        );
        assert!(edits.state(7).expect("state").dirty);
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
    fn a_cropped_document_is_not_the_file_on_disk() {
        // **The clause that was missing, and what it cost.** A cropped page kept
        // its position, its turn and its lack of marks, so `is_identity`
        // answered `true` and the print path handed the printer the file as it
        // is on disk --- uncropped. Measured before the fix: a plan carrying
        // `crop: Some(...)` reported `is_identity = true` and `print::select`
        // answered `Pages::All`.
        //
        // Beside `only_an_unedited_document_is_the_file_on_disk` rather than
        // inside it, because that test walks a sequence of edits and this is
        // about one field being consulted at all --- and a test that grows a
        // fifth `assert!` is a test whose name stops describing it.
        let edits = opened();
        let first = edits.state(7).expect("open").pages[0].id;
        assert!(edits.plan(7).expect("plan").is_identity(), "the control");

        edits
            .crop(7, first, Some([100.0, 100.0, 400.0, 500.0]))
            .expect("crop");
        assert!(
            !edits.plan(7).expect("plan").is_identity(),
            "a crop is a change to what the page shows, so the file is not this document"
        );

        // And back, which is what says the predicate reads the crop rather than
        // counting commands --- the same control the reorder test above states
        // for the order.
        edits.crop(7, first, None).expect("uncrop");
        assert!(
            edits.plan(7).expect("plan").is_identity(),
            "cleared, so the page shows what the file shows again"
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
        edits.open(7, 5, None);
        let state = edits.state(7).expect("reopened");
        assert_eq!(state.pages.len(), 5, "the new document's page count");
        assert!(state.pages.iter().all(|page| page.turns == 0));
        assert!(!state.dirty);
        assert!(!state.can_undo, "the previous document's journal is gone");
    }

    #[test]
    fn two_documents_keep_their_own_journals() {
        let edits = opened();
        edits.open(8, 2, None);
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

    /// A highlight over one line of the page with `page` as its id.
    fn a_mark(page: u64) -> NewMark {
        of_kind(MarkKind::Highlight, page)
    }

    /// The same, of whichever kind. Separate so that the existing tests read as
    /// they did and the ones about the kind name it.
    fn of_kind(kind: MarkKind, page: u64) -> NewMark {
        NewMark {
            kind,
            stamp: None,
            page,
            quads: vec![72.0, 100.0, 300.0, 118.0],
            strokes: Vec::new(),
            color: [1.0, 0.9, 0.2],
            author: "a reader".to_string(),
            note: String::new(),
        }
    }

    fn stamped() -> String {
        "D:20260818120000Z".to_string()
    }

    /// Three strokes well apart, the shape the eraser tests need.
    fn three_strokes(page: u64) -> NewMark {
        a_drawing(
            page,
            vec![
                vec![72.0, 90.0, 300.0, 90.0],
                vec![72.0, 150.0, 300.0, 150.0],
                vec![72.0, 210.0, 300.0, 210.0],
            ],
        )
    }

    #[test]
    fn one_sweep_is_one_undo() {
        // The reason `erase` takes a list rather than being called once per
        // stroke: a reader sweeping across two strokes did one thing, so one
        // press of undo puts both back. Per-stroke calls would need two.
        let edits = opened();
        let page = edits.state(7).expect("open").pages[0].id;
        let state = edits
            .annotate(7, three_strokes(page), stamped())
            .expect("drawn");
        let mark = state.marks[0].id;

        let state = edits.erase(7, mark, vec![0, 2]).expect("erased");
        assert_eq!(state.marks.len(), 1, "the drawing is still there");
        assert_eq!(state.marks[0].strokes.len(), 1, "with its middle stroke");
        assert_eq!(state.marks[0].strokes[0], vec![72.0, 150.0, 300.0, 150.0]);

        let state = edits.undo(7).expect("undo");
        assert_eq!(
            state.marks[0].strokes.len(),
            3,
            "one undo, not two -- the sweep was one command"
        );
    }

    #[test]
    fn erasing_the_last_stroke_takes_the_drawing_with_it() {
        let edits = opened();
        let page = edits.state(7).expect("open").pages[0].id;
        let state = edits
            .annotate(7, three_strokes(page), stamped())
            .expect("drawn");
        let mark = state.marks[0].id;

        let state = edits.erase(7, mark, vec![0, 1, 2]).expect("erased");
        assert!(
            state.marks.is_empty(),
            "a drawing of nothing was left behind: {:?}",
            state.marks
        );

        let state = edits.undo(7).expect("undo");
        assert_eq!(
            state.marks.len(),
            1,
            "and one undo brings the whole drawing back, not an empty one"
        );
        assert_eq!(state.marks[0].strokes.len(), 3);
    }

    #[test]
    fn a_gesture_aimed_at_a_stroke_that_is_not_there_is_refused_whole() {
        let edits = opened();
        let page = edits.state(7).expect("open").pages[0].id;
        let state = edits
            .annotate(7, three_strokes(page), stamped())
            .expect("drawn");
        let mark = state.marks[0].id;

        // Position 0 is real and position 9 is not. The refusal has to take the
        // whole gesture: acting on the half it understood would erase a stroke
        // on the strength of a sweep aimed at a drawing this is not.
        let why = edits.erase(7, mark, vec![0, 9]).expect_err("refused");
        assert!(why.contains("stroke 9"), "{why}");
        assert!(why.contains("has 3"), "{why}");
        assert_eq!(
            edits.state(7).expect("state").marks[0].strokes.len(),
            3,
            "the stroke it did understand was erased anyway"
        );
    }

    #[test]
    fn erasing_the_same_stroke_twice_in_one_sweep_is_not_an_error() {
        // A sweep reports what it touched, and a slow drag touches one stroke
        // many times. Deduplicating in the frontend would be a rule two layers
        // have to agree on; tolerating it here is one.
        let edits = opened();
        let page = edits.state(7).expect("open").pages[0].id;
        let state = edits
            .annotate(7, three_strokes(page), stamped())
            .expect("drawn");
        let mark = state.marks[0].id;
        let state = edits.erase(7, mark, vec![1, 1, 1]).expect("erased");
        assert_eq!(state.marks[0].strokes.len(), 2);
    }

    #[test]
    fn the_reply_carries_the_rectangle_the_drawing_has_now() {
        // `snapshot`'s half, and it needs a test of its own: the model's
        // `quads_of` is proved in `docmodel.rs` against the accessor directly,
        // which says nothing about whether the reply asks it. Aiming a mutation
        // of this line at that test left it SURVIVED -- the trap about unit
        // tests that build their fixtures below the layer under test.
        let edits = opened();
        let page = edits.state(7).expect("open").pages[0].id;
        let state = edits
            .annotate(7, three_strokes(page), stamped())
            .expect("drawn");
        let mark = state.marks[0].id;
        let before = state.marks[0].quads.clone();

        let state = edits.erase(7, mark, vec![2]).expect("erased");
        let after = &state.marks[0].quads;
        assert_eq!(after.len(), 4, "a drawing is one rectangle");
        assert!(
            after[3] < before[3],
            "the reply's rectangle still reaches the erased stroke: {after:?} against {before:?}"
        );
    }

    /// The green in the swatch row, and not any mark's default.
    const GREEN: [f32; 3] = [0.35, 0.8, 0.35];

    #[test]
    fn the_reply_carries_the_colour_the_mark_has_now() {
        // `snapshot`'s half, and it needs its own test for the reason the
        // rectangle's does: `docmodel`'s `color_of` is proved against the
        // accessor directly, which says nothing about whether the reply asks
        // it. The overlay paints from this field, so taking `mark.color` here
        // leaves a recoloured mark on screen in the colour it was made in.
        let edits = opened();
        let page = edits.state(7).expect("open").pages[0].id;
        let state = edits.annotate(7, a_mark(page), stamped()).expect("marked");
        let mark = state.marks[0].id;
        assert_ne!(state.marks[0].color, GREEN, "the fixture is already green");

        let state = edits.recolor(7, mark, GREEN).expect("recoloured");
        assert_eq!(state.marks[0].color, GREEN);
    }

    #[test]
    fn a_saved_file_is_written_in_the_colour_the_mark_has_now() {
        // The plan is what `save.rs` writes `/C` and the appearance stream from,
        // and it reads the same accessor. Without this a recolour would show on
        // screen and save in the old colour --- which is the shape of wrong
        // `markband.ts` was written to end: the reader cannot tell until the
        // file is reopened, and then the mark changes under them.
        let edits = opened();
        let page = edits.state(7).expect("open").pages[0].id;
        let state = edits.annotate(7, a_mark(page), stamped()).expect("marked");
        let mark = state.marks[0].id;

        edits.recolor(7, mark, GREEN).expect("recoloured");
        assert_eq!(edits.plan(7).expect("plan").marks[0].color, GREEN);
    }

    #[test]
    fn a_colour_that_is_not_a_number_is_clamped_at_this_door_too() {
        // The second route into `/C`. `channel`'s note is about a `NewMark`, and
        // a recolour reaches the same field by a different command --- so a
        // sender could put `inf` in a content stream through the one that only
        // changes an appearance, and `save.rs` would write the three letters.
        let edits = opened();
        let page = edits.state(7).expect("open").pages[0].id;
        let state = edits.annotate(7, a_mark(page), stamped()).expect("marked");
        let mark = state.marks[0].id;

        let state = edits
            .recolor(7, mark, [f32::INFINITY, -1.0, 2.0])
            .expect("recoloured");
        assert_eq!(state.marks[0].color, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn recolouring_a_mark_that_is_gone_says_which_of_the_two_it_is() {
        let edits = opened();
        let page = edits.state(7).expect("open").pages[0].id;
        let state = edits.annotate(7, a_mark(page), stamped()).expect("marked");
        let mark = state.marks[0].id;
        edits.unannotate(7, mark).expect("removed");

        assert_eq!(
            edits.recolor(7, mark, GREEN),
            Err("that mark has already been removed".to_string())
        );
        assert_eq!(
            edits.recolor(7, 9999, GREEN),
            Err("no such mark".to_string())
        );
        assert_eq!(edits.recolor(8, mark, GREEN), Err(unknown(8)));
    }

    #[test]
    fn a_saved_file_is_written_from_what_survived_the_eraser() {
        // The plan is what `save.rs` writes `/InkList` and `/Rect` from, and it
        // reads the same accessors the snapshot does. Without that, a document
        // could look right in the window and save the erased stroke.
        let edits = opened();
        let page = edits.state(7).expect("open").pages[0].id;
        let state = edits
            .annotate(7, three_strokes(page), stamped())
            .expect("drawn");
        let mark = state.marks[0].id;
        let before = edits.plan(7).expect("plan").marks[0].quads[0];

        edits.erase(7, mark, vec![2]).expect("erased");
        let planned = &edits.plan(7).expect("plan").marks[0];
        assert_eq!(
            planned.strokes.len(),
            2,
            "the plan still holds three strokes"
        );
        assert!(
            planned.quads[0].bottom < before.bottom,
            "the rectangle a save writes still reaches the erased stroke"
        );
    }

    /// A drawing sent the way the viewer sends one: strokes, and no rectangle.
    fn a_drawing(page: u64, strokes: Vec<Vec<f32>>) -> NewMark {
        NewMark {
            kind: MarkKind::Ink,
            stamp: None,
            page,
            quads: Vec::new(),
            strokes,
            color: [0.85, 0.15, 0.15],
            author: "a reader".to_string(),
            note: String::new(),
        }
    }

    #[test]
    fn a_drawings_rectangle_is_derived_here_and_padded_by_half_a_line() {
        // **This is the only test that reaches the derivation.** `docmodel`'s
        // own tests build the `Mark` by hand, so they exercise `Stroke::bounds`
        // and say nothing about whether anything calls it or with what --- which
        // is how the mutation that takes the pad to zero came back SURVIVED with
        // the model's tests all green. The harness found a real gap, and this is
        // it: the padding decision lives on this side of the boundary.
        //
        // A straight vertical line, which is what a reader ruling a margin
        // draws. Its tight bounds have **no width**, and `covers_area` rejects
        // those --- so without the pad this is refused and a reader is told the
        // line they can see covers nothing.
        let edits = opened();
        let id = edits.state(7).expect("state").pages[1].id;
        let vertical = vec![vec![50.0, 50.0, 50.0, 300.0]];

        let state = edits
            .annotate(7, a_drawing(id, vertical), stamped())
            .expect("a straight line is a drawing");
        let mark = &state.marks[0];

        // One rectangle, which the sender did not supply.
        assert_eq!(
            mark.quads.len(),
            4,
            "one derived rectangle: {:?}",
            mark.quads
        );
        let [left, top, right, bottom] =
            [mark.quads[0], mark.quads[1], mark.quads[2], mark.quads[3]];
        let pad = (crate::docmodel::INK_WIDTH / 2.0) as f32;
        assert!(
            (left - (50.0 - pad)).abs() < 0.01 && (right - (50.0 + pad)).abs() < 0.01,
            "the rectangle is the stroke grown by half a line: {left} {right}"
        );
        assert!(
            (top - (50.0 - pad)).abs() < 0.01 && (bottom - (300.0 + pad)).abs() < 0.01,
            "and in the other direction too: {top} {bottom}"
        );
        // The strokes reach the reply as well, or the overlay cannot draw them.
        assert_eq!(mark.strokes, vec![vec![50.0, 50.0, 50.0, 300.0]]);
    }

    /// A region to mark for removal, in the shape the wire sends.
    const REGION: [f32; 4] = [72.0, 100.0, 300.0, 118.0];

    #[test]
    fn a_pending_redaction_reaches_the_reply_and_no_plan() {
        // **The load-bearing test of the whole two-list arrangement.** A
        // redaction is an instruction to destroy content; a plan is what a save
        // writes into a file. If one could reach the other, tpdf would write a
        // reader's pending redactions into a document as annotations --- an
        // outline over words that are still there, in a file that has been
        // handed on. Both directions in one test, because a reply that carried
        // it and a plan that also did would satisfy either half alone.
        let edits = opened();
        let page = edits.state(7).expect("state").pages[1].id;
        edits
            .annotate(7, a_mark(page), stamped())
            .expect("the model takes the mark");
        let state = edits.redact(7, page, REGION).expect("the model takes it");

        assert_eq!(state.redactions.len(), 1);
        assert_eq!(state.redactions[0].page, page);
        assert_eq!(state.redactions[0].area, REGION);
        // The mark is still there and is still the only thing a writer sees.
        assert_eq!(state.marks.len(), 1);
        assert_eq!(edits.plan(7).expect("plan").marks.len(), 1);

        // And the control: the plan of a document with a redaction is the plan
        // of the same document without one. Asserting `marks.len() == 1` alone
        // would pass for a plan that had grown a second field carrying it.
        let clean = Edits::default();
        clean.open(8, 3, None);
        let same = clean.state(8).expect("state").pages[1].id;
        clean
            .annotate(8, a_mark(same), stamped())
            .expect("the model takes the mark");
        assert_eq!(
            edits.plan(7).expect("plan").marks,
            clean.plan(8).expect("plan").marks
        );
    }

    #[test]
    fn a_pending_redaction_makes_the_document_dirty_and_is_undoable() {
        // Dirty because it is an unsaved change to what the reader is looking
        // at --- the overlay draws it --- even though no byte of the document
        // has moved. Undoable because step 2 is a review, and a review a reader
        // cannot act on is a formality.
        let edits = opened();
        let page = edits.state(7).expect("state").pages[1].id;
        assert!(!edits.state(7).expect("state").dirty);

        let state = edits.redact(7, page, REGION).expect("the model takes it");
        assert!(state.dirty);
        assert!(state.can_undo);

        let back = edits.undo(7).expect("undo");
        assert!(back.redactions.is_empty());
        assert!(!back.dirty);
    }

    #[test]
    fn a_redaction_can_be_taken_back_off_by_the_identity_the_reply_gave_it() {
        let edits = opened();
        let page = edits.state(7).expect("state").pages[1].id;
        let id = edits.redact(7, page, REGION).expect("redact").redactions[0].id;

        assert!(edits
            .unredact(7, id)
            .expect("unredact")
            .redactions
            .is_empty());
        // A stale panel row asking twice is told which answer it is getting,
        // and an id nobody issued is the other one.
        assert_eq!(
            edits.unredact(7, id),
            Err("that redaction has already been removed".to_string())
        );
        assert_eq!(edits.unredact(7, 999), Err("no such redaction".to_string()));
    }

    #[test]
    fn a_region_whose_geometry_is_not_finite_is_refused_rather_than_marked() {
        // `a_mark_whose_geometry_is_not_finite_is_refused_rather_than_written`
        // for the redaction door. The damage differs: an infinite corner on a
        // mark writes `inf` into a content stream, and on a region it compares
        // true against every object on the page --- one drag that silently
        // covers the document.
        let edits = opened();
        let page = edits.state(7).expect("state").pages[1].id;

        for bad in [f32::INFINITY, f32::NEG_INFINITY] {
            assert!(edits
                .redact(7, page, [72.0, 100.0, bad, 118.0])
                .expect_err("refused")
                .starts_with("a redaction cannot have a corner at"));
        }
        // NaN is caught here rather than by the model's own emptiness check,
        // because `is_finite` is false for it and this guard is the outer one.
        // That is the trap about a caller that validates first: the model's
        // `EmptyRedaction` for a NaN corner is unreachable through this door,
        // and it is covered by the test that calls `Doc::redact` directly.
        assert_eq!(
            edits.redact(7, page, [72.0, 100.0, f32::NAN, 118.0]),
            Err("a redaction cannot have a corner at NaN".to_string())
        );
        assert!(edits.state(7).expect("state").redactions.is_empty());
    }

    #[test]
    fn a_mark_is_carried_into_the_plan_for_the_page_it_is_on() {
        let edits = opened();
        let id = edits.state(7).expect("state").pages[1].id;
        edits
            .annotate(7, a_mark(id), stamped())
            .expect("the model takes the mark");

        let plan = edits.plan(7).expect("plan");
        assert_eq!(plan.marks.len(), 1);
        // The *baseline* page, which is what a writer walking the page tree has.
        // A plan naming the model's id would name nothing `lopdf` knows.
        assert_eq!(plan.marks[0].source, 1);
        assert_eq!(plan.marks[0].author, "a reader");
    }

    #[test]
    fn a_crop_reaches_the_reply_and_the_plan_and_clearing_it_removes_it() {
        // The crop crosses the boundary twice, like a mark's kind: in on the
        // command, out on `PageView`, and through the plan to the writer. Both
        // directions in one test, because a reply that carried it and a plan
        // that did not would put a crop on screen that no saved file has.
        let edits = opened();
        let id = edits.state(7).expect("state").pages[1].id;
        let want = [10.0, 20.0, 300.0, 400.0];

        let state = edits.crop(7, id, Some(want)).expect("the model takes it");
        assert_eq!(state.pages[1].crop, Some(want));
        assert_eq!(edits.plan(7).expect("plan").pages[1].crop, Some(want));
        // The control: only that page. A crop written across the order would
        // satisfy every assertion above.
        assert_eq!(state.pages[0].crop, None);

        let cleared = edits.crop(7, id, None).expect("the model clears it");
        assert_eq!(cleared.pages[1].crop, None);
        assert_eq!(edits.plan(7).expect("plan").pages[1].crop, None);
    }

    #[test]
    fn a_crop_that_encloses_nothing_is_refused_in_the_reader_s_words() {
        // The corners the wrong way round, which is what a rectangle dragged
        // from bottom-right to top-left produces before anyone normalises it.
        // Refused rather than normalised: the model is not the place to guess
        // which of two readings a caller meant.
        let edits = opened();
        let id = edits.state(7).expect("state").pages[0].id;
        let why = edits
            .crop(7, id, Some([300.0, 400.0, 10.0, 20.0]))
            .expect_err("backwards corners");
        assert_eq!(why, "that crop encloses no area");
        // A refusal changes nothing, which is the model's own guarantee and is
        // worth asserting here because this is the caller that would notice.
        assert_eq!(edits.state(7).expect("state").pages[0].crop, None);
    }

    #[test]
    fn the_kind_the_caller_asked_for_reaches_the_plan_and_the_reply() {
        // The one field a caller chooses that the model does not decide, and it
        // crosses the boundary twice: in on `NewMark`, out on `MarkView`, and
        // through the plan to the writer in between. All three kinds in one
        // test rather than one each, so a `kind` hardcoded anywhere along the
        // way cannot be satisfied by whichever one happens to be the default.
        let edits = opened();
        let id = edits.state(7).expect("state").pages[1].id;
        for kind in [
            MarkKind::Highlight,
            MarkKind::Underline,
            MarkKind::StrikeOut,
        ] {
            let state = edits
                .annotate(7, of_kind(kind, id), stamped())
                .expect("the model takes the mark");
            let mark = state.marks.last().expect("a mark");
            assert_eq!(mark.kind, kind, "the reply says {:?}", mark.kind);
            let plan = edits.plan(7).expect("plan");
            assert_eq!(
                plan.marks.last().expect("a planned mark").kind,
                kind,
                "the plan says otherwise"
            );
        }
    }

    #[test]
    fn a_colour_off_the_wire_is_brought_into_the_range_the_model_promises() {
        // `Mark::color` says "in 0..=1" and nothing made it so. The value that
        // matters is not a big number, it is a *non-finite* one: JSON refuses
        // `NaN` and `Infinity` as literals, but `1e40` is valid JSON and is
        // `f32::INFINITY` by the time it is an `f32`. `save.rs` writes each
        // channel with `format!`, so that reaches the content stream as the
        // three letters `inf`, which is a syntax error -- a file tpdf wrote,
        // signed its name to, and no reader can open.
        let edits = opened();
        let id = edits.state(7).expect("state").pages[1].id;
        let mut want = a_mark(id);
        want.color = [f32::INFINITY, -2.0, 40.0];
        edits
            .annotate(7, want, stamped())
            .expect("the mark is taken");

        let plan = edits.plan(7).expect("plan");
        let color = plan.marks[0].color;
        assert_eq!(color, [0.0, 0.0, 1.0], "got {color:?}");
        // And every channel is finite, which is the property `format!` needs
        // and which a range check on its own does not state.
        assert!(color.iter().all(|c| c.is_finite()), "{color:?}");
    }

    #[test]
    fn a_move_by_a_non_finite_offset_is_refused_rather_than_ignored() {
        // The colour's trap on the other door, and the answer differs. `1e40` is
        // valid JSON and is `f32::INFINITY` as an `f32`; added to a rectangle it
        // makes one `save.rs` writes as `inf` with `format!`, which is three
        // letters in the middle of a `/Rect`.
        //
        // **Refused where a colour is substituted**, because zero is a colour and
        // "do not move" is not a move: a reader whose drag silently did nothing
        // has no way to tell that from a broken viewer. No pointer can produce
        // one, so a sender that does is broken and is told so.
        let edits = opened();
        let id = edits.state(7).expect("state").pages[1].id;
        edits
            .annotate(7, a_mark(id), stamped())
            .expect("the mark is taken");
        let mark = edits.state(7).expect("state").marks[0].id;
        let home = edits.state(7).expect("state").marks[0].quads.clone();

        for (dx, dy) in [
            (f32::INFINITY, 0.0),
            (0.0, f32::NEG_INFINITY),
            (f32::NAN, 1.0),
        ] {
            let refused = edits.displace(7, mark, dx, dy);
            assert!(refused.is_err(), "({dx}, {dy}) was taken: {refused:?}");
        }
        assert_eq!(
            edits.state(7).expect("state").marks[0].quads,
            home,
            "and nothing moved"
        );

        // The control, and without it the loop above is satisfied by a method
        // that refuses everything.
        edits.displace(7, mark, 12.0, -3.0).expect("a real offset");
        let moved = edits.state(7).expect("state").marks[0].quads.clone();
        assert_eq!(moved[0], home[0] + 12.0);
        assert_eq!(moved[1], home[1] - 3.0);
        assert!(moved.iter().all(|v| v.is_finite()), "{moved:?}");
    }

    /// The same rule on the door the mark's own geometry comes through.
    ///
    /// `displace` refuses a non-finite *offset* and `channel` clamps a colour;
    /// the coordinates of the mark itself were taken as sent, which is the
    /// number that reaches `/Rect`, `/QuadPoints` and `/InkList`. The model
    /// does not catch it: `Quad::covers_area` is `right > left && bottom > top`,
    /// and an infinity satisfies both, so `[0, 0, 1e40, 1e40]` was a mark that
    /// covered area as far as every layer below here was concerned.
    ///
    /// Both shapes are checked because they arrive by different fields --- a
    /// highlight's rectangle and an ink stroke's points --- and a guard written
    /// for one is not a guard on the other. The control is what stops a method
    /// that refuses every mark from passing.
    #[test]
    fn a_mark_whose_geometry_is_not_finite_is_refused_rather_than_written() {
        let edits = opened();
        let page = edits.state(7).expect("state").pages[1].id;

        for bad in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            let mut want = a_mark(page);
            // The corner `covers_area` reads, so this is the value that would
            // have passed the model's own check.
            want.quads = vec![0.0, 0.0, bad, bad];
            let refused = edits.annotate(7, want, stamped());
            assert!(
                refused.is_err(),
                "a rectangle cornered at {bad} was taken: {refused:?}"
            );

            let mut want = of_kind(MarkKind::Ink, page);
            want.quads = Vec::new();
            want.strokes = vec![vec![10.0, 10.0, bad, 20.0]];
            let refused = edits.annotate(7, want, stamped());
            assert!(
                refused.is_err(),
                "a stroke through {bad} was taken: {refused:?}"
            );
        }
        assert!(
            edits.state(7).expect("state").marks.is_empty(),
            "and none of them reached the model"
        );

        // The control. Without it every assertion above is satisfied by an
        // `annotate` that refuses everything it is handed.
        edits
            .annotate(7, a_mark(page), stamped())
            .expect("an ordinary rectangle");
        let mut ink = of_kind(MarkKind::Ink, page);
        ink.quads = Vec::new();
        ink.strokes = vec![vec![10.0, 10.0, 40.0, 20.0]];
        edits
            .annotate(7, ink, stamped())
            .expect("an ordinary stroke");
        let marks = edits.state(7).expect("state").marks;
        assert_eq!(marks.len(), 2, "both ordinary marks were taken");
        assert!(
            marks.iter().all(|m| m.quads.iter().all(|v| v.is_finite())),
            "and what the model holds is finite: {marks:?}"
        );
    }

    /// A note past the bound is refused at both doors it can arrive through.
    ///
    /// `annotate` and `renote` are separate entry points and a guard on one is
    /// not a guard on the other --- which is why the check is one function
    /// called twice rather than two copies, and why this drives both.
    ///
    /// What it bounds is work: `wrap` runs for every text box in every state
    /// the model produces, so an unbounded note makes every later edit re-walk
    /// it under the lock. The control is a note comfortably longer than
    /// anything a person types and comfortably inside the bound, so a check
    /// that refused every note would fail here.
    #[test]
    fn a_note_past_the_bound_is_refused_at_both_doors() {
        let edits = opened();
        let page = edits.state(7).expect("state").pages[1].id;
        let bound = crate::textbox::MAX_NOTE_CHARS;

        let mut want = a_mark(page);
        want.note = "a".repeat(bound + 1);
        let refused = edits.annotate(7, want, stamped());
        assert!(refused.is_err(), "annotate took it: {refused:?}");
        assert!(
            edits.state(7).expect("state").marks.is_empty(),
            "and no mark reached the model"
        );

        // The control for that half, and the mark the second half needs.
        let mut want = a_mark(page);
        want.note = "a".repeat(bound);
        edits
            .annotate(7, want, stamped())
            .expect("a note exactly at the bound is a note");
        let mark = edits.state(7).expect("state").marks[0].id;

        let refused = edits.renote(7, mark, "b".repeat(bound + 1));
        assert!(refused.is_err(), "renote took it: {refused:?}");
        assert_eq!(
            edits.state(7).expect("state").marks[0].note.chars().count(),
            bound,
            "and the note the reader had is untouched"
        );

        // The control for the second half.
        edits
            .renote(7, mark, "b".repeat(16))
            .expect("an ordinary note");
        assert_eq!(edits.state(7).expect("state").marks[0].note, "b".repeat(16));
    }

    #[test]
    fn a_subset_plan_carries_only_the_marks_on_the_pages_it_takes() {
        let edits = opened();
        let pages = edits.state(7).expect("state").pages;
        edits
            .annotate(7, a_mark(pages[0].id), stamped())
            .expect("first");
        edits
            .annotate(7, a_mark(pages[2].id), stamped())
            .expect("third");

        let plan = edits.plan_subset(7, &[0]).expect("subset");
        assert_eq!(plan.pages.len(), 1);
        assert_eq!(
            plan.marks.len(),
            1,
            "a mark on a page nobody extracted came along"
        );
        assert_eq!(plan.marks[0].source, 0);
    }

    #[test]
    fn a_marks_reply_names_the_page_by_identity_and_the_plan_by_position() {
        // The two vocabularies, on one mark. The frontend is handed the page's
        // *id*, because that is what it sends back; the writer is handed the
        // baseline *source*, because that is what a page tree is indexed by.
        // The same number today, and the whole reason this layer exists is that
        // they stop being the same the moment a page moves.
        let edits = opened();
        let pages = edits.state(7).expect("state").pages;
        edits
            .move_page(7, pages[2].id, None)
            .expect("put the third page first");
        let moved = edits.state(7).expect("state").pages;
        assert_eq!(moved[0].source, 2);

        let state = edits
            .annotate(7, a_mark(moved[0].id), stamped())
            .expect("mark the page that moved");
        assert_eq!(state.marks[0].page, moved[0].id);

        let plan = edits.plan(7).expect("plan");
        assert_eq!(
            plan.marks[0].source, 2,
            "the writer was given a position rather than the page"
        );
    }

    #[test]
    fn quads_that_are_not_a_multiple_of_four_are_refused_before_the_model() {
        let edits = opened();
        let id = edits.state(7).expect("state").pages[0].id;
        let mut ragged = a_mark(id);
        ragged.quads.pop();
        let why = edits
            .annotate(7, ragged, stamped())
            .expect_err("three numbers is not a rectangle");
        assert!(why.contains("four numbers"), "{why}");
        // And nothing was journalled: a refusal here must leave the document as
        // it was, which the model guarantees for its own refusals and this one
        // never reaches.
        assert!(!edits.state(7).expect("state").dirty);
    }

    #[test]
    fn a_note_reaches_the_reader_and_the_writer_as_the_same_words() {
        // The two readings of one note, which is the pair this layer exists to
        // keep in step: the frontend redraws from `MarkView` and the file is
        // written from `PlannedMark`, and they are built by different code from
        // the same model.
        let edits = opened();
        let id = edits.state(7).expect("state").pages[1].id;
        let made = edits.annotate(7, a_mark(id), stamped()).expect("annotate");
        assert_eq!(made.marks[0].note, "");

        let state = edits
            .renote(7, made.marks[0].id, "ask about this".to_string())
            .expect("note it");
        assert_eq!(state.marks[0].note, "ask about this");
        assert_eq!(edits.plan(7).expect("plan").marks[0].note, "ask about this");
    }

    #[test]
    fn a_note_on_a_mark_that_is_gone_is_refused_by_name() {
        let edits = opened();
        let id = edits.state(7).expect("state").pages[0].id;
        let made = edits.annotate(7, a_mark(id), stamped()).expect("annotate");
        let mark = made.marks[0].id;

        let why = edits
            .renote(7, mark + 1000, "hello".to_string())
            .expect_err("no such mark");
        assert!(why.contains("no such mark"), "{why}");

        edits.unannotate(7, mark).expect("remove it");
        let why = edits
            .renote(7, mark, "hello".to_string())
            .expect_err("already removed");
        assert!(why.contains("already been removed"), "{why}");
    }

    #[test]
    fn a_note_makes_the_document_dirty() {
        // A note is a change to the file, not a view setting: a reader who types
        // one and closes the window has to be asked. `dirty` is read off the
        // journal, which is the whole reason `renote` is a command.
        let edits = opened();
        let id = edits.state(7).expect("state").pages[0].id;
        let made = edits.annotate(7, a_mark(id), stamped()).expect("annotate");
        edits.undo(7).expect("undo the highlight");
        assert!(!edits.state(7).expect("state").dirty);

        edits.redo(7).expect("redo it");
        let state = edits
            .renote(7, made.marks[0].id, "typed".to_string())
            .expect("note it");
        assert!(state.dirty);
        let back = edits.undo(7).expect("undo the note");
        assert_eq!(back.marks[0].note, "", "undo left the note behind");
        assert!(back.dirty, "the highlight is still an edit");
    }
}
