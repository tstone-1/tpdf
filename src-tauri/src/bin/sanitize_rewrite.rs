//! Spike 0.4: does a garbage-collected full rewrite actually sanitize a file?
//!
//! `docs/PLAN.md` §6 requires redaction to write a fresh file from a
//! garbage-collected reachable object graph rather than re-serializing the
//! original bytes, and open question 4 asks whether `lopdf` can do that on a
//! hostile corpus or whether QPDF is required. This measures both.
//!
//! The corpus is `testdata/make_hostile_pdf.py`, which hides a distinct needle
//! in each of eleven places and records in `hostile-manifest.json` whether a
//! reachability sweep is *expected* to remove it. Half the needles are reachable
//! and must survive: a rewrite that dropped them would be losing document
//! content, not sanitizing it, and a corpus without them would let the spike
//! mistake destruction for cleanliness.
//!
//! Verification follows the rule spike 0.3 arrived at the hard way: a check that
//! cannot decode a carrier has not verified anything. Every stream is decoded
//! under a bound, and anything undecodable is reported as a blind spot that
//! makes the whole result *not verified* rather than clean.
//!
//! Usage:
//!     sanitize-rewrite [--manifest PATH] [--outdir DIR] [--only NAME] [--bench PATH]

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use lopdf::{Document, LoadOptions, Object as LoObject, ObjectId};
use pdfium_render::prelude::*;
use serde::Deserialize;

/// Ceiling on any single decoded stream. A carrier that will not fit is a blind
/// spot, never a pass -- which is exactly what the `bomb` fixture provokes.
const MAX_DECODE: usize = 64 * 1024 * 1024;

/// Renders are compared at this scale to check the rewrite kept the page.
const RENDER_SCALE: f32 = 1.5;

/// PDFium's rasterisation is not bit-deterministic once object order changes, so
/// a channel has to move by more than this to count. Same value as spike 0.3.
const CHANNEL_TOLERANCE: i16 = 8;

#[derive(Deserialize)]
struct Manifest {
    fixtures: Vec<Fixture>,
}

#[derive(Deserialize)]
struct Fixture {
    file: String,
    carriers: Vec<Carrier>,
}

#[derive(Deserialize)]
struct Carrier {
    needle: String,
    #[serde(rename = "where")]
    location: String,
    /// `removed`, `survives` or `unverifiable`; see `make_hostile_pdf.py`.
    expect: String,
}

impl Carrier {
    /// Whether a reachability sweep is supposed to clear this carrier.
    fn must_go(&self) -> bool {
        self.expect == "removed"
    }

    /// Whether the carrier is one no verifier can read under a resource bound,
    /// and where the only correct outcome is therefore *not verified*.
    fn unreadable(&self) -> bool {
        self.expect == "unverifiable"
    }
}

/// One way of writing the file out again.
struct Route {
    name: &'static str,
    what: &'static str,
    run: fn(&Path, &Path) -> Result<String, String>,
}

const ROUTES: &[Route] = &[
    Route {
        name: "copy",
        what: "byte copy -- the control, which keeps everything",
        run: route_copy,
    },
    Route {
        name: "lopdf",
        what: "lopdf load + save, no collection",
        run: route_lopdf,
    },
    Route {
        name: "lopdf-gc",
        what: "lopdf load + prune_objects + renumber + save",
        run: route_lopdf_gc,
    },
    Route {
        name: "lopdf-mark",
        what: "lopdf load + our own mark-and-sweep + save",
        run: route_lopdf_mark,
    },
    Route {
        name: "qpdf",
        what: "qpdf in out",
        run: route_qpdf,
    },
    Route {
        name: "qpdf-objstm",
        what: "qpdf --object-streams=generate",
        run: route_qpdf_objstm,
    },
];

struct Args {
    manifest: PathBuf,
    outdir: PathBuf,
    only: Option<String>,
    bench: Option<PathBuf>,
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("[FAIL] {message}");
            return ExitCode::FAILURE;
        }
    };

    let dir = pdfium_dir();
    let library = Pdfium::pdfium_platform_library_name_at_path(&dir);
    let pdfium = match Pdfium::bind_to_library(&library) {
        Ok(bindings) => Pdfium::new(bindings),
        Err(error) => {
            eprintln!("[FAIL] could not bind PDFium at {}: {error}", dir.display());
            return ExitCode::FAILURE;
        }
    };

    match run(&pdfium, &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("[FAIL] {message}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args() -> Result<Args, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("the crate has no parent directory")?
        .to_path_buf();
    let mut args = Args {
        manifest: root.join("testdata/hostile-manifest.json"),
        outdir: root.join("src-tauri/target/spike04"),
        only: None,
        bench: None,
    };

    let mut raw = std::env::args().skip(1);
    while let Some(flag) = raw.next() {
        let mut value = || raw.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--manifest" => args.manifest = PathBuf::from(value()?),
            "--outdir" => args.outdir = PathBuf::from(value()?),
            "--only" => args.only = Some(value()?),
            "--bench" => args.bench = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

fn run(pdfium: &Pdfium, args: &Args) -> Result<(), String> {
    let text = fs::read_to_string(&args.manifest).map_err(|e| {
        format!(
            "could not read {} ({e}) -- run testdata/make_hostile_pdf.py first",
            args.manifest.display()
        )
    })?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|e| format!("could not parse the manifest: {e}"))?;
    let corpus = args
        .manifest
        .parent()
        .ok_or("the manifest has no directory")?
        .to_path_buf();

    fs::create_dir_all(&args.outdir)
        .map_err(|e| format!("could not create {}: {e}", args.outdir.display()))?;

    println!("corpus:  {}", corpus.display());
    println!("outputs: {}", args.outdir.display());
    println!("decode bound: {} MiB per stream", MAX_DECODE >> 20);
    for route in ROUTES {
        println!("  {:<12} {}", route.name, route.what);
    }

    let mut vacuous = Vec::new();
    for fixture in &manifest.fixtures {
        if args.only.iter().any(|only| !fixture.file.contains(only)) {
            continue;
        }
        match fixture_report(pdfium, args, &corpus, fixture) {
            Ok(true) => {}
            Ok(false) => vacuous.push(fixture.file.clone()),
            Err(message) => return Err(format!("{}: {message}", fixture.file)),
        }
    }

    if !vacuous.is_empty() {
        println!("\n[FAIL] these fixtures could not be shown to contain their own needles");
        println!("       before any rewrite, so nothing they report afterwards means anything:");
        for name in &vacuous {
            println!("       {name}");
        }
    }

    if let Some(bench) = &args.bench {
        bench_report(args, bench)?;
    }
    Ok(())
}

/// Runs every route over one fixture. Returns whether the fixture was usable.
fn fixture_report(
    pdfium: &Pdfium,
    args: &Args,
    corpus: &Path,
    fixture: &Fixture,
) -> Result<bool, String> {
    let input = corpus.join(&fixture.file);
    let needles: Vec<String> = fixture.carriers.iter().map(|c| c.needle.clone()).collect();

    println!("\n=== {} ===", fixture.file);
    for carrier in &fixture.carriers {
        println!(
            "  {:<9} {}",
            format!("[{}]", carrier.expect),
            carrier.location
        );
    }

    // The precondition, and the reason PLAN.md calls a bare string search a smoke
    // test: a needle that cannot be found in the *input* proves nothing when it
    // is missing from the output. Spike 0.3's scanner reported a document clean
    // that was not; a scanner that reports a fixture clean it never could read is
    // the same failure one step earlier.
    let before = verify(&input, &needles)?;
    let unfound: Vec<&str> = fixture
        .carriers
        .iter()
        .filter(|c| !c.unreadable() && !before.found.contains(&c.needle))
        .map(|c| c.location.as_str())
        .collect();
    if !unfound.is_empty() {
        println!("  input: needle NOT found in {}", unfound.join(", "));
        if !before.blind.is_empty() {
            for spot in &before.blind {
                println!("         blind spot: {spot}");
            }
        }
    }
    let usable = unfound.is_empty();

    let baseline = render(pdfium, &input, RENDER_SCALE).ok();
    if baseline.is_none() {
        println!("  input: PDFium could not render page 1, so no pixel comparison");
    }

    println!(
        "  {:<12} {:>7} {:>8} {:>6} {:>5} {:>6} {:>7} {:>8} {:>4}  verdict",
        "route", "ms", "bytes", "objs", "eofs", "trail", "leaks", "dropped", "px"
    );

    for route in ROUTES {
        let output = args
            .outdir
            .join(format!("{}.{}.pdf", stem(&fixture.file), route.name));
        let rss_before = child_peak_rss();
        let started = Instant::now();
        let mut note = match (route.run)(&input, &output) {
            Ok(note) => note,
            Err(message) => {
                println!("  {:<12} {message}", route.name);
                continue;
            }
        };
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        // The figure is a running maximum over every child this process has ever
        // spawned, so only an increase can be attributed to this route -- but an
        // increase is exactly what a hostile file is trying to cause.
        let rss_after = child_peak_rss();
        if rss_after > rss_before {
            note = format!("{note} (a child process peaked at {} MiB)", rss_after >> 20);
        }

        let after = verify(&output, &needles)?;
        let leaks: Vec<&str> = fixture
            .carriers
            .iter()
            .filter(|c| c.must_go() && after.found.contains(&c.needle))
            .map(|c| c.location.as_str())
            .collect();
        let dropped: Vec<&str> = fixture
            .carriers
            .iter()
            .filter(|c| c.expect == "survives" && !after.found.contains(&c.needle))
            .map(|c| c.location.as_str())
            .collect();

        let pixels = match (&baseline, render(pdfium, &output, RENDER_SCALE)) {
            (Some(before), Ok(rendered)) => match changed_pixels(before, &rendered) {
                Ok(count) => count.to_string(),
                Err(message) => message,
            },
            (_, Err(_)) => "err".to_string(),
            (None, _) => "-".to_string(),
        };

        let verdict = if !after.blind.is_empty() {
            "NOT VERIFIED"
        } else if !leaks.is_empty() {
            "LEAKS"
        } else if !dropped.is_empty() {
            "clean, lossy"
        } else {
            "clean"
        };

        println!(
            "  {:<12} {:>7.1} {:>8} {:>6} {:>5} {:>6} {:>7} {:>8} {:>4}  {verdict}",
            route.name,
            elapsed,
            after.bytes,
            after.objects,
            after.eofs,
            after.trailing,
            leaks.len(),
            dropped.len(),
            pixels,
        );
        // A carrier nothing can read must produce "not verified". If the verifier
        // instead announces a verdict on it, the verifier is the thing that is
        // broken -- which is how spike 0.3's leak scanner passed a document that
        // was still leaking.
        if after.blind.is_empty() && fixture.carriers.iter().any(Carrier::unreadable) {
            println!("               [FAIL] a verdict was reached on a carrier nothing can read");
        }
        for spot in &after.blind {
            println!("               not verified: {spot}");
        }
        for leak in &leaks {
            println!("               leak: {leak}");
        }
        for loss in &dropped {
            println!("               dropped: {loss}");
        }
        let check = qpdf_check(&output);
        if check != "ok" {
            println!("               qpdf --check: {check}");
        }
        if !note.is_empty() {
            println!("               {note}");
        }
    }

    Ok(usable)
}

/// Times each route on one large document, to expose cost that a corpus of
/// kilobyte fixtures cannot.
fn bench_report(args: &Args, file: &Path) -> Result<(), String> {
    println!("\n=== rewrite cost on {} ===", file.display());
    let size = fs::metadata(file)
        .map_err(|e| format!("could not stat {}: {e}", file.display()))?
        .len();
    println!("  input {size} bytes");
    println!("  {:<12} {:>9} {:>10}  note", "route", "ms", "bytes");
    for route in ROUTES {
        let output = args.outdir.join(format!("bench.{}.pdf", route.name));
        let started = Instant::now();
        match (route.run)(file, &output) {
            Ok(note) => {
                let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                let written = fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
                println!(
                    "  {:<12} {:>9.1} {:>10}  {note}",
                    route.name, elapsed, written
                );
            }
            Err(message) => println!("  {:<12} {message}", route.name),
        }
    }
    Ok(())
}

fn route_copy(input: &Path, output: &Path) -> Result<String, String> {
    fs::copy(input, output).map_err(|e| format!("copy failed: {e}"))?;
    Ok(String::new())
}

fn route_lopdf(input: &Path, output: &Path) -> Result<String, String> {
    let mut doc = load(input)?;
    doc.save(output).map_err(|e| format!("save failed: {e}"))?;
    Ok(String::new())
}

fn route_lopdf_gc(input: &Path, output: &Path) -> Result<String, String> {
    let mut doc = load(input)?;
    let before = doc.objects.len();
    let pruned = doc.prune_objects();
    doc.renumber_objects();
    doc.save(output).map_err(|e| format!("save failed: {e}"))?;
    Ok(format!("collected {} of {before} objects", pruned.len()))
}

/// The same collection as `lopdf-gc`, written here rather than called.
///
/// `prune_objects` and `renumber_objects` both walk the graph through
/// `traverse_objects`, which records what it has seen in a `Vec` and asks
/// `contains` before each push -- so both are quadratic in the object count. The
/// algorithm is not; it is a mark-and-sweep over a hash set. Renumbering is
/// dropped entirely: contiguous object numbers are cosmetic, and paying a second
/// quadratic pass for tidiness is not a trade worth making.
fn route_lopdf_mark(input: &Path, output: &Path) -> Result<String, String> {
    let mut doc = load(input)?;
    let before = doc.objects.len();
    let reachable = reachable_objects(&doc);
    doc.objects.retain(|id, _| reachable.contains(id));
    // `/Size` is written from `max_id`, which sweeping does not touch. Leaving it
    // alone claims more objects than the file contains, and QPDF says so. Caught
    // by the independent-parser check on this route's first run, which is the
    // whole argument for having one: every other check passed.
    doc.max_id = doc.objects.keys().map(|id| id.0).max().unwrap_or(0);
    doc.save(output).map_err(|e| format!("save failed: {e}"))?;
    Ok(format!(
        "collected {} of {before} objects",
        before - doc.objects.len()
    ))
}

/// Every object reachable from the trailer, by breadth-first mark.
fn reachable_objects(doc: &Document) -> HashSet<ObjectId> {
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut queue: Vec<ObjectId> = Vec::new();
    let mark = |id: ObjectId, seen: &mut HashSet<ObjectId>, queue: &mut Vec<ObjectId>| {
        if seen.insert(id) {
            queue.push(id);
        }
    };

    let trailer = LoObject::Dictionary(doc.trailer.clone());
    let mut roots = Vec::new();
    collect_references(&trailer, &mut roots);
    for id in roots {
        mark(id, &mut seen, &mut queue);
    }

    while let Some(id) = queue.pop() {
        let Some(object) = doc.objects.get(&id) else {
            // A dangling reference. Not this pass's problem: it names no object,
            // so it carries no content.
            continue;
        };
        let mut referenced = Vec::new();
        collect_references(object, &mut referenced);
        for id in referenced {
            mark(id, &mut seen, &mut queue);
        }
    }
    seen
}

fn collect_references(object: &LoObject, out: &mut Vec<ObjectId>) {
    match object {
        LoObject::Reference(id) => out.push(*id),
        LoObject::Array(items) => items.iter().for_each(|i| collect_references(i, out)),
        LoObject::Dictionary(dictionary) => dictionary
            .iter()
            .for_each(|(_, v)| collect_references(v, out)),
        LoObject::Stream(stream) => stream
            .dict
            .iter()
            .for_each(|(_, v)| collect_references(v, out)),
        _ => {}
    }
}

fn route_qpdf(input: &Path, output: &Path) -> Result<String, String> {
    qpdf(&[input, output])
}

fn route_qpdf_objstm(input: &Path, output: &Path) -> Result<String, String> {
    qpdf(&[Path::new("--object-streams=generate"), input, output])
}

/// Loads a document with the same bound the verifier uses.
fn load(path: &Path) -> Result<Document, String> {
    Document::load_with_options(
        path,
        LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            ..Default::default()
        },
    )
    .map_err(|e| format!("lopdf could not parse {}: {e}", path.display()))
}

fn qpdf(args: &[&Path]) -> Result<String, String> {
    let output = Command::new("qpdf")
        .args(args)
        .output()
        .map_err(|e| format!("could not run qpdf: {e}"))?;
    let code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    // qpdf exits 0 on success, 3 on warnings, 2 on errors.
    if code != 0 && code != 3 {
        return Err(format!("qpdf exited {code}: {stderr}"));
    }
    Ok(if stderr.is_empty() {
        String::new()
    } else {
        format!("qpdf warned: {}", stderr.replace('\n', "; "))
    })
}

/// Re-parses the written file with QPDF, which PLAN.md §6 requires so that a
/// single library's blind spot cannot certify its own output.
fn qpdf_check(path: &Path) -> String {
    let output = match Command::new("qpdf").arg("--check").arg(path).output() {
        Ok(output) => output,
        Err(error) => return format!("could not run qpdf: {error}"),
    };
    let code = output.status.code().unwrap_or(-1);
    let noise: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .chain(String::from_utf8_lossy(&output.stderr).lines())
        .filter(|line| line.contains("WARNING") || line.contains("ERROR"))
        .map(|line| line.trim().to_string())
        .collect();
    match (code, noise.is_empty()) {
        (0, true) => "ok".to_string(),
        _ => format!("exit {code}: {}", noise.join("; ")),
    }
}

/// What one scan of a written file established, and what it could not look at.
struct Report {
    found: BTreeSet<String>,
    blind: Vec<String>,
    objects: usize,
    /// `%%EOF` markers, so more than one revision is visible.
    eofs: usize,
    /// Non-whitespace bytes after the last `%%EOF`.
    trailing: usize,
    bytes: u64,
}

/// Scans a file for every needle, at the byte level and through the object graph.
///
/// Both are needed and neither is sufficient. The byte scan is the only thing
/// that can see content outside the object graph -- trailing bytes past `%%EOF`
/// belong to no object at all -- and the graph walk is the only thing that can
/// see inside a compressed stream.
fn verify(path: &Path, needles: &[String]) -> Result<Report, String> {
    let raw = fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let mut report = Report {
        found: BTreeSet::new(),
        blind: Vec::new(),
        objects: 0,
        eofs: count(&raw, b"%%EOF"),
        trailing: 0,
        bytes: raw.len() as u64,
    };

    for needle in needles {
        if find_bytes(&raw, needle.as_bytes()) {
            report.found.insert(needle.clone());
        }
    }

    // More than one revision is a blind spot, not a detail. A parser resolves
    // every object through the newest cross-reference table, so an object the
    // update overwrote is never handed to the graph walk at all -- its bytes are
    // still in the file, at their old offset, addressable by nothing. If they
    // are also compressed, no check in this harness can see them.
    if report.eofs > 1 {
        report.blind.push(format!(
            "the file has {} %%EOF markers, so earlier revisions exist that no \
             parser will resolve and no scan can decode",
            report.eofs
        ));
    }

    if let Some(last) = rfind_bytes(&raw, b"%%EOF") {
        report.trailing = raw[last + 5..]
            .iter()
            .filter(|byte| !byte.is_ascii_whitespace())
            .count();
    } else {
        report
            .blind
            .push("the file has no %%EOF marker".to_string());
    }

    match load(path) {
        Err(message) => report.blind.push(message),
        Ok(doc) => {
            report.objects = doc.objects.len();
            for (id, object) in &doc.objects {
                let strings = flatten_strings(object);
                for needle in needles {
                    if find_bytes(&strings, needle.as_bytes()) {
                        report.found.insert(needle.clone());
                    }
                }
                let LoObject::Stream(stream) = object else {
                    continue;
                };
                match stream.decompressed_content_with_limit(MAX_DECODE) {
                    Ok(decoded) => {
                        for needle in needles {
                            if find_bytes(&decoded, needle.as_bytes()) {
                                report.found.insert(needle.clone());
                            }
                        }
                    }
                    Err(error) => report.blind.push(format!(
                        "object {} could not be decoded, so its contents are unknown: {error}",
                        id.0
                    )),
                }
            }
        }
    }

    report.blind.sort();
    report.blind.dedup();
    Ok(report)
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn rfind_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

/// Concatenates every string buried anywhere in an object, however nested.
fn flatten_strings(object: &LoObject) -> Vec<u8> {
    let mut out = Vec::new();
    collect_strings(object, &mut out);
    out
}

fn collect_strings(object: &LoObject, out: &mut Vec<u8>) {
    match object {
        LoObject::String(bytes, _) => {
            out.extend_from_slice(bytes);
            // A separator, so two adjacent strings cannot spell out a needle
            // that neither of them contains.
            out.push(0);
        }
        LoObject::Array(items) => items.iter().for_each(|i| collect_strings(i, out)),
        LoObject::Dictionary(dictionary) => {
            dictionary.iter().for_each(|(_, v)| collect_strings(v, out))
        }
        LoObject::Stream(stream) => {
            collect_strings(&LoObject::Dictionary(stream.dict.clone()), out)
        }
        _ => {}
    }
}

struct Render {
    rgba: Vec<u8>,
    width: i32,
    height: i32,
}

/// Renders page 1 to RGBA at the given scale.
fn render(pdfium: &Pdfium, file: &Path, scale: f32) -> Result<Render, String> {
    let doc = pdfium
        .load_pdf_from_file(file, None)
        .map_err(|e| format!("could not open {}: {e}", file.display()))?;
    let page = doc
        .pages()
        .get(0)
        .map_err(|e| format!("no first page: {e}"))?;

    let width = (page.width().value * scale).round() as i32;
    let height = (page.height().value * scale).round() as i32;
    let config = PdfRenderConfig::new()
        .set_target_width(width)
        .set_target_height(height);
    let mut bitmap = PdfBitmap::empty(width as Pixels, height as Pixels, PdfBitmapFormat::BGRA)
        .map_err(|e| format!("could not allocate {width}x{height}: {e}"))?;
    page.render_into_bitmap_with_config(&mut bitmap, &config)
        .map_err(|e| format!("render failed: {e}"))?;

    Ok(Render {
        rgba: bitmap.as_rgba_bytes(),
        width,
        height,
    })
}

/// Counts device pixels that differ between two renders of the same page.
fn changed_pixels(before: &Render, after: &Render) -> Result<usize, String> {
    if before.width != after.width || before.height != after.height {
        return Err("size".to_string());
    }
    let changed = before
        .rgba
        .chunks_exact(4)
        .zip(after.rgba.chunks_exact(4))
        .filter(|(a, b)| {
            (0..4).any(|channel| (a[channel] as i16 - b[channel] as i16).abs() > CHANNEL_TOLERANCE)
        })
        .count();
    Ok(changed)
}

/// Peak resident memory of any child process spawned so far, in bytes.
///
/// The external rewriters run out of process, so a bomb that costs a gigabyte to
/// decode shows up here and nowhere else. Rust's allocator sees none of it.
fn child_peak_rss() -> u64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &mut usage) } != 0 {
        return 0;
    }
    let reported = usage.ru_maxrss as u64;
    // macOS reports bytes; the BSD/Linux convention is kilobytes.
    if cfg!(target_os = "macos") {
        reported
    } else {
        reported * 1024
    }
}

/// The file name without its extension, for naming outputs.
fn stem(file: &str) -> String {
    file.strip_suffix(".pdf").unwrap_or(file).to_string()
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
