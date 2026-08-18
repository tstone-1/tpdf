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
use std::sync::Mutex;

use serde::Serialize;

use crate::docmodel::{Command, Doc, Mark, MarkId, MarkKind, PageId, Quad, Rect, Refusal};

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
    /// Red, green and blue in 0..=1.
    pub color: [f32; 3],
    /// What the reader typed, which may be empty.
    ///
    /// **Attacker-controlled the moment a saved file is reopened**, so it is
    /// treated exactly as `annots.rs` treats a body: it reaches the DOM as text
    /// and nothing here may carry a URL. See `docs/THREAT-MODEL.md` T8.
    pub note: String,
}

/// What the frontend asks for when a reader makes a mark.
///
/// A struct rather than a parameter list, and not only because clippy counts to
/// seven: **`made` is deliberately not in here.** The timestamp is the
/// application's, taken from its own clock at the moment the command arrives, so
/// a caller cannot choose what a mark claims about when it was made.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct NewMark {
    /// Which of the three marks this is.
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
    pub quads: Vec<f32>,
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
    /// The handle names no open document; `quads` is not a multiple of four; the
    /// mark covers no area; or the page does not exist or was deleted.
    pub fn annotate(&self, doc: u32, want: NewMark, made: String) -> Result<EditState, String> {
        if want.quads.len() % 4 != 0 {
            return Err(format!(
                "a mark is four numbers per rectangle, and this has {}",
                want.quads.len()
            ));
        }
        let quads: Vec<Quad> = want
            .quads
            .chunks_exact(4)
            .map(|q| Quad {
                left: q[0],
                top: q[1],
                right: q[2],
                bottom: q[3],
            })
            .collect();

        let mut docs = self.docs.lock().expect("edits lock");
        let model = docs.get_mut(&doc).ok_or_else(|| unknown(doc))?;
        model
            .annotate(
                Mark {
                    kind: want.kind,
                    page: PageId::from_raw(want.page),
                    quads,
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

    /// Replaces what one mark says, addressed by identity.
    ///
    /// Not routed through [`command`](Edits::command) like the other four, and
    /// the reason is the same one [`annotate`](Edits::annotate) has: the command
    /// carries an id that only the model may issue --- here the note's --- so
    /// what crosses this boundary is the text, and the [`Command`] is built on
    /// the far side of the lock.
    ///
    /// # Errors
    ///
    /// The handle names no open document; the id names no mark, or one that has
    /// already been removed.
    pub fn renote(&self, doc: u32, mark: u64, note: String) -> Result<EditState, String> {
        let mut docs = self.docs.lock().expect("edits lock");
        let model = docs.get_mut(&doc).ok_or_else(|| unknown(doc))?;
        model
            .renote(MarkId::from_raw(mark), note)
            .map_err(describe)?;
        Ok(snapshot(model))
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
        let pages = snapshot(model).pages;
        Ok(Plan {
            baseline: model.baseline(),
            // Before `pages`, which the line below moves. Field order in a
            // struct literal is evaluation order, so this reads the borrow while
            // there is still one to read.
            marks: planned_marks(model, &pages),
            pages,
        })
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
        let docs = self.docs.lock().expect("edits lock");
        let model = docs.get(&doc).ok_or_else(|| unknown(doc))?;
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
            pages,
            marks,
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
                    quads: body.quads.clone(),
                    color: body.color,
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
#[derive(Clone, PartialEq, Debug)]
pub struct Plan {
    /// How many pages the file this document was opened from had.
    pub baseline: u32,
    /// The kept pages, in reading order.
    pub pages: Vec<PageView>,
    /// The marks to write, on the pages that are kept.
    ///
    /// A mark whose page is not in `pages` is **absent rather than carried with
    /// a dangling reference**, which is what makes an extract of pages 1--3
    /// write the highlights on those three pages and nothing else.
    pub marks: Vec<PlannedMark>,
}

/// One mark as the writer needs it.
///
/// Distinct from [`MarkView`], which is the same mark as the *frontend* needs
/// it, and the difference is the one that matters here: this names the page by
/// its position in the **baseline file**, because that is what a writer walking
/// the page tree has. A `MarkView`'s page id means nothing to `lopdf`.
#[derive(Clone, PartialEq, Debug)]
pub struct PlannedMark {
    pub kind: MarkKind,
    /// Which baseline page this goes on, zero-based.
    pub source: u32,
    /// Display space, as the model holds it. Mapped into the page's own space by
    /// the writer --- see [`Edits::annotate`].
    pub quads: Vec<Quad>,
    pub color: [f32; 3],
    pub author: String,
    pub note: String,
    pub made: String,
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
        // A mark is a change the file does not have, so a plan carrying one is
        // never the file. Written first because it is the cheap half and because
        // leaving it out would let the print path hand over the original bytes
        // for a document the reader has highlighted --- which prints, correctly
        // and confusingly, without the highlights.
        self.marks.is_empty()
            && self.pages.len() == self.baseline as usize
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
        Refusal::NoSuchMark(_) => "no such mark".into(),
        Refusal::MarkRemoved(_) => "that mark has already been removed".into(),
        Refusal::EmptyMark => "that mark covers nothing".into(),
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
                page: page.get(),
                quads: mark
                    .quads
                    .iter()
                    .flat_map(|q| [q.left, q.top, q.right, q.bottom])
                    .collect(),
                color: mark.color,
                note: model.note_of(id).to_string(),
            }
        })
        .collect();

    let (applied, _) = model.depth();
    EditState {
        pages,
        marks,
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

    /// A highlight over one line of the page with `page` as its id.
    fn a_mark(page: u64) -> NewMark {
        of_kind(MarkKind::Highlight, page)
    }

    /// The same, of whichever kind. Separate so that the existing tests read as
    /// they did and the ones about the kind name it.
    fn of_kind(kind: MarkKind, page: u64) -> NewMark {
        NewMark {
            kind,
            page,
            quads: vec![72.0, 100.0, 300.0, 118.0],
            color: [1.0, 0.9, 0.2],
            author: "a reader".to_string(),
            note: String::new(),
        }
    }

    fn stamped() -> String {
        "D:20260818120000Z".to_string()
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
