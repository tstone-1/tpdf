//! What a redaction can prove, over a corpus rather than a fixture.
//!
//! `docs/PLAN.md` §6 sized its own gap once, before two of the carriers were
//! closed and before there was an OCR gate at all: *39.1% of realistic regions
//! hold an object the removal cannot take*. That number is quoted in four
//! places and in [`tpdf_lib::ocr_gate`]'s own module documentation, and nothing
//! has re-measured it since the form carrier landed on 2026-08-27 and the image
//! carrier the same day. A figure that decides which increment comes next is
//! worth exactly as much as the date on it.
//!
//! Two questions, and they need different instruments:
//!
//! 1. **How often is a region incomplete, and what carries it?** Answered by
//!    `redaction_plans` alone --- no write, no render, no engine --- so it runs
//!    over every sampled region.
//! 2. **What does the gate say about a region the removal took whole?** Answered
//!    only by writing the file and reading the pixels back, so it runs over a
//!    sample of the sample.
//!
//! **The second answer was not a leak rate until 2026-08-27**, and it is worth
//! knowing why rather than reading the number on trust.
//! [`tpdf_lib::ocr_gate`]'s `strip` renders *"the rows one point rectangle
//! covers, rendered as a full-width tile"*, so a region narrower than the line
//! it sits on was judged together with its neighbours --- which the removal was
//! never asked to take and must not. `ocr_gate::mask_columns` blanks those
//! columns now, and this probe reads
//! [`tpdf_lib::ocr_gate::judge_all`] rather than `run`, so it has the engine's
//! own rectangles and can say whether a surviving read was inside the region or
//! beside it. On the same 104 regions: 54 still-readable became 6, and all 6
//! are inside.
//!
//! `--full-width` widens every region to the page. It was the control that
//! failed to isolate what it was aimed at, and it is kept because what it found
//! instead is worth having: the row band is identical either way, so no verdict
//! should have moved, and 54 became 9. Not because the columns were read ---
//! `strip` provably ignored them --- but because a wider region covers more
//! words, which moves the control the gate chooses, which moves the render
//! scale, which moves what the engine reads. **The verdict turns heavily on the
//! control choice**, which is a fact about the gate that nothing else here
//! measures.
//!
//! **The second question is not the one the plan asks, and that is the finding
//! this probe was built to check.** §6 says of the 39.1%: *"Step 4 is what turns
//! those into an answer."* It cannot. `redact_copy` assembles its reasons as
//! `concerns` --- one per object the removal could not take --- and then
//! *extends* them with the gate's, so a region whose path the gate proves
//! illegible keeps its concern and the file stays uncertified. The gate is a
//! **catcher**, not a rescuer: the only outcome it can change is a removal that
//! looked complete and left readable pixels behind. So question 2 asks how often
//! that happens on real documents, which is the honest measure of its worth.
//!
//! ```text
//! cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-reach-probe -- \
//!     ~/Downloads --pages 3 --regions 40
//! ```
//!
//! **Counts and shapes only.** No page text, no filename beyond the stem, and no
//! recognised string leaves this probe --- it is pointed at a corpus of the
//! reader's own documents, and a measurement that prints what it read is a
//! measurement nobody can run twice.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use tpdf_lib::edits::{PageView, Plan, PlannedRedaction};
use tpdf_lib::ocr::{Legibility, NotVerifiedCause};
use tpdf_lib::ocr_gate::{self, GatePage, Judged, PageOutcome};
use tpdf_lib::render::{Backend, RenderService};
use tpdf_lib::{save, worker, worker_child};

/// A region no shorter than this many characters is one a reader might drag.
///
/// The shortest thing anyone redacts is a name or a number; a two-character word
/// is a fragment, and the population it would add is mostly punctuation the
/// removal has nothing to say about.
const MIN_WORD: usize = 4;

/// Files larger than this are not opened.
///
/// A rewrite copies the whole document, and one 337 MB scan in a corpus turns a
/// two-minute sweep into an hour. Stated as a flag so a run that wants them can
/// have them.
const DEFAULT_MAX_MB: u64 = 64;

#[derive(Default)]
struct Tally {
    /// Documents the service opened, and documents it refused.
    opened: usize,
    refused: usize,
    /// Regions asked about, and how many the removal takes whole.
    regions: usize,
    complete: usize,
    /// Documents with at least one incomplete region.
    docs_incomplete: usize,
    /// Regions holding each kind of object the removal cannot take. A region
    /// covering two kinds counts once in each, which is why these do not sum to
    /// `regions - complete`.
    kinds: BTreeMap<String, usize>,
    /// Pages the gate was run over, and what it said.
    gate_pages: usize,
    gate_regions: usize,
    /// The gate read words back out of the row band of a region the removal
    /// reported taking whole.
    ///
    /// **Not a leak count.** The band is the page's full width, so a region
    /// narrower than its line is judged with the neighbours the removal was
    /// right to keep. See the module note.
    caught: usize,
    /// Of those, the ones where **every** span the engine read overlaps the
    /// region's own columns. Since `ocr_gate::mask_columns` this should be all
    /// of them, and the gap is how much the band still reads that is not the
    /// region.
    caught_inside: usize,
    /// The gate could not answer.
    unanswered: usize,
    /// Of those, which step could not be completed --- one counter per
    /// [`NotVerifiedCause`], plus [`Tally::gate_refused`] for a run-wide refusal
    /// that has no per-region cause at all.
    ///
    /// **This is the increment.** `docs/PLAN.md` §6 asked where the regions that
    /// come back *not verified* actually go, and until 2026-08-28 this could
    /// answer for exactly one of them: it matched `why.contains("control token")`
    /// against a sentence written for a human, and threw away the verdict of
    /// every page-wide refusal and every run-wide one without reading it. So a
    /// step that never failed and a step whose sentence had been reworded
    /// produced the same number, and the page-wide steps could not be reported
    /// at all.
    ///
    /// No count of the causes is written here, for the reason `AGENTS.md` gives
    /// about the trap count: the authority is [`NotVerifiedCause::ALL`], and this
    /// sentence said *eight* for the half hour between writing the type and
    /// splitting its commonest variant into five.
    causes: BTreeMap<&'static str, usize>,
    /// Regions under a [`Judged::Refused`]: the engine would not spawn, or the
    /// written file would not reopen. Not a [`NotVerifiedCause`], because it is
    /// about the machine or the file rather than about any region --- and it is
    /// counted rather than folded in, since one of these accounts for every
    /// region in the run at once.
    ///
    /// `gate_` because [`Tally::refused`] above is already a count of *documents
    /// the render service would not open*, which is a different refusal at a
    /// different layer. The compiler caught the collision; a reader would not
    /// have.
    gate_refused: usize,
    /// Every region whose cause was [`NotVerifiedCause::ControlUnread`], bucketed
    /// by how tall the control it was shown landed in the probe image.
    ///
    /// **The bound this is measured against is the gate's own.**
    /// [`ocr_gate::MIN_CONTROL_PX`] is documented as *"the shortest line the
    /// vendored Vision build read reliably"*, and
    /// [`ocr_gate::geometry_for`] chooses the scale from
    /// [`tpdf_lib::ocr::ControlChoice::size_pt`] --- the smallest box a region
    /// covered --- not from the control word. A surviving word with neither
    /// ascender nor descender is shorter than that box, so it can land under the
    /// floor while the scale rule believes it cleared it. Whether that accounts
    /// for the bucket is the question this measures rather than argues.
    unread_px: BTreeMap<&'static str, usize>,
    /// The same regions again, bucketed by how many characters the control token
    /// draws.
    ///
    /// The second candidate, and it needs its own axis because the first one did
    /// not account for the bucket. [`tpdf_lib::ocr::MIN_CONTROL_CHARS`] is 4, and
    /// [`tpdf_lib::ocr::adjudicate`] matches by *containment* --- one recognised
    /// span has to hold the whole token. A four-character token is the shortest
    /// thing that rule accepts, and whether the unread ones cluster there is a
    /// measurement rather than an argument.
    unread_chars: BTreeMap<&'static str, usize>,
    /// Every region the gate reached a per-region verdict on, bucketed by the
    /// length of the control its page was given.
    ///
    /// **The denominator, and without it the bucket above is unreadable.** 29 of
    /// 33 unread controls drawing eight characters or more is a statement about
    /// the failures only; if nine in ten of *all* controls are that long it is
    /// exactly what a uniform failure rate produces and the length explains
    /// nothing. Counted per region rather than per page so the two maps divide.
    all_chars: BTreeMap<&'static str, usize>,
    /// The same regions a third time, split by what the engine had actually
    /// returned --- [`tpdf_lib::ocr::Unread`], one counter per shape.
    ///
    /// **The third candidate, and the one the first two made necessary.** Neither
    /// the control's rendered height nor its length accounts for the bucket: after
    /// the 2026-08-29 scale fix the failure rate is flat at 19--33% across every
    /// token length of five or more, which is the signature of a property of the
    /// page or the image rather than of the control the chooser picked. The
    /// hypothesis it was built to test is the band: [`tpdf_lib::ocr::adjudicate`]
    /// partitions the engine's spans by centre, and a span that *holds* the token
    /// while falling outside produces this exact verdict.
    ///
    /// **Measured 2026-08-30, and the band is refused: `outside` is zero over 197
    /// refusals at three densities.** The rows are kept, and `outside` most of
    /// all --- a row that has only ever been zero is the one whose absence would
    /// go unnoticed, and it is the row that would move if `stack` or the centre
    /// rule ever changed. What is left is `silent`, at 54.5% to 83.3%: the engine
    /// answers and returns nothing for a probe image holding a control it should
    /// read.
    ///
    /// Three counters rather than a map, because they are not buckets of one
    /// quantity: `silent` is a statement about the image, `absent` about what the
    /// engine could make of it, and `outside` about the band's geometry. Each
    /// names a different place to look.
    unread_silent: usize,
    unread_absent: usize,
    unread_outside: usize,
    /// Regions whose unread verdict carried no evidence at all, which the type
    /// says cannot happen --- so this is the arithmetic control on the three
    /// above rather than a fourth reading. A non-zero here is a defect in
    /// [`tpdf_lib::ocr::adjudicate`], not a finding about the gate.
    unread_no_evidence: usize,
    /// How far above its band each reading of the token sat, in points.
    ///
    /// Kept per reading rather than as a running maximum, because the repair
    /// differs by distance and a single worst case cannot say which is typical:
    /// a hair over the edge is a tolerance to widen, half the image is an engine
    /// returning one span for both strips and no tolerance fixes it.
    ///
    /// **Empty on every corpus measured so far**, since it only fills when
    /// `unread_outside` does. The line it feeds is therefore printed only when it
    /// is non-empty: a median over nothing is a number, and it would be a number
    /// about no readings.
    outside_by: Vec<f32>,
    /// The shape and the rendered height of the same region, together.
    ///
    /// **Two marginals cannot answer the question the two of them raise.** At
    /// `--regions 40` on 2026-08-30, 80 of 96 unread controls had the engine
    /// return nothing at all and 40 had rendered under 8 px --- which bounds the
    /// overlap and does not measure it: anywhere from 40 to 80 of the silent
    /// ones were shown a control the scale rule believed was legible. The
    /// difference decides whether the remaining work is the scale or the image,
    /// and no arithmetic over the two rows can settle it.
    ///
    /// **It is 40** --- the low end, so the scale accounts for exactly half the
    /// silence and the other half is a control at or above the floor that the
    /// engine returned nothing for.
    ///
    /// Keyed by shape first so the report's rows are the shapes, which is the
    /// division that names a place to look.
    unread_shape_px: BTreeMap<(&'static str, &'static str), usize>,
    /// The gate showed the region unreadable, which is the only verdict that
    /// may be presented as clean.
    proved: usize,
}

pub fn main() {
    // This binary is both workers: each spawn re-execs `current_exe`.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        worker_child::main(&args);
    }
    #[cfg(target_os = "macos")]
    if args
        .iter()
        .any(|a| a == tpdf_lib::ocr_worker::OCR_WORKER_ARGV)
    {
        tpdf_lib::ocr_worker::child_main();
    }

    let mut roots: Vec<PathBuf> = Vec::new();
    let mut library = PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR);
    let mut pages_per_doc = 3usize;
    let mut regions_per_page = 40usize;
    let mut max_mb = DEFAULT_MAX_MB;
    let mut gate = true;
    // A control over the gate rather than over the removal: `ocr_gate::strip`
    // renders "the rows one point rectangle covers, rendered as a full-width
    // tile", so widening every region to the page's width should change no
    // verdict at all. Identical totals either way is what proves the gate reads
    // a band of rows and ignores the region's columns.
    let mut full_width = false;
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        let mut next = || rest.next().cloned().unwrap_or_default();
        match arg.as_str() {
            "--lib" => library = PathBuf::from(next()),
            "--pages" => pages_per_doc = next().parse().unwrap_or(pages_per_doc),
            "--regions" => regions_per_page = next().parse().unwrap_or(regions_per_page),
            "--max-mb" => max_mb = next().parse().unwrap_or(max_mb),
            "--no-gate" => gate = false,
            "--full-width" => full_width = true,
            other if !other.starts_with("--") => roots.push(PathBuf::from(other)),
            _ => {}
        }
    }
    if roots.is_empty() {
        eprintln!(
            "usage: redact-reach-probe <file.pdf|dir> ... \
             [--pages N] [--regions N] [--max-mb N] [--no-gate] [--lib <dir>]"
        );
        std::process::exit(2);
    }

    let files = collect(&roots, max_mb);
    if files.is_empty() {
        eprintln!("[ERROR] no readable PDF under {} MB in that path", max_mb);
        std::process::exit(2);
    }
    println!(
        "corpus     {} file(s), sampling {} page(s) x {} region(s), gate {}{}",
        files.len(),
        pages_per_doc,
        regions_per_page,
        if gate { "on" } else { "off" },
        if full_width {
            ", regions widened to the page"
        } else {
            ""
        }
    );

    let service = RenderService::start_with(library, Backend::Worker);
    let mut tally = Tally::default();
    let started = Instant::now();
    for file in &files {
        measure(
            &service,
            file,
            pages_per_doc,
            regions_per_page,
            gate,
            full_width,
            &mut tally,
        );
    }
    report(&tally, started.elapsed().as_secs_f32());
}

/// Every PDF under the given paths, small enough to rewrite.
fn collect(roots: &[PathBuf], max_mb: u64) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        if root.is_dir() {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                {
                    out.push(path);
                }
            }
        } else {
            out.push(root.clone());
        }
    }
    out.retain(|p| {
        std::fs::metadata(p).is_ok_and(|m| m.len() <= max_mb.saturating_mul(1024 * 1024))
    });
    out.sort();
    out
}

/// Which pages to ask about: spread across the document rather than the front.
///
/// A document's first pages are its title and contents, which are the least
/// like the pages anybody redacts. Evenly spaced is not random --- this is a
/// sample chosen for coverage, and a run of it is reproducible, which a random
/// one would not be.
fn sampled(count: usize, want: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    if count <= want {
        return (0..count).collect();
    }
    (0..want).map(|i| i * count / want).collect()
}

#[allow(clippy::too_many_arguments)]
fn measure(
    service: &RenderService,
    file: &Path,
    pages_per_doc: usize,
    regions_per_page: usize,
    gate: bool,
    full_width: bool,
    tally: &mut Tally,
) {
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().chars().take(24).collect::<String>())
        .unwrap_or_default();
    let opened = match wait(|reply| service.open(file.to_path_buf(), false, None, reply)) {
        Ok(info) => info,
        Err(refusal) => {
            tally.refused += 1;
            println!("[SKIP] {stem:<26} {}", refusal.reason);
            return;
        }
    };
    tally.opened += 1;

    let mut planned: Vec<PlannedRedaction> = Vec::new();
    let mut gate_pages: Vec<GatePage> = Vec::new();
    let mut here_regions = 0usize;
    let mut here_complete = 0usize;
    let mut here_kinds: BTreeMap<String, usize> = BTreeMap::new();

    for page in sampled(opened.page_count, pages_per_doc) {
        let page = page as u32;
        let Ok(text) = wait(|reply| service.text(opened.id, page, None, reply)) else {
            continue;
        };
        let words = ocr_gate::words_from(&text);
        let candidates: Vec<[f32; 4]> = words
            .iter()
            .filter(|w| w.text.chars().count() >= MIN_WORD)
            // Inflated by a point for `redact_gate_probe`'s reason: a region
            // flush to a glyph box is the boundary `redact::overlaps` is most
            // likely to be wrong at, and this probe is not about that boundary.
            .map(|w| {
                [
                    w.rect[0] - 1.0,
                    w.rect[1] - 1.0,
                    w.rect[2] + 1.0,
                    w.rect[3] + 1.0,
                ]
            })
            .collect();
        let regions: Vec<[f32; 4]> = sampled(candidates.len(), regions_per_page)
            .into_iter()
            .map(|i| candidates[i])
            .collect();
        if regions.is_empty() {
            continue;
        }
        let Ok(plans) =
            wait(|reply| service.redaction_plans(opened.id, page, regions.clone(), reply))
        else {
            continue;
        };

        // Everything the page's sampled regions would remove, merged the way
        // `ask_redactions` merges them --- two regions over one line name the
        // same operator, and removing it twice is removing it once.
        let mut shows: Vec<usize> = Vec::new();
        let mut form_shows: Vec<(usize, usize)> = Vec::new();
        let mut images: Vec<usize> = Vec::new();
        let mut areas: Vec<[f32; 4]> = Vec::new();
        let mut taking: Vec<String> = Vec::new();
        let mut text_objects = 0usize;
        let mut image_objects = 0usize;
        let mut form_text_objects: Vec<(usize, usize)> = Vec::new();
        // Only the regions the removal reports taking whole. Handing the gate
        // an incomplete one would make its verdict unattributable: a reason
        // could be the carrier nobody removed rather than a leak.
        let mut provable: Vec<[f32; 4]> = Vec::new();

        for (plan, region) in plans.iter().zip(regions.iter()) {
            here_regions += 1;
            text_objects = plan.text_objects;
            image_objects = plan.image_objects;
            form_text_objects = plan.form_text_objects.clone();
            if plan.is_complete() {
                here_complete += 1;
                provable.push(if full_width {
                    [0.0, region[1], text.width_pt, region[3]]
                } else {
                    *region
                });
            } else {
                // Attributed as well as named: a form's children are refused
                // by a rule of their own, and a run that folded them into the
                // page's counts could not have found the defect this probe
                // did. `form_text_objects` lists every form on the page by its
                // index, which is the same index `Unhandled::at` carries.
                //
                // Deduplicated on the **composed** key rather than on the kind:
                // a region covering a page path and a form path is one of each,
                // and a `seen` list keyed on "path" alone silently dropped
                // whichever came second in the object order.
                let mut seen: Vec<String> = Vec::new();
                for object in &plan.unhandled {
                    let from_form = plan
                        .form_text_objects
                        .iter()
                        .any(|(at, _)| *at == object.at);
                    let key = if from_form {
                        format!("{} (in a form)", object.kind)
                    } else {
                        object.kind.clone()
                    };
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.push(key.clone());
                    *here_kinds.entry(key).or_default() += 1;
                }
            }
            shows.extend(plan.shows.iter().copied());
            form_shows.extend(plan.form_shows.iter().copied());
            images.extend(plan.images.iter().copied());
            areas.push(plan.area);
            let what = plan.taking.trim();
            if !what.is_empty() {
                taking.push(what.to_string());
            }
        }
        shows.sort_unstable();
        shows.dedup();
        form_shows.sort_unstable();
        form_shows.dedup();
        images.sort_unstable();
        images.dedup();
        if shows.is_empty() && form_shows.is_empty() && images.is_empty() {
            continue;
        }
        if gate && !provable.is_empty() {
            gate_pages.push(GatePage {
                page,
                regions: provable,
                words,
                taking: taking.join(" "),
                width_pt: text.width_pt,
                height_pt: text.height_pt,
            });
        }
        planned.push(PlannedRedaction {
            source: page,
            shows,
            text_objects,
            areas,
            taking,
            form_shows,
            form_text_objects,
            images,
            image_objects,
        });
    }

    tally.regions += here_regions;
    tally.complete += here_complete;
    if here_regions > here_complete {
        tally.docs_incomplete += 1;
    }
    for (kind, count) in &here_kinds {
        *tally.kinds.entry(kind.clone()).or_default() += count;
    }
    println!(
        "       {stem:<26} {here_regions:>5} region(s), {here_complete:>5} taken whole, \
         {} carrier kind(s)",
        here_kinds.len()
    );

    if gate && !planned.is_empty() && !gate_pages.is_empty() {
        run_gate(service, file, &opened, planned, gate_pages, tally);
    }
    let _: Result<(), String> = wait(|reply| service.close(opened.id, reply));
}

/// The buckets [`Tally::unread_px`] is reported in, in the order they print.
///
/// A fixed list rather than the map's own key order, so a bucket that no region
/// fell into prints a zero. An omitted row and a zero row read identically to
/// somebody scanning the output, and only one of them is honest --- the same
/// reason [`NotVerifiedCause::ALL`] exists.
const PX_BUCKETS: [&str; 4] = ["under 8 px", "8 to 12 px", "12 to 16 px", "16 px and over"];

/// Which bucket a control of `px` pixels falls in.
fn px_bucket(px: f32) -> &'static str {
    if px < 8.0 {
        PX_BUCKETS[0]
    } else if px < 12.0 {
        PX_BUCKETS[1]
    } else if px < ocr_gate::MIN_CONTROL_PX {
        PX_BUCKETS[2]
    } else {
        PX_BUCKETS[3]
    }
}

/// How tall the control the gate chose for this page landed, and how long it is.
///
/// Calls the gate's own two functions rather than repeating what they do, which
/// is why [`ocr_gate::geometry_for`] is public at all. `None` means the gate
/// could not have got this far either --- a page with no usable control never
/// reaches a per-region verdict, so a region asking this question always has an
/// answer.
fn control_shape(page: &GatePage) -> Option<(f32, usize)> {
    let survivors = ocr_gate::surviving(&page.words, &page.regions, &page.taking);
    let choice = tpdf_lib::ocr::control_from_page(&survivors, &page.regions).ok()?;
    let geometry = ocr_gate::geometry_for(page, &choice).ok()?;
    Some((geometry.control_px, choice.token.chars().count()))
}

/// The buckets [`Tally::unread_chars`] is reported in, in the order they print.
const CHAR_BUCKETS: [&str; 3] = ["4 characters", "5 to 7 characters", "8 or more"];

/// The three shapes an unread control comes in, from [`tpdf_lib::ocr::Unread`].
///
/// Constants rather than literals at each site, because the tally writes them
/// and the report reads them --- two copies of a string is the drift that turns a
/// row into a permanent zero, which is the reading that says a step stopped
/// failing.
const SHAPE_SILENT: &str = "read nothing at all";
const SHAPE_ABSENT: &str = "read spans, none holding it";
const SHAPE_OUTSIDE: &str = "read it, outside its band";

/// Which bucket a token of `chars` characters falls in.
fn char_bucket(chars: usize) -> &'static str {
    if chars <= tpdf_lib::ocr::MIN_CONTROL_CHARS {
        CHAR_BUCKETS[0]
    } else if chars < 8 {
        CHAR_BUCKETS[1]
    } else {
        CHAR_BUCKETS[2]
    }
}

/// Writes the redacted copy and reads its provable regions back.
fn run_gate(
    service: &RenderService,
    file: &Path,
    opened: &tpdf_lib::render::DocumentInfo,
    redactions: Vec<PlannedRedaction>,
    pages: Vec<GatePage>,
    tally: &mut Tally,
) {
    let out = std::env::temp_dir().join(format!("tpdf-reach-{}.pdf", std::process::id()));
    let plan = Plan {
        opened_as: None,
        baseline: opened.page_count as u32,
        pages: (0..opened.page_count)
            .map(|at| PageView {
                id: at as u64 + 1,
                source: at as u32,
                turns: 0,
                crop: None,
            })
            .collect(),
        marks: Vec::new(),
        redactions,
    };
    if save::write_copy(file, &plan, &out, None).is_err() {
        let _ = std::fs::remove_file(&out);
        return;
    }
    tally.gate_pages += pages.len();
    let asked: Vec<[f32; 4]> = pages.iter().flat_map(|p| p.regions.clone()).collect();
    tally.gate_regions += asked.len();

    // The verdicts, not the sentences. `ocr_gate::reason` throws the engine's
    // own rectangles away, so a probe reading its output could not tell text
    // that survived *inside* the region from a neighbour beside it on the same
    // rows -- which was the whole reason this half's numbers could not be
    // reported as a leak rate before `ocr_gate::mask_columns` existed.
    match ocr_gate::judge_all(service, &out.to_string_lossy(), None, &pages) {
        // Not a per-region cause: the engine would not spawn, or the file would
        // not reopen. Counted as itself rather than spread over the regions,
        // because one of these is a statement about the run.
        Judged::Refused(_) => {
            tally.unanswered += asked.len();
            tally.gate_refused += asked.len();
        }
        Judged::Pages(judged) => {
            let mut at = 0usize;
            for page in &judged {
                match &page.outcome {
                    // One answer for the page, so every region on it is
                    // unanswered rather than one of them being -- and every one
                    // of them is unanswered *for the page's reason*, which the
                    // version that wrote `Whole(_)` here could not say.
                    PageOutcome::Whole(verdict) => {
                        let here = pages
                            .iter()
                            .find(|p| p.page == page.page)
                            .map_or(0, |p| p.regions.len());
                        tally.unanswered += here;
                        if let Legibility::NotVerified { cause, .. } = verdict {
                            *tally.causes.entry(cause.label()).or_default() += here;
                        }
                        at += here;
                    }
                    PageOutcome::Regions(verdicts) => {
                        // The page's control serves every region on it, so the
                        // denominator is counted here once per region -- the
                        // same unit the numerator above is counted in.
                        let shape = pages
                            .iter()
                            .find(|p| p.page == page.page)
                            .and_then(control_shape);
                        for verdict in verdicts {
                            let region = asked.get(at).copied().unwrap_or_default();
                            at += 1;
                            if let Some((_, chars)) = shape {
                                *tally.all_chars.entry(char_bucket(chars)).or_default() += 1;
                            }
                            match verdict {
                                Legibility::Illegible { .. } => tally.proved += 1,
                                Legibility::NotVerified {
                                    cause, evidence, ..
                                } => {
                                    tally.unanswered += 1;
                                    *tally.causes.entry(cause.label()).or_default() += 1;
                                    // Only for this one cause: the others are
                                    // refusals to choose a control at all, so
                                    // there is no rendered height to bucket.
                                    if *cause == NotVerifiedCause::ControlUnread {
                                        // Matched exhaustively rather than by
                                        // an `if let`, so a shape that is
                                        // neither of the three is counted
                                        // rather than dropped into whichever
                                        // arm is written last.
                                        let named = match evidence {
                                            None => {
                                                tally.unread_no_evidence += 1;
                                                None
                                            }
                                            Some(e) if e.items == 0 => {
                                                tally.unread_silent += 1;
                                                Some(SHAPE_SILENT)
                                            }
                                            Some(e) => match e.token_outside_by {
                                                Some(by) => {
                                                    tally.unread_outside += 1;
                                                    tally.outside_by.push(by[1]);
                                                    Some(SHAPE_OUTSIDE)
                                                }
                                                None => {
                                                    tally.unread_absent += 1;
                                                    Some(SHAPE_ABSENT)
                                                }
                                            },
                                        };
                                        // `shape` is the page's, computed once
                                        // above; this used to look it up again
                                        // per region, which is the same value
                                        // by a second route.
                                        if let Some((px, chars)) = shape {
                                            *tally.unread_px.entry(px_bucket(px)).or_default() += 1;
                                            *tally
                                                .unread_chars
                                                .entry(char_bucket(chars))
                                                .or_default() += 1;
                                            if let Some(label) = named {
                                                *tally
                                                    .unread_shape_px
                                                    .entry((label, px_bucket(px)))
                                                    .or_default() += 1;
                                            }
                                        }
                                    }
                                }
                                Legibility::Legible { found } => {
                                    tally.caught += 1;
                                    // Inside the region's own columns, or beside
                                    // it? After masking there should be no
                                    // second kind, and a measurement that takes
                                    // that on trust is not a measurement.
                                    if found.iter().all(|item| {
                                        item.rect[0] < region[2] && region[0] < item.rect[2]
                                    }) {
                                        tally.caught_inside += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let _ = std::fs::remove_file(&out);
}

fn report(t: &Tally, seconds: f32) {
    let pct = |n: usize, d: usize| {
        if d == 0 {
            0.0
        } else {
            100.0 * n as f32 / d as f32
        }
    };
    println!("\n---- what a removal can take -------------------------------------");
    println!("documents opened {}, refused {}", t.opened, t.refused);
    println!("regions asked about {}", t.regions);
    println!(
        "  taken whole                {:>6}  ({:.1}%)",
        t.complete,
        pct(t.complete, t.regions)
    );
    let incomplete = t.regions - t.complete;
    println!(
        "  holding something it cannot take {:>6}  ({:.1}%), in {} document(s)",
        incomplete,
        pct(incomplete, t.regions),
        t.docs_incomplete
    );
    for (kind, count) in &t.kinds {
        println!(
            "      {kind:<12} {count:>6}  ({:.1}% of all regions)",
            pct(*count, t.regions)
        );
    }

    println!("\n---- what the gate caught ----------------------------------------");
    if t.gate_regions == 0 {
        println!("the gate was not run");
    } else {
        println!(
            "regions read back {} on {} page(s)",
            t.gate_regions, t.gate_pages
        );
        println!(
            "  still reads as text               {:>6}  ({:.2}%)",
            t.caught,
            pct(t.caught, t.gate_regions)
        );
        println!(
            "      of those, every span inside the region's own columns: {}",
            t.caught_inside
        );
        println!(
            "  could not be shown unreadable     {:>6}  ({:.2}%)",
            t.unanswered,
            pct(t.unanswered, t.gate_regions)
        );
        // Every cause, including the ones that never fired: a row printed as
        // zero says the step was watched, and an absent row says nothing at
        // all. `refused` is listed beside them and is not one of them.
        for cause in NotVerifiedCause::ALL {
            let label = cause.label();
            println!(
                "      {label:<32} {:>6}",
                t.causes.get(label).copied().unwrap_or(0)
            );
        }
        println!("      {:<32} {:>6}", "the run was refused", t.gate_refused);
        // How tall the control landed for the one cause where the gate got as
        // far as showing the engine something. The bound is the gate's own, so
        // a bucket below it is the scale rule missing what it aims at.
        let unread = t
            .causes
            .get(NotVerifiedCause::ControlUnread.label())
            .copied()
            .unwrap_or(0);
        if unread > 0 {
            println!(
                "  of the {unread} control-not-read-back, the control rendered at (floor is \
                 {:.0} px)",
                ocr_gate::MIN_CONTROL_PX
            );
            for bucket in PX_BUCKETS {
                println!(
                    "      {bucket:<32} {:>6}",
                    t.unread_px.get(bucket).copied().unwrap_or(0)
                );
            }
            println!("  and the control token drew (unread / all, and the rate)");
            for bucket in CHAR_BUCKETS {
                let bad = t.unread_chars.get(bucket).copied().unwrap_or(0);
                let all = t.all_chars.get(bucket).copied().unwrap_or(0);
                println!(
                    "      {bucket:<32} {bad:>6} /{all:>6}   {:.1}%",
                    pct(bad, all)
                );
            }
            // What the engine had returned when it happened, which is the one
            // reading that names a *place* rather than a property of the
            // control. Printed even at zero, for the reason every cause row is.
            println!("  and of those, what the engine had returned");
            for (label, count) in [
                (SHAPE_SILENT, t.unread_silent),
                (SHAPE_ABSENT, t.unread_absent),
                (SHAPE_OUTSIDE, t.unread_outside),
            ] {
                println!("      {label:<32} {count:>6}   {:.1}%", pct(count, unread));
                // The shape's own split by rendered height. Without it the two
                // rows are marginals, which bound the overlap and cannot
                // measure it -- and the overlap is what says whether the work
                // left is the scale or the image.
                for bucket in PX_BUCKETS {
                    let n = t
                        .unread_shape_px
                        .get(&(label, bucket))
                        .copied()
                        .unwrap_or(0);
                    if n > 0 {
                        println!("          {bucket:<28} {n:>6}");
                    }
                }
            }
            // Every shape row above has to account for every region that had a
            // control to measure. A region with no shape is already named by
            // the evidence `[WARN]`; one with no height is named by the bucket
            // `[WARN]` below. This catches the pair going out of step.
            let crossed: usize = t.unread_shape_px.values().sum();
            let bucket_total: usize = t.unread_px.values().sum();
            if crossed != bucket_total {
                println!(
                    "[WARN] {crossed} cross-tabulated of {bucket_total} with a measured control \
                     --- the two axes disagree about the same regions"
                );
            }
            if !t.outside_by.is_empty() {
                let mut by = t.outside_by.clone();
                by.sort_by(f32::total_cmp);
                let median = by[by.len() / 2];
                println!(
                    "      of those, above the band by: median {median:.1} pt, worst {:.1} pt",
                    by[by.len() - 1]
                );
            }
            // The type says a `ControlUnread` verdict always carries evidence,
            // so this is the arithmetic control on the three rows rather than a
            // fourth reading. Silence is the healthy state.
            if t.unread_no_evidence > 0 {
                println!(
                    "[WARN] {} unread verdict(s) carried no evidence --- adjudicate has a path \
                     that does not record one",
                    t.unread_no_evidence
                );
            }
            let shapes = t.unread_silent + t.unread_absent + t.unread_outside;
            if shapes + t.unread_no_evidence != unread {
                println!(
                    "[WARN] {shapes} shape(s) for {unread} unread control(s) --- this probe has \
                     lost some"
                );
            }
            let bucketed: usize = t.unread_px.values().sum();
            if bucketed != unread {
                println!(
                    "[WARN] {bucketed} bucketed of {unread} --- {} had no control to measure",
                    unread - bucketed
                );
            }
        }
        // The buckets have to account for the unanswered exactly. A region can
        // reach `unanswered` by a route that carries no cause -- which is what
        // `Whole(_)` and `Refused(_)` were before this -- and the shortfall is
        // invisible unless it is subtracted.
        let attributed: usize = t.causes.values().sum::<usize>() + t.gate_refused;
        if attributed != t.unanswered {
            println!(
                "[WARN] {attributed} attributed of {} unanswered --- {} region(s) have no cause",
                t.unanswered,
                t.unanswered.saturating_sub(attributed)
            );
        }
        println!(
            "  shown unreadable                  {:>6}  ({:.2}%)",
            t.proved,
            pct(t.proved, t.gate_regions)
        );
        // Every region has exactly one verdict, and three counters is the shape
        // a miscount hides in. The `[WARN]` is the check: a probe whose own
        // arithmetic does not close cannot be quoted.
        let counted = t.caught + t.unanswered + t.proved;
        if counted != t.gate_regions {
            println!(
                "[WARN] {counted} verdicts for {} regions --- this probe has lost some",
                t.gate_regions
            );
        }
    }
    println!("\nran in {seconds:.1}s");
}

fn wait<T: Send + 'static, E: Send + 'static + From<String>>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, E>) + Send>),
) -> Result<T, E> {
    let (tx, rx) = std::sync::mpsc::channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    match rx.recv_timeout(std::time::Duration::from_secs(300)) {
        Ok(result) => result,
        Err(_) => Err(E::from("the render service did not answer".to_string())),
    }
}
