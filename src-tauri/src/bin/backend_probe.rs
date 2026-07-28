//! Proves that moving every document behind a process boundary changed nothing
//! the reader can see --- and that it really moved.
//!
//! `bin/worker_probe.rs` compares a worker against an in-process render at the
//! *protocol* level. This compares the two at the level callers actually use:
//! one [`RenderService`] per backend, driven through the same public methods the
//! viewer calls, on the same document. The comparison has to be on **pixels**,
//! because `AGENTS.md` records a sandboxed PDFium returning `ok` while drawing a
//! different typeface with about the same amount of ink.
//!
//! The order below is load-bearing. The worker service runs **first**, and the
//! absence of `libpdfium` from the dynamic linker's image table at that point is
//! what says the app process never maps the parser at all --- dyld's own table
//! rather than a milestone of ours, because a mark reports what our code
//! believes it did and the question is what the process *is*. The in-process
//! service then makes the same image appear, which is the control saying the
//! scan can see one and the first check was not passing on a wrong substring.
//!
//! ```text
//! cargo run --release --bin backend-probe -- testdata/text-heavy.pdf
//! ```

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use tpdf_lib::outline::Outline;
use tpdf_lib::render::{
    Backend, DocumentInfo, PageSize, RenderService, Tile, TileFormat, TileOutcome, TileRequest,
};
use tpdf_lib::search::PageMatches;
use tpdf_lib::startup;
use tpdf_lib::{worker, worker_child};

/// Tiles are compared at this size: inside the useful range `AGENTS.md`
/// measured, and small enough that a fixture renders quickly.
const TILE: u16 = 512;

/// A render at least this slow can have a withdrawal delivered into it.
///
/// Derived from the *first* tile's measured time, which no defect in the
/// withdrawal path can influence --- a skip condition read off the thing under
/// test is how a broken mechanism reports `[SKIP]` instead of `[FAIL]`.
const WITHDRAWABLE_MS: f64 = 120.0;

fn main() {
    // This binary is also the worker: `Worker::spawn` re-execs `current_exe`.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        worker_child::main(&args);
    }

    let Some(document) = args.get(1).map(PathBuf::from) else {
        eprintln!("usage: backend-probe <file.pdf>");
        std::process::exit(2);
    };
    if !document.exists() {
        eprintln!(
            "[FAIL] {} does not exist --- see AGENTS.md on generating fixtures",
            document.display()
        );
        std::process::exit(1);
    }

    let mut report = Report::default();
    let library_dir = library_dir();

    // ------------------------------------------------------- the worker first
    let workers = RenderService::start_with(library_dir.clone(), Backend::Worker);
    let worker_doc = match wait(|reply| workers.open(document.clone(), false, reply)) {
        Ok(info) => info,
        Err(e) => {
            println!("[FAIL] a worker-backed service opens the document      {e}");
            std::process::exit(1);
        }
    };
    // Placed from the document's own first page, which both backends report
    // identically or the geometry check below fails first.
    let at = Placement::inside(worker_doc.pages.first().unwrap_or(&PageSize {
        width_pt: 612.0,
        height_pt: 792.0,
    }));
    let worker_tile = tile_of(&workers, &worker_doc, 1, at);

    report.check(
        "the app process has not mapped libpdfium",
        !pdfium_is_mapped(),
        if pdfium_is_mapped() {
            "it has --- something in the worker path parses in this process".into()
        } else {
            format!(
                "{} pages opened and a tile rendered without it, {} images loaded",
                worker_doc.page_count,
                loaded_images()
            )
        },
    );
    report.check(
        "a worker was spawned to do it instead",
        marked("worker spawned"),
        marks(),
    );

    // ---------------------------------------------------- then the in-process
    let in_process = RenderService::start_with(library_dir, Backend::InProcess);
    let native_doc = match wait(|reply| in_process.open(document.clone(), false, reply)) {
        Ok(info) => info,
        Err(e) => {
            println!("[FAIL] an in-process service opens the document        {e}");
            std::process::exit(1);
        }
    };
    let native_tile = tile_of(&in_process, &native_doc, 1, at);

    // The control for the first check. Without it, "libpdfium is not mapped" is
    // equally satisfied by a scan that never matches anything --- a wrong
    // substring, a table read the wrong way --- and the strongest claim in this
    // file would rest on a typo.
    report.check(
        "the in-process backend does map it, so the scan can see it",
        pdfium_is_mapped(),
        format!("{} images loaded", loaded_images()),
    );
    report.check(
        "the two services really are different backends",
        workers.backend() == Backend::Worker && in_process.backend() == Backend::InProcess,
        format!("{:?} and {:?}", workers.backend(), in_process.backend()),
    );

    // ------------------------------------------------------------- the pixels
    match (&worker_tile, &native_tile) {
        (Ok(theirs), Ok(ours)) => {
            let same = theirs.bytes == ours.bytes;
            report.check(
                "a tile is identical whichever backend rendered it",
                same,
                if same {
                    format!("{} bytes", ours.bytes.len())
                } else {
                    format!(
                        "{} vs {} bytes, {} differing",
                        theirs.bytes.len(),
                        ours.bytes.len(),
                        differing(&theirs.bytes, &ours.bytes)
                    )
                },
            );
            // Without this, "identical" is satisfied by two blank buffers ---
            // which is exactly what a render that never ran produces.
            let distinct = distinct_values(&ours.bytes);
            report.check(
                "the compared tile is not a uniform buffer",
                distinct > 1,
                format!("{distinct} distinct byte values"),
            );
        }
        (worker, native) => {
            for (which, result) in [("worker", worker), ("in-process", native)] {
                if let Err(e) = result {
                    report.check(
                        &format!("the {which} backend renders a tile"),
                        false,
                        e.clone(),
                    );
                }
            }
        }
    }

    // The two request fields the comparison above leaves at their defaults. A
    // worker that dropped `turns` or `invert` on the way through the protocol
    // would render a perfectly good upright tile, and every check so far would
    // agree with it.
    let worker_view = wait(|reply| workers.tile(view_state_request(worker_doc.id, 2, at), reply));
    let native_view =
        wait(|reply| in_process.tile(view_state_request(native_doc.id, 2, at), reply));
    match (bytes_of(&worker_view), bytes_of(&native_view)) {
        (Ok(theirs), Ok(ours)) => {
            report.check(
                "a turned and inverted tile is identical too",
                theirs == ours,
                format!(
                    "{} bytes, {} differing",
                    ours.len(),
                    differing(&theirs, &ours)
                ),
            );
            // And the control that says the view state did something at all: if
            // it were dropped on *both* sides this would match the plain tile,
            // and "identical" would be two backends agreeing about nothing.
            let plain = worker_tile
                .as_ref()
                .map(|t| t.bytes.clone())
                .unwrap_or_default();
            report.check(
                "and it is not simply the plain tile again",
                ours != plain,
                format!(
                    "{} bytes against the plain tile's {}",
                    ours.len(),
                    plain.len()
                ),
            );
        }
        (theirs, ours) => report.check(
            "a turned and inverted tile is identical too",
            false,
            format!("{theirs:?} / {ours:?}"),
        ),
    }

    // ----------------------------------------------------------- the geometry
    let same_geometry = worker_doc.page_count == native_doc.page_count
        && worker_doc.pages.len() == native_doc.pages.len()
        && worker_doc
            .pages
            .iter()
            .zip(&native_doc.pages)
            .all(|(a, b)| same_size(a, b));
    report.check(
        "page geometry crosses the boundary unchanged",
        same_geometry,
        format!(
            "{} pages / {} sizes against {} and {}",
            worker_doc.page_count,
            worker_doc.pages.len(),
            native_doc.page_count,
            native_doc.pages.len()
        ),
    );

    // --------------------------------------------------------- the text layer
    // Not page 0 where there is a choice. Every one of these carries a page
    // number through the protocol, and a worker that ignored it and always read
    // the first page would be invisible to a check that only ever asks for the
    // first page.
    let page = u32::from(worker_doc.page_count > 1);
    let worker_text = wait(|reply| workers.text(worker_doc.id, page, reply));
    let native_text = wait(|reply| in_process.text(native_doc.id, page, reply));
    match (&worker_text, &native_text) {
        (Ok(theirs), Ok(ours)) => {
            let same = theirs.codes == ours.codes
                && theirs.boxes == ours.boxes
                && theirs.quarter_turns == ours.quarter_turns;
            report.check(
                "one page's characters and boxes survive the boundary",
                same,
                format!(
                    "{} codes, {} box values",
                    ours.codes.len(),
                    ours.boxes.len()
                ),
            );
        }
        _ => report.check(
            "one page's characters and boxes survive the boundary",
            false,
            format!("{worker_text:?} / {native_text:?}").replace('\n', " "),
        ),
    }

    // The control for the page number above: on a document whose pages carry
    // the same text, asking for a different one proves nothing, and this check
    // has to say so rather than looking like coverage.
    if page > 0 {
        let first = wait(|reply| in_process.text(native_doc.id, 0, reply));
        let distinguishable = match (&first, &native_text) {
            (Ok(a), Ok(b)) => a.codes != b.codes,
            _ => false,
        };
        if distinguishable {
            report.check(
                "the page asked for is one a wrong page number would betray",
                true,
                format!("page {page} reads differently from page 0"),
            );
        } else {
            report.skip(
                "the page asked for is one a wrong page number would betray",
                "page 0 and this page carry the same characters, so the checks \
                 around it cannot see a page number that was ignored",
            );
        }
    }

    // Searched for something a text page has and a vector one does not, so the
    // count below is evidence rather than a matching pair of zeroes.
    let query = "e".to_string();
    let worker_hits = wait(|reply| workers.search(worker_doc.id, page, query.clone(), reply));
    let native_hits = wait(|reply| in_process.search(native_doc.id, page, query.clone(), reply));
    report.check(
        "a search returns the same ranges on both",
        same_matches(&worker_hits, &native_hits),
        describe_matches(&native_hits),
    );

    let worker_outline = wait(|reply| workers.outline(worker_doc.id, reply));
    let native_outline = wait(|reply| in_process.outline(native_doc.id, reply));
    report.check(
        "an outline returns the same tree on both",
        same_outline(&worker_outline, &native_outline),
        describe_outline(&native_outline),
    );

    // ------------------------------------------------------------ withdrawing
    // Two halves, and they are withdrawn at different moments on purpose ---
    // see `RenderService::cancel`. The first never reaches a worker at all; the
    // second has to arrive while Pdfium is already inside the render.
    let (ahead, queued) = {
        // Two tiles, and the second is the one withdrawn. The render thread can
        // only be inside the first, so the second is still queued when the
        // withdrawal lands --- against a single request the window is one
        // channel handoff, which is a race this check would sometimes lose and
        // report as a broken withdrawal.
        let (tx, rx) = channel();
        let echo = tx.clone();
        workers.tile(
            request(worker_doc.id, 9, at),
            Box::new(move |result| {
                let _ = echo.send(result);
            }),
        );
        workers.tile(
            request(worker_doc.id, 10, at),
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        );
        workers.cancel(10);
        let first = rx.recv().unwrap_or_else(|_| Err("no reply".into()));
        let second = rx.recv().unwrap_or_else(|_| Err("no reply".into()));
        (first, second)
    };
    report.check(
        "the tile ahead of a withdrawal still renders",
        matches!(&ahead, Ok(TileOutcome::Rendered(_))),
        outcome_of(&ahead),
    );
    report.check(
        "a tile withdrawn before it starts comes back abandoned",
        matches!(queued, Ok(TileOutcome::Abandoned)),
        outcome_of(&queued),
    );

    let render_ms = worker_tile
        .as_ref()
        .map_or(0.0, |t| t.render_us as f64 / 1e3);
    if render_ms >= WITHDRAWABLE_MS {
        let running = {
            let (tx, rx) = channel();
            workers.tile(
                request(worker_doc.id, 11, at),
                Box::new(move |result| {
                    let _ = tx.send(result);
                }),
            );
            // Long enough that the worker is inside Pdfium, short enough that
            // it cannot have finished: the render takes `render_ms`.
            std::thread::sleep(Duration::from_millis(60));
            let sent = Instant::now();
            workers.cancel(11);
            let outcome = rx.recv().unwrap_or_else(|_| Err("no reply".into()));
            (outcome, sent.elapsed().as_secs_f64() * 1e3)
        };
        // Both halves, and the second is the one that matters. `Abandoned`
        // alone is what this side's own token produces whatever the worker
        // did --- so a withdrawal that never crossed the pipe would still
        // report it, after waiting out the entire render. What says the
        // worker actually stopped is that the reply came back long before
        // the render could have finished.
        let promptly = running.1 < render_ms / 3.0;
        report.check(
            "a withdrawal reaches a render already inside Pdfium",
            matches!(running.0, Ok(TileOutcome::Abandoned)) && promptly,
            format!(
                "{} after {:.1} ms, against a {render_ms:.0} ms render",
                outcome_of(&running.0),
                running.1
            ),
        );
    } else {
        report.skip(
            "a withdrawal reaches a render already inside Pdfium",
            format!(
                "a tile of this document renders in {render_ms:.1} ms, under the \
                 {WITHDRAWABLE_MS:.0} ms a withdrawal needs to arrive --- run this on \
                 testdata/vector-heavy.pdf"
            ),
        );
    }

    // And the control: the service still works afterwards, so "abandoned" is an
    // answer to the withdrawal rather than a worker that has stopped answering.
    let after = tile_of(&workers, &worker_doc, 12, at);
    report.check(
        "the worker-backed service still renders after a withdrawal",
        after.as_ref().is_ok_and(|t| !t.bytes.is_empty()),
        match &after {
            Ok(t) => format!("{} bytes", t.bytes.len()),
            Err(e) => e.clone(),
        },
    );

    report.finish();
}

/// Where on the page the compared tile is taken from.
///
/// Computed from the document's own geometry rather than fixed, because a
/// rectangle that lands in a margin renders a uniform buffer --- and two
/// backends agreeing about an empty tile is not evidence of anything. The
/// `rotated-90` fixture is where a fixed rectangle was caught doing exactly
/// that.
#[derive(Clone, Copy)]
struct Placement {
    scale: f32,
    x: i32,
    y: i32,
    width: u16,
    height: u16,
}

impl Placement {
    /// A rectangle inside the page, deliberately asymmetric.
    ///
    /// Not square, not at the origin and not at 1x: every field here has to
    /// survive translation into the worker's protocol and back, and a request
    /// whose width equals its height and whose `x` equals its `y` cannot tell a
    /// field that was dropped from one that arrived --- the pixels come out the
    /// same either way.
    fn inside(page: &PageSize) -> Self {
        let scale = 1.25_f32;
        let scaled_width = page.width_pt * scale;
        let scaled_height = page.height_pt * scale;
        // Different fractions in each axis, so a transposed pair is visible.
        let width = clamp_side(scaled_width * 0.55);
        let height = clamp_side(scaled_height * 0.4);
        Self {
            scale,
            x: ((scaled_width - f32::from(width)) / 3.0).max(0.0) as i32,
            y: ((scaled_height - f32::from(height)) / 5.0).max(0.0) as i32,
            width,
            height,
        }
    }
}

/// A tile side, kept inside the range `AGENTS.md` measured as useful and off
/// zero, which Pdfium has nothing to render into.
fn clamp_side(pixels: f32) -> u16 {
    pixels.clamp(64.0, f32::from(TILE)) as u16
}

/// One tile request at the chosen placement.
fn request(doc: u32, rid: u64, at: Placement) -> TileRequest {
    TileRequest {
        rid,
        doc,
        page: 0,
        scale: at.scale,
        turns: 0,
        invert: false,
        x: at.x,
        y: at.y,
        width: at.width,
        height: at.height,
        format: TileFormat::Raw,
    }
}

/// The same tile as the reader would see it turned and inverted.
///
/// `turns` and `invert` are the two request fields the plain comparison leaves
/// at their defaults, so they need a request that does not.
fn view_state_request(doc: u32, rid: u64, at: Placement) -> TileRequest {
    TileRequest {
        turns: 1,
        invert: true,
        ..request(doc, rid, at)
    }
}

/// Renders one tile and waits for it, failing an abandoned reply.
fn tile_of(
    service: &RenderService,
    doc: &DocumentInfo,
    rid: u64,
    at: Placement,
) -> Result<Tile, String> {
    match wait(|reply| service.tile(request(doc.id, rid, at), reply))? {
        TileOutcome::Rendered(tile) => Ok(tile),
        TileOutcome::Abandoned => Err("the tile was abandoned, and nothing withdrew it".into()),
    }
}

/// Drives one of the service's callback-shaped calls to an answer.
fn wait<T: Send + 'static>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, String>) + Send>),
) -> Result<T, String> {
    let (tx, rx): (_, Receiver<Result<T, String>>) = channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    rx.recv()
        .unwrap_or_else(|_| Err("the render thread stopped".into()))
}

/// Whether a startup milestone has been recorded in this process.
fn marked(name: &str) -> bool {
    startup::timeline().iter().any(|(mark, _)| mark == name)
}

/// Every dynamic library this process has mapped, by path.
///
/// The dynamic linker's own table, rather than a mark of our own: a milestone
/// says what our code believes it did, and the question here is what the process
/// actually is. Same reason `print.rs` reads its output back with a parser that
/// did not write it.
fn mapped_images() -> Vec<String> {
    // Declared here rather than taken from `libc`, which deprecates both in
    // favour of the `mach2` crate --- a dependency this repository would be
    // adding for two symbols, against a licensing rule that makes every new
    // crate a decision. The signatures are dyld's own and have not changed.
    extern "C" {
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_name(index: u32) -> *const std::os::raw::c_char;
    }

    // SAFETY: the count bounds the index, and every name dyld returns is a live
    // NUL-terminated string for as long as the image stays loaded --- nothing
    // here unloads one.
    unsafe {
        (0.._dyld_image_count())
            .filter_map(|i| {
                let name = _dyld_get_image_name(i);
                (!name.is_null()).then(|| {
                    std::ffi::CStr::from_ptr(name)
                        .to_string_lossy()
                        .into_owned()
                })
            })
            .collect()
    }
}

/// Whether the Pdfium library is mapped into this process at all.
fn pdfium_is_mapped() -> bool {
    mapped_images()
        .iter()
        .any(|image| image.to_lowercase().contains("pdfium"))
}

/// How many images are mapped, as the evidence that the scan read something.
fn loaded_images() -> usize {
    mapped_images().len()
}

/// The milestones recorded so far, as evidence for whichever check asks.
fn marks() -> String {
    startup::timeline()
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Page sizes compared exactly: these are the same arithmetic on both sides, so
/// a tolerance would only hide a mapping that had genuinely changed.
fn same_size(a: &PageSize, b: &PageSize) -> bool {
    a.width_pt == b.width_pt && a.height_pt == b.height_pt
}

fn same_matches(a: &Result<PageMatches, String>, b: &Result<PageMatches, String>) -> bool {
    match (a, b) {
        (Ok(a), Ok(b)) => a.page == b.page && a.chars == b.chars && a.matches == b.matches,
        _ => false,
    }
}

fn describe_matches(result: &Result<PageMatches, String>) -> String {
    match result {
        Ok(m) => format!("{} hits over {} characters", m.matches.len(), m.chars),
        Err(e) => e.clone(),
    }
}

/// Outlines compared through their serialisation.
///
/// The tree is deep and its equality is structural, so comparing the JSON is
/// both shorter and stricter than a hand-written walk --- and it compares the
/// exact bytes the frontend would receive, which is what the claim is about.
fn same_outline(a: &Result<Outline, String>, b: &Result<Outline, String>) -> bool {
    match (a, b) {
        (Ok(a), Ok(b)) => {
            a.total == b.total
                && a.limits == b.limits
                && serde_json::to_string(&a.items).ok() == serde_json::to_string(&b.items).ok()
        }
        _ => false,
    }
}

fn describe_outline(result: &Result<Outline, String>) -> String {
    match result {
        Ok(o) => format!("{} entries, limits {:?}", o.total, o.limits),
        Err(e) => e.clone(),
    }
}

/// The pixels of a rendered tile, or why there are none.
fn bytes_of(result: &Result<TileOutcome, String>) -> Result<Vec<u8>, String> {
    match result {
        Ok(TileOutcome::Rendered(tile)) => Ok(tile.bytes.clone()),
        Ok(TileOutcome::Abandoned) => Err("abandoned, and nothing withdrew it".into()),
        Err(e) => Err(e.clone()),
    }
}

fn outcome_of(result: &Result<TileOutcome, String>) -> String {
    match result {
        Ok(TileOutcome::Abandoned) => "abandoned".into(),
        Ok(TileOutcome::Rendered(tile)) => format!("rendered {} bytes", tile.bytes.len()),
        Err(e) => e.clone(),
    }
}

/// How many bytes differ, for a failure that has to say how badly.
fn differing(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()
}

/// How many distinct values a tile holds, as evidence of content.
///
/// Over the whole tile, not a prefix: the first kilobyte of a text page is the
/// white margin, so a prefix reads `1` on a perfectly good render and makes the
/// control look like it is failing.
fn distinct_values(bytes: &[u8]) -> usize {
    let mut seen = [false; 256];
    for b in bytes {
        seen[*b as usize] = true;
    }
    seen.iter().filter(|s| **s).count()
}

/// Where Pdfium lives, matching the app's own resolution in development.
fn library_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("vendor/pdfium/lib"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Prints each result as it is recorded and exits non-zero on any failure.
///
/// Printed immediately rather than buffered: `AGENTS.md` records an afternoon
/// spent on a harness that printed only at the end, where a run that stopped
/// midway was indistinguishable from one that never started.
#[derive(Default)]
struct Report {
    checks: usize,
    failures: usize,
    skipped: usize,
}

impl Report {
    fn check(&mut self, name: &str, ok: bool, detail: impl AsRef<str>) {
        self.checks += 1;
        if !ok {
            self.failures += 1;
        }
        println!(
            "[{}] {name:56} {}",
            if ok { "OK" } else { "FAIL" },
            detail.as_ref()
        );
    }

    /// Records a check that could not apply, with the reason.
    ///
    /// Counted and named rather than omitted: a control that silently
    /// disappears on some inputs is indistinguishable from one that ran.
    fn skip(&mut self, name: &str, why: impl AsRef<str>) {
        self.checks += 1;
        self.skipped += 1;
        println!("[SKIP] {name:56} {}", why.as_ref());
    }

    fn finish(&self) -> ! {
        println!(
            "\n{}/{} checks passed, {} skipped",
            self.checks - self.failures - self.skipped,
            self.checks,
            self.skipped
        );
        std::process::exit(i32::from(self.failures > 0));
    }
}
