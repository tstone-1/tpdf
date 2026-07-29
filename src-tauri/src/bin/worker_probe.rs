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
//! cargo run --release --bin worker-probe -- testdata/text-heavy.pdf
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use tpdf_lib::progressive::{self, RawDocument};
use tpdf_lib::worker;
use tpdf_lib::worker::{Request, Worker};
// The child half exists only on unix --- see the module note in `worker.rs`.
#[cfg(unix)]
use tpdf_lib::worker_child;

/// Tiles are compared at this size, which is inside the useful range AGENTS.md
/// measured (1024²--2048²) and small enough that a fixture renders quickly.
const TILE: u16 = 512;

fn main() {
    // This binary is also the worker: `Worker::spawn` re-execs `current_exe`.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        #[cfg(unix)]
        worker_child::main(&args);
        #[cfg(not(unix))]
        {
            eprintln!("{}", worker::NO_WORKERS);
            std::process::exit(2);
        }
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
    let mut check = |name: &str, ok: bool, detail: String| {
        checks += 1;
        if !ok {
            failures += 1;
        }
        println!("[{}] {name:52} {detail}", if ok { "OK" } else { "FAIL" });
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
    let text = worker.call(&Request::Text { page: 0 });
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
    });
    check(
        "a search crosses the boundary",
        matches!(&matches, Ok(r) if r.ok && r.json.is_some()),
        describe(&matches),
    );

    // ------------------------------------------------------------ containment
    let footprint = worker.footprint();
    check(
        "the parent can read the worker's footprint",
        footprint.is_some_and(|f| f > 0),
        match footprint {
            // Zero reads exactly like a permissions problem and is usually the
            // `proc_pid_rusage` pointer mistake AGENTS.md records.
            Some(bytes) => format!("{:.1} MB", bytes as f64 / 1e6),
            None => "unavailable".into(),
        },
    );

    // Killing the worker must be visible as a *signal*, not an exit code. The
    // crash test AGENTS.md records reported "exited with code 9" where a
    // segfault should have said "killed by signal 11", and that was the tell.
    worker.kill();
    let epitaph = worker.epitaph();
    check(
        "a killed worker is reported as killed, not as having exited",
        epitaph.contains("signal"),
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

    println!("\n{}/{checks} checks passed", checks - failures);
    std::process::exit(i32::from(failures > 0));
}

/// Renders the same tile without a worker, as the comparison's other half.
fn in_process_tile(document: &Path, library_dir: &Path) -> Result<Vec<u8>, String> {
    use pdfium_render::prelude::Pdfium;

    let path = Pdfium::pdfium_platform_library_name_at_path(library_dir);
    let bindings = Pdfium::bind_to_library(&path).map_err(|e| e.to_string())?;
    let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
    let bindings = progressive::bindings_of(pdfium);

    let doc = RawDocument::open(bindings, document)?;
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
fn library_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("vendor/pdfium/lib"))
        .unwrap_or_else(|| PathBuf::from("."))
}
