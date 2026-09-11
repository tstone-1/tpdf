//! The edit journal: pages, marks, redaction regions, undo and redo.
//!
//! One state, `edits::Edits`, and every command here is a call into it followed
//! by the state it answers with. Nothing in this file touches the render
//! service or the filesystem --- an edit is a change to the plan, and the plan
//! is only written when a save asks for it.

use crate::{edits, save};

/// Removes one page from the working document, without touching the file.
///
/// Named by identity like [`super::document::page_rotate`], and here that is not a nicety: the
/// reply this id came from may already be one state behind, and a position would
/// then delete whichever page had moved into that slot. An id cannot mean the
/// wrong page --- it either names a live one, a deleted one, or nothing, and the
/// model tells the three apart.
#[tauri::command]
pub async fn page_delete(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    page: u64,
) -> Result<edits::EditState, String> {
    edits.delete(doc, page)
}

/// Moves one page of the working document, without touching the file.
///
/// `after` is the id of the page the moved one should end up behind, and `null`
/// means the front. Both ends are identities, for the reason [`page_delete`]
/// gives twice over: a destination *index* would be read against an order the
/// frontend may no longer have, and the page would land beside whatever had
/// taken that position.
#[tauri::command]
pub async fn page_move(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    page: u64,
    after: Option<u64>,
) -> Result<edits::EditState, String> {
    edits.move_page(doc, page, after)
}

/// Puts a new blank page into the working document, without touching the file.
///
/// `after` is the id of the page it should sit behind, and `null` means the
/// front --- both ends identities, for the reason [`page_move`] gives.
///
/// `size` is `[width, height]` in points and comes from the frontend because
/// that is the side holding the page the reader is looking at. **A default here
/// would be wrong for one continent or the other**, and worse, wrong invisibly:
/// a letter-size blank in an A4 document lays out and prints without complaint.
#[tauri::command]
pub async fn page_insert(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    after: Option<u64>,
    size: [f64; 2],
) -> Result<edits::EditState, String> {
    edits.insert(doc, after, size)
}

/// Puts a mark on a page, over the rectangles a reader dragged across.
///
/// **One command for all three kinds rather than three commands.** The kind is
/// a field on [`edits::NewMark`], so a highlight, an underline and a strikeout
/// travel one path, are refused by one set of preconditions and are written by
/// one writer. Three commands would be three chances for the fourth kind to
/// reach only two of them. It was called `annot_highlight` while there was only
/// one kind; the name is part of the wire format, so renaming it is a protocol
/// change and is done here rather than left to read wrongly.
///
/// The page is named by identity, as [`super::document::page_rotate`] names it, and for the
/// sharper version of the same reason: a mark is placed by *coordinates*, so a
/// stale position would put a reader's highlight on a different page at the
/// place the words used to be.
///
/// **The timestamp is taken here and cannot be sent.** `edits::NewMark` has no
/// field for it: what a mark claims about when it was made is the application's
/// statement, not the frontend's, and a `made` on the wire would be one more
/// attacker-controlled string in a file tpdf signs its name to.
///
/// Synchronous work in an `async fn`, which is right for the same reason it is
/// right in [`super::document::page_rotate`]: a lock, a journal push and a page walk. The
/// coordinate mapping and the writing happen at save time, not here.
#[tauri::command]
pub async fn annot_mark(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: edits::NewMark,
) -> Result<edits::EditState, String> {
    edits.annotate(doc, mark, save::pdf_date(std::time::SystemTime::now()))
}

/// Takes one mark off the page it is on.
///
/// `sweep` names the gesture this belongs to, or is zero for a removal that
/// stands alone --- see [`edits::Edits::erase`]. One sweep of the eraser can
/// take several whole marks, and they go back together.
#[tauri::command]
pub async fn annot_remove(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: u64,
    sweep: u64,
) -> Result<edits::EditState, String> {
    edits.unannotate(doc, mark, sweep)
}

/// Marks a region of one page for removal.
///
/// **Nothing is destroyed by this command.** It is `docs/PLAN.md` §6 step 1:
/// the region joins the review list and the overlay outlines it. Applying is a
/// separate command, and the whole point of the split is that a reader looks at
/// the list first.
///
/// Named `redact_mark` beside [`annot_mark`], and the pairing is deliberate ---
/// they are the same gesture producing two different things, and the names
/// should make the difference legible in a stack trace as well as in a menu.
///
/// The page is named by identity for [`annot_mark`]'s sharper reason: a region
/// is placed by coordinates, so a stale position would mark a different page at
/// the spot the words used to be --- and here that would be a reader certifying
/// the removal of something they never looked at.
#[tauri::command]
pub async fn redact_mark(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    page: u64,
    area: [f32; 4],
) -> Result<edits::EditState, String> {
    edits.redact(doc, page, area)
}

/// Takes one pending redaction back off its page.
#[tauri::command]
pub async fn redact_remove(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    redaction: u64,
) -> Result<edits::EditState, String> {
    edits.unredact(doc, redaction)
}

/// Rubs strokes out of one drawing.
///
/// `remove` is positions into the drawing's current stroke list, not points ---
/// see [`edits::Edits::erase`] for why the frontend does not get to send back
/// geometry through a command that only removes. One call per *drawing*, and
/// `sweep` is what makes a gesture that crossed several of them one undo; a
/// sweep that takes the last stroke takes the drawing with it.
#[tauri::command]
pub async fn annot_erase(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: u64,
    remove: Vec<usize>,
    sweep: u64,
) -> Result<edits::EditState, String> {
    edits.erase(doc, mark, remove, sweep)
}

/// Replaces what one mark says.
///
/// The whole note, not an edit to it --- see [`docmodel::Command::Renote`]. The
/// text is the reader's own words rather than the document's, which is why
/// nothing sanitises it here: it goes into `/Contents` on the way out, and the
/// path that reads it back in is `annots.rs`, where a *stranger's* string
/// arrives and is treated as one.
#[tauri::command]
pub async fn annot_note(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: u64,
    note: String,
) -> Result<edits::EditState, String> {
    edits.renote(doc, mark, note)
}

/// Replaces what a comment out of the file says.
///
/// [`annot_note`]'s counterpart for an annotation the reader did not make, and
/// the parameters are where the two part company. A mark is named by an id this
/// application issued; a foreign comment is named by the **object the file gave
/// it**, because `annots::Comment::id` is a position in one scan and every id
/// after an inserted comment moves. `object` is `annots::Comment::object`, sent
/// back exactly as it arrived.
///
/// `page` is the model's identity for the page it sits on, which the frontend
/// resolves from the comment's file page through the map it already holds. It is
/// what makes a deleted page take the edit with it --- see
/// [`docmodel::Command::Rewrite`].
///
/// The date is this application's clock, taken here, exactly as
/// [`annot_mark`]'s is: the caller does not get to choose what a comment claims
/// about when it was changed.
#[tauri::command]
pub async fn annot_rewrite(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    object: (u32, u16),
    page: u64,
    body: String,
) -> Result<edits::EditState, String> {
    edits.rewrite(
        doc,
        object,
        page,
        body,
        save::pdf_date(std::time::SystemTime::now()),
    )
}

/// Takes a comment out of the file off the page it is on.
///
/// [`annot_rewrite`]'s counterpart, and its two parameters mean exactly what
/// they mean there: `object` is the name the **file** gave the annotation, and
/// `page` is the model's identity for the page, which is what makes a deleted
/// page take the deletion with it.
///
/// **No date**, unlike every other write command here. `/M` says when a comment
/// was last modified, and a comment that is gone has nothing to say it about ---
/// so there is no clock reading to take and nothing for a caller to choose.
///
/// ⚠ **This is the one edit that forces a full rewrite of the file on save.** An
/// incremental save only adds objects; a deletion has nothing it can add. See
/// `edits::Plan::is_appendable`.
#[tauri::command]
pub async fn annot_discard(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    object: (u32, u16),
    page: u64,
) -> Result<edits::EditState, String> {
    edits.discard(doc, object, page)
}

/// Replaces what one mark is drawn in.
///
/// The whole colour, not a channel --- see [`docmodel::Command::Recolor`]. Three
/// floats from the webview, clamped into `0..=1` at the `edits.rs` boundary the
/// same way a new mark's are, because this is the second route into `/C` and a
/// non-finite channel would be three letters in the middle of a content stream.
#[tauri::command]
pub async fn annot_recolor(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: u64,
    color: [f32; 3],
) -> Result<edits::EditState, String> {
    edits.recolor(doc, mark, color)
}

/// Moves one mark by an offset, in the page's display space.
///
/// An offset rather than a new rectangle, which is what makes it a move --- see
/// [`docmodel::Doc::displace`]. The frontend clamps it against the page before
/// sending, because the page's size in points is not something the model holds.
///
/// One call per drag, so one undo puts the mark back where it was rather than
/// stepping it home a pointer event at a time.
#[tauri::command]
pub async fn annot_move(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: u64,
    dx: f32,
    dy: f32,
) -> Result<edits::EditState, String> {
    edits.displace(doc, mark, dx, dy)
}

/// Resizes a visual signature while preserving its proportions.
#[tauri::command]
pub async fn annot_resize_signature(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    mark: u64,
    width: f32,
) -> Result<edits::EditState, String> {
    edits.resize_signature(doc, mark, width)
}

/// Steps the edit journal back one command.
#[tauri::command]
pub async fn edit_undo(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
) -> Result<edits::EditState, String> {
    edits.undo(doc)
}

/// Steps the edit journal forward one command.
#[tauri::command]
pub async fn edit_redo(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
) -> Result<edits::EditState, String> {
    edits.redo(doc)
}

/// The edit state of an open document.
///
/// Asked for once after an open, so the frontend starts from the model's answer
/// rather than from an assumption that a freshly opened document is unedited.
/// Those are the same thing today and will not be once a session can carry
/// edits, and the difference is invisible until it is wrong.
#[tauri::command]
pub async fn edit_state(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
) -> Result<edits::EditState, String> {
    edits.state(doc)
}
