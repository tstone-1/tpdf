//! Phase 1: is a PDFium render actually cancellable, and what does it cost?
//!
//! The A0 vector sheet scrolls at a flawless 60 fps over a screen that is 0--4%
//! sharp (spike 0.8). That is a latency failure, not a throughput one: a render
//! takes about a second, `FPDF_RenderPageBitmap()` cannot be interrupted once
//! entered, and so the renderer stays busy on a tile the viewport left long ago.
//! The progressive API is the only lever PDFium offers, and this measures
//! whether it is a real one.
//!
//! It exercises `tpdf_lib::progressive` rather than a copy of it, so what is
//! measured here is what ships.
//!
//! Four questions, four modes:
//!
//! * `--mode identity` --- does pausing change the pixels? Renders the same tile
//!   through the safe all-or-nothing path and through the progressive path, both
//!   uninterrupted and chopped into slices, and compares byte-for-byte. The
//!   sliced variant carries its own control: **`polls` must be non-zero**, or a
//!   "pausing is lossless" result only proves that nothing paused.
//!
//! * `--mode poll` --- how often does PDFium ask? Cancellation can only happen at
//!   a poll, so the longest gap between two polls is the real latency bound, and
//!   it is a property of PDFium rather than of how small a slice we request.
//!   Interleaved A/B against an unpaused render to price the pausing itself.
//!
//! * `--mode cancel` --- does cancelling from another thread work, how fast, and
//!   what is in the bitmap afterwards? A cancellation that returns instantly and
//!   leaves nothing behind is not obviously better than one that finishes, so the
//!   bitmap is characterised as well as the latency.
//!
//! * `--mode pageload` --- prices `RawDocument`'s page cache against loading per
//!   request, interleaved. `FPDF_LoadPage` re-parses the page every call, so the
//!   answer is a function of page complexity: 0.18 ms on the text corpus and
//!   44.3 ms on the A0 sheet.
//!
//! On the `cancel` mode's similarity columns, and why there are three of them:
//! each was added because the previous one could not tell a real failure from a
//! real success. `white`/`zero` exist because "fraction of pixels with ink"
//! reports an untouched all-zero buffer as 100% ink. `matching` exists to say how
//! much of the tile is final. `mean err` exists because `matching` reads 0% on a
//! dense page even when the partial is visibly most of the way there. **All three
//! saturate on the A0 fixture**, which is antialiased random linework covering
//! every pixel --- so it can prove that a partial composite exists, and cannot
//! say whether one is worth showing. Use `--dump` and look.
//!
//! The cancelling thread touches no PDFium state --- it sets an `AtomicBool` and
//! nothing else. Concurrent PDFium calls remain undefined behaviour (see
//! `thread_probe.rs`).
//!
//! Usage:
//!   progressive-probe <file.pdf> [--page N] [--scale F] [--tile N]
//!                     [--mode identity|poll|cancel|pageload] [--rounds N]
//!                     [--slices 0,1,4,16] [--after 50] [--dump DIR] [--lib DIR]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pdfium_render::prelude::*;
use tpdf_lib::progressive::{self, CancelToken, Outcome, Progress, RawDocument, RawPage};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Identity,
    Poll,
    Cancel,
    PageLoad,
}

struct Args {
    file: PathBuf,
    page: u32,
    scale: f32,
    tile: u16,
    mode: Mode,
    rounds: usize,
    /// Pause slices in milliseconds. 0 means "pause at every opportunity".
    slices: Vec<u64>,
    /// When to cancel, in milliseconds after the render starts.
    after_ms: u64,
    /// Where to write the complete and cancelled tiles as PNGs, if anywhere.
    dump: Option<PathBuf>,
    library_dir: PathBuf,
}

/// Writes an RGBA tile out so it can be looked at.
fn write_png(path: &Path, rgba: &[u8], size: u16) {
    let file = match std::fs::File::create(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[WARN] could not write {}: {e}", path.display());
            return;
        }
    };
    let mut encoder = png::Encoder::new(file, size as u32, size as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    match encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(rgba))
    {
        Ok(()) => println!("    wrote {}", path.display()),
        Err(e) => eprintln!("[WARN] could not encode {}: {e}", path.display()),
    }
}

fn main() {
    let args = parse_args();

    let path = Pdfium::pdfium_platform_library_name_at_path(&args.library_dir);
    let bindings = match Pdfium::bind_to_library(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[FAIL] could not load Pdfium from {}: {e}", path.display());
            eprintln!("       run scripts/fetch_pdfium.py");
            std::process::exit(2);
        }
    };
    let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
    let raw = progressive::bindings_of(pdfium);

    println!("file      {}", args.file.display());
    println!("page      {}", args.page);
    println!("scale     {}", args.scale);
    println!("tile      {}x{}", args.tile, args.tile);
    println!(
        "mode      {}",
        match args.mode {
            Mode::Identity => "identity",
            Mode::Poll => "poll",
            Mode::Cancel => "cancel",
            Mode::PageLoad => "pageload",
        }
    );
    println!();

    let document = match RawDocument::open(raw, &args.file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(2);
        }
    };
    let page = match document.page(args.page) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(2);
        }
    };

    let ok = match args.mode {
        Mode::Identity => identity(pdfium, raw, &page, &args),
        Mode::Poll => poll(raw, &page, &args),
        Mode::Cancel => cancel(raw, &page, &args),
        Mode::PageLoad => pageload(raw, &document, &args),
    };

    println!();
    if ok {
        println!("[OK] all checks passed");
    } else {
        println!("[FAIL] see above");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// identity: pausing must not change a pixel
// ---------------------------------------------------------------------------

/// Renders one tile every way available and compares the results.
///
/// The baseline is the safe path, because that is what `render.rs` ships today
/// and what the frontend has been looking at through every spike. If the
/// progressive path differs from it, the progressive path is wrong.
fn identity(
    pdfium: &'static Pdfium,
    raw: progressive::Bindings,
    page: &RawPage<'_>,
    args: &Args,
) -> bool {
    let (x, y) = centre_tile_origin(page, args);

    let baseline = match safe_tile(pdfium, args, x, y) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("[FAIL] safe render: {e}");
            return false;
        }
    };
    println!(
        "safe path                      {:>10} bytes  digest {:016x}",
        baseline.len(),
        digest(&baseline)
    );

    let mut ok = true;

    // Uninterrupted: proves the raw handles, flags, clear colour and placement
    // all match the safe path before pausing is introduced as a variable.
    let (bytes, progress) = match render(raw, page, args, x, y, None, &CancelToken::new()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[FAIL] progressive render: {e}");
            return false;
        }
    };
    ok &= report_identity("progressive, no slice", &baseline, &bytes, progress, false);

    // Sliced: the render is chopped into pieces and resumed. `expect_pauses`
    // makes the control mandatory -- without it, a slice so large that PDFium
    // never pauses would report a clean identity result and prove nothing.
    for slice_ms in &args.slices {
        let slice = Duration::from_millis(*slice_ms);
        let (bytes, progress) =
            match render(raw, page, args, x, y, Some(slice), &CancelToken::new()) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[FAIL] progressive render: {e}");
                    return false;
                }
            };
        let label = format!("progressive, {slice_ms} ms slice");
        ok &= report_identity(&label, &baseline, &bytes, progress, true);
    }

    ok
}

/// Prints one identity row and returns whether it passed.
fn report_identity(
    label: &str,
    baseline: &[u8],
    bytes: &[u8],
    progress: Progress,
    expect_pauses: bool,
) -> bool {
    let same = bytes == baseline;
    let mut ok = same && progress.outcome.is_done();

    println!(
        "{label:<30} {:>10} bytes  digest {:016x}  polls {:>6}  resumes {:>5}  {:.1} ms",
        bytes.len(),
        digest(bytes),
        progress.polls,
        progress.resumes,
        progress.elapsed.as_secs_f64() * 1000.0
    );

    if !same {
        println!(
            "    [FAIL] pixels differ from the safe path: {}",
            first_difference(baseline, bytes)
        );
    }
    if !progress.outcome.is_done() {
        println!(
            "    [FAIL] outcome was {:?}, expected Done",
            progress.outcome
        );
    }
    if expect_pauses && progress.resumes == 0 {
        // The whole point of the row. A slice that never paused compares the
        // progressive path against itself.
        println!("    [FAIL] never paused, so this row proves nothing about pausing");
        ok = false;
    }

    ok
}

/// Describes where two buffers first disagree, so a mismatch is diagnosable
/// rather than just reported.
fn first_difference(a: &[u8], b: &[u8]) -> String {
    if a.len() != b.len() {
        return format!("lengths {} vs {}", a.len(), b.len());
    }
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => {
            let differing = a.iter().zip(b).filter(|(x, y)| x != y).count();
            format!(
                "first at byte {i} (pixel {}, channel {}), {differing} of {} bytes differ",
                i / 4,
                i % 4,
                a.len()
            )
        }
        None => "no differing byte, which contradicts the comparison".to_string(),
    }
}

// ---------------------------------------------------------------------------
// poll: how often does PDFium hand control back?
// ---------------------------------------------------------------------------

/// Measures poll frequency, the worst poll gap, and what pausing costs.
///
/// Variants are interleaved A,B,A,B across rounds and compared pairwise, because
/// wall clock on these machines drifts more over minutes than most differences
/// worth acting on.
fn poll(raw: progressive::Bindings, page: &RawPage<'_>, args: &Args) -> bool {
    let (x, y) = centre_tile_origin(page, args);
    let token = CancelToken::new();

    println!(
        "{:<22} {:>10} {:>10} {:>8} {:>12} {:>12}",
        "variant", "elapsed ms", "vs none", "polls", "mean gap ms", "max gap ms"
    );

    let mut ok = true;
    let mut worst_gap = Duration::ZERO;

    for round in 0..args.rounds {
        let mut unpaused = Duration::ZERO;

        for (i, slice_ms) in std::iter::once(&u64::MAX)
            .chain(args.slices.iter())
            .enumerate()
        {
            let slice = (*slice_ms != u64::MAX).then(|| Duration::from_millis(*slice_ms));
            let (_, progress) = match render(raw, page, args, x, y, slice, &token) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[FAIL] {e}");
                    return false;
                }
            };

            if i == 0 {
                unpaused = progress.elapsed;
            }
            if progress.max_poll_gap > worst_gap {
                worst_gap = progress.max_poll_gap;
            }
            if !progress.outcome.is_done() {
                println!(
                    "    [FAIL] outcome was {:?}, expected Done",
                    progress.outcome
                );
                ok = false;
            }

            let label = match slice {
                None => "none".to_string(),
                Some(_) => format!("{slice_ms} ms slice"),
            };
            let mean_gap = if progress.polls > 0 {
                progress.elapsed.as_secs_f64() * 1000.0 / progress.polls as f64
            } else {
                0.0
            };
            println!(
                "r{round} {label:<18} {:>10.1} {:>9.2}x {:>8} {:>12.3} {:>12.3}",
                progress.elapsed.as_secs_f64() * 1000.0,
                progress.elapsed.as_secs_f64() / unpaused.as_secs_f64(),
                progress.polls,
                mean_gap,
                progress.max_poll_gap.as_secs_f64() * 1000.0,
            );
        }
        println!();
    }

    println!(
        "worst poll gap over all runs: {:.3} ms",
        worst_gap.as_secs_f64() * 1000.0
    );
    println!("    That is the bound on cancellation latency. Asking for a smaller");
    println!("    slice cannot beat it: it says how long PDFium goes without asking.");

    ok
}

// ---------------------------------------------------------------------------
// cancel: does it stop, how fast, and what is left in the bitmap?
// ---------------------------------------------------------------------------

/// Cancels a render in flight from another thread and measures the latency.
fn cancel(raw: progressive::Bindings, page: &RawPage<'_>, args: &Args) -> bool {
    let (x, y) = centre_tile_origin(page, args);

    // The reference: what the tile looks like when nothing interrupts it. Also
    // the number the cancellation latency has to be compared against -- stopping
    // "in 40 ms" means nothing without knowing the render takes a second.
    let (complete, full) = match render(raw, page, args, x, y, None, &CancelToken::new()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            return false;
        }
    };
    let (full_white, full_zero) = composition(&complete);
    println!(
        "uninterrupted render: {:.1} ms, {} polls, {:.1}% white, {:.1}% zero\n",
        full.elapsed.as_secs_f64() * 1000.0,
        full.polls,
        full_white * 100.0,
        full_zero * 100.0
    );

    // `matching` alone cannot tell a bitmap PDFium never touched from one it
    // drew the wrong thing into -- both read as 0%. The composition columns
    // discriminate, and they answer the question that decides whether a
    // cancelled tile is worth showing: does PDFium composite as it goes, or only
    // at the end?
    println!(
        "{:<8} {:>12} {:>12} {:>10} {:>10} {:>9} {:>8} {:>8}",
        "round", "cancel at ms", "latency ms", "outcome", "matching", "mean err", "white", "zero"
    );

    let mut ok = true;
    let mut worst = Duration::ZERO;

    for round in 0..args.rounds {
        let token = CancelToken::new();
        let origin = Instant::now();
        let set_at_ns = Arc::new(AtomicU64::new(0));

        let watcher = {
            let token = token.clone();
            let set_at_ns = Arc::clone(&set_at_ns);
            let after = Duration::from_millis(args.after_ms);
            // Sets an AtomicBool and nothing else. It never touches Pdfium, so
            // the thread-safety trap does not apply.
            std::thread::spawn(move || {
                std::thread::sleep(after);
                token.cancel();
                set_at_ns.store(origin.elapsed().as_nanos() as u64, Ordering::Relaxed);
            })
        };

        let (bytes, progress) = match render(raw, page, args, x, y, None, &token) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[FAIL] {e}");
                return false;
            }
        };
        let returned_ns = origin.elapsed().as_nanos() as u64;
        watcher.join().expect("watcher thread panicked");

        let set_at = set_at_ns.load(Ordering::Relaxed);
        let latency = Duration::from_nanos(returned_ns.saturating_sub(set_at));
        if latency > worst {
            worst = latency;
        }

        let matching = matching_fraction(&complete, &bytes);
        let (white, zero) = composition(&bytes);
        println!(
            "r{round:<7} {:>12.1} {:>12.3} {:>10} {:>9.2}% {:>9.2} {:>7.2}% {:>7.2}%",
            set_at as f64 / 1e6,
            latency.as_secs_f64() * 1000.0,
            match progress.outcome {
                Outcome::Done => "Done",
                Outcome::Cancelled => "Cancelled",
                Outcome::Failed(_) => "Failed",
            },
            matching * 100.0,
            mean_error(&complete, &bytes),
            white * 100.0,
            zero * 100.0
        );

        if round == 0 {
            // Numbers cannot distinguish "half the linework" from "the whole
            // page at the wrong resolution" -- both score 0% matching. Write
            // both tiles out and look at them.
            if let Some(dir) = &args.dump {
                write_png(&dir.join("complete.png"), &complete, args.tile);
                write_png(&dir.join("cancelled.png"), &bytes, args.tile);
            }
        }

        if progress.outcome != Outcome::Cancelled {
            // Either PDFium never polled, or the render finished before the
            // watcher fired. Both make the row meaningless, and neither is
            // visible from a latency number alone.
            println!(
                "    [FAIL] not cancelled (outcome {:?}, {} polls). Either the render",
                progress.outcome, progress.polls
            );
            println!("           finished first -- lower --after -- or it never yielded.");
            ok = false;
        }

        // What the saturated columns above cannot say, these two can. Together
        // they establish that PDFium composites into the caller's bitmap as it
        // goes, rather than only at the end: the tile is neither untouched nor
        // finished.
        if white + zero > 0.5 {
            println!(
                "    [FAIL] {:.0}% of the tile is untouched or merely cleared, so nothing",
                (white + zero) * 100.0
            );
            println!("           was composited before the cancellation.");
            ok = false;
        }
        if bytes == complete {
            println!("    [FAIL] the cancelled tile equals the finished one, so the render");
            println!("           had already completed and this measures nothing.");
            ok = false;
        }
    }

    println!();
    println!(
        "worst cancellation latency: {:.3} ms, against a {:.1} ms render",
        worst.as_secs_f64() * 1000.0,
        full.elapsed.as_secs_f64() * 1000.0
    );

    ok
}

/// Mean absolute per-channel difference between a partial tile and the finished
/// one, in levels out of 255.
///
/// This is the metric that behaves. Exact pixel equality is a fine proxy on a
/// sparse page and useless on a dense one: the A0 fixture is antialiased random
/// linework covering every pixel, so a partial render that is visibly most of the
/// way there still matches *zero* pixels exactly. Mean error falls smoothly as
/// the render converges, on both kinds of page.
fn mean_error(complete: &[u8], partial: &[u8]) -> f64 {
    if complete.len() != partial.len() || complete.is_empty() {
        return f64::NAN;
    }
    let total: u64 = complete
        .iter()
        .zip(partial)
        .map(|(a, b)| a.abs_diff(*b) as u64)
        .sum();
    total as f64 / complete.len() as f64
}

/// Fraction of pixels in `partial` that equal the corresponding pixel in
/// `complete`.
///
/// Kept alongside [`mean_error`] because on a sparse page it is the more
/// intuitive number, and because the two disagreeing is itself informative.
fn matching_fraction(complete: &[u8], partial: &[u8]) -> f64 {
    if complete.len() != partial.len() || complete.is_empty() {
        return 0.0;
    }
    let same = complete
        .chunks_exact(4)
        .zip(partial.chunks_exact(4))
        .filter(|(a, b)| a == b)
        .count();
    same as f64 / (complete.len() / 4) as f64
}

/// How much of a bitmap is opaque white, and how much is still all-zero.
///
/// "Fraction of pixels that are not white" was the obvious diagnostic and it is
/// useless: a buffer nothing ever wrote to is all zeroes, which is not white, so
/// an untouched bitmap reports as 100% ink. These two numbers separate the three
/// states that matter --- untouched (all zero), cleared but undrawn (all white),
/// and drawn (neither).
fn composition(pixels: &[u8]) -> (f64, f64) {
    if pixels.is_empty() {
        return (0.0, 0.0);
    }
    let total = (pixels.len() / 4) as f64;
    let mut white = 0usize;
    let mut zero = 0usize;
    for pixel in pixels.chunks_exact(4) {
        match pixel {
            [0xFF, 0xFF, 0xFF, 0xFF] => white += 1,
            [0, 0, 0, 0] => zero += 1,
            _ => {}
        }
    }
    (white as f64 / total, zero as f64 / total)
}

// ---------------------------------------------------------------------------
// pageload: is holding a page worth the lifetime trouble?
// ---------------------------------------------------------------------------

/// Prices `RawDocument`'s page cache against loading per request.
///
/// This decided a design question rather than answering a curiosity, and it stays
/// here so the answer can be re-checked after a PDFium bump. `FPDF_LoadPage`
/// re-parses the page every call --- PDFium caches nothing --- so the cost is a
/// function of page complexity, and on the one document where latency is already
/// the problem it is enormous.
///
/// Variants are interleaved so drift cannot masquerade as a difference, and the
/// uncached variant evicts first, which is the only reason `evict_page` exists.
fn pageload(_raw: progressive::Bindings, document: &RawDocument, args: &Args) -> bool {
    let reps = args.rounds.max(20);
    let mut cached = Vec::with_capacity(reps);
    let mut uncached = Vec::with_capacity(reps);

    for i in 0..reps {
        // A,B,A,B within each round rather than block after block.
        for uncached_variant in [i % 2 == 0, i % 2 != 0] {
            if uncached_variant {
                document.evict_page(args.page);
            }

            let t0 = Instant::now();
            let page = match document.page(args.page) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("[FAIL] {e}");
                    return false;
                }
            };
            let ms = t0.elapsed().as_secs_f64() * 1000.0;

            // Read something off the handle, so nothing can be optimised away
            // and the page is proven usable rather than merely non-null.
            let width = page.width_pt();
            if width <= 0.0 {
                eprintln!("[FAIL] page reports a width of {width}");
                return false;
            }

            if uncached_variant {
                uncached.push(ms);
            } else {
                cached.push(ms);
            }
        }
    }

    let summarise = |label: &str, mut v: Vec<f64>| -> f64 {
        v.sort_by(f64::total_cmp);
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        println!(
            "  {label:<10} median {:>9.4} ms   mean {:>9.4} ms   worst {:>9.4} ms",
            v[v.len() / 2],
            mean,
            v[v.len() - 1]
        );
        mean
    };

    println!("page {} lookup, {reps} of each, interleaved:", args.page);
    let cached_mean = summarise("cached", cached);
    let uncached_mean = summarise("uncached", uncached);
    println!();
    println!(
        "  the cache saves {:.4} ms per tile request on this page ({:.0}x)",
        uncached_mean - cached_mean,
        uncached_mean / cached_mean.max(f64::MIN_POSITIVE)
    );

    // Not a pass/fail on speed -- a page whose load is genuinely cheap is not a
    // defect. What must hold is that the cache actually caches.
    if cached_mean > uncached_mean {
        println!("  [FAIL] the cached path is slower, so the cache is not being hit");
        return false;
    }

    true
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// Renders one tile through the progressive path.
fn render(
    raw: progressive::Bindings,
    page: &RawPage<'_>,
    args: &Args,
    x: i32,
    y: i32,
    slice: Option<Duration>,
    token: &CancelToken,
) -> Result<(Vec<u8>, Progress), String> {
    let spec = progressive::TileSpec {
        scale: args.scale,
        x,
        y,
        width: args.tile,
        height: args.tile,
    };
    progressive::render_tile(raw, page, spec, slice, token)
}

/// Renders the same tile through `pdfium-render`'s safe, uninterruptible path,
/// exactly as `render.rs` does today.
fn safe_tile(pdfium: &'static Pdfium, args: &Args, x: i32, y: i32) -> Result<Vec<u8>, String> {
    let document = pdfium
        .load_pdf_from_file(&args.file, None)
        .map_err(|e| format!("open failed: {e}"))?;
    let page = document
        .pages()
        .get(args.page as PdfPageIndex)
        .map_err(|e| format!("no such page: {e}"))?;

    let full_width = (page.width().value * args.scale).round() as i32;
    let full_height = (page.height().value * args.scale).round() as i32;

    let mut bitmap = PdfBitmap::empty(
        args.tile as Pixels,
        args.tile as Pixels,
        PdfBitmapFormat::BGRA,
    )
    .map_err(|e| format!("bitmap failed: {e}"))?;

    let config = PdfRenderConfig::new()
        .set_target_width(full_width)
        .set_target_height(full_height)
        .set_origin(-x, -y);

    page.render_into_bitmap_with_config(&mut bitmap, &config)
        .map_err(|e| format!("render failed: {e}"))?;

    Ok(bitmap.as_rgba_bytes())
}

/// A tile from the middle of the page.
///
/// The centre is used rather than the origin because a corner tile of a sparse
/// page can be empty, and an empty tile renders fast enough to make every
/// question here unanswerable.
fn centre_tile_origin(page: &RawPage<'_>, args: &Args) -> (i32, i32) {
    let full_width = (page.width_pt() * args.scale).round() as i32;
    let full_height = (page.height_pt() * args.scale).round() as i32;
    (
        ((full_width - args.tile as i32) / 2).max(0),
        ((full_height - args.tile as i32) / 2).max(0),
    )
}

/// FNV-1a, for a stable one-line fingerprint of a tile.
fn digest(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn parse_args() -> Args {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let mut file = None;
    let mut page = 0u32;
    let mut scale = 1.0f32;
    let mut tile = 1024u16;
    let mut mode = Mode::Identity;
    let mut rounds = 3usize;
    let mut slices = vec![0u64, 1, 4, 16];
    let mut after_ms = 50u64;
    let mut dump = None;
    let mut library_dir = None;

    let value = |i: usize, flag: &str| -> String {
        argv.get(i + 1)
            .unwrap_or_else(|| panic!("{flag} needs a value"))
            .clone()
    };

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--page" => page = value(i, "--page").parse().expect("--page wants an integer"),
            "--scale" => scale = value(i, "--scale").parse().expect("--scale wants a number"),
            "--tile" => tile = value(i, "--tile").parse().expect("--tile wants an integer"),
            "--rounds" => {
                rounds = value(i, "--rounds")
                    .parse()
                    .expect("--rounds wants an integer")
            }
            "--after" => after_ms = value(i, "--after").parse().expect("--after wants ms"),
            "--lib" => library_dir = Some(PathBuf::from(value(i, "--lib"))),
            "--dump" => dump = Some(PathBuf::from(value(i, "--dump"))),
            "--mode" => {
                mode = match value(i, "--mode").as_str() {
                    "identity" => Mode::Identity,
                    "poll" => Mode::Poll,
                    "cancel" => Mode::Cancel,
                    "pageload" => Mode::PageLoad,
                    other => panic!("unknown mode: {other}"),
                }
            }
            "--slices" => {
                slices = value(i, "--slices")
                    .split(',')
                    .map(|s| s.trim().parse().expect("--slices wants integers in ms"))
                    .collect()
            }
            other => {
                file = Some(PathBuf::from(other));
                i += 1;
                continue;
            }
        }
        i += 2;
    }

    Args {
        file: file.expect("usage: progressive-probe <file.pdf> [flags]"),
        page,
        scale,
        tile,
        mode,
        rounds,
        slices,
        after_ms,
        dump,
        library_dir: library_dir.unwrap_or_else(default_library_dir),
    }
}

/// `vendor/pdfium/lib` at the repo root, matching the other spike binaries.
fn default_library_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root: &Path| root.join("vendor/pdfium/lib"))
        .unwrap_or_else(|| PathBuf::from("vendor/pdfium/lib"))
}
