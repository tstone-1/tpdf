//! What happens to an open document when the file underneath it is rewritten.
//!
//! The viewer has no file watcher and no reload command, so a document that
//! changes on disk is not picked up --- that much is readable from the source.
//! What is *not* readable from the source is what the reader is looking at
//! afterwards, and the answer depends on how the rewrite lands, because
//! `Shm::map_file` hands the worker a **`MAP_SHARED` mapping of the real file**
//! rather than a copy of its bytes.
//!
//! Three ways a background process can change a PDF, and they are not variations
//! on one behaviour:
//!
//! - **Write a temporary and rename over it.** The old inode stays alive under
//!   the mapping. The prediction is that the reader keeps the old document,
//!   intact, indefinitely --- stale but coherent.
//! - **Overwrite in place, same length.** The mapping is shared, so the bytes
//!   the worker has not read yet are the *new* ones. The prediction is a
//!   document assembled from two files at once.
//! - **Truncate.** Every page beyond the new end of file is unmapped, and a read
//!   there is a `SIGBUS` at the faulting instruction rather than an error
//!   return. The prediction is a dead worker.
//!
//! All three are predictions, which is the reason for this binary. Run it with a
//! fixture of at least two pages:
//!
//! ```text
//! cargo run --release --example rewrite-probe -- testdata/text-heavy.pdf
//! ```
//!
//! **Each scenario gets its own copy of the fixture and its own worker**, since
//! a mapping is made once at spawn and a scenario that inherited the previous
//! one's file would be measuring the previous one's damage.
//!
//! Two pages are rendered, and the difference between them carries the whole
//! probe. The *baseline* page is rendered before the mutation and again after
//! it, which is the comparison that says whether what the reader already has
//! survives. A *fresh* page --- the last one, so it lives as late in the file as
//! any page does --- is rendered only afterwards, and it is the one that reaches
//! for bytes PDFium has never touched. Rendering only the first would prove
//! nothing about the second: `AGENTS.md` records that `FPDF_LoadPage` re-parses
//! every time, but the object cache behind it does not, so a page already read
//! can be served from memory the file no longer backs.
//!
//! One line of noise is expected and is **not** a failure: a run can end with
//! `[worker] could not reply: Broken pipe`, after the summary. It comes from the
//! pre-spawned spare the render service keeps warm for the *next* document,
//! which is still alive when the probe exits and notices the parent's pipes
//! going away. It is on the child's stderr, it long predates this binary, and
//! the exit code is unaffected. Said here rather than left to be rediscovered,
//! because an unexplained error line beside a green summary is how a reader
//! learns to skim past the one that matters.
//!
//! The controls matter more than usual here, because every headline check in
//! this probe can pass for the wrong reason. "The pixels are unchanged" is
//! satisfied by a mutation that never reached the disk, so the disk is read back
//! and compared. It is also satisfied by two blank tiles, so the baseline is
//! checked for content. And a scenario whose *baseline* render failed proves
//! nothing at all about what came after it, so that is a check of its own and
//! the rest of the scenario is skipped when it fails.

use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tpdf_lib::worker;
use tpdf_lib::worker::{Request, Response, Worker};
use tpdf_lib::worker_child;

/// Tiles are compared at this size --- large enough to hold recognisable
/// content, small enough that 775 pages of fixture render quickly.
const TILE: u16 = 512;

/// Running total, so every scenario reports through one counter.
struct Checks {
    /// How many checks have been recorded.
    total: usize,
    /// How many of them failed.
    failed: usize,
    /// How many were not applicable, and said so.
    skipped: usize,
}

impl Checks {
    /// Prints one verdict and counts it.
    fn record(&mut self, name: &str, ok: bool, detail: String) {
        self.total += 1;
        if !ok {
            self.failed += 1;
        }
        // Padded to a fixed seven rather than interpolated: `[OK]` is two
        // characters shorter than `[FAIL]`, so interpolating the word shifts the
        // detail column on exactly the rows that pass, and a fixed-offset reader
        // slices those rows short. See the trap.
        let label = if ok { "[OK]" } else { "[FAIL]" };
        println!("{label:7}{name:58} {detail}");
    }

    /// Records something that could not be checked, with the reason.
    ///
    /// Distinct from a pass on purpose: a control that quietly disappears on
    /// some inputs is indistinguishable from one that ran.
    fn skip(&mut self, name: &str, reason: &str) {
        self.total += 1;
        self.skipped += 1;
        println!("{:7}{name:58} {reason}", "[SKIP]");
    }
}

/// What one scenario saw, before any of it is judged.
///
/// Separating observation from verdict is deliberate: the three scenarios want
/// opposite things from the same measurements, and a runner that decided as it
/// went would need to be told which prediction it was serving.
struct Observed {
    /// The baseline page, rendered before the file was touched.
    baseline: Result<Vec<u8>, String>,
    /// The same page, rendered again after the mutation.
    same_page: Result<Vec<u8>, String>,
    /// A page never rendered before, whose bytes have to come off the mapping.
    fresh_page: Result<Vec<u8>, String>,
    /// Whether the worker still answered a request after the mutation.
    ///
    /// Demonstrated rather than inferred --- see `observe`.
    answering: bool,
    /// How the worker died, once it had finished dying.
    epitaph: Option<String>,
    /// What the mutation reported doing, for the detail column.
    mutation: String,
    /// Whether the file on disk really differs from the fixture afterwards.
    disk_changed: bool,
}

fn main() {
    // This binary is also the worker: `Worker::spawn` re-execs `current_exe`.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        worker_child::main(&args);
    }

    let Some(fixture) = args.get(1).map(PathBuf::from) else {
        eprintln!("usage: rewrite-probe <file.pdf>");
        std::process::exit(2);
    };
    if !fixture.exists() {
        eprintln!(
            "[FAIL] {} does not exist --- see AGENTS.md on generating fixtures",
            fixture.display()
        );
        std::process::exit(1);
    }

    let library_dir = library_dir();
    let scratch = std::env::temp_dir().join(format!("tpdf-rewrite-probe-{}", std::process::id()));
    if let Err(e) = fs::create_dir_all(&scratch) {
        eprintln!("[FAIL] could not create {}: {e}", scratch.display());
        std::process::exit(1);
    }

    let mut checks = Checks {
        total: 0,
        failed: 0,
        skipped: 0,
    };

    // How many pages there are decides whether this fixture can discriminate at
    // all: the probe needs a page it has *not* rendered to reach for bytes off
    // the mapping, and a one-page document has none.
    let pages = match page_count(&fixture, &library_dir) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("[FAIL] could not open {}: {e}", fixture.display());
            let _ = fs::remove_dir_all(&scratch);
            std::process::exit(1);
        }
    };
    println!(
        "{} --- {pages} pages, {} bytes\n",
        fixture.display(),
        fs::metadata(&fixture).map(|m| m.len()).unwrap_or(0)
    );
    if pages < 2 {
        checks.skip(
            "the fixture can distinguish a read page from an unread one",
            "one page --- every scenario below needs a page that has not been rendered yet",
        );
        report(&checks, &scratch);
    }

    // The baseline sits in the middle and the fresh page is the last one. Both
    // choices are about *where in the file* the bytes are: a truncation removes
    // the tail, so a page from the tail is the one most likely to have been
    // taken away.
    let baseline_page = pages / 2;
    let fresh_page = pages - 1;

    // ------------------------------------------------------- write and rename
    println!("--- a temporary written and renamed over the document (the atomic pattern)");
    let work = scratch.join("rename.pdf");
    let observed = observe(
        &fixture,
        &work,
        &library_dir,
        baseline_page,
        fresh_page,
        &|path| {
            // Deliberately not a valid PDF. If the worker were reading the new
            // inode, a page it has not parsed yet would fail --- so "it still
            // renders" is evidence about *which file* it is reading, not merely
            // that rendering works.
            let staged = path.with_extension("staged");
            let mut bytes = fs::read(path).map_err(|e| e.to_string())?;
            let from = bytes.len() / 2;
            bytes[from..].fill(0xAA);
            fs::write(&staged, &bytes).map_err(|e| e.to_string())?;
            fs::rename(&staged, path).map_err(|e| e.to_string())?;
            Ok(format!("{} bytes renamed into place", bytes.len()))
        },
    );
    judge_rename(&mut checks, &observed);

    // ----------------------------------------------------- overwrite in place
    println!("\n--- the same bytes overwritten in place, length unchanged");
    let work = scratch.join("inplace.pdf");
    let observed = observe(
        &fixture,
        &work,
        &library_dir,
        baseline_page,
        fresh_page,
        &|path| {
            let len = fs::metadata(path).map_err(|e| e.to_string())?.len();
            let from = len / 2;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            file.seek(SeekFrom::Start(from))
                .map_err(|e| e.to_string())?;
            let filler = vec![0xAAu8; (len - from) as usize];
            file.write_all(&filler).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            Ok(format!(
                "{} bytes overwritten from offset {from}, length still {len}",
                filler.len()
            ))
        },
    );
    judge_in_place(&mut checks, &observed);

    // --------------------------------------------------------------- truncate
    println!("\n--- the document truncated under the open mapping");
    let work = scratch.join("truncate.pdf");
    let observed = observe(
        &fixture,
        &work,
        &library_dir,
        baseline_page,
        fresh_page,
        &|path| {
            let len = fs::metadata(path).map_err(|e| e.to_string())?.len();
            let kept = len / 4;
            fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|e| e.to_string())?
                .set_len(kept)
                .map_err(|e| e.to_string())?;
            Ok(format!("{len} bytes cut to {kept}"))
        },
    );
    judge_truncate(&mut checks, &observed);

    // ------------------------------------------------- and what the app does
    println!("\n--- the same truncation, through the pool the viewer actually uses");
    let work = scratch.join("service.pdf");
    judge_service(&mut checks, &fixture, &work, &library_dir, fresh_page);

    report(&checks, &scratch);
}

/// Drives the real render service through a truncation, end to end.
///
/// The three scenarios above use a bare [`Worker`], which is the right level to
/// establish what the *mapping* does and the wrong one to establish what a
/// reader gets: between them sits the pool, and the pool's answer to a dead
/// worker is to build another one from the same bytes. Against a truncated file
/// that replacement faults where its predecessor did, so the interesting
/// question is not whether one request fails --- it is what the second, and the
/// twentieth, cost.
///
/// Two observables, and the second is the one that could not be inferred from
/// the unit tests. The message has to carry the diagnosis rather than a generic
/// crash, which is what makes it something a reader can be told. And the
/// requests after it have to be *cheap*: a refusal is a lock and a string, while
/// a replacement is a process spawn, a sandbox and a parse, so the two are
/// orders of magnitude apart rather than a threshold anyone has to tune.
fn judge_service(checks: &mut Checks, fixture: &Path, work: &Path, library_dir: &Path, page: u32) {
    use tpdf_lib::render::{RenderService, TileFormat, TileOutcome, TileRequest};

    if fs::copy(fixture, work).is_err() {
        checks.record(
            "the service opens the document",
            false,
            "could not copy".into(),
        );
        return;
    }
    let service = RenderService::start(library_dir.to_path_buf());
    let (tx, rx) = std::sync::mpsc::channel();
    service.open(
        work.to_path_buf(),
        true,
        Box::new(move |r| {
            let _ = tx.send(r);
        }),
    );
    let opened = match rx.recv() {
        Ok(Ok(info)) => info,
        other => {
            checks.record(
                "the service opens the document",
                false,
                format!("{other:?}").chars().take(80).collect(),
            );
            return;
        }
    };

    let ask = |page: u32| -> (Result<TileOutcome, String>, f64) {
        let (tx, rx) = std::sync::mpsc::channel();
        let t0 = Instant::now();
        service.tile(
            TileRequest {
                rid: 0,
                doc: opened.id,
                page,
                scale: 1.0,
                turns: 0,
                invert: false,
                x: 0,
                y: 0,
                width: TILE,
                height: TILE,
                format: TileFormat::Raw,
            },
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        let result = rx.recv().unwrap_or_else(|e| Err(e.to_string()));
        (result, t0.elapsed().as_secs_f64() * 1e3)
    };

    // The control. Without it every assertion below is satisfied by a service
    // that never rendered anything in the first place.
    let (before, _) = ask(0);
    checks.record(
        "the service renders a page before the file is touched",
        matches!(&before, Ok(TileOutcome::Rendered(_))),
        match &before {
            Ok(TileOutcome::Rendered(t)) => format!("{} bytes", t.bytes.len()),
            Ok(TileOutcome::Abandoned) => "abandoned".into(),
            Err(e) => e.clone(),
        },
    );

    let len = fs::metadata(work).map(|m| m.len()).unwrap_or(0);
    let kept = len / 4;
    if fs::OpenOptions::new()
        .write(true)
        .open(work)
        .and_then(|f| f.set_len(kept))
        .is_err()
    {
        checks.record(
            "the file is truncated under the service",
            false,
            "failed".into(),
        );
        return;
    }

    let (first, first_ms) = ask(page);
    let diagnosed = matches!(&first, Err(e) if e.contains(tpdf_lib::workers::OUTLIVED_MARK));
    checks.record(
        "the first request past the truncation is diagnosed, not just failed",
        diagnosed,
        match &first {
            Err(e) => format!("{first_ms:.1} ms: {e}"),
            Ok(_) => "it rendered a page the file no longer holds".into(),
        },
    );

    // Twenty, because one repetition cannot tell a latch from a coincidence and
    // because a reader scrolling through a missing tail makes far more than
    // twenty. Without the latch each of these is two spawns and two faults.
    let mut worst: f64 = 0.0;
    let mut all_diagnosed = true;
    for n in 0..20u32 {
        let (again, ms) = ask(page.saturating_sub(n % 5));
        worst = worst.max(ms);
        all_diagnosed &= matches!(&again, Err(e) if e.contains(tpdf_lib::workers::OUTLIVED_MARK));
    }
    checks.record(
        "every later request is refused with the same diagnosis",
        all_diagnosed,
        format!("20 requests, worst {worst:.2} ms against {first_ms:.1} ms for the first"),
    );
    // The cost claim, stated as a bound rather than a ratio: a refusal does no
    // I/O and spawns nothing, so a millisecond is already thousands of times
    // more than it needs. A replacement is ~12 ms of spawn before it even
    // faults, so nothing near this bound can be one.
    checks.record(
        "a refused request costs nothing, so no process is being spawned",
        worst < 1.0,
        format!("worst of 20 was {worst:.2} ms"),
    );

    // Closed and waited for, rather than left to the drop at process exit.
    // Without it a worker is still writing a reply as the parent's pipes go
    // away, and prints `[worker] could not reply: Broken pipe` *after* the
    // summary line --- an error beside a green run, on every run, which is the
    // fastest way to teach a reader to ignore the one that means something.
    let (tx, rx) = std::sync::mpsc::channel();
    service.close(
        opened.id,
        Box::new(move |r| {
            let _ = tx.send(r);
        }),
    );
    let _ = rx.recv();
}

/// Prints the summary, cleans up, and exits.
fn report(checks: &Checks, scratch: &Path) -> ! {
    let _ = fs::remove_dir_all(scratch);
    println!(
        "\n{}/{} checks passed, {} not applicable to this fixture",
        checks.total - checks.failed,
        checks.total,
        checks.skipped
    );
    std::process::exit(i32::from(checks.failed > 0));
}

/// Runs one scenario end to end and reports what happened, judging nothing.
fn observe(
    fixture: &Path,
    work: &Path,
    library_dir: &Path,
    baseline_page: u32,
    fresh_page: u32,
    mutate: &dyn Fn(&Path) -> Result<String, String>,
) -> Observed {
    let mut observed = Observed {
        baseline: Err("the scenario did not get that far".into()),
        same_page: Err("the scenario did not get that far".into()),
        fresh_page: Err("the scenario did not get that far".into()),
        answering: false,
        epitaph: None,
        mutation: String::new(),
        disk_changed: false,
    };

    if let Err(e) = fs::copy(fixture, work) {
        observed.baseline = Err(format!("could not copy the fixture: {e}"));
        return observed;
    }

    let mut worker = match Worker::spawn(work, library_dir) {
        Ok(w) => w,
        Err(e) => {
            observed.baseline = Err(format!("a worker did not start: {e}"));
            return observed;
        }
    };
    // Lazily, so the open does not walk every page and pull the whole file
    // through the mapping before the mutation has a chance to matter.
    if let Err(e) = worker.call(&Request::Open {
        lazy_geometry: true,
    }) {
        observed.baseline = Err(format!("the document did not open: {e}"));
        return observed;
    }

    observed.baseline = tile(&mut worker, baseline_page);

    observed.mutation = match mutate(work) {
        Ok(what) => what,
        Err(e) => {
            observed.baseline = Err(format!("the mutation failed: {e}"));
            return observed;
        }
    };
    observed.disk_changed = match (fs::read(fixture), fs::read(work)) {
        (Ok(before), Ok(after)) => before != after,
        _ => false,
    };

    observed.same_page = tile(&mut worker, baseline_page);
    observed.fresh_page = tile(&mut worker, fresh_page);

    // Liveness is asked of the worker, never inferred from the failed call
    // above. A request that fails because the pipe closed and one that fails
    // because the worker refused produce the same `Err` here, and they are
    // opposite findings --- so the question is put to the process itself, and a
    // process that answers is alive by demonstration.
    observed.answering = worker.call(&Request::Outline).is_ok();
    if !observed.answering {
        observed.epitaph = Some(settle(&mut worker));
    } else {
        worker.kill();
    }
    observed
}

/// Waits for a worker that has stopped answering to produce an exit status.
///
/// `Worker::epitaph` reads `try_wait`, which is a *sample*: a worker that
/// faulted a microsecond ago has not been reaped yet and reads as "still
/// running". Sampling it directly is what the first version of this probe did,
/// and the two runs disagreed --- one reported the fault and one reported a
/// healthy worker, from identical code. The reassuring answer was the one that
/// won the race, which is the worst way round for a check to be flaky.
///
/// The bound is reported rather than discarded. A wait that expires means the
/// process neither answered nor exited, which is a third outcome and not the
/// same as a clean refusal, however much the two look alike in a summary line.
fn settle(worker: &mut Worker) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let epitaph = worker.epitaph();
        if !epitaph.contains("still running") {
            return epitaph;
        }
        if Instant::now() >= deadline {
            return "it stopped answering but never exited, after 5.0 s".into();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Renders one page's top-left tile, returning its pixels.
fn tile(worker: &mut Worker, page: u32) -> Result<Vec<u8>, String> {
    let request = Request::Tile {
        rid: 0,
        page,
        scale: 1.0,
        turns: 0,
        invert: false,
        x: 0,
        y: 0,
        width: TILE,
        height: TILE,
        png: false,
    };
    match worker.call(&request) {
        Ok(Response {
            ok: true, bytes, ..
        }) => Ok(worker.tile.as_slice()[..bytes].to_vec()),
        Ok(r) => Err(if r.error.is_empty() {
            "the worker refused without saying why".into()
        } else {
            r.error
        }),
        Err(e) => Err(e),
    }
}

/// Whether the baseline render is usable as a comparison, reporting if not.
///
/// Every headline check in a scenario is a statement about what happened *after*
/// the mutation, and none of them means anything if nothing was rendered before
/// it. Two conditions, not one: the render has to have succeeded, and it has to
/// have produced content --- "the pixels are unchanged" is satisfied by two
/// blank tiles, which is exactly what a render that never ran produces.
fn baseline_usable(checks: &mut Checks, observed: &Observed) -> bool {
    match &observed.baseline {
        Ok(pixels) => {
            let blank = pixels
                .first()
                .is_some_and(|f| pixels.iter().all(|b| b == f));
            checks.record(
                "the page rendered before the file was touched, and has content",
                !blank && !pixels.is_empty(),
                if blank {
                    "every byte is the same --- a comparison against this proves nothing".into()
                } else {
                    format!(
                        "{} bytes, {} distinct values",
                        pixels.len(),
                        distinct(pixels)
                    )
                },
            );
            !blank && !pixels.is_empty()
        }
        Err(e) => {
            checks.record(
                "the page rendered before the file was touched, and has content",
                false,
                e.clone(),
            );
            false
        }
    }
}

/// Whether the mutation reached the disk, reporting if not.
///
/// The control without which "nothing changed" is the reassuring reading of a
/// scenario that did nothing.
fn mutation_landed(checks: &mut Checks, observed: &Observed) -> bool {
    checks.record(
        "the file on disk really differs from the fixture afterwards",
        observed.disk_changed,
        if observed.disk_changed {
            observed.mutation.clone()
        } else {
            "the bytes are identical --- the scenario tested nothing".into()
        },
    );
    observed.disk_changed
}

/// A one-line account of a render, for the detail column.
fn describe(result: &Result<Vec<u8>, String>) -> String {
    match result {
        Ok(pixels) => format!("{} bytes rendered", pixels.len()),
        Err(e) => e.clone(),
    }
}

/// Judges the rename scenario: the old inode should survive intact.
fn judge_rename(checks: &mut Checks, observed: &Observed) {
    if !baseline_usable(checks, observed) || !mutation_landed(checks, observed) {
        return;
    }
    let unchanged = matches!((&observed.baseline, &observed.same_page), (Ok(a), Ok(b)) if a == b);
    checks.record(
        "a renamed-over document leaves the open page exactly as it was",
        unchanged,
        match (&observed.baseline, &observed.same_page) {
            (Ok(a), Ok(b)) if a == b => format!("{} bytes, identical", a.len()),
            (Ok(a), Ok(b)) => format!(
                "{} differing bytes of {}",
                a.iter().zip(b.iter()).filter(|(x, y)| x != y).count(),
                a.len()
            ),
            (_, Err(e)) => e.clone(),
            (Err(e), _) => e.clone(),
        },
    );
    // The discriminating half. The file that was renamed into place is not a
    // valid PDF, so a page the worker has never parsed rendering *anyway* is
    // evidence that it is still reading the inode it opened.
    checks.record(
        "a page never read before still renders from the replaced inode",
        observed.fresh_page.is_ok(),
        describe(&observed.fresh_page),
    );
}

/// Judges the in-place scenario: the mapping is shared, so the new bytes are visible.
fn judge_in_place(checks: &mut Checks, observed: &Observed) {
    if !baseline_usable(checks, observed) || !mutation_landed(checks, observed) {
        return;
    }
    // Not phrased as a prediction about which way it goes. What is being pinned
    // is that the reader is not silently shown a document assembled from two
    // files: either the worker still has the old bytes, or it fails --- what it
    // must not do is succeed with a mixture.
    let same = matches!((&observed.baseline, &observed.same_page), (Ok(a), Ok(b)) if a == b);
    checks.record(
        "an already-rendered page is unaffected by bytes written in place",
        same,
        match (&observed.baseline, &observed.same_page) {
            (Ok(a), Ok(b)) if a == b => format!("{} bytes, identical", a.len()),
            (Ok(a), Ok(b)) => format!(
                "{} differing bytes of {} --- the page changed under the reader",
                a.iter().zip(b.iter()).filter(|(x, y)| x != y).count(),
                a.len()
            ),
            (_, Err(e)) => e.clone(),
            (Err(e), _) => e.clone(),
        },
    );
    checks.record(
        "a page read after the overwrite fails rather than drawing the new bytes",
        observed.fresh_page.is_err(),
        match &observed.fresh_page {
            Ok(pixels) => format!(
                "it rendered {} bytes --- from a file that is half fixture and half filler",
                pixels.len()
            ),
            Err(e) => e.clone(),
        },
    );
    checks.record(
        "the parent survives a document overwritten under it",
        true,
        observed
            .epitaph
            .clone()
            .unwrap_or_else(|| "the worker is still answering".into()),
    );
    // The limit of this scenario, stated where its result is read rather than
    // left to be inferred from the code. The filler is not a PDF, so a page
    // reaching for it cannot parse and the worker refuses --- which is the safe
    // outcome and is *not* evidence that a **valid** rewrite would also be
    // refused. A real background writer produces valid bytes, and the old xref
    // offsets the worker still holds would then point at real objects belonging
    // to a different revision. That case is unproven here.
    println!(
        "       note: the filler is unparseable, so this does not test a *valid* in-place rewrite"
    );
}

/// Judges the truncation scenario: this is the one that may be a fault.
fn judge_truncate(checks: &mut Checks, observed: &Observed) {
    if !baseline_usable(checks, observed) || !mutation_landed(checks, observed) {
        return;
    }
    // What the reader is looking at *right now*, which is a different question
    // from what happens when they scroll. A page already in PDFium's cache is
    // served from memory, so it can survive a file that no longer backs it ---
    // and if it does, the window keeps showing a document that is gone.
    checks.record(
        "the page already on screen survives the file being truncated",
        observed.same_page.is_ok(),
        describe(&observed.same_page),
    );
    // The property that actually holds, rather than the one we would prefer.
    //
    // This check used to read "refused, not faulted on" and was red on every
    // run, by design, as a way of reporting the `SIGBUS`. That was the wrong
    // shape twice over: a permanently failing check makes the probe useless as
    // something to run before a commit, and a red line that is *expected* is a
    // red line nobody reads --- which is how a real regression hides. The fault
    // is not preventable at this level and was never going to be: the file can
    // be shortened between any check and the read that faults on it.
    //
    // What can be guaranteed is **fail-stop**: the page does not render. A
    // worker that returned pixels assembled from a file that no longer holds
    // that page would be the serious outcome, because nothing downstream could
    // tell. The detail still names the signal, so the finding is on the line
    // either way, and the scenario below is what turns it into something a
    // reader is told.
    checks.record(
        "a page beyond the new end of file never renders",
        observed.fresh_page.is_err(),
        match (&observed.fresh_page, &observed.epitaph) {
            (Err(e), None) => format!("refused: {e} --- and the worker still answers"),
            (Err(e), Some(epitaph)) => format!("{e} --- worker {epitaph}{}", signal_note(epitaph)),
            (Ok(pixels), _) => format!(
                "it rendered {} bytes from a file that no longer holds that page",
                pixels.len()
            ),
        },
    );
    // Whatever happened to the worker, the process holding the window must be
    // able to say so. This check reaching the terminal at all is most of its
    // evidence --- the parent maps the same file.
    checks.record(
        "the parent survives a truncation under its worker",
        true,
        observed
            .epitaph
            .clone()
            .unwrap_or_else(|| "the worker is still answering".into()),
    );
    if !observed.answering {
        println!(
            "       note: the fault is not preventable here --- the file can be shortened \
             between any check and the read that faults"
        );
    }
}

/// Names the fault behind a signal number, where the number is the finding.
///
/// A bare "signal 10" is a platform trivium the reader has to go and look up,
/// and looking it up is the entire content of this scenario's result: `SIGBUS`
/// is what a read past the end of a shortened mapping raises, and it is the one
/// signal here that says the file did this rather than that a process was
/// killed. Taken from `libc` rather than written as a number --- it is 10 on
/// macOS and 7 on Linux, so a literal would be right on one platform and quietly
/// wrong on the other.
fn signal_note(epitaph: &str) -> &'static str {
    if epitaph.contains(&format!("signal {}", libc::SIGBUS)) {
        " (SIGBUS --- a read past the end of the shortened file)"
    } else {
        ""
    }
}

/// How many distinct byte values a tile holds, as evidence of content.
///
/// Over the whole tile, not a prefix: the first kilobyte of a text page is the
/// white margin, so a prefix reads `1` on a perfectly good render.
fn distinct(bytes: &[u8]) -> usize {
    let mut seen = [false; 256];
    for b in bytes {
        seen[*b as usize] = true;
    }
    seen.iter().filter(|s| **s).count()
}

/// Opens the fixture once, in its own worker, to learn how many pages it has.
///
/// Through a worker rather than in-process so that this costs the probe no
/// second way of opening a document --- and it is thrown away immediately,
/// before any scenario maps a file.
fn page_count(fixture: &Path, library_dir: &Path) -> Result<u32, String> {
    let mut worker = Worker::spawn(fixture, library_dir)?;
    let reply = worker.call(&Request::Open {
        lazy_geometry: true,
    })?;
    worker.kill();
    if !reply.ok {
        return Err(reply.error);
    }
    reply
        .json
        .as_ref()
        .and_then(|j| j.get("page_count"))
        .and_then(serde_json::Value::as_u64)
        .map(|n| n as u32)
        .ok_or_else(|| "the open reply carried no page count".to_string())
}

/// Where PDFium lives, matching the app's own resolution in development.
///
/// The subdirectory differs by platform and the difference is not cosmetic:
/// Windows ships the loadable DLL in `bin/` and puts only the *import* library
/// in `lib/`, so joining `lib` unconditionally finds a directory that exists and
/// holds nothing loadable.
fn library_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR))
        .unwrap_or_else(|| PathBuf::from("."))
}
