//! What a worker pool buys on the workload a viewport actually asks for.
//!
//! Spike 0.5 measured two different things and got two different answers: 3.89x
//! on four workers rendering one tile from each of many *pages*, and 2.56x on
//! four --- 3.22x on six, nothing at eight --- for six tiles of one A0 page. The
//! second is what a screenful is, and it is the one this measures, through
//! [`RenderService`] rather than through the raw protocol, because the pool being
//! benchmarked is the one the viewer will use.
//!
//! Two regimes, and they are different numbers rather than one with noise in it:
//!
//! - **cold**, where the screenful pays for growing the pool. That is the first
//!   screen of a document, and the spawns are inside the measurement because
//!   they are inside the reader's wait.
//! - **warm**, where the pool is already grown. That is every screen after.
//!
//! Interleaved A,B,A,B across rounds and compared pairwise within a round, per
//! the standing rule --- wall clock on this machine drifts several percent over
//! minutes, which is larger than some of the differences here. Round 0 is
//! discarded: `AGENTS.md` records it as a consistent warm-up outlier.
//!
//! ```text
//! cargo run --release --bin pool-bench -- testdata/vector-heavy.pdf
//! cargo run --release --bin pool-bench -- testdata/vector-heavy.pdf --tiles 6 --rounds 5
//! ```

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Instant;

use tpdf_lib::render::{
    Backend, DocumentInfo, PageSize, RenderService, TileFormat, TileOutcome, TileRequest,
};
use tpdf_lib::{worker, worker_child};

/// Tile side in device pixels.
///
/// Inside the 1024--2048 range `AGENTS.md` measured as useful: below it, PDFium's
/// ~1 s per-call constant is multiplied by the tile count rather than divided by
/// it, and a pool would be measured against a workload nobody should issue.
const TILE: u16 = 1024;

fn main() {
    // This binary is also the worker: `Worker::spawn` re-execs `current_exe`.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        worker_child::main(&args);
    }

    let Some(document) = args.get(1).map(PathBuf::from) else {
        eprintln!("usage: pool-bench <file.pdf> [--tiles N] [--rounds N] [--sizes 1,2,4,6]");
        std::process::exit(2);
    };
    if !document.exists() {
        eprintln!("[FAIL] {} does not exist", document.display());
        std::process::exit(1);
    }

    let tiles = flag(&args, "--tiles").unwrap_or(6);
    let rounds = flag(&args, "--rounds").unwrap_or(4);
    let sizes: Vec<usize> = args
        .iter()
        .position(|a| a == "--sizes")
        .and_then(|i| args.get(i + 1))
        .map_or_else(
            || vec![1, 2, 4, 6],
            |list| list.split(',').filter_map(|s| s.parse().ok()).collect(),
        );

    println!(
        "{} --- {tiles} tiles of {TILE}x{TILE}, {rounds} rounds (round 0 discarded), sizes {sizes:?}\n",
        document.display()
    );

    // One service per size, all built up front and reused across rounds, so the
    // interleaving is over rounds rather than over process startup.
    let mut lanes: Vec<Lane> = Vec::new();
    for size in &sizes {
        match Lane::start(&document, *size, tiles) {
            Ok(lane) => lanes.push(lane),
            Err(e) => {
                eprintln!("[FAIL] pool of {size}: {e}");
                std::process::exit(1);
            }
        }
    }

    // Cold: each size gets a *fresh* document, so the screenful pays for growing
    // the pool. Not interleaved, because it can only be done once per document
    // --- a second cold round would not be cold. Reported separately for that
    // reason rather than mixed into the table below.
    println!("cold --- the first screenful of a document, pool growth included");
    for lane in &mut lanes {
        match lane.cold(&document) {
            Ok(ms) => println!("  pool {:<2}  {ms:8.1} ms", lane.size),
            Err(e) => println!("  pool {:<2}  {e}", lane.size),
        }
    }

    println!("\nwarm --- every screenful after, pool already grown");
    let mut samples: Vec<Vec<f64>> = vec![Vec::new(); lanes.len()];
    for round in 0..rounds {
        for (index, lane) in lanes.iter_mut().enumerate() {
            match lane.screenful() {
                Ok(ms) => {
                    if round > 0 {
                        samples[index].push(ms);
                    }
                    println!("  round {round}  pool {:<2}  {ms:8.1} ms", lane.size);
                }
                Err(e) => {
                    eprintln!("[FAIL] pool {}: {e}", lane.size);
                    std::process::exit(1);
                }
            }
        }
    }

    // Pairwise against the single-worker lane *within each round*, which is what
    // the interleaving is for: a ratio taken across rounds would carry whatever
    // the machine did between them.
    println!("\nspeedup against a pool of one, pairwise within each round");
    let baseline = lanes.iter().position(|lane| lane.size == 1);
    for (index, lane) in lanes.iter().enumerate() {
        // Not shadowing `median`, which is called again below.
        let typical = median(&samples[index]);
        let speedup = baseline.map(|b| {
            let ratios: Vec<f64> = samples[b]
                .iter()
                .zip(&samples[index])
                .map(|(one, many)| one / many)
                .collect();
            median(&ratios)
        });
        match speedup {
            Some(x) => println!("  pool {:<2}  {typical:8.1} ms   {x:.2}x", lane.size),
            None => println!("  pool {:<2}  {typical:8.1} ms", lane.size),
        }
    }
    if baseline.is_none() {
        println!("  (no pool of 1 in --sizes, so there is nothing to compare against)");
    }
}

/// One service at one pool size, with a document open on it.
struct Lane {
    size: usize,
    tiles: usize,
    service: RenderService,
    doc: DocumentInfo,
    /// Rising, so no two requests in a run share a `rid`.
    rid: u64,
}

impl Lane {
    fn start(document: &Path, size: usize, tiles: usize) -> Result<Self, String> {
        let service = RenderService::start_with_pool(library_dir(), Backend::Worker, size);
        let doc = wait(|reply| service.open(document.to_path_buf(), false, reply))?;
        Ok(Self {
            size,
            tiles,
            service,
            doc,
            rid: 1,
        })
    }

    /// A screenful on a document that has never been rendered from.
    ///
    /// Its own service and its own open: the pool is grown by this screenful
    /// rather than before it, which is what makes the number the first screen's
    /// and not the second's.
    fn cold(&mut self, document: &Path) -> Result<f64, String> {
        let service = RenderService::start_with_pool(library_dir(), Backend::Worker, self.size);
        let doc = wait(|reply| service.open(document.to_path_buf(), false, reply))?;
        let mut rid = 1;
        screenful(&service, &doc, self.tiles, &mut rid)
    }

    /// A screenful on the already-warm service.
    fn screenful(&mut self) -> Result<f64, String> {
        screenful(&self.service, &self.doc, self.tiles, &mut self.rid)
    }
}

/// Issues `tiles` tile requests at once and waits for all of them.
///
/// All issued before any is awaited, which is the whole point --- issuing and
/// waiting one at a time would measure the same thing at every pool size, and
/// would do it without ever telling you that it had.
fn screenful(
    service: &RenderService,
    doc: &DocumentInfo,
    tiles: usize,
    rid: &mut u64,
) -> Result<f64, String> {
    let page = doc.pages.first().copied().unwrap_or(PageSize {
        width_pt: 612.0,
        height_pt: 792.0,
    });
    // A row of tiles across the page, at a scale that makes the page wider than
    // the row --- so every tile lands on content rather than off the edge, where
    // PDFium has nothing to do and every pool size looks equally fast.
    let scale = (f32::from(TILE) * tiles as f32 / page.width_pt).max(1.0);

    let (tx, rx) = channel();
    for column in 0..tiles {
        *rid += 1;
        let tx = tx.clone();
        service.tile(
            TileRequest {
                rid: *rid,
                doc: doc.id,
                page: 0,
                scale,
                turns: 0,
                invert: false,
                x: (column as i32) * i32::from(TILE),
                y: 0,
                width: TILE,
                height: TILE,
                format: TileFormat::Raw,
            },
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        );
    }
    drop(tx);

    let started = Instant::now();
    let mut rendered = 0;
    for result in rx {
        match result? {
            TileOutcome::Rendered(_) => rendered += 1,
            TileOutcome::Abandoned => {
                return Err("a tile was abandoned, and nothing withdrew it".into())
            }
        }
    }
    let elapsed = started.elapsed().as_secs_f64() * 1e3;

    // A screenful that rendered fewer tiles than it asked for is a faster
    // screenful, and would read as a win. `AGENTS.md` records a benchmark that
    // reported a perfect frame rate over a document it had not asked for.
    if rendered != tiles {
        return Err(format!("{rendered} of {tiles} tiles came back"));
    }
    Ok(elapsed)
}

/// Drives one of the service's callback-shaped calls to an answer.
fn wait<T: Send + 'static>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, String>) + Send>),
) -> Result<T, String> {
    let (tx, rx) = channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    rx.recv()
        .unwrap_or_else(|_| Err("the render thread stopped".into()))
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted[sorted.len() / 2]
}

fn flag(args: &[String], name: &str) -> Option<usize> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

/// Where Pdfium lives, matching the app's own resolution in development.
fn library_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("vendor/pdfium/lib"))
        .unwrap_or_else(|| PathBuf::from("."))
}
