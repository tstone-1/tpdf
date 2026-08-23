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

use tpdf_lib::progressive::{self, RawDocument};
use tpdf_lib::worker;
use tpdf_lib::worker::{Request, Worker};

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
        marks: vec![PlannedMark {
            kind: MarkKind::Highlight,
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
        Ok(r) if r.ok => r
            .json
            .as_ref()
            .and_then(|j| j.get("page_count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
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
        matches!(&text, Ok(r) if r.ok && r.json.is_some()),
        describe(&text),
    );
    let outline = worker.call(&Request::Outline);
    check(
        "an outline crosses the boundary",
        matches!(&outline, Ok(r) if r.ok && r.json.is_some()),
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
        matches!(&matches, Ok(r) if r.ok && r.json.is_some()),
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
    let built: Option<tpdf_lib::save::Update> = match &update {
        Ok(reply) if reply.ok => reply
            .json
            .as_ref()
            .and_then(|j| serde_json::from_value(j.clone()).ok()),
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

    let doc = RawDocument::open(bindings, document, None)?;
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
            let size = r.json.as_ref().map_or(0, |j| j.to_string().len());
            format!("{size} bytes of JSON")
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
