//! Redaction: what a plan would remove, and the two commands that remove it.
//!
//! The read-back is what makes this a group of its own. Both commands write a
//! file and both then have to answer `docs/PLAN.md` 6's question about it, so
//! the ask, the gate and the scan are stated once here and called twice ---
//! `docs/TRAPS.md` records more than one defect that was a second copy of a
//! rule drifting from the first.

use std::path::Path;

use tauri::Manager;

use super::{await_reply, outside_of, password_for, reply_channel};
use crate::render::RenderService;
use crate::{edits, ocr_gate, redact, save, verify, with_close_note, SaveFailure};

/// Scans a file this build just wrote for the words a redaction removed.
///
/// **One statement of the read-back, for the two commands that redact.** Both
/// wrote a file and both have to answer `docs/PLAN.md` §6's question about it,
/// and until 2026-09-01 both answered it by reading the bytes into the app
/// process and handing them to `lopdf` --- the last coordinator-side parse of
/// attacker-controlled input, and the one `docs/THREAT-MODEL.md` residual risk
/// 18 was left naming. `scanning` is where that parse happens now.
///
/// **By path, and that is unchanged rather than overlooked.** Both callers have
/// already renamed and closed, so there is no handle left to scan through, and
/// anything that replaces the file between that rename and this open is what
/// gets scanned. What is new is only that the *length* comes from the open
/// handle rather than from reading to end of file, which is the same question
/// asked of the same descriptor. The window is disclosed in both callers and in
/// residual risk 18; closing it means the writers returning their open file.
///
/// **`&dyn Verifier`, not `&dyn Outside`**, though every caller holds the
/// second: the seam a redaction's read-back needs is the scan, and taking the
/// wider type would hand this function a rewriter it must not call. The
/// coercion from one to the other is trait upcasting, which costs nothing and
/// is what lets a test double be a verifier and no more.
///
/// # Errors
///
/// The file could not be opened or measured. A file that cannot be *parsed* is
/// not an error --- it is a blind spot inside the report, which is what
/// [`verify::Report`] is for.
pub(crate) fn scan_written_file(
    scanning: &dyn save::Verifier,
    at: &std::path::Path,
    needles: &[String],
    password: Option<&str>,
) -> Result<verify::Report, String> {
    let mut file = std::fs::File::open(at)
        .map_err(|why| format!("the redacted file could not be read back: {why}"))?;
    // From the handle rather than from the name: the two can be different files
    // by now, and it is the one that was opened that is about to be mapped.
    let len = file
        .metadata()
        .map_err(|why| format!("the redacted file could not be measured: {why}"))?
        .len();
    let len = usize::try_from(len)
        .map_err(|_| format!("the redacted file is {len} bytes, which is more than fits here"))?;
    scanning.scan(&mut file, len, needles, password)
}

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

/// A redaction worked out against the open document, ready for either writer.
///
/// **The Ask step of `docs/PLAN.md` §6, held apart from the write.** Two
/// commands apply a redaction --- [`redact_copy`] to a new file and
/// [`redact_document`] over the open one --- and they differ in the writer and
/// in nothing else. A second copy of this loop is the drift this repository
/// keeps recording: the two would go on agreeing about the ordinals and
/// eventually disagree about which objects the reader was warned about.
struct Asked {
    /// The reader's plan with the redaction ordinals in it.
    plan: edits::Plan,
    /// The words the regions cover, to look for in what gets written.
    needles: Vec<String>,
    /// What the removal could not take. Not a refusal --- see [`redact_copy`]
    /// --- but a reason the file cannot be called clean, carried to the verdict.
    concerns: Vec<String>,
    /// How many regions were asked about.
    regions: usize,
    /// How many text-showing operations the removal names, after merging.
    shows: usize,
    /// What the OCR gate needs and only the source document can supply.
    ///
    /// Collected here rather than after the write because after the write it
    /// cannot be: the control the gate renders has to be no larger than the
    /// smallest box a region covered, and the removal takes exactly those boxes.
    /// See [`ocr_gate::GatePage`].
    gate: Vec<ocr_gate::GatePage>,
}

/// Works out what removing every marked region would take.
///
/// One call per page carrying regions, which is where the cost is: the page load
/// and the object walk are per page and the comparison is per region.
///
/// **Nothing is written and nothing is journalled.** The document is asked, and
/// a caller that goes no further has changed nothing.
///
/// # Errors
///
/// Nothing marked, or a worker that could not read a page.
async fn ask_redactions(
    edits: &edits::Edits,
    service: &RenderService,
    doc: u32,
) -> Result<Asked, String> {
    let targets = edits.redaction_targets(doc)?;
    if targets.is_empty() {
        return Err("nothing in this document is marked for removal".into());
    }

    let mut planned: Vec<edits::PlannedRedaction> = Vec::new();
    let mut needles: Vec<String> = Vec::new();
    let mut concerns: Vec<String> = Vec::new();
    let mut regions = 0usize;
    let mut shows_total = 0usize;
    let mut gate: Vec<ocr_gate::GatePage> = Vec::new();

    for target in targets {
        let page = target.source;
        regions += target.regions.len();
        // Kept before the move: the gate works in display space, which is the
        // space these arrived in, while `redaction_plans` converts them to the
        // page's own.
        let displayed = target.regions.clone();
        let (reply, rx) = reply_channel();
        service.redaction_plans(doc, page, target.regions, reply);
        let plans = await_reply("redaction_plans", rx).await?;

        // One text extraction per page, on the document as the reader has it.
        // `None` for the crop because `redaction_plans` uses the file's own, and
        // a word list measured from a different corner than the regions were
        // would put every control somewhere else on the page.
        //
        // A page whose text cannot be read is not a refusal: the gate reports
        // *not verified* for its regions, which is the answer either way.
        let (reply, rx) = reply_channel();
        service.text(doc, page, None, reply);
        let text = await_reply("redaction text", rx).await.ok();

        // **The arithmetic lives in `redact.rs`, and this loop is the two
        // questions it needs answered.** Everything between the replies used to
        // be written out here --- a hundred lines of merging, deduplication and
        // sentence-building inside a `#[tauri::command]`'s private helper, which
        // no test could construct and no mutation could aim at. See
        // [`redact::aggregate`].
        let one = redact::aggregate(page, displayed, plans, text.as_ref());
        concerns.extend(one.concerns);
        needles.extend(one.needles);
        shows_total += one.shows;
        gate.push(one.gate);
        planned.push(one.planned);
    }

    let mut plan = edits.plan(doc)?;
    plan.redactions = planned;
    Ok(Asked {
        plan,
        needles,
        concerns,
        regions,
        shows: shows_total,
        gate,
    })
}

/// Runs `docs/PLAN.md` §6 step 4 over a written file, off the async runtime.
///
/// [`ocr_gate::run`] blocks --- it waits on a render and on another process ---
/// and every other step of these two commands is already on a blocking thread
/// for the same reason. The service is reached through the app handle rather
/// than borrowed, because `spawn_blocking` needs what it captures to outlive the
/// call and a `tauri::State` borrow does not.
///
/// **Nothing here refuses.** A join that failed is one more reason the file
/// cannot be called clean, and [`redact::Applied`] is what carries it.
async fn gate_written_file(
    app: &tauri::AppHandle,
    path: String,
    pages: Vec<ocr_gate::GatePage>,
    password: Option<String>,
) -> Vec<String> {
    if pages.is_empty() {
        return Vec::new();
    }
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let service = handle.state::<RenderService>();
        ocr_gate::run(&service, &path, password, &pages)
    })
    .await
    .unwrap_or_else(|e| {
        vec![format!(
            "the removed areas could not be checked, so the file cannot be shown clean: {e}"
        )]
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
/// Four steps, and the order is the safety of it:
///
/// 1. **Ask.** For each page holding regions, a worker computes what a removal
///    would take --- against PDFium's own object list, behind the sandbox, which
///    is where every parse of the reader's bytes belongs.
/// 2. **Write.** The ordinals go into the plan and `save::write_copy` takes the
///    ordinary rewrite path, which is what applies them --- see
///    `save::apply_redactions` for why it is safe for that to happen last.
/// 3. **Verify.** The written file is scanned for the words that were supposed
///    to go, and every object the removal could not take is a reason of its own.
///    The answer is *verified*, or *not verified* with every reason --- never a
///    bare success, which is §6 step 4 and is why [`redact::Applied`] cannot
///    carry the first without the second.
///
/// **An object the removal cannot take does not stop the write, and that is a
/// decision rather than an oversight.** §6's deny-by-default rule says such an
/// object is a verification failure and not a shrug, and it is honoured here as
/// a failure to *verify*: the file is written with the text gone and the reader
/// is told, in the sentence that lands afterwards, that it could not be proved
/// clean and why. Refusing instead was tried first and measured: `text-base14`'s
/// own region overlaps a path, and a rule under a line of text is what almost
/// every real document has --- so refusing means tpdf can never redact anything
/// and the reader is told the same thing with nothing to show for it. One rule,
/// *never claim clean*, beats two.
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
) -> Result<redact::Applied, String> {
    let asked = ask_redactions(&edits, &service, doc).await?;
    let plan = asked.plan.clone();
    let needles = asked.needles.clone();
    let concerns = asked.concerns.clone();
    let regions = asked.regions;
    let shows_total = asked.shows;

    let out = std::path::PathBuf::from(path);
    let from = std::path::PathBuf::from(source);
    let written = out.clone();
    let out_path = out.to_string_lossy().into_owned();
    let password = password_for(&service, doc, "redact_copy").await;
    // **Kept for the two readers after the write, which need the same key.**
    // The file about to be written is re-encrypted whenever the source was, so
    // a verifier arriving without the password parses no objects at all and
    // reports having found nothing --- which is what a clean file looks like.
    // `verify::scan` refuses to call that verified, and this is what lets it
    // answer the question instead of declining it.
    let key = password.clone();
    // `save_copy`'s writer, and the same reason with one more: what this command
    // parses is a document a reader is about to have words removed from, so the
    // parse is the one that most wants to be somewhere that cannot reach their
    // files.
    let writing = outside_of(&app, service.backend());
    let copied = tauri::async_runtime::spawn_blocking(move || {
        save::write_copy(&from, &plan, &out, password.as_deref(), &*writing)
    })
    .await
    .map_err(|e| format!("the redaction did not run: {e}"))?
    .map_err(|why| why.message)?;

    // Read back rather than verified from what was written, which is the same
    // rule the append's own verification follows: what matters is the file on
    // disk, and the buffer that produced it agrees with itself.
    //
    // **By path, and that is a window rather than a guarantee.** `write_copy`
    // has already renamed and closed, so there is no handle left to read
    // through; anything that replaces the file between the rename and this read
    // is what gets verified, and the report would be about somebody else's
    // bytes. Closing it means `write_copy` returning its open file, which is a
    // change to every copy path in `save.rs` --- and the destination is a name
    // the reader has just chosen in a dialog, so the race needs a writer aiming
    // at it in the same second. Disclosed rather than claimed shut: the append,
    // whose destination is the reader's own open document, does hold its handle.
    let verifying = key.clone();
    // A second `Outside`, because `writing` was moved into the write above and
    // the two are the same choice made twice rather than one choice shared. It
    // costs a `Box` and a worker spawn on the path that has already written a
    // file and waited for the platter.
    let scanning = outside_of(&app, service.backend());
    let report = tauri::async_runtime::spawn_blocking(move || {
        scan_written_file(&*scanning, &written, &needles, verifying.as_deref())
    })
    .await
    .map_err(|e| format!("the verification did not run: {e}"))??;

    // The objects the removal could not take come first, because they are the
    // finding a reader can act on: a picture of the words in the region is a
    // different problem from a scan that could not decode a stream, and only the
    // first tells them the region is still readable.
    let mut why = concerns;
    if let verify::Verdict::NotVerified(reasons) = report.verdict() {
        why.extend(reasons);
    }
    // Then §6 step 4, which is the only one of the two that can see a picture of
    // the words. It runs on the file that was just written, never on the source
    // --- see `ocr::RedactedPixels`, where that is a type-level rule.
    why.extend(gate_written_file(&app, out_path, asked.gate, key).await);
    Ok(redact::Applied {
        regions,
        shows: shows_total,
        changed: copied.changed,
        verified: why.is_empty(),
        why,
    })
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
    doc: u32,
    source: String,
) -> Result<redact::Applied, SaveFailure> {
    let asked = ask_redactions(&edits, &service, doc)
        .await
        .map_err(SaveFailure::refused)?;

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
    .map_err(|e| SaveFailure::refused(format!("the redaction did not run: {e}")))?
    .map_err(SaveFailure::refused_by)?;

    // Past this line every failure is an `after_close`, for `save_document`'s
    // reason: the reader's document is being taken apart, and the honest thing
    // to report is that they have to open the file again.
    //
    // The model first --- document numbers are reused, and a journal left under a
    // handle the service is free to hand to another file is one document's edits
    // applied to another's pages. Here that close is also the truncation.
    edits.close(doc);
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
        scan_written_file(&*scanning, at, &needles, verifying.as_deref()).map_err(|why| {
            SaveFailure::after_close(format!(
                "the file was written but could not be read back to check it: {why}"
            ))
        })
    })
    .await
    .map_err(|e| SaveFailure::after_close(format!("the redaction did not finish: {e}")))?;

    let report = landed.map_err(|why| with_close_note(why, closed))?;

    // The objects the removal could not take come first, because they are the
    // finding a reader can act on --- see `redact_copy`, which orders them the
    // same way and for the same reason.
    let mut why = asked.concerns.clone();
    if let verify::Verdict::NotVerified(reasons) = report.verdict() {
        why.extend(reasons);
    }
    // Then §6 step 4, against the reader's own file --- which is now the only
    // copy, so this is the sharper of the two places it runs.
    why.extend(gate_written_file(&app, source, asked.gate, key).await);
    Ok(redact::Applied {
        regions: asked.regions,
        shows: asked.shows,
        // The source *is* the file being written, so the copy's question --- has
        // the document changed under the one on screen --- is asked and answered
        // by `stage_in_place`, which refuses rather than reporting. Reaching here
        // means it had not.
        changed: false,
        verified: why.is_empty(),
        why,
    })
}
