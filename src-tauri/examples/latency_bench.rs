//! What one tile costs, decomposed, across the worker boundary the viewer uses.
//!
//! `worker-bench --mode latency` measures this on POSIX and cannot run anywhere
//! else: it carries its own worker, its own `dup2` handover, a socket pair and
//! SBPL profile bisection, none of which has a Windows counterpart. Its own
//! refusal names the per-tile overhead decomposition as the one thing a Windows
//! spike would measure that nothing else does --- `pool-bench` covers parallel
//! scaling, `win-sandbox-probe` the authority rungs, `backend-probe` crash and
//! timeout, and the job object answers limits and footprint in the kernel.
//!
//! So this is that spike, and it is deliberately **not** a port. It drives the
//! *production* [`Worker`] rather than a private one, which buys two things a
//! port would not:
//!
//! - it builds on both platforms, so macOS can cross-check it against
//!   `worker-bench`'s independent POSIX harness --- two implementations that
//!   share no worker code agreeing on a number is worth more than either alone.
//!   **That run has not happened.** Everything recorded about this harness was
//!   measured on Windows, and "it compiles on macOS" is a claim about a compiler;
//! - it measures the boundary the viewer actually crosses. `worker-bench`'s
//!   worker is a spike's worker. This one is the one that ships.
//!
//! **There is no `pipe` variant here, and that is a finding rather than a gap.**
//! `worker-bench` compares moving pixels down the pipe against moving them
//! through shared memory. Production never does the first: `Response` documents
//! that payloads travel through the mapping and never inline, so a pipe row would
//! be measuring a route no tile takes. The same quantity --- what it costs to move
//! N more bytes across the boundary --- is recovered by differencing two variants
//! that *are* real, `raw` against `png`, which carry about a hundredfold
//! different payloads through the identical mechanism.
//!
//! # The four variants
//!
//! * `inproc`  --- rendered in this process, no boundary at all
//! * `raw`     --- `Tile { png: false }`, ~4 MB of RGBA through the mapping
//! * `png`     --- `Tile { png: true }`, usually far less through the same mapping
//! * `control` --- `Outline`, a round trip that carries no tile
//!
//! "Usually" is load-bearing on `png`. A **dense vector page barely compresses**:
//! on the A0 fixture png came back at 4027 KB against raw's 4096, so the two
//! variants move almost the same bytes and differencing them measures noise. The
//! run detects that by materiality --- not by `raw > png`, which is satisfied by a
//! 68 KB gap out of 4 MB and let a *negative* cost per 100 KB be printed once ---
//! and prints `[SKIP]` naming both sizes.
//!
//! There is no ping in the production protocol and this harness deliberately does
//! not add one: a request existing only for a benchmark is a protocol the viewer
//! does not use, which is the exact criticism made of the pipe row above. So
//! `control` is `Outline`, and what it carries besides the round trip is PDFium's
//! outline walk --- which the reply *reports*, as `walk_ms`. That is subtracted
//! rather than warned about, which makes the round trip a **measurement on any
//! fixture** instead of a bound that is only tight on some.
//!
//! The subtraction is checked rather than trusted, by two routes that must agree:
//! the entry count parsed out of the reply, and the walk time in the same reply.
//! They disagree exactly when the parse is wrong --- which happened, on the first
//! run against `outline-simple.pdf`: the reply is an object and the harness asked
//! it for an array, so `as_array()` gave `None`, a defaulted `0` read as *"the
//! document has no outline"*, and the run claimed a tight bound on the one fixture
//! in the corpus that exists to have an ordinary outline. Its own control timing
//! said 0.460 ms against 0.041 ms four lines above. Both branches of that check
//! have since been shown to fire under mutation.
//!
//! Evidence the round trip is now right rather than merely different: it reads
//! 0.039--0.068 ms across three fixtures whose outlines are 0, 0 and 10 entries.
//! Before the subtraction those same runs spanned an order of magnitude.
//!
//! Interleaved A,B,C,D within each round and compared pairwise, per the standing
//! rule: wall clock drifts several percent over minutes, which is larger than
//! some of the differences here. Round 0 is discarded as a warm-up outlier --- and
//! printed anyway, rather than quietly dropped.
//!
//! Every pixel-bearing variant folds its whole payload in the parent, timed
//! separately, so none of them can look cheap by never reading what it received.
//!
//! ```text
//! cargo run --release --example latency-bench -- testdata/vector-heavy.pdf
//! cargo run --release --example latency-bench -- testdata/text-base14.pdf --rounds 5 --reps 4
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use tpdf_lib::progressive::{self, RawDocument};
use tpdf_lib::worker;
use tpdf_lib::worker::{Reply, Request, Worker};
use tpdf_lib::worker_child;

/// Tile side in device pixels.
///
/// Inside the 1024--2048 range `AGENTS.md` records as useful: below it, PDFium's
/// large per-*call* constant is multiplied by the tile count rather than divided
/// by it, and the decomposition would be dominated by a workload nobody issues.
const TILE: u16 = 1024;

/// A rid that is withdrawable but never withdrawn.
///
/// Zero would work and is worse: `Request::Tile` documents zero as *not*
/// withdrawable, so it takes a different path through the queue than any tile the
/// viewer issues, and this measures the viewer's path.
const RID: u64 = 1;

/// One variant's timings for one round, all per-tile means in milliseconds.
struct Row {
    round: usize,
    variant: &'static str,
    wall: f64,
    render: f64,
    encode: f64,
    fold: f64,
    bytes: usize,
}

impl Row {
    /// Everything the tile cost that was not rendering, encoding or reading it.
    ///
    /// Serialization, the pipe write, waking the worker, the reply read. For
    /// `inproc` this is the harness's own overhead and should be near zero ---
    /// which makes it a control on the decomposition itself: a large `inproc`
    /// transport means the subtraction is wrong, not that the boundary is slow.
    fn transport(&self) -> f64 {
        self.wall - self.render - self.encode - self.fold
    }
}

/// Verdicts, padded to a fixed width at column 1 and counted.
///
/// Both properties are load-bearing and neither is cosmetic. **Padded**, because
/// every documented recipe in this repository slices a fixed offset --- the label
/// is seven characters and `grep -hoE "^\[[A-Z]+\] *" | awk '{print length($0)}'
/// | sort -u` must print exactly one value; interpolating `OK` into `[{}]` is what
/// made three identical name sets read as divergent once. **At column 1**, because
/// an indented verdict does not match `^\[` --- and the width recipe then passes by
/// never examining the line that would have failed it, which is what a `[SKIP]`
/// indented by two spaces did here.
///
/// The tag vocabulary is closed on purpose: `OK`, `FAIL`, `SKIP`, `WARN`. A fifth
/// one invented for a single line --- there was a `[NOTE]` here --- is dropped
/// silently by anything grepping the set every other harness emits.
#[derive(Default)]
struct Report {
    checks: usize,
    failures: usize,
    skipped: usize,
    warnings: usize,
}

impl Report {
    fn check(&mut self, ok: bool, detail: impl AsRef<str>) {
        self.checks += 1;
        if !ok {
            self.failures += 1;
        }
        let label = if ok { "[OK]" } else { "[FAIL]" };
        println!("{label:7}{}", detail.as_ref());
    }

    fn warn(&mut self, detail: impl AsRef<str>) {
        self.checks += 1;
        self.warnings += 1;
        println!("{:7}{}", "[WARN]", detail.as_ref());
    }

    fn skip(&mut self, detail: impl AsRef<str>) {
        self.checks += 1;
        self.skipped += 1;
        println!("{:7}{}", "[SKIP]", detail.as_ref());
    }

    /// Non-zero on a failure so a scripted run can see one.
    ///
    /// A `[WARN]` deliberately does not fail the run: every warning here reports
    /// that a *derived* figure is untrustworthy, not that the measurement broke,
    /// and the table above it is still worth reading. It is counted in the summary
    /// so it cannot pass unnoticed either.
    fn finish(&self) -> ! {
        println!();
        println!(
            "{}/{} checks passed, {} skipped, {} warning(s)",
            self.checks - self.failures - self.skipped - self.warnings,
            self.checks,
            self.skipped,
            self.warnings
        );
        std::process::exit(i32::from(self.failures > 0));
    }
}

fn main() {
    // This binary is also the worker: `Worker::spawn` re-execs `current_exe`.
    // No `cfg` here --- `worker_child::main` compiles on every platform and
    // refuses inside `establish_boundary` where there is no boundary to
    // establish. Gating it on `unix` is what left two sibling benchmarks unable
    // to act as the thing they measure; see the trap of that name.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        worker_child::main(&args);
    }

    let Some(document) = args.get(1).map(PathBuf::from) else {
        eprintln!("usage: latency-bench <file.pdf> [--rounds N] [--reps N] [--page N] [--scale F]");
        std::process::exit(2);
    };
    if !document.exists() {
        eprintln!("[FAIL] {} does not exist", document.display());
        std::process::exit(1);
    }

    let rounds = flag(&args, "--rounds").unwrap_or(4);
    let reps = flag(&args, "--reps").unwrap_or(3);
    let page = flag(&args, "--page").unwrap_or(0) as u32;
    let scale = args
        .iter()
        .position(|a| a == "--scale")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(1.0);

    if rounds < 2 {
        eprintln!("[FAIL] --rounds must be at least 2: round 0 is discarded as warm-up");
        std::process::exit(2);
    }

    if let Err(e) = run(&document, rounds, reps, page, scale) {
        eprintln!("[FAIL] {e}");
        std::process::exit(1);
    }
}

fn run(document: &Path, rounds: usize, reps: usize, page: u32, scale: f32) -> Result<(), String> {
    let library_dir = library_dir();

    println!("document   {}", document.display());
    println!("tile       {TILE}x{TILE} at scale {scale}, page {page}");
    println!("schedule   {rounds} rounds x {reps} reps, interleaved, round 0 discarded");
    println!();

    // The parent binds PDFium for the `inproc` variant, so *this process* maps
    // the parser. That is fine here and is not a hole in the boundary claim,
    // which is about the app process: a benchmark whose whole purpose is to
    // compare in-process against out-of-process has to have both. Stated rather
    // than left for a module scan to turn up as a surprise.
    let pdfium = progressive::bind(&library_dir)?;
    let bindings = progressive::bindings_of(pdfium);
    let doc = RawDocument::open(bindings, document, None)?;
    let pages = doc.page_count();
    if page >= pages {
        return Err(format!("--page {page} but the document has {pages}"));
    }
    let local_page = doc.page(page)?;
    let spec = progressive::TileSpec {
        scale,
        turns: 0,
        x: 0,
        y: 0,
        width: TILE,
        height: TILE,
    };
    let cancel = progressive::CancelToken::new();

    let mut worker = Worker::spawn(document, &library_dir)?;
    let opened = worker.call(&Request::Open {
        lazy_geometry: true,
    })?;
    if !opened.ok {
        return Err(format!(
            "the worker could not open the document: {}",
            opened.error
        ));
    }
    println!("worker     pid {} opened the document", worker.pid());

    // Whether `control` is a tight bound depends on this, so it is measured
    // rather than assumed, and reported either way.
    //
    // Read `total` out of the object rather than treating the reply as an array.
    // It was an array here for one run, `as_array()` returned `None` on an object
    // and the `unwrap_or(0)` made that read as **"the document has no outline"**
    // --- which is the reassuring branch, printed on `outline-simple.pdf`, the one
    // fixture in the corpus that exists to have an ordinary outline. Nothing
    // failed; the run just quietly claimed a tight bound it did not have. See the
    // trap of that name.
    let outline = worker.call(&Request::Outline)?;
    // Refused rather than defaulted. A count this harness cannot read is not a
    // count of zero, and zero is the answer that suppresses the warning --- so a
    // defaulted parse fails in the direction that looks like good news.
    //
    // It used to read `.get("total")` off a `serde_json::Value`, which is the
    // same refusal guarding a weaker question: a renamed field arrived here as a
    // missing key rather than as a compile error. `Reply::Outline` carries the
    // real `Outline`, so `total` is a field and this can now only fail on a
    // worker answering the wrong request entirely.
    let Some(Reply::Outline(read)) = &outline.reply else {
        return Err(format!(
            "the outline reply is not an outline: {:?}",
            outline.reply
        ));
    };
    let outline_total = read.total as u64;
    println!("outline    {outline_total} entries at every depth");
    println!();

    const VARIANTS: [&str; 4] = ["inproc", "raw", "png", "control"];
    let mut rows: Vec<Row> = Vec::new();

    for round in 0..rounds {
        for variant in VARIANTS {
            let t0 = Instant::now();
            let mut render_us = 0u64;
            let mut encode_us = 0u64;
            let mut fold_us = 0u64;
            let mut bytes = 0usize;
            let mut sink = 0u64;

            for _ in 0..reps {
                match variant {
                    "inproc" => {
                        let t = Instant::now();
                        let (rgba, _) =
                            progressive::render_tile(bindings, &local_page, spec, None, &cancel)?;
                        render_us += t.elapsed().as_micros() as u64;
                        bytes = rgba.len();
                        let t = Instant::now();
                        sink = sink.wrapping_add(checksum(&rgba));
                        fold_us += t.elapsed().as_micros() as u64;
                    }
                    "control" => {
                        let response = worker.call(&Request::Outline)?;
                        if !response.ok {
                            return Err(response.error);
                        }
                        // The walk is the only PDFium work in this variant, and
                        // the reply reports it. Accumulated into the `render`
                        // column so it is *subtracted* rather than warned about:
                        // that turns the round trip from an upper bound into a
                        // figure, on a fixture with an outline as well as one
                        // without.
                        let walk_ms = match &response.reply {
                            Some(Reply::Outline(read)) => read.walk_ms,
                            _ => 0.0,
                        };
                        render_us += (walk_ms * 1000.0) as u64;
                    }
                    tile => {
                        let png = tile == "png";
                        let response = worker.call(&Request::Tile {
                            crop: None,
                            rid: RID,
                            page,
                            scale,
                            turns: 0,
                            invert: false,
                            x: 0,
                            y: 0,
                            width: TILE,
                            height: TILE,
                            png,
                        })?;
                        if !response.ok {
                            return Err(response.error);
                        }
                        if response.abandoned {
                            return Err(
                                "the worker abandoned a tile nothing withdrew --- the queue is \
                                 not in the state this measurement assumes"
                                    .into(),
                            );
                        }
                        render_us += response.render_us;
                        encode_us += response.encode_us;
                        bytes = response.bytes;
                        // Folded in the parent so a variant cannot look cheap by
                        // never reading what it was sent. Timed separately so
                        // that cost lands in its own column rather than in
                        // transport.
                        let t = Instant::now();
                        sink =
                            sink.wrapping_add(checksum(&worker.tile.as_slice()[..response.bytes]));
                        fold_us += t.elapsed().as_micros() as u64;
                    }
                }
            }

            let reps_f = reps as f64;
            rows.push(Row {
                round,
                variant,
                wall: t0.elapsed().as_secs_f64() * 1000.0 / reps_f,
                render: render_us as f64 / 1000.0 / reps_f,
                encode: encode_us as f64 / 1000.0 / reps_f,
                fold: fold_us as f64 / 1000.0 / reps_f,
                bytes,
            });
            std::hint::black_box(sink);
        }
    }

    // Diverges: the summary decides the exit code, so there is no path back here.
    report(&rows, rounds, outline_total)
}

fn report(rows: &[Row], rounds: usize, outline_total: u64) -> ! {
    println!(
        "{:>5}  {:<8} {:>11} {:>10} {:>10} {:>11} {:>11} {:>10}",
        "round", "variant", "end-to-end", "render", "encode", "parent fold", "transport", "payload"
    );
    for r in rows {
        println!(
            "{:>5}  {:<8} {:>10.3}ms {:>9.3}ms {:>9.3}ms {:>10.3}ms {:>10.3}ms {:>9} KB",
            r.round,
            r.variant,
            r.wall,
            r.render,
            r.encode,
            r.fold,
            r.transport(),
            r.bytes / 1024
        );
    }
    println!();

    let steady: Vec<&Row> = rows.iter().filter(|r| r.round > 0).collect();
    let mean = |variant: &str, f: fn(&Row) -> f64| {
        let picked: Vec<f64> = steady
            .iter()
            .filter(|r| r.variant == variant)
            .map(|r| f(r))
            .collect();
        picked.iter().sum::<f64>() / picked.len() as f64
    };
    let payload = |variant: &str| {
        steady
            .iter()
            .find(|r| r.variant == variant)
            .map_or(0, |r| r.bytes)
    };

    println!(
        "means over rounds 1..{} (round 0 excluded as warm-up):",
        rounds - 1
    );
    for variant in ["inproc", "raw", "png", "control"] {
        println!(
            "  {variant:<8} {:>7.3} ms end to end = {:.3} render + {:.3} encode + \
             {:.3} parent fold + {:.3} transport",
            mean(variant, |r| r.wall),
            mean(variant, |r| r.render),
            mean(variant, |r| r.encode),
            mean(variant, |r| r.fold),
            mean(variant, Row::transport),
        );
    }
    println!();

    let raw_bytes = payload("raw");
    let png_bytes = payload("png");

    // Differenced on the **transport** column, not on end-to-end.
    //
    // The obvious estimator is `raw wall - inproc wall`, and it is wrong wherever
    // rendering dominates: on the A0 fixture that subtracts two ~2.7 s numbers to
    // recover a ~0.4 ms one, so it reports render noise and nothing else. It read
    // **-265 ms** there --- a negative boundary cost, on a run whose transport
    // columns were a perfectly sensible 0.152 against 0.445 ms. Both columns are
    // small and both exclude the render, so their difference is the quantity
    // wanted rather than the residue of two large ones.
    //
    // Computed **per round**, with the headline figure as the mean of those, so
    // the number and its spread come from one derivation rather than two. They
    // were two for a while, and a mutation swapping the estimator moved only the
    // figure while the spread went on being computed the sound way --- so the
    // check comparing them passed on a run whose estimator had been broken on
    // purpose. Two routes to one quantity have to be tied together or the
    // agreement between them means nothing.
    let per_round: Vec<f64> = (1..rounds)
        .filter_map(|round| {
            let at = |variant: &str| {
                steady
                    .iter()
                    .find(|r| r.round == round && r.variant == variant)
                    .map(|r| r.transport())
            };
            Some(at("raw")? - at("inproc")?)
        })
        .collect();
    let boundary = per_round.iter().sum::<f64>() / per_round.len() as f64;

    println!("derived:");
    println!(
        "  crossing the boundary at all         {boundary:>8.3} ms   (raw transport \
         minus inproc transport; see below for why not end-to-end)"
    );
    println!(
        "  a round trip carrying no tile        {:>8.3} ms   (the outline walk, \
         {:.3} ms, subtracted from {:.3} ms end-to-end)",
        mean("control", Row::transport),
        mean("control", |r| r.render),
        mean("control", |r| r.wall)
    );

    // The whole reason there is no `pipe` row. Both variants cross the identical
    // mechanism, so what is left when one is subtracted from the other is the
    // cost attributable to the extra bytes and nothing else.
    //
    // `raw_bytes > png_bytes` is not a sufficient guard, which the A0 fixture
    // showed: a dense vector page barely compresses, so png came back at 4027 KB
    // against raw's 4096 --- the test passed on a 68 KB difference and the run
    // divided sub-millisecond noise by it, reporting a *negative* cost per 100 KB.
    // The payloads have to differ by enough for the difference to be the signal,
    // so materiality is the condition, not ordering.
    let ratio = if raw_bytes > 0 {
        png_bytes as f64 / raw_bytes as f64
    } else {
        1.0
    };
    let differencing_applies = raw_bytes > png_bytes && ratio < 0.5;
    if differencing_applies {
        let extra_kb = (raw_bytes - png_bytes) as f64 / 1024.0;
        let extra_ms = mean("raw", Row::transport) - mean("png", Row::transport);
        println!(
            "  moving {extra_kb:.0} KB more through the mapping {extra_ms:>8.3} ms   ({:.4} ms per 100 KB)",
            extra_ms / extra_kb * 100.0
        );
    }

    // Rendering is the same work in both variants and is *supposed* to cancel.
    // How far it actually cancels is the error a wall-based estimator would have
    // carried, so it is reported beside the figure rather than left as a general
    // caution --- on the A0 sheet it is hundreds of times the quantity estimated,
    // which is why that estimator is not used.
    let render_gap = (mean("raw", |r| r.render) - mean("inproc", |r| r.render)).abs();
    println!(
        "  the same render varies by            {render_gap:>8.3} ms   (would have been the error in a wall-based estimator)"
    );
    println!();

    let mut report = Report::default();

    if !differencing_applies {
        report.skip(format!(
 "payload differencing: raw is {} KB and png {} KB ({:.0}% of it). This document does not compress, so the two variants move almost the same bytes and their difference is noise rather than the cost of the extra ones.",
            raw_bytes / 1024,
            png_bytes / 1024,
            ratio * 100.0
        ));
    }

    // Whether the boundary figure is *reproducible*, which is the property that
    // actually separates a sound estimator from a broken one here.
    //
    // The obvious check is the sign --- a process boundary cannot cost less than
    // nothing --- and it was tried first. It is far too weak, and the mutation that
    // showed so is the useful part: restoring the wall-based estimator on the A0
    // fixture *survived*, because -265.822 ms was one sample of a noisy quantity
    // and the next run of the same broken arithmetic landed positive. A check that
    // fires only when the noise falls one way is decoration on every run where it
    // does not.
    //
    // Spread across rounds does discriminate, because that is exactly where the
    // two estimators differ: the render noise a wall-based difference carries is
    // hundreds of times the quantity itself, so it cannot repeat, while a
    // difference of two transport columns lands in the same place every round.
    let lo = per_round.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = per_round.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let spread = if lo.is_finite() && hi.is_finite() {
        hi - lo
    } else {
        f64::INFINITY
    };
    let rounds_used = per_round.len();
    report.check(
        boundary > 0.0 && spread < boundary.abs(),
        format!(
            "the boundary costs {boundary:.3} ms and repeats within {spread:.3} ms across {rounds_used} rounds. It must be positive, since a boundary cannot be free, and must repeat, since an estimator carrying render noise cannot."
        ),
    );

    // A control on the arithmetic rather than on the system. `inproc` crosses no
    // boundary, so its transport column is pure subtraction error; a large value
    // there invalidates every other transport figure above, and saying so is
    // cheaper than having someone read a decomposition that does not add up.
    let residual = mean("inproc", Row::transport);
    let wall = mean("inproc", |r| r.wall);
    let share = if wall > 0.0 {
        residual / wall * 100.0
    } else {
        0.0
    };
    if share.abs() > 5.0 {
        report.warn(format!(
 "inproc has {residual:.3} ms ({share:.1}%) of unattributed time. It crosses no boundary, so the decomposition is losing time somewhere and every transport figure above is suspect by about that much."
        ));
    } else {
        report.check(
            true,
            format!(
 "inproc's unattributed time is {residual:.3} ms ({share:.1}% of its end-to-end), so the subtraction accounts for the run."
            ),
        );
    }

    // Two ways of knowing whether the control did any PDFium work --- the entry
    // count parsed out of the reply, and the walk time the reply also reports ---
    // cross-checked against each other rather than either one trusted. This is
    // here because the first version trusted the count alone, misparsed it, and
    // printed "the document has no outline" for `outline-simple.pdf` while the
    // control's own timing four lines above said 0.460 ms against 0.041 ms on a
    // document that really has none.
    let walk = mean("control", |r| r.render);
    match (outline_total > 0, walk > 0.010) {
        (true, true) => report.check(
            true,
            format!(
 "the outline has {outline_total} entries and the control reports {walk:.3} ms walking it; both agree it did real work, and it is subtracted above."
            ),
        ),
        (false, false) => report.check(
            true,
            format!(
 "the outline is empty and the control reports {walk:.3} ms walking it; both agree, so the round trip above is measured rather than bounded."
            ),
        ),
        (true, false) => report.warn(format!(
 "the outline has {outline_total} entries but the control reports only {walk:.3} ms walking it. One of the two is wrong, and the round trip above is derived from the second."
        )),
        (false, true) => report.warn(format!(
 "the outline parsed as empty but the control spent {walk:.3} ms walking it. That is the misparse this check exists to catch --- treat the round trip above as an upper bound, not a measurement."
        )),
    }

    println!();
    println!(
        "note: every pixel-bearing variant folds its whole payload in the parent, timed \
         separately above, so none of them can look cheap by never reading what it received."
    );
    report.finish();
}

/// Cheap whole-buffer fold, so a payload cannot be received and ignored.
fn checksum(bytes: &[u8]) -> u64 {
    let mut acc = 0u64;
    for chunk in bytes.chunks(8) {
        let mut word = [0u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        acc = acc.wrapping_mul(31).wrapping_add(u64::from_le_bytes(word));
    }
    acc
}

fn flag(args: &[String], name: &str) -> Option<usize> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

/// Where PDFium lives, matching the app's own resolution in development.
///
/// The subdirectory differs by platform and the difference is not cosmetic:
/// Windows ships the loadable DLL in `bin/` and puts only the *import* library in
/// `lib/`, so joining `lib` unconditionally finds a directory that exists and
/// holds nothing loadable. See the trap of that name.
fn library_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR))
        .unwrap_or_else(|| PathBuf::from("."))
}
