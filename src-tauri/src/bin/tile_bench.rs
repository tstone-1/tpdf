//! Spike 0.1, render half: what does a tile actually cost?
//!
//! Deliberately headless. The GUI benchmark answers the *transfer* question;
//! this one answers the questions that a webview would only add noise to:
//! CPU per tile, how cost scales with tile size, and whether tiling actually
//! bounds work on a pathological page or merely bounds the output bitmap.
//!
//! Discipline: variants are interleaved A,B,A,B... across rounds and compared
//! pairwise within a round, because wall clock on these machines drifts several
//! percent over minutes -- more than most differences worth acting on.
//!
//! Two modes, because the two questions need different experiments:
//!
//! * `--mode sweep` (default) renders the whole page in tiles and compares total
//!   cost across tile sizes. Right for an ordinary page; quadratically wrong for
//!   a traversal-bound one, where it degenerates into rendering the page once
//!   per tile.
//! * `--mode single` renders exactly ONE centred tile at each size, plus the
//!   full page in one bitmap. That isolates the thing that actually matters --
//!   whether asking for a small region costs proportionally less than asking for
//!   the whole page -- in a handful of renders instead of thousands.
//!
//! Usage:
//!   tile-bench <file.pdf> [--page N] [--rounds N] [--scales 1,2,4]
//!              [--tiles 256,512,1024,2048] [--mode sweep|single]

use std::path::{Path, PathBuf};
use std::time::Instant;

use pdfium_render::prelude::*;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Sweep,
    Single,
    Encode,
}

struct Args {
    file: PathBuf,
    page: PdfPageIndex,
    rounds: usize,
    scales: Vec<f32>,
    tiles: Vec<i32>,
    mode: Mode,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let file = args
        .next()
        .ok_or("usage: tile-bench <file.pdf> [options]")?;

    let mut parsed = Args {
        file: PathBuf::from(file),
        page: 0,
        rounds: 5,
        scales: vec![1.0, 2.0, 4.0],
        tiles: vec![256, 512, 1024, 2048],
        mode: Mode::Sweep,
    };

    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--page" => parsed.page = value.parse().map_err(|_| "bad --page")?,
            "--rounds" => parsed.rounds = value.parse().map_err(|_| "bad --rounds")?,
            "--scales" => {
                parsed.scales = value
                    .split(',')
                    .map(|s| s.parse().map_err(|_| "bad --scales"))
                    .collect::<Result<_, _>>()?
            }
            "--tiles" => {
                parsed.tiles = value
                    .split(',')
                    .map(|s| s.parse().map_err(|_| "bad --tiles"))
                    .collect::<Result<_, _>>()?
            }
            "--mode" => {
                parsed.mode = match value.as_str() {
                    "sweep" => Mode::Sweep,
                    "single" => Mode::Single,
                    "encode" => Mode::Encode,
                    other => return Err(format!("bad --mode {other}")),
                }
            }
            other => return Err(format!("unknown flag {other}")),
        }
    }

    Ok(parsed)
}

/// Peak resident set size in MB, via getrusage.
///
/// Pdfium allocates on the native heap, so a Rust global-allocator hook would
/// report close to nothing. On macOS ru_maxrss is bytes; on Linux, kilobytes.
/// Not measurable off unix.
///
/// `NaN` rather than `0.0`, and deliberately: this is the value the unix path
/// already returns when `getrusage` fails, it prints as `NaN`, and it cannot be
/// mistaken for a measurement. A zero here would read as "PDFium allocated
/// nothing", which is both false and exactly the kind of plausible number that
/// gets quoted.
#[cfg(not(any(unix, windows)))]
fn peak_rss_mb() -> f64 {
    f64::NAN
}

/// The high-water mark of this process's memory, in MB.
///
/// `PeakWorkingSetSize` is the closest Windows counterpart to `ru_maxrss` and it
/// is not the same quantity: a working set is resident pages, so it is trimmed
/// by memory pressure and can be lower than the peak *commit* the same run
/// reached. It is the right one here anyway --- the question this bench asks is
/// what a tile costs in resident memory, and both platforms answer it about
/// pages actually held.
///
/// Bytes, unlike macOS's kilobytes-or-bytes ambiguity next door, and unlike the
/// unix arm this needs no per-target scaling.
///
/// Keeps the `NaN` contract of the arm above on failure, for the same reason:
/// a zero would read as "PDFium allocated nothing".
#[cfg(windows)]
fn peak_rss_mb() -> f64 {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: a zeroed struct is the documented starting point and `cb` is its
    // own size, as the API requires.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = match u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()) {
        Ok(size) => size,
        Err(_) => return f64::NAN,
    };
    // SAFETY: a pseudo-handle to self, and the struct outlives the call.
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, counters.cb) };
    if ok == 0 {
        return f64::NAN;
    }
    counters.PeakWorkingSetSize as f64 / (1024.0 * 1024.0)
}

#[cfg(unix)]
fn peak_rss_mb() -> f64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return f64::NAN;
    }
    let raw = usage.ru_maxrss as f64;
    if cfg!(target_os = "macos") {
        raw / (1024.0 * 1024.0)
    } else {
        raw / 1024.0
    }
}

struct Sample {
    tile: i32,
    scale: f32,
    round: usize,
    wall_ms: f64,
    tiles: usize,
    /// Sum of per-tile Pdfium render time.
    render_ms: f64,
    /// Total RGBA bytes produced.
    bytes: usize,
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[ERROR] {e}");
            std::process::exit(2);
        }
    };

    let library_dir = pdfium_dir();
    let path = Pdfium::pdfium_platform_library_name_at_path(&library_dir);
    let bindings = match Pdfium::bind_to_library(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[ERROR] could not load Pdfium from {}: {e}", path.display());
            std::process::exit(2);
        }
    };
    let pdfium = Pdfium::new(bindings);

    let t_open = Instant::now();
    let document = match pdfium.load_pdf_from_file(&args.file, None) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[ERROR] could not open {}: {e}", args.file.display());
            std::process::exit(2);
        }
    };
    let open_ms = t_open.elapsed().as_secs_f64() * 1000.0;

    let page_count = document.pages().len();
    let page = match document.pages().get(args.page) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[ERROR] no page {}: {e}", args.page);
            std::process::exit(2);
        }
    };
    let page_w = page.width().value;
    let page_h = page.height().value;

    println!("file          {}", args.file.display());
    println!("pages         {page_count}");
    println!("page {:<9}{:.0} x {:.0} pt", args.page, page_w, page_h);
    println!("open          {open_ms:.1} ms");
    println!("rss after open {:.0} MB", peak_rss_mb());
    println!();

    // In single mode the full page is just another variant, carried as tile
    // size FULL_PAGE so it interleaves with the real tiles rather than being
    // measured in a separate block. Interleaving is the whole point.
    let mut sizes = args.tiles.clone();
    if args.mode == Mode::Single {
        sizes.push(FULL_PAGE);
    }

    // Interleave every (tile, scale) combination within each round.
    let combos: Vec<(i32, f32)> = sizes
        .iter()
        .flat_map(|t| args.scales.iter().map(move |s| (*t, *s)))
        .collect();

    let mut samples: Vec<Sample> = Vec::new();

    for round in 0..args.rounds {
        for &(tile, scale) in &combos {
            let measured = match args.mode {
                Mode::Sweep => sweep_page(&page, tile, scale),
                Mode::Single => single_tile(&page, tile, scale),
                Mode::Encode => encode_tile(&page, tile, scale),
            };
            match measured {
                Ok((wall_ms, tiles, render_ms, bytes)) => samples.push(Sample {
                    tile,
                    scale,
                    round,
                    wall_ms,
                    tiles,
                    render_ms,
                    bytes,
                }),
                Err(e) => {
                    eprintln!("[ERROR] tile={tile} scale={scale}: {e}");
                    std::process::exit(1);
                }
            }
        }
    }

    match args.mode {
        Mode::Sweep => report(&samples, args.rounds),
        Mode::Single => report_single(&samples, args.rounds),
        Mode::Encode => report_encode(&samples),
    }
    println!();
    println!("peak rss      {:.0} MB", peak_rss_mb());
}

/// Sentinel tile size meaning "the whole page in one bitmap".
const FULL_PAGE: i32 = -1;

/// Renders exactly one tile, taken from the centre of the page, returning the
/// same shape as [`sweep_page`].
///
/// The centre is deliberate: a corner tile of a sparse page can be empty, which
/// would measure nothing. The centre of a dense page is representative of what a
/// reader actually looks at.
fn single_tile(
    page: &PdfPage,
    tile: i32,
    scale: f32,
) -> Result<(f64, usize, f64, usize), PdfiumError> {
    let full_w = (page.width().value * scale).round() as i32;
    let full_h = (page.height().value * scale).round() as i32;

    let (w, h, x, y) = if tile == FULL_PAGE {
        (full_w, full_h, 0, 0)
    } else {
        let w = tile.min(full_w);
        let h = tile.min(full_h);
        (w, h, (full_w - w) / 2, (full_h - h) / 2)
    };

    let wall = Instant::now();
    let mut bitmap = PdfBitmap::empty(w, h, PdfBitmapFormat::BGRA)?;
    let config = PdfRenderConfig::new()
        .set_target_width(full_w)
        .set_target_height(full_h)
        .set_origin(-x, -y);

    let t = Instant::now();
    page.render_into_bitmap_with_config(&mut bitmap, &config)?;
    let render_ms = t.elapsed().as_secs_f64() * 1000.0;

    Ok((
        wall.elapsed().as_secs_f64() * 1000.0,
        1,
        render_ms,
        (w * h * 4) as usize,
    ))
}

/// Renders one centred tile and then PNG-encodes it, returning
/// (encode ms, 1, render ms, encoded bytes).
///
/// This is the *server* half of the raw-vs-encoded question. Encoding buys a
/// smaller payload and costs CPU on the render thread; the webview half (decode
/// plus `createImageBitmap`) is measured separately in the GUI harness. If
/// encoding alone already exceeds the frame budget, the GUI half is moot.
fn encode_tile(
    page: &PdfPage,
    tile: i32,
    scale: f32,
) -> Result<(f64, usize, f64, usize), PdfiumError> {
    let full_w = (page.width().value * scale).round() as i32;
    let full_h = (page.height().value * scale).round() as i32;

    let (w, h, x, y) = if tile == FULL_PAGE {
        (full_w, full_h, 0, 0)
    } else {
        let w = tile.min(full_w);
        let h = tile.min(full_h);
        (w, h, (full_w - w) / 2, (full_h - h) / 2)
    };

    let mut bitmap = PdfBitmap::empty(w, h, PdfBitmapFormat::BGRA)?;
    let config = PdfRenderConfig::new()
        .set_target_width(full_w)
        .set_target_height(full_h)
        .set_origin(-x, -y);

    let t = Instant::now();
    page.render_into_bitmap_with_config(&mut bitmap, &config)?;
    let render_ms = t.elapsed().as_secs_f64() * 1000.0;

    let rgba = bitmap.as_rgba_bytes();

    let t = Instant::now();
    let mut encoded: Vec<u8> = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // Default compression. A faster preset would trade payload for CPU, but
        // the point here is to find out whether the *cheapest useful* encode is
        // already too expensive.
        let mut writer = encoder
            .write_header()
            .map_err(|_| PdfiumError::ImageError)?;
        writer
            .write_image_data(&rgba)
            .map_err(|_| PdfiumError::ImageError)?;
    }
    let encode_ms = t.elapsed().as_secs_f64() * 1000.0;

    Ok((encode_ms, 1, render_ms, encoded.len()))
}

/// Renders one full page in tiles of the given size, returning
/// (wall ms, tile count, summed render ms, total RGBA bytes).
fn sweep_page(
    page: &PdfPage,
    tile: i32,
    scale: f32,
) -> Result<(f64, usize, f64, usize), PdfiumError> {
    let full_w = (page.width().value * scale).round() as i32;
    let full_h = (page.height().value * scale).round() as i32;

    let wall = Instant::now();
    let mut count = 0usize;
    let mut render_ns = 0u128;
    let mut bytes = 0usize;

    let mut y = 0;
    while y < full_h {
        let mut x = 0;
        while x < full_w {
            let w = tile.min(full_w - x);
            let h = tile.min(full_h - y);

            let mut bitmap = PdfBitmap::empty(w, h, PdfBitmapFormat::BGRA)?;
            let config = PdfRenderConfig::new()
                .set_target_width(full_w)
                .set_target_height(full_h)
                .set_origin(-x, -y);

            let t = Instant::now();
            page.render_into_bitmap_with_config(&mut bitmap, &config)?;
            render_ns += t.elapsed().as_nanos();

            bytes += (w * h * 4) as usize;
            count += 1;
            x += tile;
        }
        y += tile;
    }

    Ok((
        wall.elapsed().as_secs_f64() * 1000.0,
        count,
        render_ns as f64 / 1_000_000.0,
        bytes,
    ))
}

fn report(samples: &[Sample], rounds: usize) {
    println!(
        "{:>6} {:>6} {:>10} {:>7} {:>11} {:>10} {:>9}",
        "tile", "scale", "median ms", "tiles", "ms/tile", "MB rgba", "vs 512"
    );
    println!("{}", "-".repeat(68));

    let mut combos: Vec<(i32, u32)> = samples
        .iter()
        .map(|s| (s.tile, (s.scale * 1000.0) as u32))
        .collect();
    combos.sort_unstable();
    combos.dedup();

    // Baseline per scale is the 512 tile, so the "vs 512" column compares like
    // with like rather than across zoom levels.
    for (_, scale_milli) in combos.iter().filter(|(t, _)| *t == 512) {
        let scale = *scale_milli as f32 / 1000.0;
        let baseline = median_wall(samples, 512, *scale_milli);

        let mut tiles_for_scale: Vec<i32> = samples
            .iter()
            .filter(|s| (s.scale * 1000.0) as u32 == *scale_milli)
            .map(|s| s.tile)
            .collect();
        tiles_for_scale.sort_unstable();
        tiles_for_scale.dedup();

        for tile in tiles_for_scale {
            let mine: Vec<&Sample> = samples
                .iter()
                .filter(|s| s.tile == tile && (s.scale * 1000.0) as u32 == *scale_milli)
                .collect();
            let Some(first) = mine.first() else { continue };
            let med = median_wall(samples, tile, *scale_milli);
            let ratio = if baseline > 0.0 {
                med / baseline
            } else {
                f64::NAN
            };

            println!(
                "{:>6} {:>7.3} {:>10.1} {:>7} {:>11.2} {:>10.1} {:>9.3}",
                tile,
                scale,
                med,
                first.tiles,
                med / first.tiles as f64,
                first.bytes as f64 / (1024.0 * 1024.0),
                ratio
            );
        }
        println!();
    }

    // Pairwise per-round ratios against 512, which is the comparison that
    // survives clock drift. A stable ratio across rounds means the difference
    // is real; a wandering one means it is noise.
    println!("per-round ratio vs tile=512 (same scale, same round):");
    for (tile, scale_milli) in &combos {
        if *tile == 512 {
            continue;
        }
        let ratios: Vec<String> = (0..rounds)
            .filter_map(|round| {
                let a = samples.iter().find(|s| {
                    s.tile == 512 && (s.scale * 1000.0) as u32 == *scale_milli && s.round == round
                })?;
                let b = samples.iter().find(|s| {
                    s.tile == *tile && (s.scale * 1000.0) as u32 == *scale_milli && s.round == round
                })?;
                Some(format!("{:.3}", b.wall_ms / a.wall_ms))
            })
            .collect();
        println!(
            "  tile={:<5} scale={:<6.3} {}",
            tile,
            *scale_milli as f32 / 1000.0,
            ratios.join("  ")
        );
    }

    // Total render time summed over tiles vs the wall clock of the sweep: the
    // gap is per-tile overhead outside Pdfium (allocation, setup, conversion).
    println!();
    println!("render-time share of wall clock (higher = less per-tile overhead):");
    for (tile, scale_milli) in &combos {
        let mine: Vec<&Sample> = samples
            .iter()
            .filter(|s| s.tile == *tile && (s.scale * 1000.0) as u32 == *scale_milli)
            .collect();
        if mine.is_empty() {
            continue;
        }
        let wall: f64 = mine.iter().map(|s| s.wall_ms).sum();
        let render: f64 = mine.iter().map(|s| s.render_ms).sum();
        println!(
            "  tile={:<5} scale={:<6.3} {:.1}%",
            tile,
            *scale_milli as f32 / 1000.0,
            render / wall * 100.0
        );
    }
}

/// Reports single-tile mode: what does asking for a region cost, relative to
/// asking for the whole page?
///
/// The number to read is the last column. A renderer whose cost tracked the
/// requested area would show a tile costing its pixel fraction of the page. A
/// renderer that re-traverses the display list per request shows ~1.0 regardless
/// of tile size -- meaning tiling buys bounded memory and nothing else.
fn report_single(samples: &[Sample], rounds: usize) {
    let mut scales: Vec<u32> = samples.iter().map(|s| (s.scale * 1000.0) as u32).collect();
    scales.sort_unstable();
    scales.dedup();

    println!(
        "{:>8} {:>6} {:>10} {:>10} {:>10} {:>12}",
        "tile", "scale", "median ms", "Mpixel", "ms/Mpixel", "vs full page"
    );
    println!("{}", "-".repeat(62));

    for scale_milli in &scales {
        let scale = *scale_milli as f32 / 1000.0;
        let full = median_wall(samples, FULL_PAGE, *scale_milli);

        let mut sizes: Vec<i32> = samples
            .iter()
            .filter(|s| (s.scale * 1000.0) as u32 == *scale_milli)
            .map(|s| s.tile)
            .collect();
        sizes.sort_unstable();
        sizes.dedup();

        for tile in sizes {
            let Some(first) = samples
                .iter()
                .find(|s| s.tile == tile && (s.scale * 1000.0) as u32 == *scale_milli)
            else {
                continue;
            };
            let med = median_wall(samples, tile, *scale_milli);
            let mpixel = first.bytes as f64 / 4.0 / 1_000_000.0;
            let label = if tile == FULL_PAGE {
                "full".to_string()
            } else {
                tile.to_string()
            };

            println!(
                "{:>8} {:>7.3} {:>10.1} {:>10.2} {:>10.1} {:>12.3}",
                label,
                scale,
                med,
                mpixel,
                med / mpixel,
                if full > 0.0 { med / full } else { f64::NAN }
            );
        }
        println!();
    }

    println!("per-round ratio vs full page (same scale, same round):");
    for scale_milli in &scales {
        let mut sizes: Vec<i32> = samples
            .iter()
            .filter(|s| (s.scale * 1000.0) as u32 == *scale_milli && s.tile != FULL_PAGE)
            .map(|s| s.tile)
            .collect();
        sizes.sort_unstable();
        sizes.dedup();

        for tile in sizes {
            let ratios: Vec<String> = (0..rounds)
                .filter_map(|round| {
                    let full = samples.iter().find(|s| {
                        s.tile == FULL_PAGE
                            && (s.scale * 1000.0) as u32 == *scale_milli
                            && s.round == round
                    })?;
                    let mine = samples.iter().find(|s| {
                        s.tile == tile
                            && (s.scale * 1000.0) as u32 == *scale_milli
                            && s.round == round
                    })?;
                    Some(format!("{:.3}", mine.wall_ms / full.wall_ms))
                })
                .collect();
            println!(
                "  tile={:<5} scale={:<6.3} {}",
                tile,
                *scale_milli as f32 / 1000.0,
                ratios.join("  ")
            );
        }
    }
}

/// Reports encode mode: is PNG worth its CPU?
///
/// `wall_ms` carries encode time here, not total time -- see [`encode_tile`].
/// The column to read is "encode as % of render": if encoding costs a large
/// fraction of the render it is meant to accompany, it is competing with the
/// thing that actually produces pixels.
fn report_encode(samples: &[Sample]) {
    println!(
        "{:>8} {:>7} {:>10} {:>10} {:>9} {:>9} {:>12}",
        "tile", "scale", "render ms", "encode ms", "raw KB", "png KB", "% of render"
    );
    println!("{}", "-".repeat(72));

    let mut combos: Vec<(i32, u32)> = samples
        .iter()
        .map(|s| (s.tile, (s.scale * 1000.0) as u32))
        .collect();
    combos.sort_unstable();
    combos.dedup();

    for (tile, scale_milli) in &combos {
        let mine: Vec<&Sample> = samples
            .iter()
            .filter(|s| s.tile == *tile && (s.scale * 1000.0) as u32 == *scale_milli)
            .collect();
        let Some(first) = mine.first() else { continue };

        let encode = median_wall(samples, *tile, *scale_milli);
        let mut renders: Vec<f64> = mine.iter().map(|s| s.render_ms).collect();
        renders.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let render = renders[renders.len() / 2];

        let px = if *tile == FULL_PAGE { 0 } else { *tile };
        let raw_kb = (px as f64 * px as f64 * 4.0) / 1024.0;

        println!(
            "{:>8} {:>7.3} {:>10.1} {:>10.1} {:>9.0} {:>9.0} {:>11.0}%",
            tile,
            *scale_milli as f32 / 1000.0,
            render,
            encode,
            raw_kb,
            first.bytes as f64 / 1024.0,
            encode / render * 100.0
        );
    }
}

fn median_wall(samples: &[Sample], tile: i32, scale_milli: u32) -> f64 {
    let mut values: Vec<f64> = samples
        .iter()
        .filter(|s| s.tile == tile && (s.scale * 1000.0) as u32 == scale_milli)
        .map(|s| s.wall_ms)
        .collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if values.is_empty() {
        return f64::NAN;
    }
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    }
}

fn pdfium_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TPDF_PDFIUM_DIR") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR))
        .unwrap_or_else(|| PathBuf::from("."))
}
