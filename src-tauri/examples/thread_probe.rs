//! Does `pdfium-render`'s `thread_safe` feature actually serialize Pdfium?
//!
//! AGENTS.md and `src/render.rs` both assert that it does --- "achieves safety
//! by locking every single PDFium call behind one mutex", and therefore that
//! multiple documents in one process render strictly sequentially and that one
//! pathological page starves every other render. That claim is load-bearing:
//! it is the stated reason the whole architecture goes to worker processes, and
//! the stated reason threads are pointless rather than dangerous.
//!
//! It came from the crate's own README, which says `thread_safe` "wraps access
//! to Pdfium behind a mutex". Reading 0.9.3 does not support it: the only
//! `Mutex` in the crate guards a page-index cache, the only `RwLock` is in the
//! WASM bindings, and a native call such as `FPDF_LoadPage` dispatches straight
//! through a function pointer with no lock. What `thread_safe` actually changes
//! is that `BINDINGS` is awaited rather than unwrapped, plus
//! `unsafe impl Send/Sync for Pdfium`.
//!
//! Source reading is not proof, and this file's claim has already been wrong
//! once (AGENTS.md records an earlier version asserting Pdfium was unsafe only
//! per document handle). So measure it. The test is a page that takes about a
//! second to render, K of them at once against K in a row:
//!
//! * If a global mutex serializes calls, concurrent ~= sequential.
//! * If it does not, concurrent ~= one render, and the crate's documented
//!   guarantee does not exist.
//!
//! Either way the architectural conclusion --- render in worker processes ---
//! survives, because upstream Pdfium offers no thread-safety guarantee at all.
//! What changes is *why*: "threads buy nothing" and "threads are undefined
//! behaviour" are different statements, and only one of them is a safety
//! argument.
//!
//! **Result, 2026-07-27.** Not serialized, and worse than not serialized.
//!
//! | fixture | threads | outcome |
//! |---|---|---|
//! | `vector-heavy` (A0) | 2 | SIGSEGV |
//! | `vector-heavy` (A0) | 4 | SIGSEGV |
//! | `text-heavy` | 4 | survives, 3.85x speedup, pixel-correct, 6 runs of 6 |
//! | `text-heavy` | 8 | SIGABRT |
//! | `text-heavy` | 4, five rounds | round 0 at 3.85x, then crashes on round 1 |
//!
//! 3.85x on four threads is near-linear, which no global mutex permits, so the
//! documented guarantee does not exist. And the middle row is the part to
//! remember: concurrent Pdfium *often works*, returning correct pixels, right
//! up until it does not.
//!
//! Usage:
//!   thread-probe <file.pdf> [--page N] [--scale F] [--tile N]
//!                [--threads K] [--rounds N] [--lib DIR]
//!                [--only sequential|concurrent]
//!
//! `--only` runs one phase, which is the only way to get a clean timing for the
//! sequential half on a fixture whose concurrent half reliably faults --- the
//! crash takes the whole process down, report included.
//!
//! Variants are interleaved A,B,A,B across rounds and compared pairwise within
//! a round, because wall clock on this machine drifts several percent over
//! minutes --- more than most differences worth finding.
//!
//! Note when running this by hand: a segfaulting binary piped through `tail`
//! reports `tail`'s exit status, so it looks like a clean run. Check `$?` on
//! the program itself. That trap cost a wrong reading here too.

use std::path::{Path, PathBuf};
use std::time::Instant;

use pdfium_render::prelude::*;

/// One rendered tile, reduced to what the probe compares.
struct Rendered {
    /// FNV-1a over the RGBA bytes. Detects a concurrent render corrupting output.
    digest: u64,
    seconds: f64,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut file: Option<PathBuf> = None;
    let mut page: u16 = 0;
    let mut scale: f32 = 1.0;
    let mut tile: u16 = 1024;
    let mut threads: usize = 4;
    let mut rounds: usize = 5;
    let mut lib: Option<PathBuf> = None;
    let mut only: Option<String> = None;

    let value = |i: usize, name: &str| -> String {
        args.get(i + 1)
            .unwrap_or_else(|| panic!("{name} needs a value"))
            .clone()
    };

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--page" => page = value(i, flag).parse().expect("page"),
            "--scale" => scale = value(i, flag).parse().expect("scale"),
            "--tile" => tile = value(i, flag).parse().expect("tile"),
            "--threads" => threads = value(i, flag).parse().expect("threads"),
            "--rounds" => rounds = value(i, flag).parse().expect("rounds"),
            "--lib" => lib = Some(PathBuf::from(value(i, flag))),
            "--only" => only = Some(value(i, flag)),
            other => {
                if other.starts_with("--") {
                    panic!("unknown flag {other}");
                }
                file = Some(PathBuf::from(other));
                i += 1;
                continue;
            }
        }
        i += 2;
    }

    let file = file.expect("usage: thread-probe <file.pdf> [flags]");
    let library_dir = lib.unwrap_or_else(default_library_dir);

    let path = Pdfium::pdfium_platform_library_name_at_path(&library_dir);
    let bindings = match Pdfium::bind_to_library(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[FAIL] could not load Pdfium from {}: {e}", path.display());
            eprintln!("       run scripts/fetch_pdfium.py");
            std::process::exit(2);
        }
    };
    let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));

    println!("file      {}", file.display());
    println!("page      {page}");
    println!("scale     {scale}");
    println!("tile      {tile}x{tile}");
    println!("threads   {threads}");
    println!("rounds    {rounds}");
    println!("only      {}", only.as_deref().unwrap_or("both"));
    let only = only.as_deref();

    // A single render, both as the correctness oracle and as the unit the two
    // variants are measured against.
    let baseline = render_one(pdfium, &file, page, scale, tile);
    println!(
        "\nbaseline  one render: {:.3} s, digest {:#018x}",
        baseline.seconds, baseline.digest
    );
    println!(
        "expected  sequential ~{:.3} s; concurrent ~{:.3} s if truly parallel, \
         ~{:.3} s if serialized",
        baseline.seconds * threads as f64,
        baseline.seconds,
        baseline.seconds * threads as f64
    );

    println!("\nround  sequential  concurrent   ratio  corrupt");
    let mut ratios = Vec::new();

    for round in 0..rounds {
        // Interleaved, and ordered sequential-first only on even rounds so the
        // two variants take turns paying for whatever the previous one left in
        // the caches.
        //
        // Each phase announces itself and flushes first. This probe's most
        // likely failure is a crash rather than a wrong number, and a crash
        // reports nothing about where it happened -- so the last line on stdout
        // has to be the attribution.
        let (seq, conc) = if round % 2 == 0 {
            let s = phase(only, round, "sequential", || {
                run_sequential(pdfium, &file, page, scale, tile, threads)
            });
            let c = phase(only, round, "concurrent", || {
                run_concurrent(pdfium, &file, page, scale, tile, threads)
            });
            (s, c)
        } else {
            let c = phase(only, round, "concurrent", || {
                run_concurrent(pdfium, &file, page, scale, tile, threads)
            });
            let s = phase(only, round, "sequential", || {
                run_sequential(pdfium, &file, page, scale, tile, threads)
            });
            (s, c)
        };

        // Computed before the --only bail-out. A concurrent run that does not
        // crash is the outcome worth inspecting hardest: a wrong tile that was
        // returned successfully is worse than a segfault, because nothing
        // reports it. Not crashing is not the same as being correct.
        let corrupt = seq
            .iter()
            .chain(conc.iter())
            .filter(|r| r.digest != baseline.digest)
            .count();

        if seq.is_empty() || conc.is_empty() {
            let ran = if seq.is_empty() { &conc } else { &seq };
            println!(
                "  round {round} -> {} renders, {corrupt} with a digest differing from baseline",
                ran.len()
            );
            continue; // the other phase was skipped by --only
        }

        let seq_total: f64 = seq.iter().map(|r| r.seconds).sum();
        let conc_wall = conc
            .iter()
            .map(|r| r.seconds)
            .fold(0.0_f64, |a, b| a.max(b));

        let ratio = seq_total / conc_wall.max(f64::MIN_POSITIVE);
        ratios.push(ratio);

        println!("{round:>5}  {seq_total:>10.3}  {conc_wall:>10.3}  {ratio:>6.2}x  {corrupt:>7}");
    }

    if ratios.is_empty() {
        println!("\n[OK] phase completed; no comparison because --only was set");
        return;
    }

    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = ratios[ratios.len() / 2];

    println!("\nmedian speedup from {threads} threads: {median:.2}x");
    println!(
        "\n{}",
        if median < 1.5 {
            "[RESULT] Serialized. Concurrent renders gained nothing, which is what a \n         \
             global mutex looks like. The AGENTS.md claim holds."
        } else {
            "[RESULT] NOT serialized. Concurrent renders ran in parallel, so there is no \n         \
             global mutex and the crate's documented `thread_safe` guarantee does not \n         \
             exist in this version. Threads are not pointless here -- they are unsafe, \n         \
             because upstream Pdfium promises no thread safety. Worker processes remain \n         \
             the answer, for a different reason than AGENTS.md gives."
        }
    );

    if ratios.iter().any(|r| r.is_nan()) {
        eprintln!("[FAIL] a round produced no timing");
        std::process::exit(1);
    }
}

/// Runs one phase, announcing it first so a crash is attributable.
///
/// Returns an empty vector when `--only` excludes this phase, which lets the
/// two halves be run in separate processes -- the only way to get a clean
/// timing for one of them when the other reliably faults.
fn phase<F>(only: Option<&str>, round: usize, name: &str, run: F) -> Vec<Rendered>
where
    F: FnOnce() -> Vec<Rendered>,
{
    if let Some(only) = only {
        if only != name {
            return Vec::new();
        }
    }
    print!("  round {round} {name} ... ");
    use std::io::Write;
    std::io::stdout().flush().ok();

    let out = run();
    println!("ok");
    out
}

/// Renders `threads` tiles one after another on this thread.
fn run_sequential(
    pdfium: &'static Pdfium,
    file: &Path,
    page: u16,
    scale: f32,
    tile: u16,
    threads: usize,
) -> Vec<Rendered> {
    (0..threads)
        .map(|_| render_one(pdfium, file, page, scale, tile))
        .collect()
}

/// Renders `threads` tiles at the same time, one document per thread.
///
/// Each thread opens its own `FPDF_DOCUMENT`, which is the exact scenario
/// AGENTS.md describes as "multiple `FPDF_Document` handles in one process
/// therefore render strictly sequentially".
fn run_concurrent(
    pdfium: &'static Pdfium,
    file: &Path,
    page: u16,
    scale: f32,
    tile: u16,
    threads: usize,
) -> Vec<Rendered> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| scope.spawn(move || render_one(pdfium, file, page, scale, tile)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("render thread panicked"))
            .collect()
    })
}

/// Opens the document, renders one centred tile, and digests the pixels.
///
/// The open is inside the timed section deliberately: it is what a worker would
/// do, and on this corpus it is under a millisecond against a render measured
/// in seconds.
fn render_one(pdfium: &'static Pdfium, file: &Path, page: u16, scale: f32, tile: u16) -> Rendered {
    let t0 = Instant::now();

    let doc = pdfium
        .load_pdf_from_file(file, None)
        .unwrap_or_else(|e| panic!("could not open {}: {e}", file.display()));

    let page = doc
        .pages()
        .get(page as PdfPageIndex)
        .unwrap_or_else(|e| panic!("no such page: {e}"));

    let full_width = (page.width().value * scale).round() as i32;
    let full_height = (page.height().value * scale).round() as i32;

    // Centre the tile, so it lands on content rather than on a margin.
    let x = ((full_width - tile as i32) / 2).max(0);
    let y = ((full_height - tile as i32) / 2).max(0);

    let mut bitmap = PdfBitmap::empty(tile as Pixels, tile as Pixels, PdfBitmapFormat::BGRA)
        .expect("could not allocate tile");

    let config = PdfRenderConfig::new()
        .set_target_width(full_width)
        .set_target_height(full_height)
        .set_origin(-x, -y);

    page.render_into_bitmap_with_config(&mut bitmap, &config)
        .expect("render failed");

    let digest = fnv1a(&bitmap.as_rgba_bytes());

    Rendered {
        digest,
        seconds: t0.elapsed().as_secs_f64(),
    }
}

/// FNV-1a over the tile's pixels. Not cryptographic; it only has to notice a
/// concurrent render scribbling on someone else's bitmap.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// `vendor/pdfium/<subdir>` at the repo root, matching the other spike binaries.
///
/// The subdirectory comes from [`tpdf_lib::PDFIUM_SUBDIR`] rather than a literal
/// `lib`, which is right on macOS and wrong on Windows in the way that is
/// hardest to read: `lib/` exists there too and holds the *import* library, so
/// the path looks present and the bind fails much later.
fn default_library_dir() -> PathBuf {
    let subdir = tpdf_lib::PDFIUM_SUBDIR;
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("vendor/pdfium").join(subdir))
        .unwrap_or_else(|| PathBuf::from("vendor/pdfium").join(subdir))
}
