//! What a pre-spawned worker could recover, before one is built.
//!
//! `Worker::spawn_shared` returns as soon as `fork`/`exec` has been issued --- it
//! waits for nothing. Everything expensive happens in the child afterwards, and
//! the parent first meets it when it blocks on the opening request:
//!
//! 1. `fork` + `exec`, then dyld linking the child image
//! 2. `bind()`, which opens and maps libpdfium --- **before** the sandbox, because
//!    a policy denying file reads forbids it
//! 3. `apply_sandbox`
//! 4. `RawDocument::open_bytes`, and then the page-geometry walk
//!
//! Only step 4 needs the document. Steps 1--3 are what a worker started before any
//! file is chosen could already have done, so **they are the ceiling on what
//! pre-spawning can buy** --- and the point of this probe is to measure that
//! ceiling rather than to argue for it. `AGENTS.md` records the cost of carrying a
//! number over from a different workload; a comment in `render.rs` says "a spawn
//! is ~12 ms" and this is the measurement that says what that 12 ms contains.
//!
//! The split is measured, not modelled --- and the first run refuted the model it
//! was written with. That model said cost rises with the document, so the
//! smallest file would show the floor. It does not: a 757-byte page costs
//! **14.1 ms** and a 775-page, 1 MB document costs **7.2 ms**, reproducibly, with
//! a spread of half a millisecond.
//!
//! The variable is not size but **whether the fonts are embedded**. Documents
//! carrying their own subset (`text-truetype`, `text-cid`, `text-marked`,
//! `text-heavy`) land at 6.6--7.2 ms; documents naming base-14 faces
//! (`text-base14` at 888 bytes, `rotated-90` at 2 KB) land at 14.0--14.5 ms,
//! because PDFium goes looking for a system font. So the interval is three costs,
//! not two:
//!
//! - a **~6.6 ms floor** that no document influences,
//! - **~7.4 ms of system-font enumeration**, paid only when nothing is embedded,
//! - page-1 parse on top --- 46 ms for the A0 sheet, and near zero for the rest.
//!
//! The middle one is the interesting one for a pre-spawned worker: it is a
//! property of the *machine's font list*, not of the document, so it is warmable
//! before any file is chosen even though it does not look document-independent.
//!
//! Interleaved A,B,A,B across rounds and compared within a round, per the standing
//! rule --- this machine drifts several percent over minutes. Round 0 is discarded
//! as a warm-up outlier.
//!
//! ```text
//! cargo run --release --bin prespawn-bench
//! cargo run --release --bin prespawn-bench -- --rounds 8
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tpdf_lib::worker::{self, PreWorker, Request, Shm, Worker};
use tpdf_lib::worker_child;

/// Documents chosen to span the parse cost while sharing every fixed cost.
///
/// `hostile-objstm` is 757 bytes and one page: whatever it costs to parse is far
/// below the numbers here, so its column is the fixed cost with the document work
/// rounded away. The rest add pages, then page *complexity*, then sheer size --- a
/// 337 MB scan is included because a fixed cost that turned out to scale with the
/// mapping would show up there and nowhere else.
const FIXTURES: &[&str] = &[
    "hostile-objstm.pdf",
    "text-base14.pdf",
    "outline-simple.pdf",
    "text-heavy.pdf",
    "vector-heavy.pdf",
    "incr-scan-40p.pdf",
];

fn main() {
    // This binary is also the worker: `Worker::spawn` re-execs `current_exe`.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        worker_child::main(&args);
    }

    let rounds = flag(&args, "--rounds").unwrap_or(6);
    let root = repo_root();
    let library_dir = root.join("vendor/pdfium/lib");

    // Positional fixture names override the default list, so a hypothesis about
    // *which* documents are slow can be tested without rebuilding.
    let chosen: Vec<String> = args
        .iter()
        .skip(1)
        .filter(|a| a.ends_with(".pdf"))
        .cloned()
        .collect();
    let wanted: Vec<&str> = if chosen.is_empty() {
        FIXTURES.to_vec()
    } else {
        chosen.iter().map(String::as_str).collect()
    };

    let present: Vec<PathBuf> = wanted
        .iter()
        .map(|name| root.join("testdata").join(name))
        .filter(|path| path.exists())
        .collect();
    if present.is_empty() {
        eprintln!("[FAIL] no fixtures found --- see AGENTS.md for how to generate them");
        std::process::exit(1);
    }

    println!("spawn to first reply, {rounds} rounds (round 0 discarded)\n");
    // min and max as well as the median, because the first run of this probe
    // reported a 757-byte document costing twice a 775-page one -- which is
    // either a real effect or a spread wide enough to make the median
    // meaningless, and a single number cannot tell those apart.
    println!(
        "  {:<22} {:>8} {:>26} {:>12} {:>8}",
        "document", "size", "spawn now (min/med/max)", "pre-spawned", "saved"
    );

    let mut samples: Vec<Vec<(f64, f64)>> = vec![Vec::new(); present.len()];
    let mut warm: Vec<Vec<f64>> = vec![Vec::new(); present.len()];
    for round in 0..rounds {
        // Interleaved: every fixture is measured once per round, so a machine
        // that drifts between rounds moves all of them together. The two
        // variants are adjacent within a round for the same reason.
        for (index, path) in present.iter().enumerate() {
            // Warmed to completion *before* the control runs, and outside every
            // timer. The first version of this let the pre-spawned worker warm
            // while the control ran, so its head start was however long the
            // control happened to take -- which made `text-heavy`, whose control
            // is 8 ms, report no saving at all while the A0 sheet, whose control
            // is 55 ms, reported the true one. The head start has to be a
            // quantity chosen here, not a side effect of the row above it.
            let pre = Worker::prespawn(&library_dir).and_then(|mut pre| {
                pre.wait_warm()?;
                Ok(pre)
            });

            match once(path, &library_dir) {
                Ok(sample) => {
                    if round > 0 {
                        samples[index].push(sample);
                    }
                }
                Err(e) => {
                    eprintln!("[FAIL] {}: {e}", path.display());
                    std::process::exit(1);
                }
            }

            match pre.and_then(|pre| once_prespawned(path, pre)) {
                Ok(sample) => {
                    if round > 0 {
                        warm[index].push(sample);
                    }
                }
                Err(e) => {
                    eprintln!("[FAIL] {} (pre-spawned): {e}", path.display());
                    std::process::exit(1);
                }
            }
        }
    }

    for (index, path) in present.iter().enumerate() {
        let mut readys: Vec<f64> = samples[index].iter().map(|s| s.1).collect();
        readys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // Pairwise within a round, then the median of those -- not the difference
        // of two medians, which would let a drifting machine appear as a saving.
        let mut deltas: Vec<f64> = samples[index]
            .iter()
            .zip(&warm[index])
            .map(|(cold, hot)| cold.1 - hot)
            .collect();
        deltas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        println!(
            "  {:<22} {:>8} {:>7.2} /{:>7.2} /{:>7.2} ms {:>9.2} ms {:>+7.2}",
            path.file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            bytes(path),
            readys.first().copied().unwrap_or(f64::NAN),
            median(&readys),
            readys.last().copied().unwrap_or(f64::NAN),
            median(&warm[index]),
            median(&deltas),
        );
    }

    // Deliberately no verdict printed here. The first version of this probe
    // closed with "the cheapest document is the floor", and the data said a
    // 757-byte file cost twice what a 775-page one did -- a conclusion the
    // numbers contradicted, printed underneath them. Read the table.
}

/// What a reader waits for when a warm worker already exists.
///
/// The pre-spawn itself is deliberately outside the timer, and that is the claim
/// rather than a convenience: a worker started while the shell is still coming up
/// has ~250 ms of someone else's work to hide behind, so the number that matters
/// is what remains once it is warm. `PreWorker::adopt` returns only after the
/// child has announced itself warm, so this cannot accidentally include the link,
/// the sandbox or the font walk --- if the worker were not ready, the wait would
/// appear here in full rather than being quietly excluded.
fn once_prespawned(path: &Path, pre: PreWorker) -> Result<f64, String> {
    let doc = Arc::new(Shm::map_file(path)?);
    let t0 = Instant::now();
    let mut worker = pre.adopt(doc)?;
    let response = worker.call(&Request::Open {
        lazy_geometry: true,
    })?;
    let ready = t0.elapsed().as_secs_f64() * 1e3;
    check_opened(&response)?;
    Ok(ready)
}

/// Rejects a reply that is fast because it failed.
fn check_opened(response: &tpdf_lib::worker::Response) -> Result<(), String> {
    if !response.ok {
        return Err(format!("the worker refused to open it: {}", response.error));
    }
    if response.json.is_none() {
        return Err("the open reply carried no geometry".into());
    }
    Ok(())
}

/// One spawn, and the first request that forces the child to be ready.
///
/// `Open` is the request the service itself makes first, so this measures the
/// interval the viewer actually waits through rather than a synthetic one.
fn once(path: &Path, library_dir: &Path) -> Result<(f64, f64), String> {
    let t0 = Instant::now();
    let mut worker = Worker::spawn(path, library_dir)?;
    let forked = t0.elapsed().as_secs_f64() * 1e3;

    // Lazy geometry, so the page-geometry walk is not folded into the number.
    // `AGENTS.md` measures that walk at 86 ms on the 775-page corpus --- large
    // enough to swamp everything being separated here.
    let response = worker.call(&Request::Open {
        lazy_geometry: true,
    })?;
    let ready = t0.elapsed().as_secs_f64() * 1e3;

    // Checked rather than assumed. A worker that answered with an error answers
    // fast, so an unchecked reply would make a broken run the best-looking
    // column in the table.
    if !response.ok {
        return Err(format!("the worker refused to open it: {}", response.error));
    }
    if response.json.is_none() {
        return Err("the open reply carried no geometry".into());
    }
    Ok((forked, ready))
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted[sorted.len() / 2]
}

fn bytes(path: &Path) -> String {
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if len >= 1 << 20 {
        format!("{} MB", len >> 20)
    } else if len >= 1 << 10 {
        format!("{} KB", len >> 10)
    } else {
        format!("{len} B")
    }
}

fn flag(args: &[String], name: &str) -> Option<usize> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}
