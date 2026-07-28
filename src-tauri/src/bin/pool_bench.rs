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
//! `--mode retire` measures the other half of the pool: what it costs to hold and
//! what retiring it gives back. The speedup above is the reason to have a pool;
//! this is the reason not to keep one.
//!
//! ```text
//! cargo run --release --bin pool-bench -- testdata/vector-heavy.pdf
//! cargo run --release --bin pool-bench -- testdata/vector-heavy.pdf --tiles 6 --rounds 5
//! cargo run --release --bin pool-bench -- testdata/vector-heavy.pdf --mode retire
//! ```

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use tpdf_lib::render::{
    Backend, DocumentInfo, PageSize, RenderService, TileFormat, TileOutcome, TileRequest,
};
use tpdf_lib::worker::phys_footprint;
use tpdf_lib::{worker, worker_child};

/// An idle timeout no run of the speedup mode reaches.
///
/// Pinned rather than left at the default, and it is a correctness fix rather
/// than a precaution. The warm regime is defined as "the pool is already grown",
/// and each lane sits untouched while the other sizes take their turn in a round
/// --- which on a slow corpus at five sizes is easily past the default thirty
/// seconds. A lane whose pool had been retired in the meantime would be measured
/// **cold** and reported as warm, and the number would look like nothing in
/// particular: a few hundred milliseconds, in a table full of them.
const NO_RETIRE: Duration = Duration::from_secs(3600);

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
    // Read as the value of `--mode`, not as any argument that says "retire": the
    // first positional is a path, and a fixture called `retire.pdf` selecting a
    // different measurement is the kind of thing nobody debugs twice.
    let mode = args
        .iter()
        .position(|a| a == "--mode")
        .and_then(|i| args.get(i + 1))
        .map_or("speedup", String::as_str);
    match mode {
        "speedup" => {}
        "retire" => {
            let idle = Duration::from_millis(flag(&args, "--idle-ms").unwrap_or(4_000) as u64);
            retire_mode(&document, tiles, rounds, idle);
            return;
        }
        other => {
            eprintln!("[FAIL] unknown --mode {other}: expected speedup or retire");
            std::process::exit(2);
        }
    }
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

/// What holding a grown pool costs, and what retiring it gives back.
///
/// Two quantities, and they are the two sides of the same decision:
///
/// - **Memory**, sampled at three points --- one worker, a grown pool, and the
///   pool after it has been retired. Footprint rather than RSS, for the reason
///   `AGENTS.md` gives: a footprint excludes clean file-backed pages, so a worker
///   is not charged for the document it has mapped, and an RSS figure here would
///   mostly be measuring the fixture.
/// - **The bill for it**, which lands on the *next* screenful after a retirement:
///   a spawn, plus a fresh parse of the document. That is the number the idle
///   timeout is chosen against, and it is corpus-dependent by an order of
///   magnitude, so it is measured per corpus rather than quoted once.
///
/// Interleaved within a round --- warm, retire, cold-after-retirement --- rather
/// than in blocks, per the standing rule. The order inside a round is forced
/// rather than chosen: there is no way to measure a screenful after a retirement
/// without first having retired, and the regrowth then leaves the pool warm for
/// the next round, which is what the round after wants. Round 0 is discarded as
/// everywhere else here.
fn retire_mode(document: &Path, tiles: usize, rounds: usize, idle: Duration) {
    println!(
        "{} --- {tiles} tiles of {TILE}x{TILE}, idle timeout {:.1} s, {rounds} rounds \
         (round 0 discarded)\n",
        document.display(),
        idle.as_secs_f64(),
    );

    let service =
        RenderService::start_tuned(library_dir(), Backend::Worker, retire_pool_size(), idle);
    let doc = match wait(|reply| service.open(document.to_path_buf(), false, reply)) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(1);
        }
    };
    let mut rid = 1;

    let lean = footprint(&service);
    if let Err(e) = screenful(&service, &doc, tiles, &mut rid) {
        eprintln!("[FAIL] {e}");
        std::process::exit(1);
    }
    let grown = footprint(&service);

    println!("footprint --- resident bytes charged to the pool, document excluded");
    print_footprint("at open", lean);
    print_footprint("grown", grown);

    let mut warm = Vec::new();
    let mut cold = Vec::new();
    for round in 0..rounds {
        let Ok(a) = screenful(&service, &doc, tiles, &mut rid) else {
            eprintln!("[FAIL] a warm screenful did not come back");
            std::process::exit(1);
        };

        // Waited for rather than slept through, and the wait is bounded: a
        // retirement that does not happen must read as a failed run, not as a
        // suspiciously cheap "cold" number. Without the bound this loop would
        // simply measure two warm screenfuls and report them as a win.
        let retired = settle(idle * 4, Duration::from_millis(100), || {
            workers_of(&service).len() <= 1
        });
        if !retired {
            eprintln!(
                "[FAIL] round {round}: the pool did not retire within {:.0} s --- \
                 the second column would be a warm screenful wearing a cold label",
                (idle * 4).as_secs_f64()
            );
            std::process::exit(1);
        }
        if round == 0 {
            print_footprint("retired", footprint(&service));
            println!("\nscreenful --- warm against the first one after a retirement");
        }

        let Ok(b) = screenful(&service, &doc, tiles, &mut rid) else {
            eprintln!("[FAIL] a screenful after a retirement did not come back");
            std::process::exit(1);
        };
        if round > 0 {
            warm.push(a);
            cold.push(b);
        }
        println!(
            "  round {round}   warm {a:8.1} ms   after retiring {b:8.1} ms   +{:.1} ms",
            b - a
        );
    }

    // Pairwise within a round, which is what the interleaving is for: a
    // difference of medians taken across rounds would carry whatever the machine
    // did between them.
    let penalty: Vec<f64> = warm.iter().zip(&cold).map(|(a, b)| b - a).collect();
    println!(
        "\nmedian warm {:.1} ms, median after retiring {:.1} ms, median regrowth {:+.1} ms",
        median(&warm),
        median(&cold),
        median(&penalty),
    );
    println!(
        "  gave back {:.1} MB of {:.1} MB, and charges the next screenful {:.1} ms for it",
        (grown.saturating_sub(lean)) as f64 / 1e6,
        grown as f64 / 1e6,
        median(&penalty),
    );
}

/// Pool size the retirement mode measures at.
///
/// The app's own, `TPDF_POOL` included, rather than a size from `--sizes`: the
/// question here is what a session actually holds, not how the pool scales, so
/// the number to measure is the one the app would use. The speedup mode above is
/// the one that sweeps sizes.
fn retire_pool_size() -> usize {
    tpdf_lib::render::pool_size()
}

/// The pool's total physical footprint, in bytes.
fn footprint(service: &RenderService) -> u64 {
    workers_of(service)
        .into_iter()
        .filter_map(phys_footprint)
        .sum()
}

fn print_footprint(label: &str, bytes: u64) {
    println!("  {label:<10}{:8.1} MB", bytes as f64 / 1e6);
}

/// This service's worker processes, from the OS table.
///
/// `pgrep`, not bookkeeping of ours: the claim is about processes, and a count
/// derived from the pool's own `Vec` would report what the code under test
/// believes. The spare is excluded by identity --- it is a child too, and it is
/// deliberately not retired, so counting it would put a floor under every sample.
///
/// **Matched on argv, not on parentage alone.** "Every child of this process is a
/// worker" is false under `caffeinate -du <utility>`, which forks a helper to hold
/// the power assertion and then `exec`s the utility in the *parent* --- leaving the
/// helper a child of the process it wrapped. `AGENTS.md` says to wrap long batches
/// in exactly that, so without this filter the count never falls below one and
/// the retirement wait below times out. It was caught by that wait, which is the
/// only reason it is not silently in the numbers.
fn workers_of(service: &RenderService) -> Vec<u32> {
    let spares = service.spare_pids();
    let out = std::process::Command::new("pgrep")
        .arg("-P")
        .arg(std::process::id().to_string())
        // Matches the full argument list, where the marker is. `--` because the
        // pattern begins with a dash.
        .arg("-f")
        .arg("--")
        .arg(worker::WORKER_ARGV)
        .output();
    out.map(|out| {
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .filter(|pid| !spares.contains(pid))
            .collect()
    })
    .unwrap_or_default()
}

/// Polls until a condition holds, or the bound expires.
fn settle(bound: Duration, every: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + bound;
    while Instant::now() < deadline {
        if ready() {
            return true;
        }
        std::thread::sleep(every);
    }
    ready()
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
        let service = RenderService::start_tuned(library_dir(), Backend::Worker, size, NO_RETIRE);
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
        let service =
            RenderService::start_tuned(library_dir(), Backend::Worker, self.size, NO_RETIRE);
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
