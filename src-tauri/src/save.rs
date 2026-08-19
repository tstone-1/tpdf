//! Writing the working document to a file.
//!
//! **Save a copy: the pages the reader kept, each with its turn applied.** The
//! plan is [`edits::Plan`](crate::edits::Plan) --- the same answer the viewer is
//! drawing from --- so a saved copy and a rendered page cannot disagree about
//! what the reader was looking at. Two readings of one answer rather than two
//! derivations of one rule.
//!
//! **What the plan says.** The kept pages, in the order the reader put them,
//! each with the quarter turns to add. A page nobody kept is deleted from the
//! page tree; a page nobody turned is written back byte for byte; and a plan
//! whose pages have *moved* is written as a new page tree rather than refused,
//! which is what changed when `Command::Move` was wired.
//!
//! **A move is paid for by the whole tree, and a plan in document order must not
//! pay it.** `pagetree::reorder_pages` flattens the page tree so that every page
//! carries what it used to inherit; run over a document nobody rearranged, that
//! would rewrite pages nobody touched. So the order is compared against the
//! file's before anything is written, and the common case goes nowhere near it.
//!
//! **Three refusals, and none of them is defensive.**
//!
//!  - An **encrypted** document. `docs/TRAPS.md` records that `lopdf` silently
//!    drops encryption on save, so writing one produces a file whose restrictions
//!    are gone and whose reader has no way to know. 3 of the 39 PDFs in a real
//!    Downloads folder carry `/Encrypt` (measured for `progressive::open_failure`),
//!    so this is a case a reader meets, not a hypothetical.
//!  - A **page count that disagrees** with the plan's baseline. That is the
//!    external modification §5 of `docs/PLAN.md` is about: the file changed under
//!    the open document, and the edits the reader applied no longer name the pages
//!    they were applied to. Compared against the *baseline* rather than against
//!    the plan's length, which is what makes it survive a deletion --- a plan of
//!    three pages for a five-page file is what deleting two of them looks like.
//!  - Writing **over the source**. A copy is never the file it was copied from,
//!    and a reader who types the open document's own name into the panel means
//!    Save rather than Save a copy. Saving in place is the operation below, and
//!    it is not this one with a different destination.
//!
//! **The write is atomic**: the bytes go to a sibling temporary file and are
//! renamed over the destination, so an interrupted save leaves either the old
//! file or the new one. A partially written PDF is the worst of the three
//! outcomes --- it opens, and it is missing pages.
//!
//! **Save, in place, is that write split in two.** [`stage_in_place`] does
//! everything up to and including the temporary file; [`commit_in_place`] does
//! the rename. Between them the caller closes the document, and that is not
//! tidiness: a `rename` over a memory-mapped file succeeds on macOS and leaves
//! the mapping serving the inode that is no longer there, so a worker would
//! render the document as it was before the save for as long as it stayed open.
//! Windows refuses the rename instead. One order is correct on both.
//!
//! What the caller does *not* get back is a rebase in §5's full sense. The
//! journal is spent rather than compacted, and the document is reopened from the
//! file that was just written --- which is the same answer for a save that
//! succeeded, and is why a save that fails after the close costs the reader
//! their unsaved commands. Carrying a journal across a reopen is what §5 calls
//! rebasing, and nothing here does it.
//!
//! The page-tree surgery itself is `pagetree.rs`, shared with the print path,
//! which needs every one of the same operations for the same reasons.

use std::path::{Path, PathBuf};

use lopdf::Document;

use lopdf::{Dictionary, Object, ObjectId};

use crate::docmodel::MarkKind;
use crate::edits::{Plan, PlannedMark};
use crate::fingerprint::Fingerprint;
use crate::pagetree::{
    agreed_turns, apply_crops, apply_turns, displayed_page, drop_outline, drop_pages,
    ordered_pages, reorder_pages, DisplayedPage,
};
use crate::print::MAX_DECODE;

/// Extension of the file the bytes are written to before the rename.
///
/// Sibling rather than in the system temp directory, because a rename across
/// filesystems is not atomic and the temp directory is routinely on another
/// one.
const PARTIAL: &str = "tpdf-partial";

/// Why a save was refused, and whether the file having changed is the reason.
///
/// **`changed` is a field rather than a wording**, for exactly the reason
/// `SaveFailure::reopen` in `lib.rs` is one: a caller that decided by matching
/// on the message would be parsing a string this end is free to reword, and the
/// message here is deliberately reworded whenever a reader is served badly by
/// it. See `docs/TRAPS.md` on the two-moments message, which is this same file
/// getting that wrong within the week.
///
/// What it buys is one decision the window cannot otherwise make safely.
/// Reloading throws away every edit in the journal, so offering it is right for
/// *this* refusal --- the file underneath is a different file, and there is
/// nothing else the reader can do with their edits but write them elsewhere ---
/// and wrong for "a document must keep at least one page", where reloading would
/// discard the reader's work in exchange for nothing at all.
///
/// Every other refusal reaches this through `From<String>`, which is what keeps
/// the `?` in the middle of `planned_bytes` unchanged: the flag is set at the
/// one call site that knows, and defaults to false everywhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// What to tell the reader.
    pub message: String,
    /// The source changed on disk since the reader opened it.
    pub changed: bool,
}

impl Refusal {
    /// A refusal because the file is not the file that was opened.
    #[must_use]
    pub fn changed(message: impl Into<String>) -> Refusal {
        Refusal {
            message: message.into(),
            changed: true,
        }
    }
}

impl From<String> for Refusal {
    fn from(message: String) -> Refusal {
        Refusal {
            message,
            changed: false,
        }
    }
}

impl From<&str> for Refusal {
    fn from(message: &str) -> Refusal {
        Refusal::from(message.to_string())
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What a source that changed under the reader means for the operation.
///
/// The two save paths answer this differently and always have --- `stage_in_place`
/// refuses what `write_copy` tolerates --- and until 2026-08-19 that was true of a
/// missing fingerprint and **not** of a changed file, which stranded the reader in
/// the exact way the comment in `stage_in_place` warns about. A save in place was
/// refused with a message naming Save a copy, and Save a copy was refused by the
/// same guard one function down. The escape hatch was closed and the message
/// pointed at it anyway.
///
/// The asymmetry is the same one, for the same reason. A copy writes a new file
/// and leaves the original exactly as it is, so the worst case is a file the
/// reader can look at and delete. Writing in place spends the original, and there
/// is no looking at it afterwards.
///
/// What still protects the copy is everything else `planned_bytes` checks: the
/// page count against the plan's baseline, every page the plan names existing,
/// and the shared-page refusals. A changed file that also changed shape is
/// refused by those, whichever value this carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OnChange {
    /// Refuse, because what is at stake is the reader's only copy.
    Refuse,
    /// Write it, and say so. `changed` on the result is how the reader is told.
    Proceed,
}

/// The bytes of a working document, and what the file was when they were built.
///
/// `verified` is `None` only where the plan carried no fingerprint, which
/// [`write_copy`] tolerates and [`stage_in_place`] refuses --- so the two fields
/// are not independent, and which of the two paths you are on decides.
struct Planned {
    bytes: Vec<u8>,
    verified: Option<Fingerprint>,
    /// The source had changed since it was opened, and `OnChange::Proceed` let
    /// it through. Always false under `OnChange::Refuse`, which returns an error
    /// instead of setting this.
    changed: bool,
}

/// A copy that was written, and whether its source had changed underneath.
///
/// `changed` is a fact the reader has to be told rather than a failure: the file
/// is on disk and is the best tpdf can produce, and it was built from a document
/// that is no longer the one they opened. Silence here would be the worst of the
/// three options --- a copy that is quietly built from different pages reads as a
/// copy that is right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Copied {
    /// The source changed since it was opened, and the copy was written anyway.
    pub changed: bool,
}

/// A staged save: the file to rename, and what the source was when it was built.
///
/// `verified` is not an `Option`, and that is the point of the type. The caller
/// takes its last look before the rename through this field, and an `Option`
/// there would give that look a `None` arm --- which could only be written as
/// "skip the check", i.e. the one save that most needs it proceeding unchecked.
/// A missing fingerprint is already refused by [`stage_in_place`] several lines
/// above; making it unsayable here is what stops that refusal being undone by a
/// later `if let`.
///
/// It is the fingerprint taken **during** staging, not the one taken at open.
/// That is the whole reason this struct exists rather than a bare path: the
/// caller's last look before the rename should ask whether anything moved since
/// the file was read and rewritten, not whether anything moved since the reader
/// opened it an hour ago. The second question refuses a `touch` that the deep
/// check already forgave.
#[derive(Debug)]
pub struct Staged {
    /// The sibling temporary file, which nothing has renamed yet.
    pub path: PathBuf,
    /// The source as it was when its bytes were read.
    pub verified: Fingerprint,
}

/// Writes the pages `plan` keeps, each with its own turn, from `source` to `out`.
///
/// # Errors
///
/// Everything [`planned_bytes`] refuses; `out` is the source; or the write
/// fails. The temporary file is removed on every failing path that created one.
pub fn write_copy(source: &Path, plan: &Plan, out: &Path) -> Result<Copied, Refusal> {
    if same_file(source, out) {
        return Err(
            "tpdf cannot save over the document it is reading --- choose another name".into(),
        );
    }
    // Named differently from `stage_in_place`'s, deliberately. The two calls are
    // otherwise character-for-character identical, which made one mutation's
    // anchor ambiguous the moment this line gained a binding --- and an ambiguous
    // anchor is refused, so the mutation stops being able to fail. Distinct names
    // are the fix; a longer anchor is only the workaround.
    let copy = planned_bytes(source, plan, OnChange::Proceed)?;
    write_atomically(out, &copy.bytes)?;
    Ok(Copied {
        changed: copy.changed,
    })
}

/// Writes the working document beside `source`, ready to be put in its place.
///
/// The first half of saving over the open file, and the half that can still be
/// refused for free: every guard [`planned_bytes`] states runs here, before
/// anything the reader has is disturbed. What comes back is the path of the
/// sibling temporary file, which nothing has renamed yet.
///
/// **The split exists because the document has to be closed in between.** A
/// `rename` over a file some process has memory-mapped succeeds on macOS and
/// leaves the mapping serving the inode that is no longer at that path ---
/// measured, and the reason it is worth splitting a function over: the reader's
/// worker would go on rendering the document as it was before the save,
/// indefinitely and without anything looking wrong. Windows refuses the rename
/// outright while a section is open, so the two platforms fail differently and
/// only one of them fails visibly.
///
/// So the caller stages, closes the document, and only then commits. See
/// `lib.rs`'s `save_document`, which is the only caller and holds the ordering.
///
/// # Errors
///
/// Everything [`planned_bytes`] refuses, and a temporary file that cannot be
/// written. The temporary file is removed on the failing path that created one.
pub fn stage_in_place(source: &Path, plan: &Plan) -> Result<Staged, Refusal> {
    // Refused here and tolerated by `write_copy`, which is the one place the two
    // paths differ on purpose. A fingerprint that could not be taken means tpdf
    // cannot tell whether this file is still the file -- and the two operations
    // have different stakes: writing a copy risks a bad new file beside an intact
    // original, writing in place risks the original. So the fallback the message
    // names has to keep working, or the refusal strands the reader.
    if plan.opened_as.is_none() {
        return Err(
            "tpdf could not record what this file looked like when it was opened, \
                    so it cannot tell whether saving over it is safe --- use Save a copy"
                .into(),
        );
    }
    let planned = planned_bytes(source, plan, OnChange::Refuse)?;
    // Refused rather than unwrapped. It cannot fire --- `planned_bytes` derives
    // this from the same `plan.opened_as` the guard above just proved present ---
    // but the two are eight lines and one function call apart, and the failure a
    // panic would replace is a *refused save*, which is the outcome every other
    // branch here already produces. A guard that turns an internal inconsistency
    // into the safe answer costs three lines and is not the unreachable guard the
    // repository has a rule about deleting.
    let verified = planned.verified.ok_or_else(|| {
        "tpdf could not confirm what this file was when it read it --- use Save a copy".to_string()
    })?;
    let path = stage(source, &planned.bytes)?;
    Ok(Staged { path, verified })
}

/// The last look before the rename, and the cleanup when it refuses.
///
/// Split out of `save_document` rather than written inline there for the reason
/// the other two guards are: a check inside a Tauri command has no failing case
/// a test can reach, and `docs/TRAPS.md` records that costing real defects twice
/// over. The comment in `lib.rs` cited that rule for the deep check while this
/// one sat inline three lines below it.
///
/// Only length and modification time, and only against what [`stage_in_place`]
/// read. Staging rewrites the whole document and closing it is a round trip to
/// the worker, so a window exists; a third full read to narrow one measured in
/// milliseconds is the wrong trade, and the timestamp is the best evidence
/// available at that price.
///
/// The staged file is removed before returning, because nothing else will: the
/// caller is past the point where it tracks temporary files, and a refusal that
/// leaves one behind puts a file the reader never named beside their document.
///
/// # Errors
///
/// `source` changed since staging read it, or its metadata cannot be read. The
/// message says that nothing was written and that the document is closed, which
/// are the two facts the reader needs --- and stops there. What to *do* is the
/// caller's, because the caller is the only one that knows what it did next.
pub fn verify_before_commit(staged: &Staged, source: &Path) -> Result<(), Refusal> {
    if let Err(why) = staged.verified.agrees_shallowly(source) {
        let _ = std::fs::remove_file(&staged.path);
        // Its own advice, not `agrees_with`'s. That one says the reader's edits
        // are still here and to save them under another name, which was true
        // where it is written and is false here: the close two statements up in
        // `save_document` has already spent the journal. The two sentences used
        // to arrive in one message, contradicting each other.
        // The fact, and no instruction. It ended "open the file again to see what
        // is there now" until 2026-08-19, and `save_document`'s caller reopens
        // the file by itself on every `after_close` --- so the advice was
        // addressed to somebody it had already been carried out for. Same shape
        // as the two-moments message this file was caught on the same week, one
        // layer up: the producer states what happened, and the caller, which is
        // the only one that knows what it did next, owns what to do about it.
        return Err(Refusal::changed(format!(
            "{why} --- nothing was written, so the file on disk is untouched. \
             Your document has been closed and the edits you had made are gone."
        )));
    }
    Ok(())
}

/// Puts a file [`stage_in_place`] wrote where `source` is.
///
/// The second half, and the only step that is not reversible: after it the file
/// the reader opened holds the edits they made, and the journal that describes
/// them is spent. `staged` is removed if the rename fails, so a refused save
/// leaves the directory as it found it.
pub fn commit_in_place(staged: &Path, source: &Path) -> Result<(), String> {
    commit(staged, source)
}

/// The bytes of the working document, ready to be written somewhere.
///
/// Everything both save paths share: the parse, the three refusals, the page
/// tree, the marks, the turns and the crops. Neither path names a destination
/// here --- a copy and a save in place differ in where the bytes go and in what
/// has to happen around the write, never in what is written.
///
/// # Errors
///
/// The plan is empty; `source` cannot be read or parsed; it is encrypted; it has
/// a different number of pages than the plan's baseline; the plan names a page
/// the file does not have; two of its pages are one object and disagree about
/// the turn or the crop, or one of them is dropped without the other; or a mark
/// maps to nothing.
fn planned_bytes(source: &Path, plan: &Plan, on_change: OnChange) -> Result<Planned, Refusal> {
    if plan.pages.is_empty() {
        return Err("a document must keep at least one page".into());
    }

    // Before the parse, and before anything is written anywhere. Every operation
    // below rewrites the object graph this plan was made against, so a `source`
    // that changed since the reader opened it is a different graph and the edits
    // no longer name what they were made on.
    //
    // This is the general form of the page-count refusal below it, which shipped
    // first and catches exactly one shape of the same problem: a file whose page
    // count changed. Everything that keeps the count -- a re-export over the top,
    // a sync client landing a newer copy, a signing tool rewriting in place --
    // was invisible to it. See `docs/PLAN.md` §5 and `fingerprint.rs`.
    let mut changed = false;
    let verified = match (&plan.opened_as, on_change) {
        (Some(opened_as), OnChange::Refuse) => {
            Some(opened_as.agrees_with(source).map_err(Refusal::changed)?)
        }
        // Written anyway, and recorded. `verified` stays `None` because there is
        // nothing verified about it -- which is also what stops this result being
        // committed in place by a caller that changed its mind, since
        // `stage_in_place` refuses a `None`.
        (Some(opened_as), OnChange::Proceed) => match opened_as.agrees_with(source) {
            Ok(now) => Some(now),
            Err(_) => {
                changed = true;
                None
            }
        },
        (None, _) => None,
    };

    let mut doc = Document::load_with_options(
        source,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            ..Default::default()
        },
    )
    .map_err(|e| format!("could not parse {source:?}: {e}"))?;

    // Before anything is written, and before the page walk: a refusal that
    // arrives after a temporary file exists has to clean up, and a refusal that
    // arrives after a rename has nothing to clean up at all.
    if doc.trailer.has(b"Encrypt") {
        return Err(
            "This document is encrypted, and saving a copy would silently remove that. \
             tpdf will not write it."
                .into(),
        );
    }

    let pages = ordered_pages(&doc);
    if pages.len() != plan.baseline as usize {
        // `Refusal::changed`, and it is not a formality: this refusal says the
        // file changed, and it is the one that fires when a changed file also
        // changed shape --- which is precisely the case `OnChange::Proceed` does
        // *not* wave through, so a copy lands here rather than being written
        // from pages the plan cannot name. A reader who reaches it needs the
        // same Reload the fingerprint refusal offers.
        return Err(Refusal::changed(format!(
            "the document on disk has {} page(s) and the edits were made against {} --- it has \
             changed since it was opened, so reopen it before saving",
            pages.len(),
            plan.baseline
        )));
    }

    // Whether the reader moved anything. Read here, off the plan, because after
    // the deletion below the document's own page numbers are not the plan's any
    // more --- and because a plan that is already in document order must not go
    // near `reorder_pages`, which flattens the tree.
    let moved = plan
        .pages
        .windows(2)
        .any(|two| two[0].source >= two[1].source);

    // One-based, because that is how `lopdf` numbers pages and how
    // `pagetree::drop_pages` reads them. The model's `source` is the zero-based
    // baseline page, and `ordered_pages` is that same order, so the two line up
    // by position rather than by anything either of them stores.
    let kept: Vec<u32> = plan.pages.iter().map(|page| page.source + 1).collect();
    if let Some(past) = kept.iter().find(|&&number| number as usize > pages.len()) {
        return Err(Refusal::changed(format!(
            "the edits name page {past}, which this document does not have"
        )));
    }

    let turns: Vec<(lopdf::ObjectId, u8)> = plan
        .pages
        .iter()
        .filter_map(|page| Some((*pages.get(page.source as usize)?, page.turns)))
        .collect();

    if kept.len() != pages.len() {
        let dropped: Vec<u32> = (1..=pages.len() as u32)
            .filter(|number| !kept.contains(number))
            .collect();
        unshared(&pages, &kept, &dropped)?;
        drop_pages(&mut doc, &dropped)?;
        // Its destinations name pages that are no longer in the file. Dropped
        // whole rather than repaired --- `pagetree::drop_outline` carries what
        // repairing it would take, and it is its own piece of work.
        drop_outline(&mut doc)?;
    }

    // After the deletion, so that the tree written here holds exactly the pages
    // that survived it. The outline is *not* dropped for a move: a destination
    // names a page object, and the object is still there --- a bookmark follows
    // its page to wherever the reader put it, which is what a reader who
    // rearranged a document means.
    if moved {
        let order: Vec<lopdf::ObjectId> = turns.iter().map(|(id, _)| *id).collect();
        reorder_pages(&mut doc, &order)?;
    }

    // Before `apply_turns`, and the order is load-bearing rather than tidy: a
    // mark was made against the rotation the file had when it was opened, and
    // the mapping below reads the rotation the file has *now*. Turn the page
    // first and every quad is a quarter turn out, on exactly the pages a reader
    // rotated.
    write_marks(&mut doc, &pages, &kept, &plan.marks)?;

    // After the deletion, and it has to be: `drop_pages` removes objects, and a
    // rotation written onto a page that is about to go is work thrown away. The
    // ids are unaffected --- the survivors are the same objects they were.
    apply_turns(&mut doc, &agreed_turns(&turns)?)?;

    // After `write_marks` for the same reason `apply_turns` is: a mark's quads
    // are in the display space the file had when it was opened, and cropping
    // moves that space. Writing the crop first would place every highlight by
    // the *new* origin while the reader made it against the old one.
    //
    // A page named twice by the plan is refused rather than cropped twice ---
    // the two entries would be one object, so the second write would silently
    // win for both positions. `agreed_turns` refuses the same shape for the same
    // reason, and this reuses neither because the values it compares are four
    // numbers rather than one.
    let crops = agreed_crops(plan)?;
    let crops: Vec<(lopdf::ObjectId, [f64; 4])> = crops
        .into_iter()
        .filter_map(|(source, box_pt)| Some((*pages.get(source as usize)?, box_pt)))
        .collect();
    apply_crops(&mut doc, &crops)?;

    let mut bytes = Vec::new();
    doc.save_to(&mut bytes)
        .map_err(|e| format!("could not serialise the document: {e}"))?;

    Ok(Planned {
        bytes,
        verified,
        changed,
    })
}

/// A PDF date string for an instant, in UTC.
///
/// `D:YYYYMMDDHHmmSSZ`, which PDF 32000-1 §7.9.4 calls a date and `annots.rs`
/// reads back. UTC with a literal `Z` rather than a local offset: the offset
/// form is `+HH'mm'`, the apostrophes are load-bearing, and a machine's timezone
/// is not something a reader of the file needs in order to know when a mark was
/// made.
///
/// A clock before the epoch reads as the epoch, which is the same answer
/// `diag.rs` gives and for the same reason --- a machine whose clock is wrong by
/// decades should still get its mark written.
///
/// The civil-date arithmetic is `diag.rs`'s, shared rather than copied: it is
/// pinned there by a table of known instants including a leap day, and a second
/// copy here would have no table.
pub fn pdf_date(at: std::time::SystemTime) -> String {
    let seconds = at
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (year, month, day) = crate::diag::civil_from_days(seconds / 86_400);
    let rest = seconds % 86_400;
    format!(
        "D:{year:04}{month:02}{day:02}{:02}{:02}{:02}Z",
        rest / 3_600,
        (rest / 60) % 60,
        rest % 60
    )
}

/// The opacity of a highlight's wash, as `/CA`.
///
/// Below 1 because a wash is meant to be read through. The blend mode does most
/// of that work --- see [`appearance_stream`] --- and this is what keeps the mark
/// legible in a reader that ignores the blend and paints the fill flat.
const WASH_ALPHA: f32 = 0.4;

/// Writes the reader's marks into the document as real annotations.
///
/// **One object per mark, appended to its page's `/Annots`.** That array may be
/// absent, written inline, or an indirect reference to an array; `AGENTS.md`
/// records that the distinction decides how large an annotation edit is, and
/// [`attach`] handles all three.
///
/// **The coordinates are the whole of the difficulty.** A mark is held in
/// display space --- what the reader dragged across, y downwards from the
/// displayed page's top-left corner --- and `/QuadPoints` is the page's own
/// space, y upwards, before `/Rotate` and measured from the media box. The
/// mapping is [`crate::text::from_device`] followed by the crop box's origin,
/// which is exactly the two steps `annots.rs` performs in reverse when it reads
/// one back. Those are separate implementations, which is what makes
/// `annot-probe`'s round trip a differential rather than a tautology.
///
/// `kept` is the one-based page numbers being written, used only for the
/// shared-object refusal.
///
/// # Errors
///
/// A mark naming a page the file does not have; a mark on a page object that
/// more than one kept page number names; or a mark whose quads map to nothing.
fn write_marks(
    doc: &mut Document,
    pages: &[ObjectId],
    kept: &[u32],
    marks: &[PlannedMark],
) -> Result<(), String> {
    for mark in marks {
        let page = *pages.get(mark.source as usize).ok_or_else(|| {
            format!(
                "a mark names page {}, which this document does not have",
                mark.source + 1
            )
        })?;

        // The same refusal as `unshared` and for the same reason, one level on:
        // an annotation is attached to a page *object*, so a mark made on page 3
        // would appear on page 7 as well when `/Kids` names one object twice.
        // `docs/TRAPS.md` has this shape twice already, once live in `print.rs`
        // for months.
        if kept
            .iter()
            .filter(|number| pages.get(**number as usize - 1) == Some(&page))
            .count()
            > 1
        {
            return Err(format!(
                "page {} is the same page object as another page in this file, so a mark on it \
                 would appear on both. tpdf will not write it.",
                mark.source + 1
            ));
        }

        let shown = displayed_page(doc, page);
        let quads = user_quads(mark, shown);
        if quads.is_empty() {
            return Err(format!(
                "a mark on page {} covers no area in that page's own space",
                mark.source + 1
            ));
        }

        let rect = bounds(&quads);
        // **A comment gets no appearance stream from us, and that is not a gap.**
        // Every reader synthesises the icon for a `/Text` annotation --- the
        // specification describes `/Name` as choosing one and readers are
        // expected to draw it --- and they draw their own house style at their
        // own size whatever we write. Supplying one would mean shipping a
        // hand-drawn speech bubble that looks foreign in Acrobat and in Preview,
        // and `--mode noap` already measures that PDFium renders these without
        // one: the note icon it generates fills 637 of the 756 pixels in its
        // rectangle.
        //
        // The three markup kinds are the opposite case and keep theirs: a
        // reader that declines to synthesise a highlight shows nothing at all,
        // which is why `appearance_stream` exists. **A box is on that side of
        // the line, not the comment's**, and it is the reason this asks `ink`
        // rather than `is_note`: nothing synthesises a rectangle, so a
        // `/Square` with no `/AP` is an annotation Acrobat draws as nothing.
        let appearance = if ink(mark.kind) == Ink::None {
            None
        } else {
            Some(appearance_stream(doc, mark, &quads, rect))
        };
        let dictionary = mark_dictionary(mark, page, &quads, rect, appearance);
        let annotation = doc.add_object(dictionary);
        attach(doc, page, annotation)?;
    }
    Ok(())
}

/// A mark's quads in the page's own space, `[llx, lly, urx, ury]` each.
///
/// Degenerate quads are dropped rather than written. The model already refuses a
/// mark where *every* quad is empty ([`crate::docmodel::Refusal::EmptyMark`]);
/// this is the per-quad half, and it exists because a selection that runs to the
/// end of a line legitimately produces one empty rectangle after a real one.
fn user_quads(mark: &PlannedMark, shown: DisplayedPage) -> Vec<[f64; 4]> {
    let (ox, oy) = (f64::from(shown.origin.0), f64::from(shown.origin.1));
    mark.quads
        .iter()
        .filter(|quad| quad.covers_area())
        .map(|quad| {
            let page = crate::text::from_device(
                shown.turns,
                shown.width,
                shown.height,
                [quad.left, quad.top, quad.right, quad.bottom],
            );
            // The origin comes back on *after* the turn, because it came off
            // before one: `annots.rs` shifts into crop space and then maps.
            [page[0] + ox, page[1] + oy, page[2] + ox, page[3] + oy]
        })
        .collect()
}

/// The rectangle enclosing every quad.
fn bounds(quads: &[[f64; 4]]) -> [f64; 4] {
    quads
        .iter()
        .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |acc, q| {
            [
                acc[0].min(q[0]),
                acc[1].min(q[1]),
                acc[2].max(q[2]),
                acc[3].max(q[3]),
            ]
        })
}

/// The annotation dictionary for one mark.
///
/// `/F 4` sets the Print flag, which is what makes a highlight appear on paper
/// and in a print-to-PDF --- an annotation without it is a screen-only mark, and
/// a reader who highlights a document in order to print it would get a blank
/// page back.
///
/// `/NM` is the mark's own id. It has to be unique within the page and ours are
/// unique within the document, which is the stronger property; it is written
/// because a reader that reopens the file and edits it needs a name for the
/// annotation that is not its position in an array.
fn mark_dictionary(
    mark: &PlannedMark,
    page: ObjectId,
    quads: &[[f64; 4]],
    rect: [f64; 4],
    appearance: Option<ObjectId>,
) -> Dictionary {
    let note = is_note(mark.kind);
    let mut dictionary = Dictionary::new();
    dictionary.set("Type", Object::Name(b"Annot".to_vec()));
    dictionary.set("Subtype", Object::Name(subtype(mark.kind).to_vec()));
    dictionary.set("Rect", numbers(rect));
    // **`/QuadPoints` is a text-markup key**, and neither of the two kinds a
    // reader places themselves may carry one --- see [`is_text_markup`], which
    // is the question this used to ask as "is it a comment" because the comment
    // was then the only kind it was true of. Writing quads on a `/Square` is the
    // kind of thing most readers ignore and one day something does not, and it
    // would also be a lie: the quad there is the mark's own box, not a run of
    // words it covers.
    if is_text_markup(mark.kind) {
        dictionary.set("QuadPoints", quad_points(quads));
    }
    dictionary.set(
        "C",
        Object::Array(mark.color.iter().map(|c| Object::Real(*c)).collect()),
    );
    dictionary.set(
        "CA",
        Object::Real(if is_wash(mark.kind) { WASH_ALPHA } else { 1.0 }),
    );
    dictionary.set("F", Object::Integer(4));
    dictionary.set("P", Object::Reference(page));
    if note {
        // The icon a reader sees. `/Comment` is the speech bubble in every
        // reader that draws these; `/Note` is the folded page, which is the
        // name a reader would guess from our own serde spelling and the wrong
        // picture for the thing this command makes.
        dictionary.set("Name", Object::Name(b"Comment".to_vec()));
        // Closed. A file whose comments all spring open on load buries the page
        // under popups, and every reader offers its own way to open one.
        dictionary.set("Open", Object::Boolean(false));
    }
    if let Some(appearance) = appearance {
        dictionary.set("AP", {
            let mut ap = Dictionary::new();
            ap.set("N", Object::Reference(appearance));
            Object::Dictionary(ap)
        });
    }
    // Written as PDFDocEncoded literals. Both are the reader's own text rather
    // than a document's, so the encoding question `annots.rs` answers on the way
    // in does not arise on the way out --- but a non-ASCII author would be
    // mangled by a literal, so anything outside ASCII goes out as UTF-16BE with
    // the byte-order mark the specification asks for.
    dictionary.set("T", text_string(&mark.author));
    dictionary.set("Contents", text_string(&mark.note));
    dictionary.set("M", text_string(&mark.made));
    dictionary
}

/// The PDF name for a mark's kind.
///
/// A `match` rather than a table, so that adding a [`MarkKind`] is a compile
/// error here rather than a mark that silently writes as a highlight.
fn subtype(kind: MarkKind) -> &'static [u8] {
    match kind {
        MarkKind::Highlight => b"Highlight",
        MarkKind::Underline => b"Underline",
        // `/StrikeOut`, with that capitalisation. The variant is `StrikeOut`
        // and the serde name is `strikeout`; this is the only place all three
        // spellings meet, which is why it is a `match` and not a `to_lowercase`.
        MarkKind::StrikeOut => b"StrikeOut",
        // `/Text`, which is a comment bubble and not text on the page. The
        // reader's word for it is "comment", the serde name is `note`, and this
        // is the third spelling --- the same arrangement as `StrikeOut` above,
        // and the reason both are a `match` rather than a `to_lowercase`.
        MarkKind::Note => b"Text",
        // `/Square`, which is a rectangle and is not necessarily square: the
        // specification uses that name for the family holding `/Circle` too.
        // The word a reader sees is "box"; the third spelling once again.
        MarkKind::Square => b"Square",
    }
}

/// How a kind's ink is laid down.
///
/// **One question with one exhaustive `match`.** This started as two booleans
/// and the box would have made it three, which is where copies of a distinction
/// begin to drift --- the trap index has that under its own title, and it is the
/// same argument `markband.ts` makes for being one function. What the writer
/// needs is a single value that decides the geometry, the blend mode and both
/// opacities together, because those four have never been independent.
///
/// `markband.ts` mirrors this across the language boundary for the overlay.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ink {
    /// The whole quad, multiplied, so the words underneath stay readable.
    Wash,
    /// A band inside the quad, opaque and on top --- see [`line_rect`]. A
    /// translucent line reads as a smudge, and multiplied red over black text
    /// is black.
    Line,
    /// The quad's edge, opaque, leaving whatever is inside it visible. Which is
    /// the entire point of a box: it says "this", it does not cover it.
    Outline,
    /// None of ours. The reader draws its own, which for `/Text` is the only
    /// way the icon can look like that reader's other comments.
    None,
}

/// Which of the four a kind uses.
///
/// A `match` for [`subtype`]'s reason: adding a [`MarkKind`] has to be a compile
/// error here rather than a mark that silently draws as a highlight.
fn ink(kind: MarkKind) -> Ink {
    match kind {
        MarkKind::Highlight => Ink::Wash,
        MarkKind::Underline | MarkKind::StrikeOut => Ink::Line,
        MarkKind::Square => Ink::Outline,
        MarkKind::Note => Ink::None,
    }
}

/// Whether a kind is a comment bubble.
///
/// Narrower than it looks, and deliberately so. It used to answer three
/// questions at once --- whether to write `/Name` and `/Open`, whether to skip
/// `/QuadPoints`, and whether to write an appearance stream --- because the
/// comment was the only kind for which all three answers happened to coincide.
/// The box separated them: it also skips `/QuadPoints` and it very much needs an
/// appearance stream. So this now answers only the first, [`is_text_markup`]
/// answers the second and [`Ink::None`] the third.
fn is_note(kind: MarkKind) -> bool {
    matches!(kind, MarkKind::Note)
}

/// Whether a kind is a text-markup annotation, and therefore carries quads.
///
/// PDF 32000-1 lists `/QuadPoints` on `/Highlight`, `/Underline`, `/Squiggly`
/// and `/StrikeOut` and on no other subtype. The two kinds a reader places
/// themselves are positioned by `/Rect` alone, and writing quads on one would be
/// a lie as well as an unlisted key: the quad there is the mark's own box rather
/// than a run of words it covers.
fn is_text_markup(kind: MarkKind) -> bool {
    matches!(
        kind,
        MarkKind::Highlight | MarkKind::Underline | MarkKind::StrikeOut
    )
}

/// Whether a kind covers its quads rather than drawing inside or around them.
///
/// Derived from [`ink`] rather than matching again, so that a kind can never be
/// a wash here and something else there. It decides the blend mode and `/CA`.
fn is_wash(kind: MarkKind) -> bool {
    ink(kind) == Ink::Wash
}

/// A line's thickness as a fraction of the marked text's height.
///
/// Proportional rather than PDFium's fixed 1 pt. Both are defensible for body
/// text and only one survives a heading: a 1 pt strikeout across 36 pt type is
/// a hairline, and a reader who cannot see the line they just drew tries again.
/// No floor is needed --- a quad with no area is dropped by [`user_quads`]
/// before this is reached.
const LINE_FRACTION: f64 = 0.07;

/// A line's own rectangle inside a quad: `(bottom, height)` in the page's space.
///
/// **It stays inside the quad**, which is not a nicety. The appearance stream's
/// `/BBox` is the bounds of every quad, so anything drawn outside is clipped ---
/// an underline centred on the quad's bottom edge would lose its lower half in
/// every reader, and look like a thinner line rather than like a bug.
///
/// So an underline sits *on* the bottom edge and a strikeout is centred on the
/// middle. Both are expressed here rather than as an offset the caller applies,
/// because the two need different arithmetic and an offset that had to be
/// `LINE_FRACTION / 2.0` for one of them is a coincidence waiting to be tidied
/// into a defect.
fn line_rect(kind: MarkKind, bottom: f64, top: f64) -> (f64, f64) {
    let full = top - bottom;
    let thickness = full * LINE_FRACTION;
    match kind {
        // Not reached: a wash fills its quad and `appearance_stream` branches
        // before asking. Answered rather than `unreachable!()`, so that a fourth
        // kind added without reading this is a wrong-looking mark rather than a
        // panic in front of a reader.
        MarkKind::Highlight => (bottom, full),
        MarkKind::Underline => (bottom, thickness),
        MarkKind::StrikeOut => (bottom + full / 2.0 - thickness / 2.0, thickness),
        // Not reached either, and one step further out than the highlight
        // above: a comment has no appearance stream of ours at all, so nothing
        // asks where its line goes. Answered with the whole quad for the same
        // reason.
        MarkKind::Note => (bottom, full),
        // Not reached, and for a third reason: a box has an appearance stream,
        // it is drawn by `outline_path` rather than by a filled rectangle, and
        // it has no band inside its quad to describe. The whole quad again.
        MarkKind::Square => (bottom, full),
    }
}

/// How thick a box's outline is, in points.
///
/// **Fixed, where [`LINE_FRACTION`] is proportional**, and the reason the two
/// differ is worth stating because the obvious move is to make them agree. A
/// line through text scales with the text because the *text* decides how big
/// that mark is; nothing decides how big a box is except the reader, so a
/// border that grew with the rectangle would draw a box round a figure four
/// times heavier than one round a word. `markband.ts` holds the same number.
///
/// Public because `annot-probe --mode outline` measures the stroke it draws and
/// has to know how thick to expect it. A second copy of the number in the probe
/// would agree with a wrong value here as readily as with a right one.
pub const OUTLINE_WIDTH: f64 = 1.5;

/// A box's path inside its quad: `[x, y, width, height]` in the page's space.
///
/// **Inset by half the stroke width**, which is the same trap [`line_rect`] is
/// written around and it bites harder here. A stroke straddles its path, so a
/// rectangle stroked exactly on the quad's edge puts half of every side outside
/// the appearance stream's `/BBox`, and a `/BBox` clips. The result is a box
/// with hairline edges rather than a missing one --- it looks like a thin
/// border, not like a bug, which is why it is arithmetic here rather than a
/// comment somewhere.
///
/// A quad thinner than the stroke is not special-cased: the inset then crosses
/// over and the rectangle is drawn inside out, which PDF renders as nothing.
/// [`crate::docmodel`] refuses an empty mark and the frontend refuses a box
/// under four points, so reaching this needs a caller that has bypassed both.
fn outline_path(quad: [f64; 4]) -> [f64; 4] {
    let inset = OUTLINE_WIDTH / 2.0;
    [
        quad[0] + inset,
        quad[1] + inset,
        (quad[2] - quad[0]) - OUTLINE_WIDTH,
        (quad[3] - quad[1]) - OUTLINE_WIDTH,
    ]
}

/// `/QuadPoints`: four corners per quad, upper-left, upper-right, lower-left,
/// lower-right.
///
/// **That order is not the one PDF 32000-1 §12.5.6.10 appears to describe**, and
/// it is the one every producer writes and every consumer expects --- the
/// specification's wording is a known erratum. Writing the specification's
/// literal reading produces a highlight that draws as an hourglass or not at
/// all, which is why this is stated here rather than left to look arbitrary.
fn quad_points(quads: &[[f64; 4]]) -> Object {
    Object::Array(
        quads
            .iter()
            .flat_map(|&[llx, lly, urx, ury]| {
                [
                    Object::Real(llx as f32),
                    Object::Real(ury as f32),
                    Object::Real(urx as f32),
                    Object::Real(ury as f32),
                    Object::Real(llx as f32),
                    Object::Real(lly as f32),
                    Object::Real(urx as f32),
                    Object::Real(lly as f32),
                ]
            })
            .collect(),
    )
}

fn numbers(values: [f64; 4]) -> Object {
    Object::Array(values.iter().map(|v| Object::Real(*v as f32)).collect())
}

/// A PDF text string: an ASCII literal, or UTF-16BE with a byte-order mark.
///
/// The two encodings `annots.rs` reads are PDFDocEncoding and UTF-16BE, and this
/// writes the subset of the first that needs no table --- ASCII --- falling back
/// to the second for anything else. Choosing by content rather than always
/// writing UTF-16 keeps an ordinary author's name readable in a hex dump, which
/// is worth something when the next person to debug this is reading bytes.
fn text_string(value: &str) -> Object {
    if value.is_ascii() {
        return Object::string_literal(value);
    }
    let mut bytes = vec![0xFE, 0xFF];
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Object::String(bytes, lopdf::StringFormat::Hexadecimal)
}

/// The appearance stream a reader draws when it does not generate its own.
///
/// **Not optional, even though both PDFKit and PDFium generate one.** Measured
/// before this was written: a `/Highlight` with no `/AP` renders in Preview, so
/// the file would look right in the two readers this repository can drive --- and
/// what a reader generates is *its* wash, not ours, so the same file would
/// differ between them and could differ again after an update. An `/AP` is what
/// makes the appearance the document's own.
///
/// `/BBox` is the annotation's rectangle in page coordinates and the matrix is
/// the identity, so §12.5.5's mapping of the transformed box onto `/Rect` is a
/// no-op and the content below can be written in page coordinates rather than in
/// a translated space of its own.
///
/// `/Multiply` is what keeps the glyphs readable: a flat fill over the text
/// hides it, and the text is already in the pixels underneath. It is the same
/// choice, for the same reason, that the viewer's overlay makes with
/// `globalCompositeOperation`.
fn appearance_stream(
    doc: &mut Document,
    mark: &PlannedMark,
    quads: &[[f64; 4]],
    rect: [f64; 4],
) -> ObjectId {
    let style = ink(mark.kind);
    let mut state = Dictionary::new();
    state.set("Type", Object::Name(b"ExtGState".to_vec()));
    // Multiply for a wash so the words show through it, Normal for anything
    // opaque so it is the colour it says it is. A multiplied red line over black
    // text is black, which is a strikeout nobody can see.
    state.set(
        "BM",
        Object::Name(if style == Ink::Wash {
            b"Multiply".to_vec()
        } else {
            b"Normal".to_vec()
        }),
    );
    state.set("CA", Object::Real(1.0));
    state.set("ca", Object::Real(1.0));
    let state = doc.add_object(state);

    let mut states = Dictionary::new();
    states.set("GS0", Object::Reference(state));
    let mut resources = Dictionary::new();
    resources.set("ExtGState", Object::Dictionary(states));

    // `rg` sets the *fill* colour and `RG` the stroke's, and one operator does
    // not imply the other: a path stroked after only `rg` comes out black,
    // which on a red box looks like a colour that was ignored rather than one
    // that was never set. Both are written, in one colour, so the two can never
    // disagree.
    let mut content = format!(
        "/GS0 gs {r} {g} {b} rg {r} {g} {b} RG {OUTLINE_WIDTH} w\n",
        r = mark.color[0],
        g = mark.color[1],
        b = mark.color[2],
    );
    for quad in quads {
        match style {
            // The whole quad, filled.
            Ink::Wash => {
                let (x, y) = (quad[0], quad[1]);
                let (width, height) = (quad[2] - quad[0], quad[3] - quad[1]);
                content.push_str(&format!("{x} {y} {width} {height} re f\n"));
            }
            // A band inside it, filled. Same operator, different rectangle.
            Ink::Line => {
                let (x, width) = (quad[0], quad[2] - quad[0]);
                let (y, height) = line_rect(mark.kind, quad[1], quad[3]);
                content.push_str(&format!("{x} {y} {width} {height} re f\n"));
            }
            // Its edge, stroked. `re S` rather than `re f`, and the path is
            // inset so the stroke lands inside the /BBox -- see `outline_path`.
            Ink::Outline => {
                let [x, y, width, height] = outline_path(*quad);
                content.push_str(&format!("{x} {y} {width} {height} re S\n"));
            }
            // Nothing. Unreachable, because the caller does not build an
            // appearance stream for a kind that has none; written out rather
            // than caught by a wildcard so that a kind added later is a compile
            // error here as well as everywhere else.
            Ink::None => {}
        }
    }

    let mut dictionary = Dictionary::new();
    dictionary.set("Type", Object::Name(b"XObject".to_vec()));
    dictionary.set("Subtype", Object::Name(b"Form".to_vec()));
    dictionary.set("FormType", Object::Integer(1));
    dictionary.set("BBox", numbers(rect));
    dictionary.set("Resources", Object::Dictionary(resources));
    doc.add_object(lopdf::Stream::new(dictionary, content.into_bytes()))
}

/// Appends an annotation to a page's `/Annots`, whatever shape that array is in.
///
/// Three cases, and the middle one is why this is a function rather than three
/// lines at the call site: the entry may be missing, an inline array, or a
/// reference to an array object that other pages may also name. The reference is
/// followed and the array it points at is extended, so the file's own structure
/// is preserved rather than replaced with an inline copy.
///
/// # Errors
///
/// The page is not a dictionary, or `/Annots` is a reference to something that
/// is not an array. Both are malformed documents, and a mark written into one
/// anyway would be a mark nothing displays.
fn attach(doc: &mut Document, page: ObjectId, annotation: ObjectId) -> Result<(), String> {
    let existing = doc
        .get_object(page)
        .and_then(Object::as_dict)
        .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?
        .get(b"Annots")
        .ok()
        .cloned();

    match existing {
        Some(Object::Reference(array_id)) => {
            let array = doc
                .get_object_mut(array_id)
                .and_then(Object::as_array_mut)
                .map_err(|e| format!("this page's /Annots is not an array: {e}"))?;
            array.push(Object::Reference(annotation));
        }
        Some(Object::Array(mut array)) => {
            array.push(Object::Reference(annotation));
            doc.get_object_mut(page)
                .and_then(Object::as_dict_mut)
                .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?
                .set("Annots", Object::Array(array));
        }
        _ => {
            doc.get_object_mut(page)
                .and_then(Object::as_dict_mut)
                .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?
                .set("Annots", Object::Array(vec![Object::Reference(annotation)]));
        }
    }
    Ok(())
}

/// Refuses a deletion that cannot be expressed by removing page *objects*.
///
/// `/Kids` may name one page object twice, so two page numbers can be one page.
/// [`drop_pages`] works in objects and correctly keeps any object a surviving
/// number names --- which means "delete page 2" on such a document deletes
/// nothing, and the copy comes out with the page the reader removed still in it.
///
/// The alternative to refusing is removing one *entry* from the `/Kids` array
/// that holds it, which is a different operation on a different unit, and it is
/// worth saying plainly that this is a refusal of a real request rather than a
/// guard against a malformed one. It is the same shape as the conflicting-turns
/// refusal in [`agreed_turns`]: no output satisfies the plan, so the reader is
/// told instead of handed a file they would have to check.
///
/// `pages` is every page object in document order; `kept` and `dropped` are
/// one-based page numbers into it.
///
/// # Errors
///
/// A dropped page whose object a kept page also names.
fn unshared(pages: &[lopdf::ObjectId], kept: &[u32], dropped: &[u32]) -> Result<(), String> {
    let at = |number: &u32| pages.get(*number as usize - 1).copied();
    for gone in dropped {
        let Some(id) = at(gone) else { continue };
        let Some(shared) = kept.iter().find(|keep| at(keep) == Some(id)) else {
            continue;
        };
        return Err(format!(
            "pages {gone} and {shared} are the same page in this file, so page {gone} cannot be \
             removed on its own. Remove both, or keep both."
        ));
    }
    Ok(())
}

/// Writes `bytes` to `out` via a sibling temporary file and a rename.
/// The crop each kept source page is to get, refusing two that disagree.
///
/// One entry per **source** page rather than per plan position, because a page
/// object can appear in the order twice --- nothing in tpdf duplicates a page
/// today, and `docmodel` states what would have to be proved first, but the
/// writer must not be the thing that discovers it. Two positions naming one
/// object with different boxes have no output that satisfies both, so this
/// refuses rather than letting the later write win for both.
///
/// The same shape as `pagetree::agreed_turns` and deliberately not sharing its
/// code: that one compares a `u8` and this compares four `f64`, and folding them
/// together would mean a generic whose only two instantiations are these.
fn agreed_crops(plan: &Plan) -> Result<Vec<(u32, [f64; 4])>, String> {
    let mut order: Vec<u32> = Vec::new();
    let mut chosen: std::collections::HashMap<u32, ([f64; 4], usize)> =
        std::collections::HashMap::new();
    for (at, page) in plan.pages.iter().enumerate() {
        let Some(want) = page.crop else { continue };
        match chosen.get(&page.source) {
            None => {
                chosen.insert(page.source, (want, at));
                order.push(page.source);
            }
            Some(&(first, first_at)) if first != want => {
                return Err(format!(
                    "pages {} and {} are the same page in this file, so they cannot be cropped \
                     differently. Crop them the same way, or leave both as they are.",
                    first_at + 1,
                    at + 1
                ));
            }
            Some(_) => {}
        }
    }
    Ok(order.into_iter().map(|s| (s, chosen[&s].0)).collect())
}

fn write_atomically(out: &Path, bytes: &[u8]) -> Result<(), String> {
    let staged = stage(out, bytes)?;
    commit(&staged, out)
}

/// Writes `bytes` to the sibling temporary file for `out`, and names it.
///
/// One definition of where the partial file goes, read by both save paths ---
/// the in-place one stages and commits with the document's close between them,
/// and a second copy of `with_extension(PARTIAL)` is how the two halves would
/// come to disagree about which file the other meant.
fn stage(out: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    let partial = out.with_extension(PARTIAL);
    std::fs::write(&partial, bytes).map_err(|e| {
        // Nothing to remove: the failure is the write itself, and a file that
        // may or may not exist is removed below rather than guessed about here.
        let _ = std::fs::remove_file(&partial);
        format!("could not write {partial:?}: {e}")
    })?;
    Ok(partial)
}

/// Renames a staged file over `out`, removing it if that fails.
fn commit(staged: &Path, out: &Path) -> Result<(), String> {
    std::fs::rename(staged, out).map_err(|e| {
        let _ = std::fs::remove_file(staged);
        format!("could not put {out:?} in place: {e}")
    })
}

/// Whether two paths name the same file.
///
/// Canonicalized, so `./a.pdf` and an absolute path to the same file are one
/// file, and a symlink to the source is caught. A destination that does not
/// exist yet cannot be canonicalized --- which is the ordinary case --- so it
/// falls back to comparing the parent directory and the file name, and that
/// comparison is what makes the ordinary case answer correctly rather than
/// answering "different" for everything.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => canonical_parent(a) == canonical_parent(b) && a.file_name() == b.file_name(),
    }
}

fn canonical_parent(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or(Path::new("."));
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edits::PageView;
    use crate::pagetree::effective_rotation;
    use lopdf::Object;
    use std::collections::HashSet;

    /// A plan that keeps every page of an `n`-page document, turning each by
    /// `turns[i]`.
    ///
    /// The ids are the model's own numbering --- one per baseline page, from 1 ---
    /// and nothing here reads them: a plan is addressed by `source`, and the id
    /// travels only so that this is the shape the model really produces.
    /// [`plan_of`], fingerprinted against a real file.
    ///
    /// Every in-place test needs one: `stage_in_place` refuses a plan with no
    /// fingerprint, which is the point of that refusal and is why three existing
    /// tests went red the moment it was added. They are the ones that exercise
    /// the path where the reader's own file is at stake.
    fn plan_opened_as(turns: &[u8], source: &Path) -> Plan {
        Plan {
            opened_as: Some(
                crate::fingerprint::Fingerprint::of(source).expect("fingerprint the fixture"),
            ),
            ..plan_of(turns)
        }
    }

    fn plan_of(turns: &[u8]) -> Plan {
        Plan {
            opened_as: None,
            baseline: turns.len() as u32,
            pages: turns
                .iter()
                .enumerate()
                .map(|(at, &turns)| PageView {
                    id: at as u64 + 1,
                    source: at as u32,
                    turns,
                    crop: None,
                })
                .collect(),
            marks: Vec::new(),
        }
    }

    /// A plan over a `baseline`-page document that keeps only `kept`.
    ///
    /// `kept` is `(source, turns)`, zero-based, in the order the pages are to
    /// come out, which need not be the order the file has them.
    fn keeping(baseline: u32, kept: &[(u32, u8)]) -> Plan {
        Plan {
            opened_as: None,
            baseline,
            pages: kept
                .iter()
                .map(|&(source, turns)| PageView {
                    id: u64::from(source) + 1,
                    source,
                    turns,
                    crop: None,
                })
                .collect(),
            marks: Vec::new(),
        }
    }

    #[cfg(target_os = "macos")]
    use crate::print_macos as os_pdf;
    #[cfg(not(target_os = "macos"))]
    use crate::print_win as os_pdf;

    /// A scratch directory that removes itself.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("tpdf-save-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Scratch(dir)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(name: &str) -> Option<PathBuf> {
        let path = Path::new("../testdata").join(name);
        path.exists().then_some(path)
    }

    fn page_count(path: &Path) -> usize {
        Document::load(path).expect("load").get_pages().len()
    }

    /// A rotation applied here has to be visible to a parser that shares no code
    /// with the one that wrote it.
    ///
    /// `rotated.pdf` is the fixture because its four pages carry 0/90/180/270 and
    /// are otherwise identical: on a document with one rotation throughout, a
    /// writer that turned the *wrong* page would produce the same set of
    /// rotations and nothing here could tell. The run says which of the two cases
    /// each fixture was, for the reason `print.rs` says it.
    #[test]
    fn a_third_parser_sees_the_turn_on_the_page_it_was_applied_to() {
        let scratch = Scratch::new("third-parser");
        let mut examined = 0;
        for name in ["rotated.pdf", "text-heavy.pdf", "mixed.pdf", "links.pdf"] {
            let Some(path) = fixture(name) else {
                println!("[SKIP] {name}: fixture not generated");
                continue;
            };
            let source = std::fs::read(&path).expect("read source");
            let Some(before) = os_pdf::read(&source) else {
                println!("[SKIP] {name}: the OS parser refused the source document");
                continue;
            };
            let count = before.pages.len();
            if count < 2 {
                println!("[SKIP] {name}: {count} page, nothing to leave alone");
                continue;
            }

            // One quarter turn on the second page and nothing anywhere else, so
            // the check sees both directions: the page that moved, and the pages
            // that must not have.
            let mut turns = vec![0u8; count];
            turns[1] = 1;

            let out = scratch.join(&format!("{name}.out.pdf"));
            write_copy(&path, &plan_of(&turns), &out).unwrap_or_else(|e| panic!("{name}: {e}"));

            let written = std::fs::read(&out).expect("read written");
            let after = os_pdf::read(&written)
                .unwrap_or_else(|| panic!("{name}: the OS parser could not read the saved copy"));

            assert_eq!(after.pages.len(), count, "{name}: page count");
            let expected: Vec<i64> = before
                .pages
                .iter()
                .enumerate()
                .map(|(at, page)| (page.rotation + if at == 1 { 90 } else { 0 }).rem_euclid(360))
                .collect();
            let got: Vec<i64> = after
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect();
            assert_eq!(got, expected, "{name}: rotations");

            let distinct: HashSet<i64> = before
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect();
            let discriminating = if distinct.len() > 1 {
                "pins which page was turned"
            } else {
                "pins the composition only"
            };
            println!("[OK] {name:16} {count} pages, rotations {distinct:?} --- {discriminating}");
            examined += 1;
        }
        assert!(
            examined > 0,
            "no fixture was examined --- generate testdata/ (BUILD.md, Test fixtures)"
        );
    }

    /// A plan over `n` pages, cropping the page at `at` to `box_pt`.
    fn plan_cropping(n: usize, at: usize, box_pt: [f64; 4]) -> Plan {
        let mut plan = plan_of(&vec![0u8; n]);
        if let Some(page) = plan.pages.get_mut(at) {
            page.crop = Some(box_pt);
        }
        plan
    }

    /// A crop in the plan reaches the written file, on that page and no other.
    #[test]
    fn a_crop_reaches_the_file_it_was_planned_for() {
        let Some(path) = fixture("text.pdf") else {
            println!("[SKIP] text.pdf not generated");
            return;
        };
        let scratch = Scratch::new("crop");
        let count = page_count(&path);
        assert!(count > 1, "this needs a second page to be the control");
        let out = scratch.join("cropped.pdf");
        let want = [72.0, 100.0, 400.0, 600.0];
        write_copy(&path, &plan_cropping(count, 0, want), &out).expect("write");

        let after = Document::load(&out).expect("load written");
        let ids = ordered_pages(&after);
        let read = |id| crate::pagetree::box_on(&after, id, b"CropBox").map(|b| b.map(f64::from));
        assert_eq!(read(ids[0]), Some(want), "the cropped page");
        // The control: every other page is as it was. Without it a write onto
        // the shared `/Pages` node would satisfy the assertion above and crop
        // the whole document.
        for (at, id) in ids.iter().enumerate().skip(1) {
            assert_eq!(read(*id), None, "page {at} was cropped too");
        }
    }

    /// Two positions naming one page with different crops have no output.
    ///
    /// Nothing duplicates a page today, so this cannot arise from the model ---
    /// it is the writer refusing to be the thing that discovers it, the same
    /// shape `agreed_turns` refuses.
    #[test]
    fn one_page_cropped_two_ways_is_refused_and_cropped_one_way_is_not() {
        let mut plan = plan_of(&[0, 0]);
        plan.pages[1].source = 0;
        plan.pages[0].crop = Some([0.0, 0.0, 100.0, 100.0]);
        plan.pages[1].crop = Some([0.0, 0.0, 200.0, 200.0]);
        let why = agreed_crops(&plan).expect_err("two crops for one page");
        assert!(why.contains("cannot be cropped"), "{why}");

        // The control: the same document with the same crop twice is one entry
        // rather than a refusal, so this refuses a disagreement and not a
        // repetition.
        plan.pages[1].crop = plan.pages[0].crop;
        assert_eq!(
            agreed_crops(&plan).expect("one crop, twice"),
            vec![(0, [0.0, 0.0, 100.0, 100.0])]
        );
    }

    /// A plan with no crops asks the writer for nothing.
    #[test]
    fn a_plan_with_no_crop_writes_no_crop_box() {
        // The emptiness control. Without it, a version returning every page
        // unconditionally would pass the two tests above and write a `/CropBox`
        // onto every page of every document tpdf saves.
        assert_eq!(
            agreed_crops(&plan_of(&[0, 0, 0])).expect("no crops"),
            Vec::new()
        );
    }

    #[test]
    fn a_turn_composes_with_the_rotation_the_page_already_had() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("compose");
        let count = page_count(&path);
        // Two turns on every page, so each page's answer is its own start plus
        // 180 --- a writer that *set* rather than composed would produce 180
        // everywhere and this is the fixture where those differ.
        let turns = vec![2u8; count];
        let out = scratch.join("composed.pdf");
        write_copy(&path, &plan_of(&turns), &out).expect("write");

        let before = Document::load(&path).expect("load source");
        let after = Document::load(&out).expect("load written");
        let source_ids: Vec<_> = ordered_pages(&before);
        let written_ids: Vec<_> = ordered_pages(&after);
        for (at, (from, to)) in source_ids.iter().zip(&written_ids).enumerate() {
            let expected = (effective_rotation(&before, *from) + 180).rem_euclid(360);
            let got = effective_rotation(&after, *to).rem_euclid(360);
            assert_eq!(got, expected, "page {at}");
        }
    }

    /// A page nobody turned must come out byte-for-byte as it went in, and the
    /// interesting case is a page that *inherits* its rotation.
    ///
    /// Writing `/Rotate 0` onto such a page would be a change --- it would
    /// override the inherited value --- and it would look like a no-op in any
    /// check that only compares the pages that were turned.
    #[test]
    fn a_page_that_was_not_turned_keeps_an_inherited_rotation() {
        let scratch = Scratch::new("inherit");
        let source = scratch.join("inherited.pdf");
        std::fs::write(&source, inheriting_document()).expect("write fixture");

        let out = scratch.join("out.pdf");
        // Turn the second page only. The first inherits 90 from the tree and is
        // left alone.
        write_copy(&source, &plan_of(&[0, 1]), &out).expect("write");

        let after = Document::load(&out).expect("load written");
        let ids = ordered_pages(&after);
        assert_eq!(
            effective_rotation(&after, ids[0]).rem_euclid(360),
            90,
            "the untouched page still inherits its rotation"
        );
        assert_eq!(
            effective_rotation(&after, ids[1]).rem_euclid(360),
            180,
            "the turned page composed onto the inherited 90"
        );

        // The assertion that can actually fail, and the reason the two above
        // cannot. `effective_rotation` answers 90 whether the page states it or
        // inherits it, so writing the composed value onto an untouched page
        // leaves every number above unchanged --- the mutation that does exactly
        // that survived until this line existed.
        //
        // Absence of the key is the property the guard is for: a page the reader
        // did not turn comes out as it went in. It is not cosmetic. The walk in
        // `effective_rotation` is bounded at 64 and answers 0 when it gives up,
        // so writing its answer onto every page would silently *flatten* the
        // rotation of any page whose `/Parent` chain is longer than that, or
        // whose chain loops --- pages nobody asked to change.
        assert!(
            after
                .get_object(ids[0])
                .and_then(Object::as_dict)
                .expect("the untouched page is a dictionary")
                .get(b"Rotate")
                .is_err(),
            "the untouched page states no rotation of its own; it still inherits one"
        );
        assert!(
            after
                .get_object(ids[1])
                .and_then(Object::as_dict)
                .expect("the turned page is a dictionary")
                .get(b"Rotate")
                .is_ok(),
            "the control: the page that WAS turned does state one, so the \
             assertion above is about the guard rather than about lopdf dropping \
             every key it writes"
        );
    }

    /// Two pages under a `/Pages` node carrying `/Rotate 90`, built by hand so
    /// that nothing under test wrote the input.
    fn inheriting_document() -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let kids: Vec<Object> = (0..2)
            .map(|_| {
                doc.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => pages_id,
                    "Contents" => content,
                })
                .into()
            })
            .collect();
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => 2,
                "Rotate" => 90,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    /// Two page numbers, one page object: `/Kids` names the same page twice.
    ///
    /// Hand-built, because nothing in this repository writes a document like this
    /// and no fixture in the corpus is malformed this way --- which is exactly how
    /// the shape survived review twice.
    fn shared_page_document() -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page), Object::Reference(page)],
                "Count" => 2,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    /// The precondition every shared-page check rests on, asserted rather than
    /// assumed.
    ///
    /// `lopdf`'s `PageTreeIter` keeps no visited set today. If a future version
    /// deduplicates, the guards below become reachable by nothing while their
    /// outcome assertions keep passing --- so this is the check that says so, and
    /// it is the one to read first when it goes red.
    #[test]
    fn the_fixture_really_does_present_one_object_under_two_page_numbers() {
        let scratch = Scratch::new("shared-precondition");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");

        let doc = Document::load(&source).expect("load fixture");
        let ids = ordered_pages(&doc);
        assert_eq!(ids.len(), 2, "two page numbers");
        assert_eq!(
            ids[0], ids[1],
            "and both resolve to ONE object --- if lopdf has started deduplicating \
             its page walk, every shared-page guard is now dead code"
        );
    }

    #[test]
    fn a_page_reached_twice_is_turned_once() {
        let scratch = Scratch::new("shared-turn");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        // One quarter-turn asked for on each page number. They are one page, so
        // the answer is one quarter-turn, not two.
        write_copy(&source, &plan_of(&[1, 1]), &out).expect("agreeing turns are honoured");

        let after = Document::load(&out).expect("load written");
        let ids = ordered_pages(&after);
        assert_eq!(
            effective_rotation(&after, ids[0]).rem_euclid(360),
            90,
            "turned once. Composing per page number would read back the 90 it had \
             just written and leave 180"
        );
    }

    #[test]
    fn a_page_reached_twice_cannot_be_turned_two_ways() {
        let scratch = Scratch::new("shared-conflict");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        let why = write_copy(&source, &plan_of(&[1, 2]), &out).expect_err("must refuse");
        assert!(
            why.message.contains("same page"),
            "the message says why rather than naming an internal id: {why}"
        );
        assert!(
            why.message.contains('1') && why.message.contains('2'),
            "and names the two pages the reader can see: {why}"
        );
        assert!(
            !out.exists(),
            "and nothing was written --- the refusal comes before any bytes"
        );
    }

    /// The over-refusal control, and the reason this is not a blanket refusal.
    ///
    /// A document nobody edited has a plan of zeros. Refusing it because its page
    /// tree is malformed would deny a save that has nothing to reconcile, which is
    /// the common case by a wide margin.
    #[test]
    fn a_page_reached_twice_is_saved_normally_when_nothing_conflicts() {
        let scratch = Scratch::new("shared-benign");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        write_copy(&source, &plan_of(&[0, 0]), &out).expect("an unedited document still saves");
        assert!(out.exists());
    }

    /// A deleted page is gone from the copy --- and the check says *which* pages
    /// are left, not how many.
    ///
    /// `rotated.pdf` again, and for the same reason it is the fixture for the
    /// turn: its four pages carry 0/90/180/270 and are otherwise identical, so a
    /// save that dropped the *wrong* page produces a document with the right page
    /// count and the wrong contents. The rotations are the only thing that tells
    /// those two apart, which is why a page-count assertion on its own would be
    /// satisfied by either.
    ///
    /// Read back through the platform's own parser, never through the `lopdf`
    /// that wrote it.
    #[test]
    fn a_third_parser_sees_the_pages_that_were_kept_and_not_the_one_that_was_not() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let source = std::fs::read(&path).expect("read source");
        let Some(before) = os_pdf::read(&source) else {
            println!("[SKIP] the OS parser refused rotated.pdf");
            return;
        };
        assert_eq!(
            before.pages.len(),
            4,
            "the fixture this check is written for"
        );
        let rotations: Vec<i64> = before
            .pages
            .iter()
            .map(|page| page.rotation.rem_euclid(360))
            .collect();
        assert_eq!(
            rotations.iter().collect::<HashSet<_>>().len(),
            4,
            "the fixture discriminates: four pages, four different rotations. \
             Without that, dropping the wrong page is invisible here"
        );

        let scratch = Scratch::new("delete");
        let out = scratch.join("kept.pdf");
        // Page 2 removed; the other three keep their own rotations.
        write_copy(&path, &keeping(4, &[(0, 0), (2, 0), (3, 0)]), &out).expect("write");

        let written = std::fs::read(&out).expect("read written");
        let after = os_pdf::read(&written).expect("the OS parser reads the saved copy");
        assert_eq!(
            after
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            vec![rotations[0], rotations[2], rotations[3]],
            "pages 1, 3 and 4 in that order --- a count of three is equally true of \
             the three WRONG pages"
        );
        println!(
            "[OK] rotated.pdf 4 pages {rotations:?} --- kept 1,3,4 and read back \
             through the platform parser"
        );
    }

    /// Deleting and turning in one plan, since the two arrive together.
    ///
    /// The turn is aimed at a page *after* the deleted one, which is the case
    /// that fails if anything resolves a plan entry against the document's page
    /// numbers after the pages have gone: `get_pages` renumbers from 1, so the
    /// old page 4 becomes page 3 and a turn aimed at "page 4" lands on nothing.
    #[test]
    fn a_turn_on_a_page_after_the_deleted_one_lands_where_it_was_aimed() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("delete-turn");
        let out = scratch.join("out.pdf");

        let before = Document::load(&path).expect("load source");
        let source_ids = ordered_pages(&before);
        // Drop page 2, and turn what was page 4 by a quarter.
        write_copy(&path, &keeping(4, &[(0, 0), (2, 0), (3, 1)]), &out).expect("write");

        let after = Document::load(&out).expect("load written");
        let ids = ordered_pages(&after);
        assert_eq!(ids.len(), 3);
        assert_eq!(
            effective_rotation(&after, ids[2]).rem_euclid(360),
            (effective_rotation(&before, source_ids[3]) + 90).rem_euclid(360),
            "the last page is the old page 4, a quarter past where it was"
        );
        assert_eq!(
            effective_rotation(&after, ids[1]).rem_euclid(360),
            effective_rotation(&before, source_ids[2]).rem_euclid(360),
            "and the page before it, which nobody turned, is untouched"
        );
    }

    /// The outline goes when pages do, with the control that says it stays.
    ///
    /// Its destinations name pages that are no longer in the file. Dropping it
    /// whole is a real loss and is the only option that cannot leave a
    /// *malformed* one --- see `pagetree::drop_outline`.
    #[test]
    fn deleting_a_page_drops_the_outline_and_keeping_them_all_does_not() {
        let Some(path) = fixture("outline-simple.pdf") else {
            println!("[SKIP] outline-simple.pdf not generated");
            return;
        };
        let scratch = Scratch::new("outline");
        let count = page_count(&path);
        assert!(count > 1, "the fixture needs a page to spare");
        assert!(
            has_outline(&Document::load(&path).expect("load source")),
            "the fixture carries one to begin with"
        );

        let kept: Vec<(u32, u8)> = (1..count as u32).map(|source| (source, 0)).collect();
        let trimmed = scratch.join("trimmed.pdf");
        write_copy(&path, &keeping(count as u32, &kept), &trimmed).expect("write");
        assert!(
            !has_outline(&Document::load(&trimmed).expect("load written")),
            "a page was dropped, so its destinations are gone"
        );

        // The control. Without it this check passes for a save that drops every
        // outline it ever sees, which is a different and much worse rule.
        let whole = scratch.join("whole.pdf");
        write_copy(&path, &plan_of(&vec![0u8; count]), &whole).expect("write");
        assert!(
            has_outline(&Document::load(&whole).expect("load written")),
            "nothing was dropped, so the bookmarks survive"
        );
    }

    fn has_outline(doc: &Document) -> bool {
        doc.catalog()
            .expect("a catalog")
            .get(b"Outlines")
            .map(|entry| {
                // A dangling reference is not an outline. `drop_outline` removes
                // the key, but a reader of this helper should not have to know
                // that to trust the answer.
                entry
                    .as_reference()
                    .is_ok_and(|id| doc.get_object(id).is_ok())
            })
            .unwrap_or(false)
    }

    /// Deleting one of two page numbers that are one page is refused.
    ///
    /// Not a guard against a malformed document --- it is a refusal of a request
    /// no output satisfies. `drop_pages` removes page *objects* and correctly
    /// keeps any object a surviving number names, so the deletion would silently
    /// do nothing: the reader would be handed a copy with the page they removed
    /// still in it, which is the plausible-wrong-answer shape this file is built
    /// against. Found by writing the test that expected the deletion to work.
    #[test]
    fn deleting_one_of_two_numbers_that_are_one_page_is_refused() {
        let scratch = Scratch::new("shared-delete");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        let why = write_copy(&source, &keeping(2, &[(0, 0)]), &out).expect_err("must refuse");
        assert!(
            why.message.contains("same page") && why.message.contains("on its own"),
            "the message says what cannot be done and what can: {why}"
        );
        assert!(!out.exists(), "and nothing was written");
    }

    /// The over-refusal control for the check above.
    ///
    /// Removing *both* numbers of a shared page is expressible --- the object goes
    /// --- and a save that refused every shared page outright would pass the test
    /// above while denying this one.
    #[test]
    fn deleting_both_numbers_of_a_shared_page_is_not_refused() {
        let scratch = Scratch::new("shared-delete-both");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_and_a_spare()).expect("write fixture");
        let out = scratch.join("out.pdf");

        // Pages 1 and 2 are one object; page 3 is its own. Keep only page 3.
        write_copy(&source, &keeping(3, &[(2, 0)]), &out).expect("write");
        let after = Document::load(&out).expect("load written");
        assert_eq!(
            ordered_pages(&after).len(),
            1,
            "both numbers went, so the object they shared went with them"
        );
    }

    /// The shared-page fixture with one ordinary page after it.
    ///
    /// `shared_page_document` cannot express "delete both", because a document
    /// must keep at least one page and both of its numbers are the same object.
    fn shared_page_and_a_spare() -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let shared = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
        });
        let spare = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![
                    Object::Reference(shared),
                    Object::Reference(shared),
                    Object::Reference(spare),
                ],
                "Count" => 3,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    /// A moved page comes out where the reader put it, read by a third parser.
    ///
    /// `rotated.pdf` for the third time, and for the third reason: its four pages
    /// carry 0/90/180/270 and are otherwise identical, so the rotations are a
    /// *name* for each page. A save that wrote them in file order produces the
    /// same four pages and the same page count, and nothing but the sequence of
    /// rotations can tell the two apart.
    #[test]
    fn a_plan_whose_pages_have_moved_comes_out_in_the_order_the_reader_put_them() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let source = std::fs::read(&path).expect("read source");
        let Some(before) = os_pdf::read(&source) else {
            println!("[SKIP] the OS parser refused rotated.pdf");
            return;
        };
        let at: Vec<i64> = before
            .pages
            .iter()
            .map(|page| page.rotation.rem_euclid(360))
            .collect();
        assert_eq!(
            at.iter().collect::<HashSet<_>>().len(),
            4,
            "the fixture discriminates: four pages, four different rotations"
        );

        let scratch = Scratch::new("reordered");
        let out = scratch.join("out.pdf");
        write_copy(&path, &keeping(4, &[(2, 0), (0, 0), (3, 0), (1, 0)]), &out).expect("write");

        let written = std::fs::read(&out).expect("read written");
        let after = os_pdf::read(&written).expect("the OS parser reads the saved copy");
        assert_eq!(
            after
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            vec![at[2], at[0], at[3], at[1]],
            "the pages are in the reader's order, not the file's"
        );

        // The control, and it is not ceremony: a save that reordered *every*
        // plan would pass the assertion above and would flatten the page tree of
        // every document anyone ever saved.
        let untouched = scratch.join("untouched.pdf");
        write_copy(
            &path,
            &keeping(4, &[(0, 0), (1, 0), (2, 0), (3, 0)]),
            &untouched,
        )
        .expect("in order");
        let read_back = std::fs::read(&untouched).expect("read");
        assert_eq!(
            os_pdf::read(&read_back)
                .expect("read back")
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            at,
            "a plan in document order is the document"
        );
    }

    /// Moving, deleting and turning in one plan, since a reader does all three.
    ///
    /// The turn is on a page that both moved *and* sits after the deleted one,
    /// which is the entry that goes wrong if anything resolves the plan against
    /// the document's page numbers after the tree has been rewritten under it.
    #[test]
    fn a_page_that_moved_carries_its_turn_to_where_it_landed() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("move-delete-turn");
        let out = scratch.join("out.pdf");

        let before = Document::load(&path).expect("load source");
        let source_ids = ordered_pages(&before);
        // Page 2 dropped; the old page 4 moved to the front and turned a quarter.
        write_copy(&path, &keeping(4, &[(3, 1), (0, 0), (2, 0)]), &out).expect("write");

        let after = Document::load(&out).expect("load written");
        let ids = ordered_pages(&after);
        assert_eq!(ids, vec![source_ids[3], source_ids[0], source_ids[2]]);
        assert_eq!(
            effective_rotation(&after, ids[0]).rem_euclid(360),
            (effective_rotation(&before, source_ids[3]) + 90).rem_euclid(360),
            "the page that moved to the front is a quarter past where it was"
        );
        assert_eq!(
            effective_rotation(&after, ids[2]).rem_euclid(360),
            effective_rotation(&before, source_ids[2]).rem_euclid(360),
            "and a page that only moved is at the angle it always was"
        );
    }

    /// A moved page keeps a rotation it *inherited*, read by a third parser.
    ///
    /// The mechanism is `pagetree::reorder_pages`, which has its own checks on a
    /// document in memory. This is the end of the same wire: a real file, the
    /// save path, and PDFKit rather than the `lopdf` that wrote it. `/Rotate` is
    /// the inheritable attribute the OS parser reports, which is what makes the
    /// property observable here at all --- the size would need a field neither
    /// platform reading has.
    ///
    /// Without the push-down, the page hanging under the node that states
    /// `/Rotate 90` is reparented to a root that states nothing and comes back
    /// upright, in a file that opens and looks plausible.
    #[test]
    fn a_third_parser_sees_an_inherited_rotation_survive_a_move() {
        let scratch = Scratch::new("inherit-move");
        let source = scratch.join("nested.pdf");
        std::fs::write(&source, nested_document()).expect("write fixture");

        let original = std::fs::read(&source).expect("read");
        let Some(before) = os_pdf::read(&original) else {
            println!("[SKIP] the OS parser refused the hand-built nested document");
            return;
        };
        assert_eq!(
            before
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            vec![0, 0, 90, 90],
            "the precondition: two pages inherit 90 from a node the root knows \
             nothing about, and two inherit nothing"
        );

        let out = scratch.join("moved.pdf");
        // The last page to the front, so it leaves the node it inherited from.
        write_copy(
            &source,
            &keeping(4, &[(3, 0), (0, 0), (1, 0), (2, 0)]),
            &out,
        )
        .expect("write");

        let written = std::fs::read(&out).expect("read written");
        let after = os_pdf::read(&written).expect("the OS parser reads the saved copy");
        assert_eq!(
            after
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            vec![90, 0, 0, 90],
            "the moved page took its inherited rotation with it"
        );
    }

    /// A plan the reader did not rearrange leaves the page tree where it is.
    ///
    /// The control for the two checks above, and the one that says why `moved`
    /// is computed at all. Rebuilding the tree produces the same *document* for
    /// a plan in document order --- same pages, same order, same rotations ---
    /// so nothing about the reader's view can tell the two apart. What differs
    /// is every page's ancestry, and a copy that reparented all 775 pages of a
    /// document nobody rearranged is a rewrite nobody asked for.
    #[test]
    fn a_plan_in_document_order_leaves_the_page_tree_as_it_found_it() {
        let scratch = Scratch::new("tree-untouched");
        let source = scratch.join("nested.pdf");
        std::fs::write(&source, nested_document()).expect("write fixture");
        assert_eq!(
            first_kid_type(&Document::load(&source).expect("load")),
            "Pages",
            "the precondition: the root's first child is a tree node, not a page"
        );

        let out = scratch.join("copied.pdf");
        write_copy(
            &source,
            &keeping(4, &[(0, 0), (1, 0), (2, 0), (3, 0)]),
            &out,
        )
        .expect("write");
        assert_eq!(
            first_kid_type(&Document::load(&out).expect("load written")),
            "Pages",
            "the tree is the one the file had"
        );

        // The control for the control: the same document rearranged does come
        // out flat, so the assertion above is about this plan rather than about
        // a reorder that never happens.
        let moved = scratch.join("moved.pdf");
        write_copy(
            &source,
            &keeping(4, &[(3, 0), (0, 0), (1, 0), (2, 0)]),
            &moved,
        )
        .expect("write");
        assert_eq!(
            first_kid_type(&Document::load(&moved).expect("load written")),
            "Page"
        );
    }

    /// The `/Type` of the first thing the catalog's page tree points at.
    fn first_kid_type(doc: &Document) -> String {
        let root = doc
            .catalog()
            .expect("a catalog")
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("a page tree");
        let first = doc
            .get_object(root)
            .and_then(Object::as_dict)
            .expect("the root")
            .get(b"Kids")
            .and_then(Object::as_array)
            .expect("kids")
            .first()
            .and_then(|entry| entry.as_reference().ok())
            .expect("a first kid");
        String::from_utf8_lossy(
            doc.get_object(first)
                .and_then(Object::as_dict)
                .expect("a kid")
                .get(b"Type")
                .and_then(Object::as_name)
                .expect("a type"),
        )
        .into_owned()
    }

    /// Four pages under two `/Pages` nodes, one of which states `/Rotate 90`.
    ///
    /// Hand-built because the corpus has nothing like it: `text-heavy.pdf` is the
    /// only nested fixture, three levels deep, and every inheritable attribute it
    /// has sits on the *root* --- so flattening it onto the root preserves
    /// everything and it cannot tell a reorder that carries inherited attributes
    /// from one that drops them.
    fn nested_document() -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let root_id = doc.new_object_id();
        let left_id = doc.new_object_id();
        let right_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));

        let page = |parent: lopdf::ObjectId, doc: &mut Document| {
            doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => parent,
                "Contents" => content,
            })
        };
        let a = page(left_id, &mut doc);
        let b = page(left_id, &mut doc);
        let c = page(right_id, &mut doc);
        let d = page(right_id, &mut doc);

        doc.objects.insert(
            left_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => root_id,
                "Kids" => vec![a.into(), b.into()],
                "Count" => 2,
            }),
        );
        doc.objects.insert(
            right_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => root_id,
                "Kids" => vec![c.into(), d.into()],
                "Count" => 2,
                "Rotate" => 90,
            }),
        );
        doc.objects.insert(
            root_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![left_id.into(), right_id.into()],
                "Count" => 4,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => root_id,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    /// A reorder keeps the bookmarks, where a deletion drops them.
    ///
    /// The two are one operation apart and land in opposite places, which is the
    /// reason this check exists beside the deletion one rather than instead of
    /// it: an outline destination names a page *object*, and a move leaves every
    /// object exactly where it was in the file. Dropping the outline for a move
    /// as well would be a loss nothing requires.
    #[test]
    fn a_reorder_keeps_the_outline_that_a_deletion_would_have_dropped() {
        let Some(path) = fixture("outline-simple.pdf") else {
            println!("[SKIP] outline-simple.pdf not generated");
            return;
        };
        let scratch = Scratch::new("outline-move");
        let count = page_count(&path);
        assert!(count > 2, "the fixture needs two pages to swap");

        // Every page kept, the first two swapped.
        let mut kept: Vec<(u32, u8)> = (0..count as u32).map(|source| (source, 0)).collect();
        kept.swap(0, 1);
        let out = scratch.join("swapped.pdf");
        write_copy(&path, &keeping(count as u32, &kept), &out).expect("write");

        assert!(
            has_outline(&Document::load(&out).expect("load written")),
            "nothing was deleted, so the bookmarks are still there --- they point \
             at page objects, and a move does not remove one"
        );
    }

    #[test]
    fn a_plan_naming_a_page_the_file_does_not_have_is_refused() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("past-end");
        let out = scratch.join("out.pdf");

        // A baseline that agrees with the file, and a page past its end. Only
        // reachable from a frontend sending something the model never produced,
        // which is exactly the argument for not trusting the number.
        let why = write_copy(&path, &keeping(4, &[(0, 0), (9, 0)]), &out).expect_err("must refuse");
        assert!(why.message.contains("does not have"), "{why}");
        assert!(!out.exists());
    }

    #[test]
    fn an_encrypted_document_is_refused_rather_than_quietly_decrypted() {
        let scratch = Scratch::new("encrypted");
        let source = scratch.join("locked.pdf");
        std::fs::write(&source, encrypted_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        let why = write_copy(&source, &plan_of(&[0]), &out).expect_err("must refuse");
        assert!(
            why.message.contains("encrypted"),
            "the message names the reason: {why}"
        );
        assert!(
            !out.exists(),
            "a refusal writes nothing, not even a temporary"
        );
        assert!(!out.with_extension(PARTIAL).exists());
    }

    /// A one-page document carrying an `/Encrypt` entry in its trailer.
    ///
    /// The encryption is not real --- nothing here encrypts any stream --- and it
    /// does not need to be: the guard is about the *presence* of the dictionary,
    /// which is what `lopdf` drops. A genuinely encrypted fixture would test the
    /// same branch and would additionally not load.
    fn encrypted_document() -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        let encrypt = doc.add_object(dictionary! {
            "Filter" => "Standard",
            "V" => 2,
            "R" => 3,
            "Length" => 128,
            "P" => -44i64,
        });
        doc.trailer.set("Root", catalog);
        doc.trailer.set("Encrypt", encrypt);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    #[test]
    fn a_plan_that_does_not_match_the_file_on_disk_is_refused() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("count");
        let out = scratch.join("out.pdf");
        let count = page_count(&path);

        let why =
            write_copy(&path, &plan_of(&vec![0u8; count + 1]), &out).expect_err("must refuse");
        assert!(why.message.contains("changed since it was opened"), "{why}");
        assert!(!out.exists());

        // And the matching plan is accepted, so the refusal is about the
        // mismatch rather than about this document.
        write_copy(&path, &plan_of(&vec![0u8; count]), &out).expect("the matching plan writes");
        assert!(out.exists());
    }

    #[test]
    fn an_empty_plan_is_refused() {
        let scratch = Scratch::new("empty");
        let out = scratch.join("out.pdf");
        let why = write_copy(Path::new("../testdata/rotated.pdf"), &plan_of(&[]), &out)
            .expect_err("must refuse");
        assert!(why.message.contains("at least one page"), "{why}");
    }

    /// A **copy** is never the source, and that is what this refuses.
    ///
    /// Saving in place is a real operation now --- [`stage_in_place`] and
    /// [`commit_in_place`] below --- so what makes this refusal right is no
    /// longer "tpdf does not do that". It is that `write_copy` writes and
    /// renames in one step, with no room between them for the close that an
    /// in-place save needs, so letting the source through here would replace a
    /// mapped file and leave the reader's worker serving the document that was.
    #[test]
    fn saving_over_the_open_document_is_refused() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("inplace");
        let copy = scratch.join("copy.pdf");
        std::fs::copy(&path, &copy).expect("copy fixture");
        let before = std::fs::read(&copy).expect("read");

        let why = write_copy(&copy, &plan_of(&[1, 0, 0, 0]), &copy).expect_err("must refuse");
        assert!(why.message.contains("save over"), "{why}");
        assert_eq!(
            std::fs::read(&copy).expect("read"),
            before,
            "the document is untouched"
        );

        // The same file reached by a different spelling of the path is still the
        // same file --- a comparison of the strings would let this through.
        let indirect = scratch.join(".").join("copy.pdf");
        assert!(write_copy(&copy, &plan_of(&[1, 0, 0, 0]), &indirect).is_err());
    }

    /// Staging writes the whole document and changes nothing the reader has.
    ///
    /// This is the property the `reopen: false` half of `lib.rs`'s
    /// `SaveFailure` rests on: everything expensive and everything refusable
    /// happens while the source is still the source, so a save that fails here
    /// has cost the reader nothing.
    ///
    /// It is also the **control** for the test below it. "The file holds the
    /// edits after a commit" is satisfied by an implementation that wrote them
    /// during the staging, and the two tests together are what separate the
    /// steps: here the source must be untouched, there it must not be.
    #[test]
    fn staging_a_save_in_place_writes_beside_the_source_and_leaves_it_alone() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("stage");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");
        let before = std::fs::read(&open).expect("read");

        let staged = stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open)).expect("stage");

        assert!(staged.path.exists(), "the staged file is written");
        assert_ne!(staged.path, open, "and it is not the source");
        assert_eq!(
            std::fs::read(&open).expect("read"),
            before,
            "the document the reader has is untouched until the commit"
        );
    }

    /// A file that changed under the open document is not saved over.
    ///
    /// The general form of the page-count refusal, and this test is built so that
    /// the page-count guard **cannot** be what fires: the modification appends
    /// bytes after `%%EOF`, which every parser here ignores, so the document still
    /// has exactly the pages the plan names. Before the fingerprint, this staged
    /// happily and the reader's edits were written onto a graph parsed from
    /// somebody else's bytes.
    #[test]
    fn a_save_in_place_is_refused_when_the_file_changed_under_it() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("changed");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        // Taken while the file is what the reader opened, exactly as `open_document`
        // takes it.
        let plan = plan_opened_as(&[1, 0, 0, 0], &open);

        // Somebody else writes to it. Appended rather than rewritten so the page
        // count is untouched -- if this test passed because the count changed it
        // would be testing the guard that already existed.
        let mut bytes = std::fs::read(&open).expect("read");
        assert_eq!(
            page_count(&open),
            4,
            "the fixture has the pages the plan names"
        );
        bytes.extend_from_slice(
            b"
% written by something else
",
        );
        std::fs::write(&open, &bytes).expect("rewrite");
        assert_eq!(
            page_count(&open),
            4,
            "and still has them afterwards, so the page-count guard cannot fire"
        );

        let why = stage_in_place(&open, &plan).expect_err("must refuse");
        assert!(why.message.contains("changed on disk"), "{why}");
        // The message has to leave the reader somewhere to go: their edits are
        // still in the journal, and Save a copy is the way to keep them.
        assert!(why.message.contains("another name"), "{why}");
        assert!(
            !open.with_extension(PARTIAL).exists(),
            "and nothing is staged beside the document"
        );
    }

    /// The control for the test above, and it is not the same fixture untouched.
    ///
    /// A guard that refused every save would pass that test and protect nothing.
    /// What this asserts is that the *same* plan, against a file nobody wrote to,
    /// stages -- so the refusal is about the change rather than about the check
    /// existing.
    #[test]
    fn an_unchanged_file_still_stages() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("unchanged");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let plan = plan_opened_as(&[1, 0, 0, 0], &open);
        let staged = stage_in_place(&open, &plan).expect("must stage");
        assert!(staged.path.exists());
    }

    /// No fingerprint means no save in place, and the message names the way out.
    ///
    /// Fail closed. "Could not look" and "looked, and it was fine" are different
    /// facts, and a save path that treats them alike writes over a file it has no
    /// evidence about. The way out has to keep working, which the next test
    /// asserts -- a refusal pointing at a fallback that is also refused is a
    /// dead end wearing a helpful sentence.
    #[test]
    fn a_save_in_place_with_no_fingerprint_is_refused_and_points_at_save_a_copy() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("noprint");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let why = stage_in_place(&open, &plan_of(&[1, 0, 0, 0])).expect_err("must refuse");
        assert!(why.message.contains("could not record"), "{why}");
        assert!(why.message.contains("Save a copy"), "{why}");
        // The message is one a reader reads, so it has to be one sentence rather
        // than one that happens to contain the right words. This assertion is
        // here because it was not: a lost `\` continuation left a run of 21
        // spaces mid-sentence, and both assertions above passed over it --- they
        // check the ends and the defect was in the middle.
        assert!(
            !why.message.contains("  "),
            "run of spaces in a reader-facing message: {why}"
        );
    }

    /// The last look before the rename refuses a file that moved under the save.
    ///
    /// This guard lived inline in `save_document` until 2026-08-19, where no test
    /// could reach it --- which is the shape `docs/TRAPS.md` records twice and
    /// which `lib.rs`'s own comment cited about the guard three lines above it.
    #[test]
    fn the_last_look_before_the_rename_refuses_a_source_that_moved() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("last-look");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let staged = stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open)).expect("stage");
        assert!(staged.path.exists(), "there is something to lose");

        // Something else writes while the document is being closed. Longer, so
        // the length is what answers -- the timestamp would too, and a test that
        // cannot say which mechanism refused is one this module has already been
        // caught writing.
        let mut bytes = std::fs::read(&open).expect("read");
        bytes.extend_from_slice(
            b"
% written by something else
",
        );
        std::fs::write(&open, &bytes).expect("rewrite");

        let why = verify_before_commit(&staged, &open).expect_err("must refuse");
        assert!(why.message.contains("length"), "{why}");
        assert!(why.message.contains("nothing was written"), "{why}");
        // And it must not carry the advice that belongs to the check before it.
        // The document is closed by the time a reader reads this, so telling
        // them their edits are still here and to save them under another name is
        // an instruction they cannot follow --- and it used to arrive in the same
        // sentence as "the document has been closed".
        assert!(!why.message.contains("still here"), "{why}");
        assert!(!why.message.contains("another name"), "{why}");
        assert!(why.message.contains("has been closed"), "{why}");
        // And **no instruction at all**. `save_document`'s caller reopens the
        // file itself on every `after_close`, so "open the file again" is advice
        // addressed to somebody it has already been carried out for --- which is
        // the two-moments failure this module was caught on, one layer up.
        assert!(!why.message.contains("open the file"), "{why}");
        // The flag, not the wording, is what the window reads.
        assert!(why.changed, "{why}");
        // The staged file goes with the refusal. Nothing else is tracking it by
        // this point, so leaving it puts a file the reader never named beside
        // their document.
        assert!(!staged.path.exists(), "and the staged file is cleaned up");
        // And the reader's file is exactly what the other writer left.
        assert_eq!(std::fs::read(&open).expect("read"), bytes);
    }

    /// The control, and it is the half that says the guard is not simply strict.
    #[test]
    fn the_last_look_lets_an_untouched_source_through() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("last-look-ok");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let staged = stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open)).expect("stage");
        assert_eq!(verify_before_commit(&staged, &open), Ok(()));
        assert!(
            staged.path.exists(),
            "and leaves the staged file to be committed"
        );
    }

    /// A copy is written even with no fingerprint, because it risks nothing.
    ///
    /// The asymmetry is deliberate and is the whole reason `stage_in_place` has a
    /// refusal `planned_bytes` does not: a copy that turns out to be built from
    /// changed bytes is a bad new file beside an intact original, and a save in
    /// place is the original. This is also what keeps the refusal above honest.
    #[test]
    fn a_copy_is_written_even_when_no_fingerprint_was_taken() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("copy-noprint");
        let open = scratch.join("open.pdf");
        let out = scratch.join("out.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        write_copy(&open, &plan_of(&[1, 0, 0, 0]), &out).expect("a copy needs no fingerprint");
        assert!(out.exists());
    }

    /// A copy IS written when the source changed, and says that it was.
    ///
    /// **This test asserted the opposite until 2026-08-19, and the assertion was
    /// the defect.** Refusing here closed the only door the in-place refusal
    /// points at: a reader whose file changed was told to save their edits under
    /// another name, and Save a copy was refused by the same guard one function
    /// down. `stage_in_place`'s own comment states the rule that was being broken
    /// --- "the fallback the message names has to keep working, or the refusal
    /// strands the reader" --- and it had been applied to a missing fingerprint
    /// and not to a changed file.
    ///
    /// The copy is not claimed to be correct, and `changed` is how it says so.
    /// What still refuses is a file whose *shape* changed, which the page-count
    /// guard catches whichever path asks.
    #[test]
    fn a_copy_is_written_when_the_source_changed_and_reports_it() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("copy-changed");
        let open = scratch.join("open.pdf");
        let out = scratch.join("out.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let plan = plan_opened_as(&[1, 0, 0, 0], &open);
        let mut bytes = std::fs::read(&open).expect("read");
        bytes.extend_from_slice(
            b"
% written by something else
",
        );
        std::fs::write(&open, &bytes).expect("rewrite");

        let copied = write_copy(&open, &plan, &out).expect("a copy risks nothing");
        assert!(copied.changed, "and it says the source had changed");
        assert!(out.exists(), "and the reader's edits are somewhere");
        // Still a real document rather than a placeholder, which is the half a
        // boolean cannot say.
        assert_eq!(page_count(&out), 4);
    }

    /// And the same source, unchanged, reports `changed: false`.
    ///
    /// The control. Without it the flag above is satisfied by a `changed` that is
    /// always true, which would put a warning on every copy a reader ever writes
    /// and teach them to ignore it.
    #[test]
    fn a_copy_from_an_untouched_source_does_not_claim_it_changed() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("copy-unchanged");
        let open = scratch.join("open.pdf");
        let out = scratch.join("out.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let copied = write_copy(&open, &plan_opened_as(&[1, 0, 0, 0], &open), &out)
            .expect("an untouched source copies");
        assert!(!copied.changed);
        assert!(out.exists());
    }

    /// A changed file whose page count also changed is still refused.
    ///
    /// The bound on the tolerance above, and the reason it is not simply "ignore
    /// the fingerprint for copies": the plan names pages by position, so a file
    /// that gained or lost one would have the edits land on pages nobody chose.
    /// That refusal carries `changed` too, so the window offers the same way out.
    #[test]
    fn a_copy_is_refused_when_the_changed_source_has_a_different_page_count() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("copy-reshaped");
        let open = scratch.join("open.pdf");
        let out = scratch.join("out.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");
        let plan = plan_opened_as(&[1, 0, 0, 0], &open);

        // A different document entirely, at the same path.
        let Some(other) = fixture("outline-simple.pdf") else {
            println!("[SKIP] outline-simple.pdf not generated");
            return;
        };
        std::fs::copy(&other, &open).expect("replace the source");
        assert_ne!(
            page_count(&open),
            4,
            "the fixture really is a different shape"
        );

        let why = write_copy(&open, &plan, &out).expect_err("must refuse");
        assert!(why.changed, "and it is offered as a change: {why}");
        assert!(!out.exists(), "and writes nothing");
    }

    /// Committing is what makes the file the reader opened the edited one.
    ///
    /// Read back through a second parse rather than by comparing bytes: the
    /// question is whether another reader of this file sees the turn, and a
    /// byte comparison would pass for a file that merely differs.
    #[test]
    fn committing_a_staged_save_puts_the_edits_in_the_file_the_reader_opened() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("commit");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let before = Document::load(&open).expect("load source");
        let was: Vec<i64> = ordered_pages(&before)
            .iter()
            .map(|id| effective_rotation(&before, *id).rem_euclid(360))
            .collect();

        let staged = stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open)).expect("stage");
        commit_in_place(&staged.path, &open).expect("commit");

        assert!(!staged.path.exists(), "nothing of the staged file survives");
        let after = Document::load(&open).expect("load the file the reader opened");
        let now: Vec<i64> = ordered_pages(&after)
            .iter()
            .map(|id| effective_rotation(&after, *id).rem_euclid(360))
            .collect();
        assert_eq!(
            now[0],
            (was[0] + 90).rem_euclid(360),
            "the page the reader turned is turned in their own file"
        );
        assert_eq!(&now[1..], &was[1..], "and nothing else moved");
    }

    /// A refusal on the way to a save in place leaves no trace beside the file.
    ///
    /// The page-count mismatch stands in for every guard `planned_bytes` states:
    /// they all run before a byte is written. What is asserted is the *absence*
    /// of the partial file, because a staged file nobody commits is a `.pdf`'s
    /// worth of bytes sitting next to the reader's document with a name they
    /// never chose.
    #[test]
    fn a_refused_save_in_place_stages_nothing() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("refused");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");
        let before = std::fs::read(&open).expect("read");
        let count = page_count(&open);

        let why = stage_in_place(&open, &plan_opened_as(&vec![0u8; count + 1], &open))
            .expect_err("must refuse");
        assert!(why.message.contains("changed since it was opened"), "{why}");
        assert!(
            !open.with_extension(PARTIAL).exists(),
            "no partial file is left beside the document"
        );
        assert_eq!(
            std::fs::read(&open).expect("read"),
            before,
            "and the document is untouched"
        );

        // The control: the same document with a plan that matches does stage,
        // so the refusal is about the mismatch rather than about this fixture.
        let staged =
            stage_in_place(&open, &plan_opened_as(&vec![0u8; count], &open)).expect("stage");
        assert!(staged.path.exists());
    }

    #[test]
    fn a_destination_that_does_not_exist_yet_is_not_mistaken_for_the_source() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("fresh");
        let out = scratch.join("brand-new.pdf");
        assert!(!out.exists(), "the control: it really is absent");
        write_copy(&path, &plan_of(&[0, 0, 0, 0]), &out).expect("a fresh destination is accepted");
        assert!(out.exists());
    }

    #[test]
    fn nothing_of_the_partial_file_survives_a_successful_write() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("partial");
        let out = scratch.join("done.pdf");
        write_copy(&path, &plan_of(&[1, 1, 1, 1]), &out).expect("write");
        assert!(out.exists());
        assert!(
            !out.with_extension(PARTIAL).exists(),
            "the temporary was renamed, not copied"
        );
    }

    /// The rename is what makes the write atomic, and the way to show it is to
    /// put something at the destination first.
    ///
    /// A reader that finds the *old* bytes has seen an interrupted save leave a
    /// whole file; a reader that finds a truncated file has seen the thing this
    /// avoids. The check plants a distinguishable old file and asserts it was
    /// replaced whole --- see `docs/TRAPS.md` on why an atomic-write test has to
    /// plant the intermediate.
    #[test]
    fn the_destination_is_replaced_whole_rather_than_written_through() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("replace");
        let out = scratch.join("existing.pdf");
        let planted = b"this is not a PDF, and it is longer than nothing".to_vec();
        std::fs::write(&out, &planted).expect("plant");

        // A second name for the *same file*, kept as a witness. A rename replaces
        // the directory entry `out` and leaves this one holding the old bytes; a
        // write straight through `out` writes through the shared file and changes
        // this one too. That difference is the whole of "atomic" here, and it is
        // the only observable of it that does not need the write interrupted.
        //
        // Everything below this comment used to be the whole test, and a mutation
        // replacing the temporary path with the destination survived it: the old
        // bytes are gone, it starts with %PDF and it has four pages under a direct
        // write as well. The docstring above claimed atomicity and the assertions
        // could not see it.
        let witness = scratch.join("witness.pdf");
        std::fs::hard_link(&out, &witness).expect("link the witness to the destination");
        assert_eq!(
            std::fs::read(&witness).expect("read witness"),
            planted,
            "the control: the witness really is the same file as the destination, \
             so a change to one is visible in the other"
        );

        write_copy(&path, &plan_of(&[0, 0, 0, 0]), &out).expect("write");

        // Deliberately not `assert_eq!`: the failing side is a whole PDF, and
        // `assert_eq!` on two `Vec<u8>` prints every byte as a decimal number ---
        // ~1,700 of them here, which buries the one line that says what went
        // wrong. The lengths and the first bytes are what a reader needs.
        let witnessed = std::fs::read(&witness).expect("read witness");
        assert!(
            witnessed == planted,
            "the destination was renamed into place, not written through: the file \
             that was there is untouched and still holds its own bytes. It holds {} \
             bytes beginning {:?}, where the planted file was {} bytes",
            witnessed.len(),
            String::from_utf8_lossy(&witnessed[..witnessed.len().min(8)]),
            planted.len()
        );

        let after = std::fs::read(&out).expect("read");
        assert_ne!(after, planted, "the old bytes are gone");
        assert!(
            after.starts_with(b"%PDF"),
            "and what is there is a whole document, not a prefix of one"
        );
        assert_eq!(
            page_count(&out),
            4,
            "the replacement is the document, not a fragment of it"
        );
    }

    // --- Marks ---------------------------------------------------------------

    /// A one-page document whose `/Annots` is written in the shape `annots`
    /// describes: absent, inline, or an indirect reference to an array.
    ///
    /// The three exist because `AGENTS.md` records that this distinction decides
    /// how large an annotation edit is --- and because a writer tested only
    /// against the absent case would corrupt the other two silently, by
    /// replacing an array other objects point at.
    fn document_with_annots(annots: AnnotShape) -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        };
        // The existing annotation is created only where it is referenced. An
        // unreferenced one left in the file for the absent case would be counted
        // by every check below -- which is how the first version of this made
        // "absent" report two annotations and look like a writer defect.
        match annots {
            AnnotShape::Absent => {}
            AnnotShape::Inline => {
                let existing = doc.add_object(existing_note());
                page.set("Annots", vec![Object::Reference(existing)]);
            }
            AnnotShape::Indirect => {
                let existing = doc.add_object(existing_note());
                let array = doc.add_object(Object::Array(vec![Object::Reference(existing)]));
                page.set("Annots", Object::Reference(array));
            }
        }
        let page_id = doc.add_object(page);
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
                "Resources" => resources,
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    /// A comment the document already had, so that extending its `/Annots` can
    /// be told from replacing it.
    fn existing_note() -> lopdf::Dictionary {
        use lopdf::dictionary;
        dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            "Contents" => Object::string_literal("already here"),
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum AnnotShape {
        Absent,
        Inline,
        Indirect,
    }

    /// A plan for a one-page document carrying one highlight.
    fn plan_with_mark(quads: Vec<crate::docmodel::Quad>) -> Plan {
        plan_of_kind(MarkKind::Highlight, quads)
    }

    fn plan_of_kind(kind: MarkKind, quads: Vec<crate::docmodel::Quad>) -> Plan {
        Plan {
            opened_as: None,
            baseline: 1,
            pages: vec![PageView {
                id: 1,
                source: 0,
                turns: 0,
                crop: None,
            }],
            marks: vec![PlannedMark {
                kind,
                source: 0,
                quads,
                color: [1.0, 0.9, 0.2],
                author: "a reader".to_string(),
                note: "a note".to_string(),
                made: "D:20260818120000Z".to_string(),
            }],
        }
    }

    fn one_quad() -> Vec<crate::docmodel::Quad> {
        vec![crate::docmodel::Quad {
            left: 72.0,
            top: 100.0,
            right: 300.0,
            bottom: 118.0,
        }]
    }

    /// The subtypes a saved page **lists** in its own `/Annots`, in order.
    ///
    /// **Not a scan of the file's objects**, which is what this was first
    /// written as --- and a mutation that cleared the page's array before
    /// appending survived it, because the annotation the array used to name is
    /// still an object in the file. An orphaned annotation is on no page and is
    /// reported by every reader as absent, so counting objects answers a
    /// question nobody is asking.
    fn listed_on_page(path: &Path, page: usize) -> Vec<String> {
        let doc = Document::load(path).expect("reopen");
        let id = ordered_pages(&doc)[page];
        let entry = doc
            .get_object(id)
            .and_then(Object::as_dict)
            .expect("a page dictionary")
            .get(b"Annots")
            .cloned();
        let array = match entry {
            Ok(Object::Array(array)) => array,
            Ok(Object::Reference(at)) => doc
                .get_object(at)
                .and_then(Object::as_array)
                .expect("an /Annots reference points at an array")
                .clone(),
            Ok(other) => panic!("/Annots is neither an array nor a reference: {other:?}"),
            Err(_) => Vec::new(),
        };
        array
            .iter()
            .map(|item| {
                let dictionary = match item {
                    Object::Reference(at) => doc
                        .get_object(*at)
                        .and_then(Object::as_dict)
                        .expect("an /Annots entry points at a dictionary"),
                    Object::Dictionary(inline) => inline,
                    other => panic!("an /Annots entry is neither: {other:?}"),
                };
                let subtype = dictionary
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .expect("an annotation has a /Subtype");
                String::from_utf8_lossy(subtype).to_string()
            })
            .collect()
    }

    #[test]
    fn a_mark_is_written_whatever_shape_the_page_s_annots_is_in() {
        for shape in [AnnotShape::Absent, AnnotShape::Inline, AnnotShape::Indirect] {
            let scratch = Scratch::new(&format!("annots-{shape:?}"));
            let source = scratch.join("in.pdf");
            let out = scratch.join("out.pdf");
            std::fs::write(&source, document_with_annots(shape)).expect("write fixture");

            write_copy(&source, &plan_with_mark(one_quad()), &out)
                .unwrap_or_else(|e| panic!("{shape:?}: {e}"));

            // What the *page* lists, in order. The comment that was already
            // there must still be first and the mark must be appended after it:
            // an `/Annots` replaced rather than extended loses the first, and
            // one written in the wrong order would put a new highlight above
            // comments the document came with.
            let listed = listed_on_page(&out, 0);
            let expected: Vec<&str> = match shape {
                AnnotShape::Absent => vec!["Highlight"],
                _ => vec!["Text", "Highlight"],
            };
            assert_eq!(listed, expected, "{shape:?}: the page lists {listed:?}");
        }
    }

    #[test]
    fn a_marked_page_lists_the_mark_in_its_own_annots() {
        // Written into the *page*, not merely into the file. An annotation
        // object nothing points at is in the document and on no page, which
        // every reader reports as a document with no comments -- and which every
        // assertion counting objects would pass.
        let scratch = Scratch::new("annots-reachable");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");
        write_copy(&source, &plan_with_mark(one_quad()), &out).expect("save");

        let doc = Document::load(&out).expect("reopen");
        let page = ordered_pages(&doc)[0];
        let listed = doc
            .get_object(page)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Annots"))
            .cloned()
            .expect("the page has an /Annots");
        let array = match listed {
            Object::Array(array) => array,
            Object::Reference(id) => doc
                .get_object(id)
                .and_then(Object::as_array)
                .expect("an /Annots reference points at an array")
                .clone(),
            other => panic!("/Annots is neither an array nor a reference: {other:?}"),
        };
        assert_eq!(array.len(), 1);
    }

    #[test]
    fn a_mark_on_a_page_two_numbers_share_is_refused() {
        // The same refusal `unshared` makes for a deletion, one level on: an
        // annotation hangs off a page *object*, so a mark made on page 1 would
        // appear on page 2 as well. `docs/TRAPS.md` records this shape twice
        // already, once live in `print.rs` for months.
        let scratch = Scratch::new("annots-shared");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");

        let plan = Plan {
            opened_as: None,
            baseline: 2,
            pages: vec![
                PageView {
                    id: 1,
                    source: 0,
                    turns: 0,
                    crop: None,
                },
                PageView {
                    id: 2,
                    source: 1,
                    turns: 0,
                    crop: None,
                },
            ],
            marks: vec![PlannedMark {
                kind: MarkKind::Highlight,
                source: 0,
                quads: one_quad(),
                color: [1.0, 0.9, 0.2],
                author: String::new(),
                note: String::new(),
                made: "D:20260818120000Z".to_string(),
            }],
        };
        let why = write_copy(&source, &plan, &out).expect_err("a shared page must be refused");
        assert!(
            why.message.contains("same page object"),
            "the refusal does not say why: {why}"
        );
        assert!(!out.exists(), "a refused save left a file behind");
    }

    #[test]
    fn a_mark_on_an_unshared_page_of_a_document_that_has_a_shared_one_is_written() {
        // The control for the refusal above, and the first version of it could
        // not run: it kept one of the two shared numbers, which `unshared`
        // refuses first for the deletion that implies -- so it exercised the
        // deletion guard and never reached the mark guard at all.
        //
        // This one keeps every page of a document where pages 1 and 2 are one
        // object and page 3 is its own, and marks page 3. A guard written as
        // "this file contains a shared page" rather than "this mark's page is
        // shared" would refuse it, and a reader would be told they cannot
        // highlight a page that has nothing to do with the malformed one.
        let scratch = Scratch::new("annots-shared-spare");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, shared_page_and_a_spare()).expect("write fixture");

        let plan = Plan {
            opened_as: None,
            baseline: 3,
            pages: (0..3)
                .map(|source| PageView {
                    id: u64::from(source) + 1,
                    source,
                    turns: 0,
                    crop: None,
                })
                .collect(),
            marks: vec![PlannedMark {
                kind: MarkKind::Highlight,
                source: 2,
                quads: one_quad(),
                color: [1.0, 0.9, 0.2],
                author: String::new(),
                note: String::new(),
                made: "D:20260818120000Z".to_string(),
            }],
        };
        write_copy(&source, &plan, &out).expect("a mark on the unshared page is fine");
        assert_eq!(listed_on_page(&out, 2), vec!["Highlight".to_string()]);
        // And nowhere else: a writer that put the mark on the first page it
        // found would satisfy the line above on a one-page document and is
        // exactly what this three-page fixture is for.
        assert!(listed_on_page(&out, 0).is_empty());
    }

    #[test]
    fn a_mark_whose_quads_all_collapse_is_refused_rather_than_written_empty() {
        let scratch = Scratch::new("annots-empty");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");

        let flat = vec![crate::docmodel::Quad {
            left: 72.0,
            top: 100.0,
            right: 72.0,
            bottom: 118.0,
        }];
        let why = write_copy(&source, &plan_with_mark(flat), &out)
            .expect_err("a mark covering nothing must be refused");
        assert!(why.message.contains("no area"), "{why}");
    }

    #[test]
    fn a_plan_carrying_a_mark_is_not_the_file_on_disk() {
        // `is_identity` is what lets the print path hand the original bytes over
        // untouched. A plan with a mark in it must never qualify, or a reader
        // prints a highlighted document and gets an unhighlighted one -- with
        // nothing failing, because the file it printed is a perfectly good file.
        let plain = Plan {
            opened_as: None,
            baseline: 1,
            pages: vec![PageView {
                id: 1,
                source: 0,
                turns: 0,
                crop: None,
            }],
            marks: Vec::new(),
        };
        assert!(plain.is_identity());
        assert!(!plan_with_mark(one_quad()).is_identity());
    }

    #[test]
    fn a_date_is_written_in_the_form_the_scan_reads_back() {
        // Fixed instants rather than `now`, and the epoch among them: the
        // arithmetic is shared with `diag.rs`, so what this pins is the *format*
        // -- the `D:` prefix, the zero padding and the trailing `Z`.
        assert_eq!(
            pdf_date(std::time::UNIX_EPOCH),
            "D:19700101000000Z",
            "the epoch"
        );
        assert_eq!(
            pdf_date(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_781_438_400)),
            "D:20260614120000Z"
        );
        // A clock before the epoch reads as the epoch rather than refusing, for
        // the reason `diag.rs` gives: the mark is worth more than the timestamp.
        assert_eq!(
            pdf_date(std::time::UNIX_EPOCH - std::time::Duration::from_secs(60)),
            "D:19700101000000Z"
        );
    }

    #[test]
    fn a_marked_document_still_refuses_what_it_refused_before() {
        // Marks are written before the turns and after the deletions, so the
        // three refusals `write_copy` documents have to survive one being
        // present. Encryption is the one that would be worst to lose: a mark
        // would then be the thing that silently stripped it.
        let scratch = Scratch::new("annots-encrypted");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, encrypted_document()).expect("write fixture");
        let why = write_copy(&source, &plan_with_mark(one_quad()), &out)
            .expect_err("an encrypted source must still be refused");
        assert!(why.message.contains("encrypted"), "{why}");
    }

    /// The written annotation for a mark of `kind`, reopened from the file.
    fn written_mark(kind: MarkKind, scratch: &Scratch) -> Dictionary {
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");
        write_copy(&source, &plan_of_kind(kind, one_quad()), &out).expect("save");
        let doc = Document::load(&out).expect("reopen");
        // The one annotation on the page, followed rather than searched for:
        // the fixture is written with none, so anything found here is ours.
        let page = ordered_pages(&doc)[0];
        let entry = doc
            .get_object(page)
            .and_then(Object::as_dict)
            .expect("a page dictionary")
            .get(b"Annots")
            .cloned()
            .expect("the page has an /Annots");
        let array = match entry {
            Object::Array(array) => array,
            Object::Reference(at) => doc
                .get_object(at)
                .and_then(Object::as_array)
                .expect("an /Annots reference points at an array")
                .clone(),
            other => panic!("/Annots is neither an array nor a reference: {other:?}"),
        };
        assert_eq!(array.len(), 1, "the fixture starts with no annotations");
        let Object::Reference(id) = array[0] else {
            panic!("the mark is not an indirect object")
        };
        doc.get_object(id)
            .and_then(Object::as_dict)
            .expect("the mark is a dictionary")
            .clone()
    }

    /// The blend mode the appearance stream's graphics state sets.
    ///
    /// In the `/ExtGState` the stream's `/Resources` names, not in the content
    /// -- which is where the first version of the test beside this looked, and
    /// why it wrote an assertion with an `||` in it that passed for the wrong
    /// reason. The content only ever says `/GS0 gs`.
    fn blend_of(kind: MarkKind, scratch: &Scratch) -> String {
        let (doc, stream) = written_appearance(kind, scratch);
        let states = stream
            .dict
            .get(b"Resources")
            .and_then(Object::as_dict)
            .and_then(|r| r.get(b"ExtGState"))
            .and_then(Object::as_dict)
            .expect("the appearance names an /ExtGState");
        let state = match states.get(b"GS0").expect("GS0") {
            Object::Reference(id) => doc
                .get_object(*id)
                .and_then(Object::as_dict)
                .expect("GS0 points at a dictionary")
                .clone(),
            Object::Dictionary(inline) => inline.clone(),
            other => panic!("GS0 is {other:?}"),
        };
        String::from_utf8(
            state
                .get(b"BM")
                .and_then(Object::as_name)
                .expect("the state sets a blend mode")
                .to_vec(),
        )
        .expect("a blend mode is a name")
    }

    /// The appearance stream's content for a written mark.
    fn appearance_of(kind: MarkKind, scratch: &Scratch) -> String {
        let (_, stream) = written_appearance(kind, scratch);
        String::from_utf8(
            stream
                .decompressed_content()
                .unwrap_or(stream.content.clone()),
        )
        .expect("the appearance stream is text")
    }

    /// The reopened document and the one form XObject a written mark adds.
    fn written_appearance(kind: MarkKind, scratch: &Scratch) -> (Document, lopdf::Stream) {
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");
        write_copy(&source, &plan_of_kind(kind, one_quad()), &out).expect("save");
        let doc = Document::load(&out).expect("reopen");
        // Every form XObject in the file, of which the fixture has none.
        let stream = doc
            .objects
            .values()
            .find_map(|object| match object {
                Object::Stream(stream)
                    if stream.dict.get(b"Subtype").and_then(Object::as_name).ok()
                        == Some(b"Form".as_slice()) =>
                {
                    Some(stream.clone())
                }
                _ => None,
            })
            .expect("the mark has an appearance stream");
        (doc, stream)
    }

    #[test]
    fn a_box_is_stroked_on_a_path_inset_by_half_its_own_width() {
        // **Two assertions about one line of the content stream, and neither is
        // sufficient alone.** `re S` says the box is a frame; the inset says the
        // frame is all there. A stroke straddles its path, so a rectangle
        // stroked on the quad's own edge puts half of every side outside the
        // appearance stream's `/BBox`, which clips. The result is a box with
        // hairline edges --- it looks like a thin border rather than like a bug,
        // and `annot-probe --mode outline` measures the same thing in pixels.
        let scratch = Scratch::new("annots-box-stroke");
        let content = appearance_of(MarkKind::Square, &scratch);

        assert!(
            content.contains(" re S"),
            "a box is stroked, not filled: {content}"
        );
        assert!(
            !content.contains(" re f"),
            "a filled box hides what it was drawn around: {content}"
        );
        // The stroke colour as well as the fill colour, because `rg` does not
        // imply `RG` and a path stroked after only `rg` comes out black.
        assert!(
            content.contains(" RG"),
            "a stroke needs its own colour operator: {content}"
        );

        // The quad is 72..300 by 100..118 in display space, so 228 by 18 in
        // page space whichever way up it is; the path is that less one stroke
        // width, anchored half a width in. Written as numbers rather than
        // derived from `outline_path`, so the test cannot agree with a wrong
        // implementation of the arithmetic it is checking.
        // Named `half` rather than `inset`: `let inset = OUTLINE_WIDTH / 2.0;`
        // here is a superstring of the same line in `outline_path`, and the
        // mutation anchored on that one then matched twice.
        let half = OUTLINE_WIDTH / 2.0;
        let path = content
            .lines()
            .find(|line| line.ends_with(" re S"))
            .expect("a stroked rectangle");
        let numbers: Vec<f64> = path
            .split_whitespace()
            .take(4)
            .map(|n| n.parse().expect("a number"))
            .collect();
        assert!((numbers[0] - (72.0 + half)).abs() < 1e-3, "x: {path}");
        assert!(
            (numbers[2] - (228.0 - OUTLINE_WIDTH)).abs() < 1e-3,
            "width: {path}"
        );
        assert!(
            (numbers[3] - (18.0 - OUTLINE_WIDTH)).abs() < 1e-3,
            "height: {path}"
        );
        // The y is the *lower* edge in page space, which is not 100: the quad
        // arrives in display space and `user_quads` maps it. Asserted through
        // the height above and the round trip in `annot-probe` rather than
        // restated here, because a number copied out of a failing run is a
        // second implementation of the mapping and agrees with any of them.
        assert!(numbers[1] > 0.0, "the path starts on the page: {path}");

        // And the width the reader will see, stated once so the stroke cannot
        // silently become a hairline.
        assert!(
            content.contains(&format!("{OUTLINE_WIDTH} w")),
            "the stroke names its width: {content}"
        );
    }

    #[test]
    fn only_a_box_is_stroked() {
        // The control for the test above. "Contains `re S`" is satisfied by a
        // writer that stroked *everything*, which would turn every highlight
        // into an outline of itself -- and that is a change no assertion about
        // the box alone can see.
        for kind in [
            MarkKind::Highlight,
            MarkKind::Underline,
            MarkKind::StrikeOut,
        ] {
            let scratch = Scratch::new("annots-not-stroked");
            let content = appearance_of(kind, &scratch);
            assert!(
                content.contains(" re f"),
                "{kind:?} fills its rectangle: {content}"
            );
            assert!(
                !content.contains(" re S"),
                "{kind:?} is not an outline: {content}"
            );
        }
    }

    #[test]
    fn each_kind_writes_its_own_subtype() {
        // The one thing every other reader keys on. A wrong subtype produces a
        // mark that draws correctly from our own `/AP` and is reported as the
        // wrong kind by Acrobat, Preview and the sidebar -- which is the failure
        // that looks like nothing is wrong.
        for (kind, expected) in [
            (MarkKind::Highlight, "Highlight"),
            (MarkKind::Underline, "Underline"),
            (MarkKind::StrikeOut, "StrikeOut"),
            (MarkKind::Note, "Text"),
            (MarkKind::Square, "Square"),
        ] {
            let scratch = Scratch::new("annots-subtype");
            let written = written_mark(kind, &scratch);
            assert_eq!(
                written.get(b"Subtype").and_then(Object::as_name).ok(),
                Some(expected.as_bytes()),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_comment_carries_no_text_markup_keys_and_the_others_do() {
        // Two absence assertions and their control, in one test because apart
        // they are worth much less: "the comment has no /QuadPoints" is
        // satisfied by a writer that stopped emitting them for everything, and
        // this repository has the entry about an absence assertion that could
        // not fail. The three markup kinds in the same loop are what make the
        // two `is_none` lines mean something.
        //
        // `/QuadPoints` is listed by PDF 32000-1 on the text-markup subtypes
        // and on no other; a comment is positioned by `/Rect`. `/AP` is ours to
        // write for a markup kind and the reader's to synthesise for a comment
        // icon --- see the note at the call site.
        for kind in [
            MarkKind::Highlight,
            MarkKind::Underline,
            MarkKind::StrikeOut,
        ] {
            let scratch = Scratch::new("annots-markup-keys");
            let written = written_mark(kind, &scratch);
            assert!(
                written.get(b"QuadPoints").is_ok(),
                "{kind:?} should carry /QuadPoints"
            );
            assert!(written.get(b"AP").is_ok(), "{kind:?} should carry an /AP");
            assert!(
                written.get(b"Name").is_err(),
                "{kind:?} should carry no icon name"
            );
        }

        // **A box is the other kind with no quads, and it separates two things
        // this test used to assert together.** Until the box existed, "not a
        // markup kind" and "no appearance stream of ours" were true of exactly
        // the same one variant, so a single predicate satisfied both and no
        // test could tell which of them it was checking. A box carries no
        // `/QuadPoints` *and* needs an `/AP`, so the two assertions below now
        // disagree about it --- which is what makes them two assertions.
        let scratch = Scratch::new("annots-box-keys");
        let written = written_mark(MarkKind::Square, &scratch);
        assert!(
            written.get(b"QuadPoints").is_err(),
            "a box is not a text-markup annotation and must not carry /QuadPoints"
        );
        assert!(
            written.get(b"AP").is_ok(),
            "nothing synthesises a rectangle, so a box needs its own appearance"
        );
        assert!(
            written.get(b"Name").is_err(),
            "an icon name belongs to a comment, not to a box"
        );
        assert!(written.get(b"Open").is_err(), "a box has no popup to open");

        let scratch = Scratch::new("annots-comment-keys");
        let written = written_mark(MarkKind::Note, &scratch);
        assert!(
            written.get(b"QuadPoints").is_err(),
            "a comment must not carry /QuadPoints"
        );
        assert!(
            written.get(b"AP").is_err(),
            "a comment leaves its icon to the reader"
        );
        assert_eq!(
            written.get(b"Name").and_then(Object::as_name).ok(),
            Some(b"Comment".as_slice()),
            "a comment names the speech-bubble icon"
        );
        assert_eq!(
            written.get(b"Open").and_then(Object::as_bool).ok(),
            Some(false),
            "a comment opens closed"
        );
        // And the keys it shares with every other mark, so that "carries fewer
        // keys" cannot be satisfied by a dictionary that lost the rest of them.
        assert!(written.get(b"Rect").is_ok(), "a comment needs a rectangle");
        assert!(
            written.get(b"Contents").is_ok(),
            "a comment needs what it says"
        );
    }

    #[test]
    fn a_line_is_opaque_and_a_wash_is_not() {
        // Two dictionary entries and one stream entry, all deciding the same
        // thing: a wash multiplies with the words under it at 40%, a line is
        // drawn over them at full strength. A multiplied red line over black
        // text is black, which is a strikeout nobody can see.
        for (kind, alpha, blend) in [
            (MarkKind::Highlight, WASH_ALPHA, "Multiply"),
            (MarkKind::Underline, 1.0, "Normal"),
            (MarkKind::StrikeOut, 1.0, "Normal"),
            // A box is opaque for the same reason a line is, and it matters
            // more: a translucent frame over a figure reads as a printing
            // artifact rather than as something a reader drew.
            (MarkKind::Square, 1.0, "Normal"),
        ] {
            let scratch = Scratch::new("annots-alpha");
            let written = written_mark(kind, &scratch);
            let got = written
                .get(b"CA")
                .and_then(Object::as_float)
                .unwrap_or_else(|_| panic!("{kind:?} has no /CA"));
            assert!((got - alpha).abs() < 1e-6, "{kind:?}: /CA is {got}");
            assert_eq!(blend_of(kind, &scratch), blend, "{kind:?}");
        }
    }

    #[test]
    fn a_line_stays_inside_the_quad_it_marks() {
        // The `/BBox` is the bounds of the quads, so anything drawn outside is
        // clipped -- an underline centred on the bottom edge would lose its
        // lower half in every reader and look like a thinner line rather than
        // like a defect. The quad is 100..118 from the page top on a 792 pt
        // page, so in the page's own space it runs 674..692.
        for kind in [MarkKind::Underline, MarkKind::StrikeOut] {
            let scratch = Scratch::new("annots-inside");
            let content = appearance_of(kind, &scratch);
            let rect: Vec<f64> = content
                .lines()
                .find(|line| line.ends_with("re f"))
                .expect("the appearance draws a rectangle")
                .split_whitespace()
                .take(4)
                .map(|n| n.parse().expect("a number"))
                .collect();
            let (bottom, height) = (rect[1], rect[3]);
            assert!(
                bottom >= 674.0 - 1e-6 && bottom + height <= 692.0 + 1e-6,
                "{kind:?}: the line runs {bottom}..{} outside 674..692",
                bottom + height
            );
            // And it is a line rather than the wash: a quad 18 pt tall gives a
            // rule about 1.3 pt thick, so anything over a quarter of the quad
            // is the fill this is meant to be distinguishable from.
            assert!(height < 18.0 / 4.0, "{kind:?}: {height} pt is not a line");
        }
    }

    #[test]
    fn a_strikeout_crosses_the_text_and_an_underline_sits_under_it() {
        // The discrimination the test above cannot make: both kinds draw a thin
        // rule inside the quad, and only where it sits tells them apart. A
        // strikeout drawn at the bottom is an underline with the wrong subtype,
        // which every check keyed on the subtype would pass.
        let scratch = Scratch::new("annots-where");
        let bottom_of = |kind| {
            appearance_of(kind, &scratch)
                .lines()
                .find(|line| line.ends_with("re f"))
                .and_then(|line| line.split_whitespace().nth(1).map(str::to_string))
                .expect("a rectangle")
                .parse::<f64>()
                .expect("a number")
        };
        let under = bottom_of(MarkKind::Underline);
        let through = bottom_of(MarkKind::StrikeOut);
        // The quad is 674..692, so its middle is 683.
        assert!((under - 674.0).abs() < 1e-6, "underline sits at {under}");
        assert!(
            (through - 683.0).abs() < 1.0,
            "strikeout sits at {through}, not near the middle"
        );
    }
}
