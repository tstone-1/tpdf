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
use tpdf_lib::ocr::Legibility;
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
    /// Of those, the ones where the **control** was not read back --- the engine
    /// ran and could not read text of that size on that image, so its finding
    /// nothing else says nothing. Counted apart because it is the cost of
    /// masking: a mostly-blank probe image is a different image to recognise.
    no_control: usize,
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
    if save::write_copy(file, &plan, &out).is_err() {
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
        Judged::Refused(_) => tally.unanswered += asked.len(),
        Judged::Pages(judged) => {
            let mut at = 0usize;
            for page in &judged {
                match &page.outcome {
                    // One answer for the page, so every region on it is
                    // unanswered rather than one of them being.
                    PageOutcome::Whole(_) => {
                        let here = pages
                            .iter()
                            .find(|p| p.page == page.page)
                            .map_or(0, |p| p.regions.len());
                        tally.unanswered += here;
                        at += here;
                    }
                    PageOutcome::Regions(verdicts) => {
                        for verdict in verdicts {
                            let region = asked.get(at).copied().unwrap_or_default();
                            at += 1;
                            match verdict {
                                Legibility::Illegible { .. } => tally.proved += 1,
                                Legibility::NotVerified { why } => {
                                    tally.unanswered += 1;
                                    if why.contains("control token") {
                                        tally.no_control += 1;
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
        println!(
            "      of those, the control was not read back: {}",
            t.no_control
        );
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
