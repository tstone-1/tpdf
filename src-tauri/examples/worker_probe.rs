//! Proves the worker boundary is transparent, and that it is actually a boundary.
//!
//! Two claims, and they need different evidence. That the worker *works* is
//! shown by comparing it against the in-process renderer --- and the comparison
//! has to be on **pixels**, because `AGENTS.md` records a sandboxed PDFium
//! returning `ok` while drawing a different typeface with about the same amount
//! of ink. That it is a *boundary* cannot be shown that way at all: a worker
//! whose sandbox was never applied renders identically, so the containment has
//! to be provoked rather than inferred.
//!
//! Run with a fixture:
//!
//! ```text
//! cargo run --release --example worker-probe -- testdata/text-heavy.pdf
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;
use tpdf_lib::document::OpenDocument;

use tpdf_lib::progressive::{self};
use tpdf_lib::save;
use tpdf_lib::worker;
use tpdf_lib::worker::{Reply, Request, Worker};

/// Below this much headroom against the commit cap, the run says so.
///
/// A judgement, not a measurement, and it is a `[WARN]` rather than a `[FAIL]`
/// for that reason. 128 MiB is roughly what 42 MB of scanned document costs a
/// worker to prepare a save for, so it is the margin at which the next
/// ordinary-sized document would not fit.
const THIN_HEADROOM_MIB: f64 = 128.0;
use tpdf_lib::worker_child;

/// Tiles are compared at this size, which is inside the useful range AGENTS.md
/// measured (1024²--2048²) and small enough that a fixture renders quickly.
const TILE: u16 = 512;

/// A plan that turns every page a quarter turn and changes nothing else.
///
/// **Deliberately *not* append-shaped**, which is the whole point of it existing
/// beside [`highlight_plan`]: a turn is an edit an update section cannot express,
/// so `save::mode_for` sends it down the rewriting path. That is the path this
/// probe's rewrite checks, and a plan that could have been appended would have
/// exercised the other one.
fn turn_plan(page_count: u64) -> tpdf_lib::edits::Plan {
    use tpdf_lib::edits::PageView;

    let pages = u32::try_from(page_count).unwrap_or(u32::MAX);
    tpdf_lib::edits::Plan {
        baseline: pages,
        opened_as: None,
        pages: (0..pages)
            .map(|at| PageView {
                id: u64::from(at) + 1,
                source: at,
                turns: 1,
                crop: None,
            })
            .collect(),
        redactions: Vec::new(),
        notes: Vec::new(),
        marks: Vec::new(),
    }
}

/// A plan that adds one highlight to page 1 and changes nothing else.
///
/// Append-shaped by construction --- every baseline page kept, in order,
/// unturned and uncropped --- because `save::append_update` refuses anything
/// else, and a probe that got refused for the wrong reason would report a
/// boundary failure that is really a plan defect.
///
/// The quad is **display** space --- points from the displayed page's top-left
/// corner, y increasing *downwards* --- which is what the model holds and what
/// `save.rs` maps into the page's own space as it writes. Written the other way
/// round first, with `top` above `bottom` as a `/CropBox` has it, and the worker
/// refused it: *"a mark on page 1 covers no area in that page's own space"*.
/// That refusal is the probe working. Where the ink lands does not matter here;
/// what is checked is that a worker can build an update section at all.
fn highlight_plan(page_count: u64) -> tpdf_lib::edits::Plan {
    use tpdf_lib::docmodel::{MarkKind, Quad};
    use tpdf_lib::edits::{PageView, PlannedMark};

    let pages = u32::try_from(page_count).unwrap_or(u32::MAX);
    tpdf_lib::edits::Plan {
        baseline: pages,
        // Never set here, and it could not be: `Plan::opened_as` is
        // `#[serde(skip)]`, so a fingerprint cannot cross this boundary in
        // either direction. The worker builds from bytes; the caller decides
        // about files.
        opened_as: None,
        pages: (0..pages)
            .map(|at| PageView {
                id: u64::from(at) + 1,
                source: at,
                turns: 0,
                crop: None,
            })
            .collect(),
        redactions: Vec::new(),
        notes: Vec::new(),
        marks: vec![PlannedMark {
            kind: MarkKind::Highlight,
            stamp: None,
            source: 0,
            quads: vec![Quad {
                left: 72.0,
                top: 72.0,
                right: 216.0,
                bottom: 108.0,
            }],
            strokes: Vec::new(),
            color: [1.0, 0.9, 0.2],
            author: "worker-probe".to_string(),
            note: String::new(),
            made: "D:20260822120000Z".to_string(),
        }],
    }
}

fn main() {
    // This binary is also the worker: `Worker::spawn` re-execs `current_exe`.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        // No platform gate: `worker_child::main` establishes its own boundary
        // and refuses to serve a document without one, which is a stronger
        // guarantee than a `cfg` here could make and is checked at run time on
        // the process that actually parses the PDF.
        worker_child::main(&args);
    }

    let Some(document) = args.get(1).map(PathBuf::from) else {
        eprintln!("usage: worker-probe <file.pdf>");
        std::process::exit(2);
    };
    if !document.exists() {
        eprintln!(
            "[FAIL] {} does not exist --- see AGENTS.md on generating fixtures",
            document.display()
        );
        std::process::exit(1);
    }

    let library_dir = library_dir();
    let mut failures = 0;
    let mut checks = 0;
    let mut skipped = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        checks += 1;
        if !ok {
            failures += 1;
        }
        // Padded to a fixed seven, not interpolated: `[OK]` is two characters
        // shorter than `[FAIL]`, so interpolating the word shifts the detail
        // column on exactly the rows that pass, and a fixed-offset `cut` reading
        // the name set slices those rows short. See the trap.
        let label = if ok { "[OK]" } else { "[FAIL]" };
        println!("{label:7}{name:52} {detail}");
    };

    // ---------------------------------------------------------------- worker
    let mut worker = match Worker::spawn(&document, &library_dir) {
        Ok(w) => w,
        Err(e) => {
            println!("[FAIL] a worker starts on this document           {e}");
            std::process::exit(1);
        }
    };

    let opened = worker.call(&Request::Open {
        lazy_geometry: false,
    });
    let page_count = match &opened {
        Ok(r) if r.ok => match &r.reply {
            Some(Reply::Open { page_count, .. }) => *page_count as u64,
            _ => 0,
        },
        _ => 0,
    };
    check(
        "a sandboxed worker opens a document it has no path to",
        page_count > 0,
        match &opened {
            Ok(r) if r.ok => format!("{page_count} pages"),
            Ok(r) => r.error.clone(),
            Err(e) => e.clone(),
        },
    );
    if page_count == 0 {
        std::process::exit(1);
    }

    // ------------------------------------------------------------ the pixels
    let request = Request::Tile {
        crop: None,
        rid: 1,
        page: 0,
        scale: 1.0,
        turns: 0,
        invert: false,
        x: 0,
        y: 0,
        width: TILE,
        height: TILE,
        png: false,
    };
    let t0 = Instant::now();
    let through_worker = match worker.call(&request) {
        Ok(r) if r.ok => Some(worker.tile.as_slice()[..r.bytes].to_vec()),
        Ok(r) => {
            check("a worker renders a tile", false, r.error);
            None
        }
        Err(e) => {
            check("a worker renders a tile", false, e);
            None
        }
    };
    let worker_ms = t0.elapsed().as_secs_f64() * 1e3;

    let in_process = in_process_tile(&document, &library_dir);

    match (&through_worker, &in_process) {
        (Some(theirs), Ok(ours)) => {
            let same = theirs == ours;
            check(
                "the worker's pixels are identical to the in-process render",
                same,
                if same {
                    format!(
                        "{} bytes, {worker_ms:.1} ms across the boundary",
                        ours.len()
                    )
                } else {
                    format!(
                        "{} vs {} bytes, {} differing",
                        theirs.len(),
                        ours.len(),
                        theirs
                            .iter()
                            .zip(ours.iter())
                            .filter(|(a, b)| a != b)
                            .count()
                    )
                },
            );
            // The control. Without it "identical" is satisfied by two blank
            // buffers, which is exactly what a render that never ran produces.
            let blank = ours.iter().all(|b| *b == ours[0]);
            check(
                "the compared tile is not a uniform buffer",
                !blank,
                if blank {
                    "every byte is the same --- the comparison proves nothing".into()
                } else {
                    format!("{} distinct byte values", distinct_values(ours))
                },
            );
        }
        (_, Err(e)) => check(
            "an in-process render is available to compare against",
            false,
            e.clone(),
        ),
        _ => {}
    }

    // ------------------------------------------------------------- withdrawal
    // Pipelined on purpose. `Queue::withdraw` ignores an id it has never seen
    // --- deliberately, since remembering them is what would let its tables grow
    // without bound --- so a withdrawal sent *before* its request is a no-op,
    // and the first version of this check tested exactly that and failed.
    //
    // Two tiles are sent back to back and the second is withdrawn. The render
    // thread can only be inside the first, so the second is still queued when
    // the withdrawal arrives. The margin is the first tile's render time
    // (printed below) against three pipe writes, which is microseconds.
    let tile_at = |rid: u64| Request::Tile {
        crop: None,
        rid,
        page: 0,
        scale: 1.0,
        turns: 0,
        invert: false,
        x: 0,
        y: 0,
        width: TILE,
        height: TILE,
        png: false,
    };
    let _ = worker.send(&tile_at(20));
    let _ = worker.send(&tile_at(21));
    let _ = worker.send(&Request::Withdraw { rid: 21 });

    let first = worker.read_reply();
    let second = worker.read_reply();
    check(
        "the tile ahead of a withdrawal still renders",
        matches!(&first, Ok(r) if r.ok && !r.abandoned && r.bytes > 0),
        match &first {
            Ok(r) => format!("{} bytes in {:.1} ms", r.bytes, r.render_us as f64 / 1e3),
            Err(e) => e.clone(),
        },
    );
    check(
        "a withdrawn tile comes back abandoned, not blank and not failed",
        matches!(&second, Ok(r) if r.ok && r.abandoned && r.bytes == 0),
        match &second {
            Ok(r) if r.ok && !r.abandoned => {
                "it rendered --- the withdrawal lost the race with the render thread".into()
            }
            Ok(r) => format!("ok={} abandoned={} bytes={}", r.ok, r.abandoned, r.bytes),
            Err(e) => e.clone(),
        },
    );

    // And the control: an un-withdrawn request on the same worker still
    // renders, so "abandoned" is a reply to the withdrawal rather than a worker
    // that has stopped working.
    let after = worker.call(&tile_at(22));
    check(
        "the worker still renders after a withdrawal",
        matches!(&after, Ok(r) if r.ok && !r.abandoned && r.bytes > 0),
        match &after {
            Ok(r) => format!("{} bytes", r.bytes),
            Err(e) => e.clone(),
        },
    );

    // ------------------------------------------------------- the other routes
    let text = worker.call(&Request::Text {
        page: 0,
        crop: None,
    });
    check(
        "text extraction crosses the boundary",
        matches!(&text, Ok(r) if r.ok && matches!(r.reply, Some(Reply::Text(_)))),
        describe(&text),
    );
    let outline = worker.call(&Request::Outline);
    check(
        "an outline crosses the boundary",
        matches!(&outline, Ok(r) if r.ok && matches!(r.reply, Some(Reply::Outline(_)))),
        describe(&outline),
    );
    let matches = worker.call(&Request::Search {
        page: 0,
        query: "the".into(),
        options: Default::default(),
        carry: None,
    });
    check(
        "a search crosses the boundary",
        matches!(&matches, Ok(r) if r.ok && matches!(r.reply, Some(Reply::Search(_)))),
        describe(&matches),
    );

    // ------------------------------------------------------------ the append
    //
    // **The one request whose answer becomes a file**, and the reason it is
    // checked here rather than only in a unit test: a unit test calls
    // `save::append_update` in this process, which says nothing about whether a
    // *worker* can build one. What is being shown is that a process with no
    // filesystem authority, holding the document through a read-only mapping,
    // produces bytes that are a real PDF revision.
    //
    // Asserted on the bytes rather than on `ok`, for the reason the tile
    // comparison above exists: a sandbox that quietly starved the builder of
    // something would still answer, and an update section that parses and keeps
    // the page count cannot be faked by a reply that went wrong.
    // **What the append costs the worker, measured before and after.** The
    // number matters beyond curiosity: a Windows worker is capped at 1 GB of
    // commit by its job object, and `save::append_update` parses the document a
    // second time --- `Document::load_mem` builds owned objects out of what is
    // otherwise a read-only mapping. If that copy is large it is private commit
    // on Windows, where the mapping itself is not, and the cap is a real bound
    // rather than a distant one. Nothing here can measure Windows; what this
    // gives is the size of the term to worry about.
    let before_append = memory_reading(&worker);
    let update = worker.call(&Request::Append {
        plan: highlight_plan(page_count),
    });
    let after_append = memory_reading(&worker);
    // No `from_value` and no clone: `Reply::Append` carries the real `Update`,
    // so this is the same type the worker built rather than a re-parse of its
    // JSON that could disagree with it.
    let built: Option<tpdf_lib::save::Update> = match &update {
        Ok(reply) if reply.ok => match &reply.reply {
            Some(Reply::Append(update)) => Some(update.clone()),
            _ => None,
        },
        _ => None,
    };
    if let (Some((was, metric)), Some((now, _))) = (before_append, after_append) {
        // Not a check: it has no pass condition, because what a parse costs is a
        // property of the document rather than of the boundary. Reported so that
        // a run against a large fixture says the number out loud instead of
        // leaving it to be inferred from the file size, which the measurement in
        // `save.rs` shows is not the same thing.
        //
        // **Printed on both platforms since 2026-08-22, and it printed on
        // neither but macOS before.** It was guarded on `Worker::footprint`,
        // which is `None` off macOS, so the line silently did not exist on the
        // one platform where a cap makes the number decide something --- while
        // `BUILD.md` told a reader to run this and read it. The metric names
        // itself for the reason `memory_reading` gives.
        println!(
            "[INFO] the append moved the worker's {metric} {:.1} -> {:.1} MB (+{:.1})",
            was as f64 / 1e6,
            now as f64 / 1e6,
            (now as f64 - was as f64) / 1e6,
        );
        // The headroom, where there is a bound to have headroom against. Said
        // out loud rather than left to arithmetic because it is the number that
        // decides whether the append can be built at all, and on the largest
        // fixture in the repository it is 4.3%.
        if let Some(cap) = memory_cap() {
            let used = now as f64 / cap as f64 * 100.0;
            let head = (cap as f64 - now as f64) / (1024.0 * 1024.0);
            println!(
                "[INFO] that is {used:.1}% of the {} MiB the job object allows, \
                 leaving {head:.1} MiB",
                cap / (1024 * 1024),
            );
            // A warning rather than a failure: the threshold is a judgement and
            // the measurement is not, so a run that is close to the bound should
            // say so without deciding on the reader's behalf that it is wrong.
            // It fires today on `incr-scan-40p.pdf`, which is correct --- a
            // 361.9 MB scan aborts here, and the fixture is 336.6 MB.
            if head < THIN_HEADROOM_MIB {
                println!(
                    "[WARN] {head:.1} MiB of headroom against the commit cap --- \
                     a larger document cannot have its save prepared in the worker"
                );
            }
        }
    }
    check(
        "a save's update section is built across the boundary",
        built.as_ref().is_some_and(|u| !u.update.is_empty()),
        match &built {
            Some(u) => format!("{} bytes, {} pages", u.update.len(), u.pages),
            None => describe(&update),
        },
    );
    match &built {
        Some(u) => {
            let mut revised = std::fs::read(&document).unwrap_or_default();
            let was = revised.len();
            revised.extend_from_slice(&u.update);
            let reread = lopdf::Document::load_mem(&revised);
            check(
                "and those bytes are a revision a parser accepts",
                matches!(&reread, Ok(d) if d.get_pages().len() == u.pages),
                match &reread {
                    Ok(d) => format!(
                        "{} + {} bytes, {} pages",
                        was,
                        u.update.len(),
                        d.get_pages().len()
                    ),
                    Err(e) => format!("{e}"),
                },
            );
            check(
                "built against the document the worker was given",
                u.built_against == was,
                format!("{} against {was}", u.built_against),
            );
        }
        None => {
            check(
                "and those bytes are a revision a parser accepts",
                false,
                "no update was built".into(),
            );
            check(
                "built against the document the worker was given",
                false,
                "no update was built".into(),
            );
        }
    }

    // ------------------------------------------------------------ containment
    //
    // The two platforms bound a worker's memory by different mechanisms, and this
    // follows the mechanism rather than asserting one exists where it does not.
    // macOS refuses `RLIMIT_AS`, `RLIMIT_DATA` and `RLIMIT_RSS` outright, so a
    // poll from the parent is the *only* bound there and being able to read it is
    // the property. Windows caps commit in the kernel through the job object, so
    // there is nothing to poll and nothing that a poll would add.
    //
    // Printed as a skip with the reason rather than dropped, because AGENTS.md
    // records that a control which silently disappears on some inputs cannot be
    // told apart from one that ran.
    //
    // **A real check on both platforms since 2026-08-22, where it was a `[SKIP]`
    // on Windows.** The skip's stated reason was that the job object caps memory
    // in the kernel so there is nothing to poll, which is true and was the wrong
    // conclusion: a cap makes the reading matter *more*, since what a reader
    // needs to know is how close the worker came to being refused. What was
    // missing was not a reason to look but a way to --- `Contained::peak_commit`
    // is it, and the quantity it reads is the one the cap is charged against.
    if cfg!(any(target_os = "macos", windows)) {
        let reading = memory_reading(&worker);
        check(
            "the parent can read what bounds the worker's memory",
            reading.is_some_and(|(bytes, _)| bytes > 0),
            match reading {
                // Zero reads exactly like a permissions problem and is usually
                // the `proc_pid_rusage` pointer mistake AGENTS.md records.
                Some((bytes, metric)) => format!("{metric} {:.1} MB", bytes as f64 / 1e6),
                None => "unavailable".into(),
            },
        );
    } else {
        skipped += 1;
        println!(
            "[SKIP] {:52} not applicable --- this platform neither polls nor caps",
            "the parent can read what bounds the worker's memory"
        );
    }

    // Killing the worker must be distinguishable from the worker exiting on its
    // own. The crash test AGENTS.md records reported "exited with code 9" where a
    // segfault should have said "killed by signal 11", and that was the tell.
    //
    // The word differs because the mechanism does: unix kills with a signal, and
    // Windows has none --- `TerminateJobObject` sets an exit *code*, so the tell
    // has to be carried by a code no ordinary failure produces. Same property,
    // same check name, different evidence. See `sandbox_win::KILLED_EXIT`.
    worker.kill();
    let epitaph = worker.epitaph();
    let tell = if cfg!(windows) { "killed" } else { "signal" };
    check(
        "a killed worker is reported as killed, not as having exited",
        epitaph.contains(tell),
        epitaph,
    );
    let after_death = worker.call(&Request::Outline);
    check(
        "the parent survives its worker and says so",
        after_death.is_err(),
        match &after_death {
            Ok(_) => "it answered, which means it is not the worker that died".into(),
            Err(e) => e.clone(),
        },
    );

    // ------------------------------------------- a document that does not open
    //
    // The failure a reader actually meets, and until 2026-08-21 the one tpdf
    // could not describe. PDFium refusing a document made `serve` return `Err`,
    // so the worker printed its reason to stderr and exited 1 --- and a GUI
    // process has no stderr, so the reader was shown
    // `worker stopped answering (exited with 1 (0x00000001))` for a file whose
    // problem `open_failure` had already diagnosed in their own words.
    //
    // Written here rather than as a `#[test]`, because the thing being tested is
    // that a **process** answers instead of dying, and `Worker::spawn` re-execs
    // `current_exe` --- which under `cargo test` is a test binary that does not
    // serve. This probe is its own worker, so it is the only harness that can
    // spawn one.
    //
    // Nine bytes of nothing rather than a real PDF, so no fixture is needed and
    // the corpus is not enrolled in anything: `FPDF_ERR_FORMAT` is the same door
    // as `FPDF_ERR_PASSWORD`, which is what the reported file most likely hit.
    let mut refused_checks = |name: &str, ok: bool, detail: String| check(name, ok, detail);
    let junk = std::env::temp_dir().join("tpdf-worker-probe-not-a-pdf.pdf");
    match std::fs::write(&junk, b"not a pdf") {
        Err(e) => {
            refused_checks(
                "a document PDFium refuses is answered rather than died on",
                false,
                format!("could not write {}: {e}", junk.display()),
            );
        }
        Ok(()) => {
            match Worker::spawn(&junk, &library_dir) {
                Err(e) => refused_checks(
                    "a document PDFium refuses is answered rather than died on",
                    false,
                    format!("no worker started: {e}"),
                ),
                Ok(mut refused) => {
                    let reply = refused.call(&Request::Open {
                        lazy_geometry: false,
                    });
                    // `Ok(_)` is the whole assertion: a reply at all means the
                    // process was alive to send one. `Err` here is the defect,
                    // and it carries the epitaph rather than the reason.
                    refused_checks(
                        "a document PDFium refuses is answered rather than died on",
                        matches!(&reply, Ok(r) if !r.ok),
                        describe(&reply),
                    );
                    // And the reason has to be the document's. The control for
                    // the check above on its own is weak --- a worker that
                    // answered every open with an empty error would satisfy it
                    // --- so this asserts the message names a cause a reader can
                    // act on and is not the parent's epitaph for a dead child.
                    let said = match &reply {
                        Ok(r) => r.error.clone(),
                        Err(e) => e.clone(),
                    };
                    refused_checks(
                        "and the reason names the document, not the worker",
                        !said.is_empty()
                            && !said.contains("stopped answering")
                            && !said.contains("exited with"),
                        format!("said {said:?}"),
                    );
                }
            }
            let _ = std::fs::remove_file(&junk);
        }
    }

    // ---------------------------------------- a worker that cannot bind at all
    //
    // The same failure one layer earlier, and this is the one that shipped.
    // 26.8.8's Windows installer laid `pdfium.dll` down under the name `pdfium`,
    // so `bind` failed in every worker, `serve` returned `Err`, and the process
    // exited 1 --- for every document, by every route, with the message naming
    // the missing library written to a stderr a GUI process does not have. What
    // a reader could see was `worker stopped answering (exited with 1)`.
    //
    // The fixture is a directory with no PDFium in it. Like the block above this
    // cannot be a `#[test]`: the claim is about a **process** answering rather
    // than dying, and under `cargo test` `current_exe` is a test binary that does
    // not serve.
    let mut engine_checks = |name: &str, ok: bool, detail: String| check(name, ok, detail);
    let nowhere = std::env::temp_dir().join("tpdf-worker-probe-no-pdfium-here");
    match std::fs::create_dir_all(&nowhere) {
        Err(e) => engine_checks(
            "a worker that cannot load PDFium answers rather than exiting",
            false,
            format!("could not create {}: {e}", nowhere.display()),
        ),
        Ok(()) => match Worker::spawn(&document, &nowhere) {
            Err(e) => engine_checks(
                "a worker that cannot load PDFium answers rather than exiting",
                false,
                format!("no worker started: {e}"),
            ),
            Ok(mut blind) => {
                let reply = blind.call(&Request::Open {
                    lazy_geometry: false,
                });
                // `Ok(_)` is the assertion: a reply at all means the process was
                // alive to send one, where before it had exited.
                engine_checks(
                    "a worker that cannot load PDFium answers rather than exiting",
                    matches!(&reply, Ok(r) if !r.ok),
                    describe(&reply),
                );
                // And it has to name the engine. Without this the check above is
                // satisfied by any error at all, including the epitaph of a
                // worker that died for some other reason entirely.
                let said = match &reply {
                    Ok(r) => r.error.clone(),
                    Err(e) => e.clone(),
                };
                engine_checks(
                    "and it names the engine rather than an exit code",
                    said.contains("PDF engine") && !said.contains("exited with"),
                    format!("said {said:?}"),
                );
            }
        },
    }
    let _ = std::fs::remove_dir_all(&nowhere);

    // --- The save's read-back, across the boundary -------------------------
    //
    // **The shipped verifier, which nothing else exercises.** Every test in
    // `save.rs` and every other probe passes `save::Here`, so before these
    // checks `save::InWorker` was proved only by compiling --- the shape of a
    // check that cannot fail. What is asserted is a *differential*: the worker
    // and the coordinator are asked the identical question about the identical
    // bytes, and have to agree in both directions.
    //
    // Agreement alone would not be enough, and the second pair is why. Two
    // readers that both answer "fine" agree perfectly on a file neither
    // examined, so the refusal is the half with teeth: a file `lopdf` cannot
    // parse has to come back as a refusal *through the pipe*, which only a
    // worker that really parsed it can produce.
    let here: &dyn save::Reread = &save::Here;
    let in_worker = save::InWorker::at(library_dir.clone());

    let good = std::env::temp_dir().join("tpdf-worker-probe-reread-good.pdf");
    let bad = std::env::temp_dir().join("tpdf-worker-probe-reread-bad.pdf");
    // **A real document with a trailer pointing into nothing**, which is the
    // failure this whole path exists to catch, and the first draft of this probe
    // got it wrong in a way worth recording. It planted a file that was not a PDF
    // at all --- and the worker duly refused it, with *PDFium's* message, from
    // `Worker::spawn_shared`, before `Request::Reread` was ever sent. Both
    // checks passed and neither had exercised `lopdf`: a control refused by a
    // different guard than the one it was written for.
    //
    // The discriminating fixture is the one where the two parsers *disagree*.
    // PDFium reconstructs a broken cross-reference table and opens this happily,
    // so the worker starts and the request is served; `lopdf` refuses it, which
    // is the answer that has to cross back. It is the same shape
    // `an_append_that_cannot_be_read_back_puts_the_file_back_as_it_was` plants,
    // and it is why this check is `lopdf` rather than the page count
    // `Request::Open` already returns.
    let planted = std::fs::copy(&document, &good).and_then(|_| {
        let mut broken = std::fs::read(&document)?;
        broken.extend_from_slice(b"\nstartxref\n999999999\n%%EOF\n");
        std::fs::write(&bad, broken)
    });

    match planted {
        Err(e) => {
            check(
                "the worker and the coordinator agree on a good file",
                false,
                format!("could not plant the fixtures: {e}"),
            );
        }
        Ok(()) => {
            let ask = |who: &dyn save::Reread, at: &Path| -> Result<usize, String> {
                let mut handle = std::fs::File::open(at).map_err(|e| e.to_string())?;
                let len = handle.metadata().map_err(|e| e.to_string())?.len() as usize;
                who.pages(&mut handle, len, None)
            };

            let mine = ask(here, &good);
            let theirs = ask(&in_worker, &good);
            check(
                "the worker and the coordinator agree on a good file",
                matches!((&mine, &theirs), (Ok(a), Ok(b)) if a == b && *a > 0),
                format!("coordinator {mine:?}, worker {theirs:?}"),
            );

            let mine = ask(here, &bad);
            let theirs = ask(&in_worker, &bad);
            check(
                "and both refuse a trailer that points into nothing",
                mine.is_err() && theirs.is_err(),
                format!("coordinator {mine:?}, worker {theirs:?}"),
            );
            // **The refusal has to be `lopdf`'s.** Without this the check above
            // is satisfied by a worker that never started, or by one PDFium
            // refused at open --- and the second is not hypothetical, it is what
            // the first version of this probe measured while reading as a pass.
            // The two parsers word it differently, and that difference is the
            // evidence: PDFium says the file is not a PDF, `lopdf` names the
            // cross-reference table.
            check(
                "and the worker's refusal came from the re-read, not from opening it",
                match &theirs {
                    Err(said) => said.contains("cross reference"),
                    Ok(_) => false,
                },
                format!("worker said {theirs:?}"),
            );

            // **And that a worker is genuinely involved**, which none of the
            // three above can say: they compare two answers, and an `InWorker`
            // secretly delegating to `Here` would produce identical ones on
            // every fixture. An outcome two mechanisms can produce cannot test
            // either of them.
            //
            // So it is asked for something only the worker path needs. Pointed
            // at a directory with no PDFium in it, `Here` still answers --- it
            // parses in this process, which has no engine to load --- and
            // `InWorker` cannot start a child at all. The two disagreeing is the
            // evidence.
            let nowhere = std::env::temp_dir().join("tpdf-worker-probe-reread-no-engine");
            let _ = std::fs::create_dir_all(&nowhere);
            let engineless = save::InWorker::at(nowhere.clone());
            let without = ask(&engineless, &good);
            let still = ask(here, &good);
            check(
                "and the worker path really needs a worker",
                without.is_err() && matches!(&still, Ok(pages) if *pages > 0),
                format!("worker {without:?}, coordinator {still:?}"),
            );
            let _ = std::fs::remove_dir_all(&nowhere);
        }
    }
    let _ = std::fs::remove_file(&good);
    let _ = std::fs::remove_file(&bad);

    // --- The rewriting save, across the boundary ---------------------------
    //
    // **The shipped writer, which nothing else exercises.** Every test in
    // `save.rs` and every other probe passes `save::Here`, so before these
    // checks `save::InWorker`'s `Rewriter` half was proved only by compiling.
    //
    // It is the same four-check shape the read-back above uses, for the same
    // reasons, and one more that only this path can make: a rewrite's answer is
    // the whole document, so it travels down an output channel rather than in a
    // reply, and the check that the channel is real is a worker asked to write
    // when it was given nowhere to write.
    let rewriting: &dyn save::Rewriter = &save::Here;
    let rewrite_in_worker = save::InWorker::at(library_dir.clone());
    let turning = turn_plan(page_count);

    let by_hand = std::env::temp_dir().join("tpdf-worker-probe-rewrite-here.pdf");
    let by_worker = std::env::temp_dir().join("tpdf-worker-probe-rewrite-worker.pdf");

    let rewrite_to = |who: &dyn save::Rewriter,
                      to: &Path,
                      plan: &tpdf_lib::edits::Plan|
     -> Result<usize, String> {
        let mut source = std::fs::File::open(&document).map_err(|e| e.to_string())?;
        let len = source.metadata().map_err(|e| e.to_string())?.len() as usize;
        let mut out = std::fs::File::create(to).map_err(|e| e.to_string())?;
        let wrote = who
            .write(&mut source, len, &mut out, plan, None)
            .map_err(|why| why.message)?;
        // The same check the coordinator makes, for the same reason: the length
        // reported and the length on disk are two independent statements.
        let landed = out.metadata().map_err(|e| e.to_string())?.len() as usize;
        if landed != wrote {
            return Err(format!("reported {wrote} bytes and the file has {landed}"));
        }
        Ok(wrote)
    };

    // **What the move costs, interleaved and reported as minima.** A reader who
    // presses the save key waits for this, so the number is worth having --- and
    // two blocks back to back would measure whatever else the machine was doing
    // between them, which `AGENTS.md` records as the way to get an A/B wrong.
    // The minimum rather than the mean, because the question is what the work
    // costs and not what the machine was doing while it ran.
    let mut here_ms = f64::MAX;
    let mut worker_ms = f64::MAX;
    let mut mine = Err("not run".to_string());
    let mut theirs = Err("not run".to_string());
    for _ in 0..5 {
        let at = Instant::now();
        mine = rewrite_to(rewriting, &by_hand, &turning);
        here_ms = here_ms.min(at.elapsed().as_secs_f64() * 1000.0);
        let at = Instant::now();
        theirs = rewrite_to(&rewrite_in_worker, &by_worker, &turning);
        worker_ms = worker_ms.min(at.elapsed().as_secs_f64() * 1000.0);
    }
    println!(
        "[INFO] the rewrite is {here_ms:.1} ms here and {worker_ms:.1} ms in a worker \
         (+{:.1}, best of 5 interleaved)",
        worker_ms - here_ms
    );
    // **Byte for byte, which is stronger than the read-back's number and is
    // affordable here.** A rewrite of one document under one plan is
    // deterministic --- every date in the output comes from the plan's own marks,
    // not from the clock --- so the two processes have no licence to differ. A
    // comparison of lengths or page counts would pass for a worker that dropped
    // the turns.
    let same = match (&mine, &theirs) {
        (Ok(a), Ok(b)) if a == b => std::fs::read(&by_hand).ok() == std::fs::read(&by_worker).ok(),
        _ => false,
    };
    check(
        "the worker and the coordinator write the same document",
        same,
        format!("coordinator {mine:?}, worker {theirs:?}"),
    );

    // **A refusal that only a parse can produce.** The plan claims one more page
    // than the file has, which `save::checked` catches *after* `lopdf` has read
    // the document --- so a worker that never parsed anything cannot produce this
    // message. It is the discriminating fixture the read-back's own comment
    // explains the need for: a file PDFium refuses at open would be refused by a
    // different guard, before the request was ever sent.
    let miscounted = turn_plan(page_count + 1);
    let mine = rewrite_to(rewriting, &by_hand, &miscounted);
    let theirs = rewrite_to(&rewrite_in_worker, &by_worker, &miscounted);
    check(
        "and both refuse a plan whose baseline is not this document",
        mine.is_err() && theirs.is_err(),
        format!("coordinator {mine:?}, worker {theirs:?}"),
    );
    check(
        "and the worker's refusal came from the rewrite, not from opening it",
        match &theirs {
            Err(said) => said.contains("changed since it was opened"),
            Ok(_) => false,
        },
        format!("worker said {theirs:?}"),
    );

    // **And that a worker is genuinely involved**, which none of the three above
    // can say --- an `InWorker` secretly delegating to `Here` answers identically
    // on every fixture. Pointed at a directory with no PDFium in it, `Here` still
    // writes and `InWorker` cannot start a child at all.
    let nowhere = std::env::temp_dir().join("tpdf-worker-probe-rewrite-no-engine");
    let _ = std::fs::create_dir_all(&nowhere);
    let engineless = save::InWorker::at(nowhere.clone());
    let without = rewrite_to(&engineless, &by_worker, &turning);
    let still = rewrite_to(rewriting, &by_hand, &turning);
    check(
        "and the rewrite path really needs a worker",
        without.is_err() && still.is_ok(),
        format!("worker {without:?}, coordinator {still:?}"),
    );
    let _ = std::fs::remove_dir_all(&nowhere);

    // **The output channel exists, said by its absence.** Every check above
    // would pass if the descriptor were handed over unconditionally and the argv
    // marker did nothing. This one asks an ordinary pooled-shape worker --- one
    // spawned with no output file --- to rewrite, and requires it to say so in
    // words rather than writing a document into whatever fd 6 happens to be.
    match tpdf_lib::worker_shm::Shm::map_file(&document)
        .and_then(|mapped| Worker::spawn_shared(std::sync::Arc::new(mapped), &library_dir))
    {
        Err(e) => check(
            "a worker with nowhere to write refuses to rewrite",
            false,
            format!("could not start one: {e}"),
        ),
        Ok(mut worker) => {
            let said = worker.call(&Request::Rewrite {
                plan: turning.clone(),
                view: 0,
            });
            check(
                "a worker with nowhere to write refuses to rewrite",
                match &said {
                    Ok(r) => !r.ok && r.error.contains("anywhere to write"),
                    Err(_) => false,
                },
                format!("worker answered {said:?}"),
            );
        }
    }
    let _ = std::fs::remove_file(&by_hand);
    let _ = std::fs::remove_file(&by_worker);

    println!(
        "\n{}/{checks} checks passed, {skipped} not applicable to this platform",
        checks - failures
    );
    std::process::exit(i32::from(failures > 0));
}

/// Renders the same tile without a worker, as the comparison's other half.
fn in_process_tile(document: &Path, library_dir: &Path) -> Result<Vec<u8>, String> {
    use pdfium_render::prelude::Pdfium;

    let path = Pdfium::pdfium_platform_library_name_at_path(library_dir);
    let bindings = Pdfium::bind_to_library(&path).map_err(|e| e.to_string())?;
    let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
    let bindings = progressive::bindings_of(pdfium);

    let doc = OpenDocument::open(bindings, document, None)?;
    let page = doc.page(0)?;
    let spec = progressive::TileSpec {
        scale: 1.0,
        turns: 0,
        x: 0,
        y: 0,
        width: TILE,
        height: TILE,
    };
    let cancel = progressive::CancelToken::new();
    let (rgba, _) = progressive::render_tile(bindings, &page, spec, None, &cancel)?;
    Ok(rgba)
}

/// How many distinct values the tile holds, as evidence of content.
///
/// Over the whole tile, not a prefix: the first kilobyte of a text page is the
/// white margin, so a prefix reads `1` on a perfectly good render and makes the
/// control look like it is failing.
fn distinct_values(bytes: &[u8]) -> usize {
    let mut seen = [false; 256];
    for b in bytes {
        seen[*b as usize] = true;
    }
    seen.iter().filter(|s| **s).count()
}

/// A one-line summary of a reply.
/// What this platform can say about a worker's memory, and which quantity it is.
///
/// **Two different measurements, and they are kept apart rather than merged
/// behind one name.** macOS polls a physical footprint because its kernel
/// refuses every relevant rlimit and a poll is the only thing left; Windows
/// reads a peak commit charge because commit is what its job object refuses on.
/// The string travels with the number so that a run says which it took --- on
/// `incr-scan-40p.pdf` the two agree to 0.2%, which is close enough to invite
/// treating them as one thing and is not a licence to.
///
/// Exactly one arm answers on each platform, so the order is not a preference.
fn memory_reading(worker: &Worker) -> Option<(u64, &'static str)> {
    if let Some(bytes) = worker.peak_commit() {
        return Some((bytes, "peak commit"));
    }
    worker.footprint().map(|bytes| (bytes, "footprint"))
}

/// What the kernel refuses a worker at, where a kernel refuses one at all.
///
/// `None` on macOS, and that is the property rather than a gap: there is no
/// bound to be near, which is why the footprint poll exists there at all.
fn memory_cap() -> Option<u64> {
    #[cfg(windows)]
    {
        Some(tpdf_lib::sandbox_win::WORKER_MEMORY_CAP as u64)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn describe(result: &Result<tpdf_lib::worker::Response, String>) -> String {
    match result {
        Ok(r) if r.ok => {
            // Sized by re-serialising rather than by a hand-kept table of
            // variant names, which would be exactly the second inventory the
            // typed reply exists to remove.
            let size = r
                .reply
                .as_ref()
                .and_then(|reply| serde_json::to_string(reply).ok())
                .map_or(0, |line| line.len());
            format!("{size} bytes of payload")
        }
        Ok(r) => r.error.clone(),
        Err(e) => e.clone(),
    }
}

/// Where PDFium lives, matching the app's own resolution in development.
///
/// The subdirectory differs by platform and the difference is not cosmetic:
/// Windows ships the loadable DLL in `bin/` and puts only the *import* library
/// in `lib/`, so joining `lib` unconditionally finds a directory that exists and
/// holds nothing loadable --- see `pdfium_library_dir` in `lib.rs`, which had
/// exactly this wrong. The other spike binaries still hardcode `lib`; they have
/// never been run on Windows, and this one now is.
fn library_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR))
        .unwrap_or_else(|| PathBuf::from("."))
}
