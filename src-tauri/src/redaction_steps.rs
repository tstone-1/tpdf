//! Redaction from the ask to the verdict, for the window and the command line
//! alike: what a removal of every marked region would take, the rewrite, the
//! read-back of the written file, the OCR gate and the fill.
//!
//! The window's commands (`commands::redact`) and `tpdf redact` (`cli::redact`)
//! both reach these, so the ask, the gate and the scan are stated once.
//! `docs/TRAPS.md` records more than one defect that was a second copy of a
//! rule drifting from the first.

use std::path::Path;

use crate::docmodel::PageSource;
use crate::render::{Backend, RenderService};
use crate::render_reply::{await_reply, outside_at, password_for, reply_channel};
use crate::{edits, ocr_gate, redact, save, verify};

// Paint only after both verification passes read the uncovered result. The
// fingerprint prevents applying that verdict to a file changed in between.
//
// By library directory rather than by app handle, so that `tpdf redact` reaches
// this exactly as the window does --- the command-line tool has no handle, and
// a second copy of the fill for it would be the drift this file exists to stop.
pub(crate) async fn finish_redaction_fill(
    library: std::path::PathBuf,
    backend: Backend,
    path: String,
    plan: edits::Plan,
    fingerprint: crate::fingerprint::Fingerprint,
    password: Option<String>,
    fill: crate::redaction_fill::Fill,
) -> Result<(), String> {
    let writer = outside_at(library, backend);
    tauri::async_runtime::spawn_blocking(move || {
        save::fill_redactions(
            Path::new(&path),
            &plan,
            &fingerprint,
            password.as_deref(),
            &*writer,
            fill,
        )
    })
    .await
    .map_err(|e| said!("The fill did not run: {e}"))?
    .map_err(|e| {
        said!(
            "Content removal finished, but the fill could not be saved: {why}",
            why = e.message
        )
    })
}

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
        .map_err(|why| said!("the redacted file could not be read back: {why}"))?;
    // From the handle rather than from the name: the two can be different files
    // by now, and it is the one that was opened that is about to be mapped.
    let len = file
        .metadata()
        .map_err(|why| said!("the redacted file could not be measured: {why}"))?
        .len();
    let len = usize::try_from(len)
        .map_err(|_| said!("the redacted file is {len} bytes, which is more than fits here"))?;
    scanning.scan(&mut file, len, needles, password)
}

/// A redaction worked out against the open document, ready for either writer.
///
/// **The Ask step of `docs/PLAN.md` §6, held apart from the write.** Two
/// commands apply a redaction --- `commands::redact::redact_copy` to a new file
/// and `commands::redact::redact_document` over the open one --- and they differ in the writer and
/// in nothing else. A second copy of this loop is the drift this repository
/// keeps recording: the two would go on agreeing about the ordinals and
/// eventually disagree about which objects the reader was warned about.
///
/// **Public for the command-line tool**, which is the third caller: `tpdf
/// redact` asks with this, reports [`Asked::pages`] for `--dry-run`, and hands
/// the rest to [`redact_copy_asked`] --- the window's write, verification and
/// verdict, not a restatement of them.
pub struct Asked {
    /// The reader's plan with the redaction ordinals in it.
    pub plan: edits::Plan,
    /// The colour of the boxes drawn over what went. Black until a caller
    /// that was given another sets it.
    pub fill: crate::redaction_fill::Fill,
    /// The words the regions cover, to look for in what gets written.
    pub needles: Vec<String>,
    /// What the removal could not take. Not a refusal --- see `commands::redact::redact_copy`
    /// --- but a reason the file cannot be called clean, carried to the verdict.
    pub concerns: Vec<String>,
    /// What the result says beside its verdict that is true whatever the
    /// verdict: a picture taken off a page that the document draws elsewhere.
    pub notes: Vec<String>,
    /// How many regions were asked about.
    pub regions: usize,
    /// How many text-showing operations the removal names, after merging.
    pub shows: usize,
    /// What each page's removal takes, counted, in page order.
    ///
    /// Read off the same [`redact::PageAggregate`] the plan is built from, so a
    /// dry run and the write it predicts cannot count two different things.
    /// The window reads none of it; its review panel asks per region.
    pub pages: Vec<redact::PageSummary>,
    /// What the OCR gate needs and only the source document can supply.
    ///
    /// Collected here rather than after the write because after the write it
    /// cannot be: the control the gate renders has to be no larger than the
    /// smallest box a region covered, and the removal takes exactly those boxes.
    /// See [`ocr_gate::GatePage`].
    pub gate: Vec<ocr_gate::GatePage>,
    /// How many of the written pages came from another file.
    ///
    /// Not a count of anything being removed --- a page of another file cannot
    /// be marked ([`crate::docmodel::Refusal::RedactionOnImportedPage`]). It is
    /// what [`redact::inserted_pages_note`] needs to say, when the scan finds a
    /// word, that the scan cannot say which page it was on.
    pub inserted: usize,
    /// Which slots of the **written** file carry a region somebody marked.
    ///
    /// Taken off [`Asked::gate`] after [`redact::gate_at_output_slots`] has
    /// moved it, because that is the one list already in the space
    /// `verify::scan` answers in --- the file that gets written, not the one it
    /// came from. Kept as a field rather than read back off `gate` because
    /// `gate` is moved into the OCR run before the report is read.
    ///
    /// It is what [`redact::marked_pages_note`] compares the walk's answer
    /// against; without it a page number in a reason is trivia.
    pub marked: Vec<u32>,
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
pub async fn ask_redactions(
    edits: &edits::Edits,
    service: &RenderService,
    doc: u32,
) -> Result<Asked, String> {
    let targets = edits.redaction_targets(doc)?;
    if targets.is_empty() {
        return Err(said!("nothing in this document is marked for removal"));
    }

    let mut planned: Vec<edits::PlannedRedaction> = Vec::new();
    let mut needles: Vec<String> = Vec::new();
    let mut concerns: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut regions = 0usize;
    let mut shows_total = 0usize;
    let mut gate: Vec<ocr_gate::GatePage> = Vec::new();
    let mut pages: Vec<redact::PageSummary> = Vec::new();

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
        pages.push(one.summary());
        concerns.extend(one.concerns);
        notes.extend(one.notes);
        needles.extend(one.needles);
        shows_total += one.shows;
        gate.push(one.gate);
        planned.push(one.planned);
    }

    let mut plan = edits.plan(doc)?;

    // **The form answers under the regions, as needles.** A widget's answer is
    // drawn by its appearance stream rather than the page, so no plan above
    // reports it and the scan would never look for it --- which is how a field
    // with a second widget outside the region once kept its answer while the
    // redaction reported itself verified. The writer now takes such a field
    // (`redact::covered_fields`); these needles are what would notice if any
    // copy of the answer survived anyway. One form read for the whole document,
    // in a worker like every other parse of it.
    //
    // A form that cannot be read is a reason, not a refusal: the answers under
    // the regions are then unknown, and an unknown is *not verified*.
    let (reply, rx) = reply_channel();
    service.form(doc, reply);
    match await_reply("form", rx).await {
        Ok(form) => {
            for redaction in &planned {
                needles.extend(redact::covered_answers(
                    &form,
                    redaction.source,
                    &redaction.areas,
                    &plan.forms,
                ));
            }
        }
        Err(why) => concerns.push(said!(
            "the form fields could not be read, so an answer under a marked area \
             cannot be checked: {why}"
        )),
    }
    plan.redactions = planned;
    // **After the plan, because the plan is what says where each page lands.**
    // Everything above addresses the file the reader opened; the gate reopens
    // what gets written and renders a page of it by number. See
    // [`redact::gate_at_output_slots`], which also records that a deletion was
    // already enough to part the two.
    let gate = redact::gate_at_output_slots(&plan.pages, gate);
    // The same slots, kept: this is the reader's answer to "which pages did I
    // mark", in the space the verification answers in.
    //
    // **A page that remap drops cannot make this list wrong**, which is worth
    // stating because dropping one silently is the shape that usually does.
    // `gate_at_output_slots` drops a marked page the plan does not place --- and
    // `redaction_targets` and `plan` take *two* snapshots of the model with a
    // worker round trip between them, so the two page lists are not guaranteed
    // to agree. The reason it is harmless is the space: both this and
    // `verify::Report::placed_pages` are output slots, and a page that is not in
    // the written file has no slot, so the walk cannot attribute a word to it
    // either. An omission here can only remove a page nothing could have been
    // found on. Adding a marked page that is *not* in the output would be the
    // dangerous direction, and the remap cannot do that: it takes the slot from
    // the plan or drops the entry.
    let marked: Vec<u32> = gate.iter().map(|page| page.page).collect();
    // How many of the written pages came from another file, counted off the
    // plan the write uses rather than off the model --- one list, one reader.
    let inserted = plan
        .pages
        .iter()
        .filter(|page| matches!(page.source, PageSource::Imported { .. }))
        .count();
    Ok(Asked {
        plan,
        fill: crate::redaction_fill::Fill::default(),
        needles,
        concerns,
        notes,
        regions,
        shows: shows_total,
        pages,
        gate,
        inserted,
        marked,
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
///
/// The service is taken by value --- a clone is a handle to the same render
/// thread --- rather than fetched from an app handle, so the command-line tool,
/// which has a service and no app, runs the same gate.
pub(crate) async fn gate_written_file(
    service: RenderService,
    path: String,
    pages: Vec<ocr_gate::GatePage>,
    password: Option<String>,
) -> Vec<String> {
    if pages.is_empty() {
        return Vec::new();
    }
    tauri::async_runtime::spawn_blocking(move || ocr_gate::run(&service, &path, password, &pages))
        .await
        .unwrap_or_else(|e| {
            vec![said!(
                "the removed areas could not be checked, so the file cannot be shown clean: {e}"
            )]
        })
}

/// Why a redaction to a new file stopped before it had a verdict.
///
/// **Two kinds, because a caller that can delete has to know which one.** The
/// window shows either as a sentence --- [`Stopped::into_message`] is the one it
/// always showed --- but `tpdf redact` promises that a failure leaves nothing
/// behind, and whether there *is* anything behind is the difference between the
/// two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    /// Refused before anything was written: `save::write_copy`'s refusals ---
    /// an XFA form, a page count that disagrees with the baseline, writing over
    /// the source.
    Refused(String),
    /// Failed. `written` says whether the output was already on disk, as a file
    /// that has not been verified or not been filled.
    Failed {
        /// The sentence.
        message: String,
        /// Whether the output now exists.
        written: bool,
    },
}

impl Stopped {
    /// The sentence, as `commands::redact::redact_copy` has always reported it.
    #[must_use]
    pub fn into_message(self) -> String {
        match self {
            Stopped::Refused(message) | Stopped::Failed { message, .. } => message,
        }
    }
}

/// One more reader of a written, unfilled redaction: handed its path and key,
/// it returns the reasons it found not to call the file clean. See
/// [`redact_copy_asked`]'s `also`.
pub type ReadBack<'a> = dyn Fn(&str, Option<&str>) -> Vec<String> + Sync + 'a;

/// `commands::redact::redact_copy` after the ask: write, verify, gate, fill.
///
/// **The body of the window's command, reached by the command-line tool too.**
/// `tpdf redact` builds the same model the window builds --- regions on an
/// [`edits::Edits`], through [`edits::Edits::redact`] --- asks with
/// [`ask_redactions`], and hands the answer here, so the writer, the read-back,
/// the OCR gate, the black fill and the rule that `verified` is true only when
/// `why` is empty are this function's and nobody else's. `library` is where the
/// writing and scanning workers find PDFium: the app's resource directory, or
/// the tool's, found by the same search.
///
/// Four steps, and the order is the safety of it:
///
/// 1. **Ask.** For each page holding regions, a worker computes what a removal
///    would take --- against PDFium's own object list, behind the sandbox, which
///    is where every parse of the reader's bytes belongs. ([`ask_redactions`].)
/// 2. **Write.** The ordinals go into the plan and `save::write_copy` takes the
///    ordinary rewrite path, which is what applies them --- see
///    `save::apply_redactions` for why it is safe for that to happen last.
/// 3. **Verify.** The written file is scanned for the words that were supposed
///    to go, and every object the removal could not take is a reason of its own.
///    The answer is *verified*, or *not verified* with every reason --- never a
///    bare success, which is §6 step 4 and is why [`redact::Applied`] cannot
///    carry the first without the second.
/// 4. **Fill.** The black boxes are drawn only after both verification passes
///    have read the uncovered file, and against the fingerprint they read.
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
/// **Two classes of object reach that rule and only one of them is about tpdf.**
/// A path or a shading is left because nothing here knows how to take it. A
/// picture or a form the document draws more than once is left because taking
/// the one drawing the reader marked would leave every other copy, and the
/// object itself, in the file --- so the count saying how many times it is drawn
/// travels with the finding, and `redact::Unhandled::sentence` says the two
/// differently. The second class refused the whole write until 2026-09-07, which
/// is this paragraph's own argument arriving as a defect report: a logo on all
/// 22 pages of a standard made a region over the header remove nothing at all.
///
/// # Errors
///
/// [`Stopped::Refused`] for anything `save::write_copy` refuses (an encrypted
/// source, a page count that disagrees with the baseline, writing over the
/// source, an XFA form); [`Stopped::Failed`] when a step did not run or the
/// written file could not be read back or filled.
///
/// **`also` is one more reader of the unfilled file, and it can only take a
/// verdict away.** It is handed the written path and the key, after the byte
/// scan and before the OCR gate and the fill, and every sentence it returns is
/// one more reason in `why`. The window passes `None`. `tpdf redact` passes a
/// search of the written file for its own queries --- the check a reader would
/// make by hand --- which has no counterpart in the window because the window
/// marks by hand. It runs before the fill so that the sentence the application
/// shows, *"Checks before adding the fill found: ..."*, stays true of it.
pub async fn redact_copy_asked(
    service: &RenderService,
    library: std::path::PathBuf,
    doc: u32,
    asked: Asked,
    source: String,
    path: String,
    also: Option<&ReadBack<'_>>,
) -> Result<redact::Applied, Stopped> {
    let plan = asked.plan.clone();
    let needles = asked.needles.clone();
    let concerns = asked.concerns.clone();
    let regions = asked.regions;
    let shows_total = asked.shows;
    let backend = service.backend();

    let out = std::path::PathBuf::from(path);
    let from = std::path::PathBuf::from(source);
    let written = out.clone();
    let out_path = out.to_string_lossy().into_owned();
    let password = password_for(service, doc, "redact_copy").await;
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
    let writing = outside_at(library.clone(), backend);
    let (copied, fingerprint) = tauri::async_runtime::spawn_blocking(move || {
        let copied = save::write_copy(&from, &plan, &out, password.as_deref(), &*writing)?;
        let fingerprint = crate::fingerprint::Fingerprint::of(&out)?;
        Ok::<_, save::Refusal>((copied, fingerprint))
    })
    .await
    .map_err(|e| Stopped::Failed {
        message: said!("the redaction did not run: {e}"),
        written: false,
    })?
    .map_err(|why| Stopped::Refused(why.message))?;

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
    let scanning = outside_at(library.clone(), backend);
    let report = tauri::async_runtime::spawn_blocking(move || {
        scan_written_file(&*scanning, &written, &needles, verifying.as_deref())
    })
    .await
    .map_err(|e| Stopped::Failed {
        message: said!("the verification did not run: {e}"),
        written: true,
    })?
    .map_err(|message| Stopped::Failed {
        message,
        written: true,
    })?;

    // The objects the removal could not take come first, because they are the
    // finding a reader can act on: a picture of the words in the region is a
    // different problem from a scan that could not decode a stream, and only the
    // first tells them the region is still readable.
    let mut why = concerns;
    if let verify::Verdict::NotVerified(reasons) = report.verdict() {
        why.extend(reasons);
    }
    // Directly under them, because they are about how to read them rather than
    // findings of their own --- and only when there is something to read. The
    // two are exclusive by construction: one speaks when every word was placed
    // on a page and the other when at least one was not.
    why.extend(redact::short_needle_note(&report));
    why.extend(redact::marked_pages_note(&report, &asked.marked));
    why.extend(redact::inserted_pages_note(
        asked.inserted,
        &report.found,
        report.placed(),
    ));
    if let Some(also) = also {
        why.extend(also(&out_path, key.as_deref()));
    }
    // Then §6 step 4, which is the only one of the two that can see a picture of
    // the words. It runs on the file that was just written, never on the source
    // --- see `ocr::RedactedPixels`, where that is a type-level rule.
    let notes = ocr_gate::sizing_notes(&asked.gate);
    let taken_here = asked.notes;
    why.extend(gate_written_file(service.clone(), out_path.clone(), asked.gate, key.clone()).await);
    finish_redaction_fill(
        library,
        backend,
        out_path,
        asked.plan,
        fingerprint,
        key,
        asked.fill,
    )
    .await
    .map_err(|message| Stopped::Failed {
        message,
        written: true,
    })?;
    Ok(redact::Applied {
        regions,
        shows: shows_total,
        changed: copied.changed,
        verified: why.is_empty(),
        notes: redact::notes_for(why.is_empty(), notes, taken_here),
        why,
    })
}
