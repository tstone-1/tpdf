//! Redaction from the ask to the verdict, for the window and the command line
//! alike: what a removal of every marked region would take, the rewrite, the
//! read-back of the written file, the OCR gate and the fill.
//!
//! The window's commands (`commands::redact`) and `tpdf redact` (`cli::redact`)
//! all reach these: the ask is [`ask_redactions`], and the readers of a written
//! file and the verdict they come to are [`read_back`], which the copy, the
//! in-place redaction and the image-only copy each call after their own write.
//! `docs/TRAPS.md` records more than one defect that was a second copy of a
//! rule drifting from the first, and the image-only copy was a third copy that
//! had drifted all the way to a verdict written down as `true`.

use std::path::Path;

use crate::docmodel::PageSource;
use crate::redact::ReasonKind;
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
/// **The Ask step of `docs/PLAN.md` §6, held apart from the write.** Three
/// commands apply a redaction --- `commands::redact::redact_copy` to a new file,
/// `commands::redact::redact_document` over the open one and
/// `commands::redact::redact_raster_copy` as pictures --- and they differ in the
/// writer and in which readers apply to what it wrote ([`Written`]). A second
/// copy of this loop is the drift this repository keeps recording: the copies
/// would go on agreeing about the ordinals and eventually disagree about which
/// objects the reader was warned about.
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
    /// The concerns that say [`Asked::needles`] may be short of something the
    /// regions cover: today one, a form whose answers could not be read.
    ///
    /// Also in [`Asked::concerns`]. Kept apart because the image-only copy
    /// takes no object out and so leaves none of the others standing, while
    /// this one limits the code reader that copy still runs.
    pub needles_short: Vec<String>,
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
    let mut needles_short: Vec<String> = Vec::new();
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
        Err(why) => {
            let unread = said!(
                "the form fields could not be read, so an answer under a marked area \
                 cannot be checked: {why}"
            );
            needles_short.push(unread.clone());
            concerns.push(unread);
        }
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
        needles_short,
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
/// and the writes before it are already on a blocking thread for the same
/// reason.
///
/// **Nothing here refuses.** A join that failed is one more reason the file
/// cannot be called clean, and [`redact::Applied`] is what carries it.
///
/// The service is taken by value --- a clone is a handle to the same render
/// thread --- because `spawn_blocking` needs what it captures to outlive the
/// call and a `tauri::State` borrow does not, and because the command-line
/// tool, which has a service and no app, runs the same gate.
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

/// What reading the codes of a written file found.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct CodesRead {
    /// A reason for each code that still carries a removed text, and for each
    /// page or code that could not be compared
    /// ([`crate::codes::reasons_of_kind`]), with what it asks of the reader.
    pub why: Vec<(ReasonKind, String)>,
    /// What a clean verdict says about the codes: how many were read. `None`
    /// when nothing was compared, because nothing was removed as text.
    pub note: Option<String>,
}

/// Reads the QR codes and other 2D codes on every page of the written file,
/// and returns a reason for each code that still carries a removed text
/// ([`crate::codes::reasons`]).
///
/// **Every page, as the byte scan reads every byte.** A removed name that is
/// still written on page 5 withholds *verified* whichever page was marked, and
/// a code on page 5 that holds it is the same finding: an invoice prints its
/// payment code on the last page and the payee on the first. Until 2026-11-18
/// only the marked pages were read, on the estimate that a full-page render at
/// up to 3x and a decode with rxing's `TryHarder` was too much for pages nobody
/// marked. Timed that day on Windows x64, release, against the build without
/// this reader: 16 to 48 ms a page, the higher figure on dense text, so about
/// 15 s for 300 pages.
///
/// **A clean result says how many codes were read** ([`CodesRead::note`]),
/// because rxing reports nothing about a code it found and could not decode.
/// A reader who sees a code on the page and a count of 0 knows it was not
/// compared; without the count that code is silent.
///
/// **Nothing here refuses**, as in [`gate_written_file`]: a file that will not
/// reopen, or a page that will not draw, is a reason the file cannot be called
/// clean. The decoded content never leaves this function.
pub(crate) async fn codes_written_file(
    service: RenderService,
    path: String,
    password: Option<String>,
    needles: Vec<String>,
) -> CodesRead {
    if needles.is_empty() {
        return CodesRead::default();
    }
    tauri::async_runtime::spawn_blocking(move || read_codes(&service, &path, password, &needles))
        .await
        .unwrap_or_else(|e| CodesRead {
            why: vec![(
                ReasonKind::Unchecked,
                said!(
                "the codes in the written file could not be checked, so the file cannot be shown \
                 clean: {e}"
            ),
            )],
            note: None,
        })
}

/// [`codes_written_file`], on the blocking thread it runs on.
pub(crate) fn read_codes(
    service: &RenderService,
    path: &str,
    password: Option<String>,
    needles: &[String],
) -> CodesRead {
    let opened = match ocr_gate::wait(|reply| {
        service.open(std::path::PathBuf::from(path), true, password, reply)
    }) {
        Ok(info) => info,
        Err(refusal) => {
            return CodesRead {
                why: vec![(
                    ReasonKind::Unchecked,
                    said!(
                    "the written file could not be reopened to read the codes on its pages, so \
                     it cannot be shown clean: {reason}",
                    reason = refusal.reason
                ),
                )],
                note: None,
            }
        }
    };
    let mut why = Vec::new();
    let mut codes = 0usize;
    // A page count past `u32` is not a file this opened; the cast cannot cut.
    let pages = u32::try_from(opened.page_count).unwrap_or(u32::MAX);
    for page in 0..pages {
        match ocr_gate::wait(|reply| service.codes(opened.id, page, reply)) {
            Ok(found) => {
                codes += found.codes.len();
                why.extend(crate::codes::reasons_of_kind(page, &found, needles));
            }
            Err(e) => why.push((
                ReasonKind::Unchecked,
                said!(
                "the codes on page {n} could not be read, so whether one still carries what was \
                 removed is not known: {e}",
                n = page + 1
            ),
            )),
        }
    }
    let _: Result<(), String> = ocr_gate::wait(|reply| service.close(opened.id, reply));
    CodesRead {
        why,
        note: Some(crate::codes::read_note(codes)),
    }
}

/// What a redaction wrote, as far as the readers after it are concerned.
pub(crate) enum Written {
    /// The content was rewritten with the marked objects taken out: the copy
    /// and the in-place redaction. The byte scan has read it, and `also` is
    /// what [`redact_copy_asked`]'s extra reader found, empty for the others.
    Rewritten {
        /// [`scan_written_file`]'s answer.
        report: verify::Report,
        /// Reasons from one more reader of the unfilled file.
        also: Vec<String>,
    },
    /// Every page replaced by a picture of itself with the marked areas
    /// painted over: the image-only copy.
    ///
    /// `save::write_raster_copy` has already proved, before the file was
    /// published (`raster_redact::rewrite`), that its object graph is exactly
    /// the pictures, content streams and page tree it generated, that PDFium
    /// finds no text layer on any page, and that every pixel inside a marked
    /// area is the fill colour. So three readers do not apply: the byte scan
    /// has no text to read and would call every picture unread, the OCR gate
    /// would look at areas already proved to be one colour, and an object the
    /// removal could not take is not in a file that keeps no object of the
    /// source. A code printed beside the marked area is still in the picture,
    /// and the code reader is the one that sees it.
    ///
    /// `elsewhere` is [`text_elsewhere`]'s answer: a removed word the source
    /// still shows as text on a page no region marks, which the copy turns
    /// into pixels that none of those readers look at.
    Pictures {
        /// Reasons, one per word found on an unmarked page, and one per page
        /// that could not be searched.
        elsewhere: Vec<(ReasonKind, String)>,
    },
}

/// The output slots of `plan` with no marked region, each with the page of the
/// opened document it is drawn from.
///
/// The same pairing `raster_redact::rewrite` paints the marks by: a redaction
/// names a page of the opened document, and the slot drawing that page is the
/// marked one. A slot drawn from another file carries no source page here; the
/// image-only copy refuses such a plan before anything is written.
fn unmarked_slots(plan: &edits::Plan) -> Vec<(u32, u32)> {
    slots_without(
        plan.pages.iter().map(|page| page.source),
        plan.redactions.iter().map(|one| one.source),
    )
}

/// [`unmarked_slots`] over the page sources in slot order and the marked
/// pages of the opened document.
fn slots_without(
    sources: impl Iterator<Item = PageSource>,
    marked: impl Iterator<Item = u32>,
) -> Vec<(u32, u32)> {
    let marked: std::collections::BTreeSet<u32> = marked.collect();
    sources
        .enumerate()
        .filter_map(|(slot, source)| match source {
            PageSource::Baseline(source) if !marked.contains(&source) => {
                Some((u32::try_from(slot).ok()?, source))
            }
            _ => None,
        })
        .collect()
}

/// Where the source still shows a removed word as text, outside every marked
/// page, as reasons for the verdict of an image-only copy.
///
/// The rewritten copy's byte scan finds such a copy and names its page; the
/// image-only copy has no text to scan, so a second copy of the name on an
/// unmarked page came out of it as pixels and was certified (residual 42,
/// `docs/THREAT-MODEL.md`). This asks the source's text layer instead, per
/// page and per word, matching case as the byte scan does. `count` answers how
/// many times a word occurs on a page of the opened document; an answer it
/// cannot give is a reason too, because a page not searched is not a page
/// shown clean.
///
/// What this does not see: a removed word drawn as a picture on an unmarked
/// page, and a second copy on a marked page outside its regions.
///
/// `slots` is [`unmarked_slots`]'s answer.
pub(crate) fn text_elsewhere(
    slots: &[(u32, u32)],
    needles: &[String],
    count: impl Fn(u32, &str) -> Result<usize, String>,
) -> Vec<(ReasonKind, String)> {
    let mut why = Vec::new();
    for needle in needles {
        let mut pages = std::collections::BTreeSet::new();
        for &(slot, source) in slots {
            match count(source, needle) {
                Ok(0) => {}
                Ok(_) => {
                    pages.insert(slot);
                }
                Err(e) => why.push((
                    ReasonKind::Unchecked,
                    said!(
                        "page {n} could not be searched for what was removed, so whether it still \
                     shows it is not known: {e}",
                        n = slot + 1
                    ),
                )),
            }
        }
        if !pages.is_empty() {
            why.push((
                ReasonKind::Found,
                said!(
                "{needle} is also shown as text on {pages}, which no region marks; the image-only \
                 copy keeps it there as a picture",
                pages = verify::Placed::of(&pages).sentence()
            ),
            ));
        }
    }
    why.dedup();
    why
}

/// [`text_elsewhere`] against the opened document `doc`, one search a page and
/// a word, on a blocking thread.
pub(crate) async fn text_elsewhere_in(
    service: RenderService,
    doc: u32,
    plan: edits::Plan,
    needles: Vec<String>,
) -> Vec<(ReasonKind, String)> {
    tauri::async_runtime::spawn_blocking(move || {
        let options = crate::search::Options {
            match_case: true,
            whole_word: false,
            regex: false,
        };
        text_elsewhere(&unmarked_slots(&plan), &needles, |page, needle| {
            let found = ocr_gate::wait(|reply| {
                service.search(doc, vec![page], needle.to_string(), options, None, reply)
            })?;
            match found.problem {
                Some(problem) => Err(problem),
                None => Ok(found.matches.len()),
            }
        })
    })
    .await
    .unwrap_or_else(|e| {
        vec![(
            ReasonKind::Unchecked,
            said!("the pages no region marks could not be searched for what was removed: {e}"),
        )]
    })
}

/// What [`read_back`] needs from an [`Asked`], borrowed, so a test can build it.
pub(crate) struct Parts<'a> {
    /// [`Asked::concerns`].
    pub concerns: &'a [String],
    /// [`Asked::needles_short`].
    pub needles_short: &'a [String],
    /// [`Asked::marked`].
    pub marked: &'a [u32],
    /// [`Asked::inserted`].
    pub inserted: usize,
    /// [`Asked::gate`].
    pub gate: &'a [ocr_gate::GatePage],
    /// [`Asked::notes`].
    pub taken_here: &'a [String],
}

impl Asked {
    /// The parts of this the verdict reads.
    pub(crate) fn parts(&self) -> Parts<'_> {
        Parts {
            concerns: &self.concerns,
            needles_short: &self.needles_short,
            marked: &self.marked,
            inserted: self.inserted,
            gate: &self.gate,
            taken_here: &self.notes,
        }
    }
}

/// A redaction's verdict: every reason it is not clean, and what it says beside.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Judged {
    /// Empty exactly when the file can be called clean.
    pub why: Vec<String>,
    /// What each of [`Judged::why`] asks of the reader, one each, in order
    /// ([`redact::ReasonKind`]).
    pub kinds: Vec<ReasonKind>,
    /// [`redact::notes_for`]'s answer.
    pub notes: Vec<String>,
}

/// Runs the readers that apply to what was written, then comes to the verdict.
///
/// **The one place a redaction's verdict is assembled**, for all three
/// commands: each does its own write and its own fill and calls this in
/// between. Until it existed the chain was written out in the copy and again
/// in the in-place redaction, held equal by a test that looked for five names
/// in both bodies, and the image-only copy had no chain at all: it returned
/// `verified: true` without reading anything, so a QR code beside the marked
/// name came out of it certified.
///
/// Runs the OCR gate only for [`Written::Rewritten`] (see [`Written::Pictures`]
/// for why) and the code reader for both; [`judge`] says what their answers
/// and the report come to.
pub(crate) async fn read_back(
    service: RenderService,
    path: String,
    key: Option<String>,
    asked: &Asked,
    written: Written,
) -> Judged {
    // §6 step 4, which is the only reader that can see a picture of the words.
    // It runs on the file that was just written, never on the source --- see
    // `ocr::RedactedPixels`, where that is a type-level rule.
    let gate = match written {
        Written::Rewritten { .. } => {
            gate_written_file(
                service.clone(),
                path.clone(),
                asked.gate.clone(),
                key.clone(),
            )
            .await
        }
        Written::Pictures { .. } => Vec::new(),
    };
    // Then the codes in the file, which no other reader can see: a payment
    // code beside the marked name holds the name too (`codes.rs`).
    let codes = codes_written_file(service, path, key, asked.needles.clone()).await;
    judge(&asked.parts(), written, gate, codes)
}

/// What the readers' answers come to: [`read_back`] without the reading.
///
/// The order of `why` is the order a reader should meet the reasons in, and
/// it is fixed here so all three commands keep it:
///
/// 1. The objects the removal could not take, because they are the finding a
///    reader can act on: a picture of the words in the region is a different
///    problem from a scan that could not decode a stream, and only the first
///    tells them the region is still readable.
/// 2. The scan's own reasons, and directly under them the notes on how to read
///    them, which only speak when there is something to read. The short-piece
///    note and the marked-pages note are exclusive by construction: one speaks
///    when every word was placed and the other when at least one was not.
/// 3. The extra reader's reasons, then the OCR gate's, then the code reader's.
pub(crate) fn judge(
    asked: &Parts<'_>,
    written: Written,
    gate: Vec<String>,
    codes: CodesRead,
) -> Judged {
    let mut why: Vec<(ReasonKind, String)> = Vec::new();
    let mut add = |kind: ReasonKind, reasons: &mut dyn Iterator<Item = String>| {
        why.extend(reasons.map(|reason| (kind, reason)));
    };
    let mut sizing: Vec<String> = Vec::new();
    let mut taken_here: Vec<String> = Vec::new();
    match written {
        Written::Rewritten { report, also } => {
            // The objects a region covers that the removal could not take are
            // pictures and drawings; the form that could not be read is not.
            for concern in asked.concerns {
                let kind = if asked.needles_short.contains(concern) {
                    ReasonKind::Unchecked
                } else {
                    ReasonKind::Pictures
                };
                add(kind, &mut std::iter::once(concern.clone()));
            }
            // `Report::verdict`'s order: the words found, then what could not
            // be read, then the pictures nobody read.
            if let verify::Verdict::NotVerified(reasons) = report.verdict() {
                let found = report.found.len();
                let blind = report.blind.len();
                for (at, reason) in reasons.into_iter().enumerate() {
                    let kind = if at < found {
                        ReasonKind::Found
                    } else if at < found + blind {
                        ReasonKind::Unchecked
                    } else {
                        ReasonKind::Pictures
                    };
                    add(kind, &mut std::iter::once(reason));
                }
            }
            add(
                ReasonKind::Unchecked,
                &mut redact::short_needle_note(&report).into_iter(),
            );
            add(
                ReasonKind::Found,
                &mut redact::marked_pages_note(&report, asked.marked).into_iter(),
            );
            add(
                ReasonKind::Unchecked,
                &mut redact::inserted_pages_note(asked.inserted, &report.found, report.placed())
                    .into_iter(),
            );
            add(ReasonKind::Found, &mut also.into_iter());
            add(ReasonKind::Pictures, &mut gate.into_iter());
            sizing = ocr_gate::sizing_notes(asked.gate);
            // A picture taken off one page and left in the file for the
            // others: true of a rewrite, and of nothing a picture copy keeps.
            taken_here = asked.taken_here.to_vec();
        }
        Written::Pictures { elsewhere } => {
            // Only what limits the one reader that still applies: a form
            // answer under a marked area that could not be read is a word the
            // code reader was never given to look for. Then a removed word the
            // source shows on an unmarked page, which the copy keeps.
            add(
                ReasonKind::Unchecked,
                &mut asked.needles_short.iter().cloned(),
            );
            why.extend(elsewhere);
        }
    }
    why.extend(codes.why);
    sizing.extend(codes.note);
    let notes = redact::notes_for(why.is_empty(), sizing, taken_here);
    let (kinds, why) = why.into_iter().unzip();
    Judged { why, kinds, notes }
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
/// [`ask_redactions`], and hands the answer here, so the writer, the byte scan
/// and the black fill are this function's and nobody else's, and the readers
/// after the scan, with the rule that `verified` is true only when `why` is
/// empty, are [`read_back`]'s, which the other two commands call too. `library` is where the
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
/// marks by hand.
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

    // Before the other readers, on the unfilled file, so that the sentence the
    // application shows, *"Checks before adding the fill found: ..."*, stays
    // true of it.
    let also = also.map_or_else(Vec::new, |also| also(&out_path, key.as_deref()));
    let judged = read_back(
        service.clone(),
        out_path.clone(),
        key.clone(),
        &asked,
        Written::Rewritten { report, also },
    )
    .await;
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
        verified: judged.why.is_empty(),
        notes: judged.notes,
        why: judged.why,
        kinds: judged.kinds,
    })
}

#[cfg(test)]
mod tests {
    use super::{judge, CodesRead, Judged, Parts, ReasonKind, Written};

    /// Every part a verdict reads, each a sentence naming itself, so an
    /// assertion on `why` says which reader put each line there.
    struct Asked {
        concerns: Vec<String>,
        needles_short: Vec<String>,
        taken_here: Vec<String>,
    }

    impl Asked {
        fn full() -> Self {
            Asked {
                concerns: vec!["page 1: object 7 was left".to_string()],
                needles_short: vec!["the form fields could not be read".to_string()],
                taken_here: vec!["page 1: object 9 is still in the file".to_string()],
            }
        }

        fn parts(&self) -> Parts<'_> {
            Parts {
                concerns: &self.concerns,
                needles_short: &self.needles_short,
                marked: &[],
                inserted: 0,
                gate: &[],
                taken_here: &self.taken_here,
            }
        }
    }

    /// A scan that found `needle` and placed it nowhere.
    fn found(needle: &str) -> crate::verify::Report {
        let mut report = crate::verify::Report::default();
        report.found.insert(needle.to_string());
        report
    }

    fn code(why: &[&str]) -> CodesRead {
        CodesRead {
            why: why
                .iter()
                .map(|reason| (ReasonKind::Found, (*reason).to_string()))
                .collect(),
            note: Some("1 code was read".to_string()),
        }
    }

    #[test]
    fn a_rewrite_is_judged_on_every_reader_in_order() {
        let asked = Asked::full();
        let judged = judge(
            &asked.parts(),
            Written::Rewritten {
                report: found("4711-0815"),
                also: vec!["the search found it".to_string()],
            },
            vec!["the gate read it".to_string()],
            code(&["a code still carries it"]),
        );
        assert_eq!(
            judged.why,
            vec![
                // `needles_short` is a copy of lines `concerns` already holds,
                // so a rewrite does not say it twice.
                "page 1: object 7 was left".to_string(),
                "4711-0815 is still in the file".to_string(),
                "the search found it".to_string(),
                "the gate read it".to_string(),
                "a code still carries it".to_string(),
            ],
            "the objects left first, then the scan, the extra reader, the gate and the codes"
        );
        assert_eq!(
            judged.kinds,
            vec![
                ReasonKind::Pictures,
                ReasonKind::Found,
                ReasonKind::Found,
                ReasonKind::Pictures,
                ReasonKind::Found,
            ],
            "one kind per reason, decided by the reader that gave it"
        );
        // Not clean, so no note qualifies a clean verdict; the picture left in
        // the file is said whatever the verdict.
        assert_eq!(
            judged.notes,
            vec!["page 1: object 9 is still in the file".to_string()]
        );
    }

    #[test]
    fn a_code_reader_finding_alone_withholds_a_rewrite_s_verdict() {
        let asked = Asked {
            concerns: Vec::new(),
            needles_short: Vec::new(),
            taken_here: Vec::new(),
        };
        let rewritten = || Written::Rewritten {
            report: crate::verify::Report::default(),
            also: Vec::new(),
        };
        let judged = judge(
            &asked.parts(),
            rewritten(),
            Vec::new(),
            code(&["a code still carries it"]),
        );
        assert_eq!(judged.why, vec!["a code still carries it".to_string()]);
        // The control: nothing found anywhere is clean, and says how many
        // codes were compared.
        let clean = judge(&asked.parts(), rewritten(), Vec::new(), code(&[]));
        assert_eq!(
            clean,
            Judged {
                why: Vec::new(),
                kinds: Vec::new(),
                notes: vec!["1 code was read".to_string()],
            }
        );
    }

    #[test]
    fn an_image_only_copy_reports_a_code_that_still_carries_the_name() {
        let asked = Asked {
            concerns: Vec::new(),
            needles_short: Vec::new(),
            taken_here: Vec::new(),
        };
        let judged = judge(
            &asked.parts(),
            pictures(Vec::new()),
            Vec::new(),
            code(&["the QR code on page 1 still holds 4711-0815"]),
        );
        assert_eq!(
            judged.why,
            vec!["the QR code on page 1 still holds 4711-0815".to_string()]
        );
    }

    /// What a picture copy leaves behind is not what a rewrite leaves: no
    /// object of the source is in it, so the objects a removal could not take
    /// and the picture taken off one page are not reasons or notes about it.
    /// A form that could not be read still is, because the words under it
    /// were never handed to the code reader.
    #[test]
    fn an_image_only_copy_is_judged_on_what_applies_to_pictures() {
        let asked = Asked::full();
        let judged = judge(&asked.parts(), pictures(Vec::new()), Vec::new(), code(&[]));
        assert_eq!(
            judged.why,
            vec!["the form fields could not be read".to_string()]
        );
        // A word the source still shows on an unmarked page comes after it.
        let elsewhere = judge(
            &asked.parts(),
            pictures(vec!["4711-0815 is also shown".to_string()]),
            Vec::new(),
            code(&[]),
        );
        assert_eq!(
            elsewhere.why,
            vec![
                "the form fields could not be read".to_string(),
                "4711-0815 is also shown".to_string(),
            ]
        );
        let readable = Asked {
            needles_short: Vec::new(),
            ..Asked::full()
        };
        let clean = judge(
            &readable.parts(),
            pictures(Vec::new()),
            Vec::new(),
            code(&[]),
        );
        assert_eq!(
            clean,
            Judged {
                why: Vec::new(),
                kinds: Vec::new(),
                notes: vec!["1 code was read".to_string()],
            }
        );
    }

    fn pictures(elsewhere: Vec<String>) -> Written {
        Written::Pictures {
            elsewhere: elsewhere
                .into_iter()
                .map(|reason| (ReasonKind::Found, reason))
                .collect(),
        }
    }

    #[test]
    fn only_the_slots_of_unmarked_pages_of_this_document_are_searched() {
        use crate::docmodel::PageSource;
        // Slot 0 draws marked page 2, slot 1 unmarked page 0, slot 2 a blank
        // page, slot 3 unmarked page 1.
        let sources = [
            PageSource::Baseline(2),
            PageSource::Baseline(0),
            PageSource::Blank(crate::docmodel::Size {
                width: 595.0,
                height: 842.0,
            }),
            PageSource::Baseline(1),
        ];
        assert_eq!(
            super::slots_without(sources.into_iter(), [2].into_iter()),
            vec![(1, 0), (3, 1)]
        );
    }

    #[test]
    fn a_removed_word_on_an_unmarked_page_is_a_reason_naming_the_output_page() {
        let needles = vec!["4711-0815".to_string(), "Smith".to_string()];
        // Page 5 of the opened document is output slot 1, and shows the
        // number once; nothing shows the name.
        let why = super::text_elsewhere(&[(0, 4), (1, 5)], &needles, |page, needle| {
            Ok(usize::from(page == 5 && needle == "4711-0815"))
        });
        assert_eq!(
            why,
            vec![(
                ReasonKind::Found,
                "4711-0815 is also shown as text on page 2, which no region marks; the \
                 image-only copy keeps it there as a picture"
                    .to_string()
            )]
        );
        // The control: no hit is no reason.
        assert!(super::text_elsewhere(&[(0, 4), (1, 5)], &needles, |_, _| Ok(0)).is_empty());
        // A page that cannot be searched is not one shown clean.
        let failed = super::text_elsewhere(&[(0, 4)], &needles[..1], |_, _| {
            Err("the worker stopped".to_string())
        });
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, ReasonKind::Unchecked);
        assert!(
            failed[0].1.starts_with("page 1 could not be searched"),
            "{failed:?}"
        );
    }
}
