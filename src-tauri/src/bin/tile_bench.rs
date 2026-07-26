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
//! Usage:
//!   tile-bench <file.pdf> [--page N] [--rounds N] [--scales 1,2,4]
//!              [--tiles 256,512,1024,2048]

use std::path::{Path, PathBuf};
use std::time::Instant;

use pdfium_render::prelude::*;

struct Args {
    file: PathBuf,
    page: PdfPageIndex,
    rounds: usize,
    scales: Vec<f32>,
    tiles: Vec<i32>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let file = args.next().ok_or("usage: tile-bench <file.pdf> [options]")?;

    let mut parsed = Args {
        file: PathBuf::from(file),
        page: 0,
        rounds: 5,
        scales: vec![1.0, 2.0, 4.0],
        tiles: vec![256, 512, 1024, 2048],
    };

    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?;
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
            other => return Err(format!("unknown flag {other}")),
        }
    }

    Ok(parsed)
}

/// Peak resident set size in MB, via getrusage.
///
/// Pdfium allocates on the native heap, so a Rust global-allocator hook would
/// report close to nothing. On macOS ru_maxrss is bytes; on Linux, kilobytes.
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

    // Interleave every (tile, scale) combination within each round.
    let combos: Vec<(i32, f32)> = args
        .tiles
        .iter()
        .flat_map(|t| args.scales.iter().map(move |s| (*t, *s)))
        .collect();

    let mut samples: Vec<Sample> = Vec::new();

    for round in 0..args.rounds {
        for &(tile, scale) in &combos {
            match sweep_page(&page, tile, scale) {
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

    report(&samples, args.rounds);
    println!();
    println!("peak rss      {:.0} MB", peak_rss_mb());
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
            let ratio = if baseline > 0.0 { med / baseline } else { f64::NAN };

            println!(
                "{:>6} {:>6.1} {:>10.1} {:>7} {:>11.2} {:>10.1} {:>9.3}",
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
                let a = samples
                    .iter()
                    .find(|s| s.tile == 512 && (s.scale * 1000.0) as u32 == *scale_milli && s.round == round)?;
                let b = samples
                    .iter()
                    .find(|s| s.tile == *tile && (s.scale * 1000.0) as u32 == *scale_milli && s.round == round)?;
                Some(format!("{:.3}", b.wall_ms / a.wall_ms))
            })
            .collect();
        println!(
            "  tile={:<5} scale={:<4.1} {}",
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
            "  tile={:<5} scale={:<4.1} {:.1}%",
            tile,
            *scale_milli as f32 / 1000.0,
            render / wall * 100.0
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
        .map(|root| root.join("vendor/pdfium/lib"))
        .unwrap_or_else(|| PathBuf::from("."))
}
