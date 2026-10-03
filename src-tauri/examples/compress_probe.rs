//! How much smaller does a copy get, over a corpus?
//!
//! Not a check: it passes nothing and fails nothing. It is the instrument
//! behind the figures in `compress.rs` and `docs/PLAN.md`, and it prints sizes
//! and counts only --- no page text and no file name --- because it is pointed
//! at the reader's own documents.
//!
//! Each document is loaded with `lopdf`, made smaller in each of the four ways
//! `tpdf compress` offers, and serialised as that command serialises it. An
//! encrypted document, and one `lopdf` will not load, is counted and skipped.
//!
//! Usage:
//!   compress-probe <directory> [--emit DIR]
//!
//! `--emit` keeps every output, as `<n>-<way>.pdf`, for `qpdf --check` and for
//! looking at.

use std::path::PathBuf;

use lopdf::Document;
use tpdf_lib::compress::{apply, Compress, Preset};
use tpdf_lib::save::serialise_packed;

fn main() {
    let ways: [(&str, Compress); 4] = [
        ("lossless", Compress::Lossless),
        ("print", Compress::Pictures(Preset::Print.into())),
        ("balanced", Compress::Pictures(Preset::Balanced.into())),
        ("screen", Compress::Pictures(Preset::Screen.into())),
    ];
    let mut args = std::env::args().skip(1);
    let Some(directory) = args.next() else {
        eprintln!("usage: compress-probe <directory> [--emit DIR]");
        std::process::exit(2);
    };
    let emit = (args.next().as_deref() == Some("--emit"))
        .then(|| args.next().map(PathBuf::from))
        .flatten();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&directory)
        .expect("the directory reads")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        })
        .collect();
    files.sort();

    let (mut opened, mut skipped, mut before) = (0usize, 0usize, 0u64);
    let mut after = [0u64; 4];
    let mut smaller = [0usize; 4];
    let mut failed = [0usize; 4];
    let (mut pictures, mut changed) = ([0usize; 4], [0usize; 4]);
    let started = std::time::Instant::now();
    for (n, file) in files.iter().enumerate() {
        let Ok(bytes) = std::fs::read(file) else {
            skipped += 1;
            continue;
        };
        let loaded = Document::load_mem(&bytes);
        let Ok(source) = loaded else {
            skipped += 1;
            continue;
        };
        if source.is_encrypted() || source.get_pages().is_empty() {
            skipped += 1;
            continue;
        }
        opened += 1;
        before += bytes.len() as u64;
        for (at, (name, way)) in ways.iter().enumerate() {
            let mut doc = source.clone();
            let done = apply(&mut doc, *way);
            pictures[at] += done.pictures;
            changed[at] += done.pictures_changed;
            match serialise_packed(&mut doc, "the copy") {
                Ok(out) => {
                    // A copy that is not smaller is not written by the tool.
                    if out.len() < bytes.len() {
                        smaller[at] += 1;
                        after[at] += out.len() as u64;
                    } else {
                        after[at] += bytes.len() as u64;
                    }
                    if let Some(dir) = &emit {
                        let _ = std::fs::write(dir.join(format!("{n:03}-{name}.pdf")), &out);
                    }
                }
                Err(_) => {
                    failed[at] += 1;
                    after[at] += bytes.len() as u64;
                }
            }
        }
    }
    println!("documents opened {opened}, skipped {skipped}");
    #[allow(clippy::cast_precision_loss)]
    let mb = |bytes: u64| bytes as f64 / 1e6;
    println!("before {:>24.1} MB", mb(before));
    for (at, (name, _)) in ways.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let saved = 100.0 * (before - after[at]) as f64 / before.max(1) as f64;
        println!(
            "{name:<9} {:>8.1} MB  saves {saved:>5.1}%  {} of {opened} smaller, {} not serialised, \
             {} of {} pictures stored smaller",
            mb(after[at]),
            smaller[at],
            failed[at],
            changed[at],
            pictures[at],
        );
    }
    println!("ran in {:.1}s", started.elapsed().as_secs_f64());
}
