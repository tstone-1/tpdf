//! Redaction: what a plan would remove, and the two commands that remove it.
//!
//! The read-back is what makes this a group of its own. Both commands write a
//! file and both then have to answer `docs/PLAN.md` 6's question about it, so
//! the ask, the gate and the scan are stated once (`redaction_steps.rs`) and
//! called twice --- `docs/TRAPS.md` records more than one defect that was a
//! second copy of a rule drifting from the first.

use std::path::Path;

use super::{await_reply, outside_of, password_for, reply_channel};
use crate::redaction_steps::{
    ask_redactions, finish_redaction_fill, gate_written_file, redact_copy_asked, scan_written_file,
    Stopped,
};
use crate::render::RenderService;
use crate::{
    edits, ocr_gate, pdfium_library_dir, redact, save, verify, webopen, with_close_note,
    SaveFailure,
};

/// What removing each of one page's marked regions would take, and what it would miss.
///
/// **The one thing the redaction review panel cannot work out for itself.** The
/// frontend knows which words a region *covers* --- it holds the character boxes
/// --- and cannot know which text-showing operations those characters belong to,
/// because that is a fact about the content stream. Route B removes a whole
/// operation when any of its glyphs is inside, so the difference between the two
/// answers is exactly the collateral a reader is reviewing for.
///
/// One call per page carrying every region on it, for `page_text`'s reason: the
/// page load and the object walk are the cost and they are per page, while the
/// comparison is per region. `regions` are in the file's display space, the
/// space the model holds a pending redaction in, and the turn into the page's
/// own space happens in the worker --- the same split [`super::document::page_crop_box`] exists
/// for.
///
/// **Nothing is removed by asking.** The answer is a count, some sentences and
/// the text those operations draw; the document is not touched, and there is no
/// command that applies one of these yet.
#[tauri::command]
pub async fn redaction_plans(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
    regions: Vec<[f32; 4]>,
) -> Result<Vec<redact::RegionPlan>, String> {
    let (reply, rx) = reply_channel();
    service.redaction_plans(doc, page, regions, reply);
    await_reply("redaction_plans", rx).await
}

/// Creates a fresh image-only copy; verification finishes before publication.
#[tauri::command]
pub async fn redact_raster_copy(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    fill: Option<crate::redaction_fill::Fill>,
) -> Result<redact::Applied, String> {
    let asked = ask_redactions(&edits, &service, doc).await?;
    let password = password_for(&service, doc, "redact_raster_copy").await;
    let fill = fill.unwrap_or_default();
    let writing = outside_of(&app, service.backend());
    let from = std::path::PathBuf::from(source);
    let out = std::path::PathBuf::from(path);
    let regions = asked.regions;
    tauri::async_runtime::spawn_blocking(move || {
        save::write_raster_copy(
            &from,
            &asked.plan,
            &out,
            password.as_deref(),
            &*writing,
            fill,
        )
    })
    .await
    .map_err(|e| said!("The image-only copy did not run: {e}"))?
    .map_err(|why| why.message)?;
    Ok(redact::Applied {
        regions,
        shows: 0,
        changed: false,
        verified: true,
        why: Vec::new(),
        notes: Vec::new(),
    })
}

/// Writes a copy of the document with every marked region removed, and verifies it.
///
/// **The destructive step, pointed at a new file.** `docs/PLAN.md` §6 describes
/// apply as an in-place rewrite with the journal truncated; this is the same
/// removal written somewhere else, which is the form that can ship first because
/// nothing the reader has can be lost by it. The open document is untouched and
/// the regions stay pending, so a reader who does not like the result closes the
/// file and still has their marks.
///
/// The ask is [`ask_redactions`], and everything after it --- the write, the
/// read-back, the OCR gate, the fill, and the order those happen in --- is
/// [`redact_copy_asked`], where the reasons for that order are written down.
/// It is a function of its own since 2026-09-27 so that `tpdf redact` reaches
/// the same body with no app handle; this command is the two calls joined.
///
/// # Errors
///
/// Nothing marked; the worker refusing to read a page; anything
/// `save::write_copy` refuses (an encrypted source, a page count that disagrees
/// with the baseline, writing over the source); or the written file not being
/// readable back.
#[tauri::command]
pub async fn redact_copy(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    doc: u32,
    source: String,
    path: String,
    fill: Option<crate::redaction_fill::Fill>,
) -> Result<redact::Applied, String> {
    let mut asked = ask_redactions(&edits, &service, doc).await?;
    asked.fill = fill.unwrap_or_default();
    redact_copy_asked(
        &service,
        pdfium_library_dir(&app),
        doc,
        asked,
        source,
        path,
        None,
    )
    .await
    .map_err(Stopped::into_message)
}

/// Removes every marked region from the file the reader opened, and verifies it.
///
/// **The destructive step pointed at the reader's own file**, which is
/// `docs/PLAN.md` §6 step 3 as that section states it. [`redact_copy`] is the
/// same removal written somewhere else, and it shipped first because nothing a
/// reader has can be lost by it; this one is the operation a reader actually
/// wants, and there is no original left afterwards.
///
/// **The journal truncation §6 asks for is the close, and it is stronger than a
/// truncation.** Truncating the journal at the apply would leave every earlier
/// command undoable, which for a redaction means a reader could step back to a
/// state whose regions were still pending and wonder which file they were
/// looking at. [`super::save::save_document`]'s close spends the journal whole: the model is
/// dropped, the reader reopens from the path, and there is no undo that reaches
/// across it. Nothing here had to be built for that --- it is what an in-place
/// write already does --- and saying so is worth more than a mechanism would be.
///
/// **Always a rewrite, and that is what a redaction is rather than a choice.**
/// [`super::save::save_document`] asks `save::mode_for_source` whether a plan can be appended;
/// this does not ask, because an append adds objects and never touches a content
/// stream, so appending a redaction would write a file with every word still in
/// it. `Plan::is_appendable` refuses a plan carrying a redaction for exactly
/// that reason and has a test named for it --- so the property holds at the
/// predicate as well as here, and neither place is relying on the other.
///
/// The order is [`super::save::save_document`]'s, for [`super::save::save_document`]'s reasons: stage
/// beside the source while the document is still open and every refusal can
/// arrive harmlessly, close, then rename. What is added is the verification, and
/// it happens **after** the rename, against the file the reader now has --- the
/// same rule the copy follows, and sharper here, because the bytes on that path
/// are the only bytes left.
///
/// **A file that could not be proved clean is still the file.** §6's rule is
/// *never claim clean*, not *never write*; the removal happened, the reader is
/// told what could not be shown gone, and the alternative --- rolling back to the
/// unredacted document --- would hand them the words they asked to destroy while
/// reporting a failure. [`redact_copy`] carries the same decision and the same
/// worked reason.
///
/// # Errors
///
/// Nothing marked, a worker that could not read a page, or anything
/// `save::stage_in_place` refuses --- all of them with the document untouched.
/// Past the close, a rename that did not happen or a file that could not be read
/// back, both of which say `reopen`.
#[tauri::command]
pub async fn redact_document(
    app: tauri::AppHandle,
    service: tauri::State<'_, RenderService>,
    edits: tauri::State<'_, edits::Edits>,
    web: tauri::State<'_, webopen::Registry>,
    doc: u32,
    source: String,
    fill: Option<crate::redaction_fill::Fill>,
) -> Result<redact::Applied, SaveFailure> {
    let mut asked = ask_redactions(&edits, &service, doc)
        .await
        .map_err(SaveFailure::refused)?;
    asked.fill = fill.unwrap_or_default();

    let staging = source.clone();
    let plan = asked.plan.clone();
    let password = password_for(&service, doc, "redact_document").await;
    // Kept past the close for the two readers after the write --- `redact_copy`
    // says why, and here it is also the only copy of the document left. Taken
    // now rather than later because the service is closed a few lines below and
    // there is nothing to ask by then.
    let key = password.clone();
    let writing = outside_of(&app, service.backend());
    let staged = tauri::async_runtime::spawn_blocking(move || {
        save::stage_in_place(Path::new(&staging), &plan, password.as_deref(), &*writing)
    })
    .await
    .map_err(|e| SaveFailure::refused(said!("the redaction did not run: {e}")))?
    .map_err(SaveFailure::refused_by)?;

    // Past this line every failure is an `after_close`, for `save_document`'s
    // reason: the reader's document is being taken apart, and the honest thing
    // to report is that they have to open the file again.
    //
    // The model first, for `close_document`'s reason: it goes even when the
    // service refuses, so an edit arriving late for this id finds no journal
    // rather than one whose pages the service no longer holds. Here that close
    // is also the truncation.
    //
    // ⚠ **These two calls were written against a document that could not hold
    // an inserted page, and both said so**: *"empty today --- a redaction is
    // refused beside an imported page --- and released anyway, because a
    // refusal elsewhere is not a reason to leak here"*. Since 2026-09-20 a
    // redaction on an own page is allowed beside them, so the lists are
    // routinely non-empty and the release is load-bearing rather than
    // defensive. Nothing here had to change, which is the argument for having
    // written it that way: a document number would otherwise be handed out
    // again while another file's pool and web addresses were still under it.
    let sources = edits.close(doc);
    // The addresses with them, this document's and theirs. `close_document`
    // makes the argument; this path is the third place that closes a document
    // and the second that had not applied it.
    web.forget(doc, &sources);
    super::document::release_sources(&service, sources);
    let (reply, rx) = reply_channel();
    service.close(doc, reply);
    let closed = await_reply("redact_document", rx).await;

    let committing = source.clone();
    let needles = asked.needles.clone();
    let verifying = key.clone();
    // A second `Outside` --- `redact_copy` says why, and here there is one more
    // reason: the service is closed above, so there is no worker left to ask and
    // this one is spawned for the scan and dropped after it.
    let scanning = outside_of(&app, service.backend());
    let landed = tauri::async_runtime::spawn_blocking(move || {
        let at = Path::new(&committing);
        // One more look before the rename, closing the window staging opens.
        save::verify_before_commit(&staged, at).map_err(SaveFailure::after_close_by)?;
        save::commit_in_place(&staged.path, at).map_err(SaveFailure::after_close)?;
        // Read back rather than verified from what was written, which is the
        // rule the copy and the append both follow: what matters is the file on
        // disk, and the buffer that produced it agrees with itself.
        let fingerprint =
            crate::fingerprint::Fingerprint::of(at).map_err(SaveFailure::after_close)?;
        let report =
            scan_written_file(&*scanning, at, &needles, verifying.as_deref()).map_err(|why| {
                SaveFailure::after_close(said!(
                    "the file was written but could not be read back to check it: {why}"
                ))
            })?;
        Ok::<_, SaveFailure>((report, fingerprint))
    })
    .await
    .map_err(|e| SaveFailure::after_close(said!("the redaction did not finish: {e}")))?;

    let (report, fingerprint) = landed.map_err(|why| with_close_note(why, closed))?;

    // The objects the removal could not take come first, because they are the
    // finding a reader can act on --- see `redact_copy`, which orders them the
    // same way and for the same reason.
    let mut why = asked.concerns.clone();
    if let verify::Verdict::NotVerified(reasons) = report.verdict() {
        why.extend(reasons);
    }
    // Under them for `redact_copy`'s reason, and sharper here: this is the only
    // copy of the document left, so a reader deciding whether to act on a hit
    // has nothing else to compare it against.
    why.extend(redact::short_needle_note(&report));
    why.extend(redact::marked_pages_note(&report, &asked.marked));
    why.extend(redact::inserted_pages_note(
        asked.inserted,
        &report.found,
        report.placed(),
    ));
    // Then §6 step 4, against the reader's own file --- which is now the only
    // copy, so this is the sharper of the two places it runs.
    let notes = ocr_gate::sizing_notes(&asked.gate);
    let taken_here = asked.notes;
    why.extend(
        gate_written_file(
            service.inner().clone(),
            source.clone(),
            asked.gate,
            key.clone(),
        )
        .await,
    );
    finish_redaction_fill(
        pdfium_library_dir(&app),
        service.backend(),
        source,
        asked.plan,
        fingerprint,
        key,
        asked.fill,
    )
    .await
    .map_err(SaveFailure::after_close)?;
    Ok(redact::Applied {
        regions: asked.regions,
        shows: asked.shows,
        // The source *is* the file being written, so the copy's question --- has
        // the document changed under the one on screen --- is asked and answered
        // by `stage_in_place`, which refuses rather than reporting. Reaching here
        // means it had not.
        changed: false,
        verified: why.is_empty(),
        notes: redact::notes_for(why.is_empty(), notes, taken_here),
        why,
    })
}
