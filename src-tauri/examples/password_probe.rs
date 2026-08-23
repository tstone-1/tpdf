//! Whether a locked document can actually be opened, through the real worker.
//!
//! ## Why this cannot be a unit test
//!
//! Everything that decides the answer lives on the other side of a process
//! boundary. The load is in `worker_child::serve`, the retry is in
//! `worker_child::unlock`, the password crosses on the worker's stdin, and the
//! pool replays it in `Workers::spawn_into`. A test in the app process can
//! reach none of that: `docs/TRAPS.md` has the entry --- *where the parse runs is
//! not observable from a unit test*, and it was written about this exact
//! boundary.
//!
//! So this drives a real `RenderService` in worker mode against a real encrypted
//! document, and reads the answers the reader would get.
//!
//! ## What it establishes, and the control for each
//!
//! - **A locked document refuses with `locked` set**, not with an error. The
//!   control is the unencrypted fixture, which opens with no password at all ---
//!   without it, a backend that reported *everything* as locked would pass.
//! - **A wrong password is refused, and says something different.** PDFium
//!   answers `FPDF_ERR_PASSWORD` for "no password" and "wrong password" alike, so
//!   the two sentences can only differ if `unlock` tracked that it had tried one.
//!   Comparing them is what proves the retry happened rather than the first
//!   refusal being repeated.
//! - **The right password opens it, and the document then renders.** Opening is
//!   not enough: the worker has to leave `unlock` and reach the ordinary serve
//!   loop, and a tile with ink in it is what says it did.
//! - **A second worker for the same document also opens it.** The one with no
//!   other coverage. Every worker after the first is built by `spawn_into` from
//!   the same bytes, so it meets the same encryption; a pool that unlocked only
//!   its first worker would render the first page a reader looked at and refuse
//!   the next. Growth is *forced* rather than hoped for --- see `grow`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --manifest-path src-tauri/Cargo.toml \
//!   --example password-probe
//! ```
//!
//! It takes no arguments: the fixtures it needs are the two it names, because
//! the properties are about encryption rather than about content.

use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Duration;

use tpdf_lib::progressive::Refusal;
use tpdf_lib::render::{
    Backend, DocumentInfo, PageSize, RenderService, TileFormat, TileOutcome, TileRequest,
};
use tpdf_lib::{worker, worker_child};

/// The encrypted fixture, and the password `make_incremental_pdf.py` gives it.
const LOCKED: &str = "testdata/incr-encrypted-pw.pdf";
const PASSWORD: &str = "swordfish";
/// One that is not encrypted at all, so every check above has a control.
const PLAIN: &str = "testdata/comments.pdf";

/// The longest any single answer is waited for.
///
/// A wrong password is answered in microseconds and an open in milliseconds, so
/// this is far above every legitimate wait --- and it exists because the failure
/// shape that matters here is a **wait**: a worker parked in `unlock` for a
/// message the parent never sends does not answer wrongly, it does not answer.
/// `docs/TRAPS.md`: *a check whose failure mode is a wait cannot fail*.
const ANSWER_BOUND: Duration = Duration::from_secs(30);

fn main() {
    // This binary is its own worker, exactly as the other probes are: the
    // service re-execs `current_exe` with the worker marker.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        worker_child::main(&args);
    }

    let mut report = Report::default();

    let locked = PathBuf::from(LOCKED);
    let plain = PathBuf::from(PLAIN);
    if !locked.exists() {
        // Named rather than omitted, and it is worth being loud: this fixture
        // needs qpdf, and `scripts/ci_fixtures.py` records that a hosted runner
        // has none. A silent pass here would be a password path with no
        // coverage anywhere, which is the defect 5850cf2 was about.
        for name in CHECKS {
            report.skip(name, format!("{LOCKED} is not generated (it needs qpdf)"));
        }
        return report.finish();
    }

    let service = RenderService::start_with(library_dir(), Backend::Worker);

    // ------------------------------------------------- refused, and answerably
    let refused = open(&service, &locked, None);
    report.check(
        CHECKS[0],
        refused.as_ref().err().is_some_and(|r| r.locked),
        describe(&refused),
    );

    // The control, and it is what stops "everything is locked" from passing.
    let plain_open = open(&service, &plain, None);
    report.check(CHECKS[1], plain_open.is_ok(), describe(&plain_open));

    // ------------------------------------------------------- a wrong password
    let wrong = open(&service, &locked, Some("not the password"));
    let first = refused.as_ref().err().map(|r| r.reason.clone());
    let second = wrong.as_ref().err().map(|r| r.reason.clone());
    report.check(
        CHECKS[2],
        wrong.as_ref().err().is_some_and(|r| r.locked),
        describe(&wrong),
    );
    report.check(
        CHECKS[3],
        // Both present and different. `Option` inequality would also be
        // satisfied by one of them being absent, which is a refusal that never
        // arrived rather than one that was worded differently.
        first.is_some() && second.is_some() && first != second,
        format!("{first:?} then {second:?}"),
    );

    // -------------------------------------------------- the right one, and ink
    let opened = open(&service, &locked, Some(PASSWORD));
    report.check(
        CHECKS[4],
        opened.as_ref().is_ok_and(|d| d.page_count == 2),
        describe(&opened),
    );

    match &opened {
        Err(e) => {
            for name in &CHECKS[5..] {
                report.skip(name, format!("the document did not open: {}", e.reason));
            }
        }
        Ok(doc) => {
            let at = Placement::inside(doc.pages.first().unwrap_or(&PageSize {
                width_pt: 612.0,
                height_pt: 792.0,
            }));

            // Ink, not bytes. A tile of the right size full of paper is what a
            // worker that answered without rendering would produce, and it is
            // the reassuring outcome --- `docs/TRAPS.md` has the entry.
            let tile = tile_of(&service, doc.id, 1, at);
            report.check(
                CHECKS[5],
                tile.as_ref().is_ok_and(|pixels| ink(pixels) > 0),
                match &tile {
                    Ok(pixels) => format!(
                        "{} of {} pixels are not paper",
                        ink(pixels),
                        pixels.len() / 4
                    ),
                    Err(e) => e.clone(),
                },
            );

            // And the pool. Every tile has to come back, because a second worker
            // that never unlocked is parked in `unlock` answering `locked`.
            let (served, failed, detail) = grow(&service, doc.id, at);
            report.check(
                CHECKS[6],
                served > 1 && failed.is_none(),
                match failed {
                    Some(e) => format!("{served} served, then: {e}"),
                    None => format!("{served} concurrent tiles served; {detail}"),
                },
            );
        }
    }

    report.finish();
}

/// Every check name, in order, so a phase that cannot run still prints them.
const CHECKS: [&str; 7] = [
    "an encrypted document is refused as locked rather than as broken",
    "a document that is not encrypted opens with no password",
    "a wrong password is refused as locked",
    "the second refusal is worded differently from the first",
    "the right password opens the document",
    "an unlocked document renders a tile with ink in it",
    "every worker of an unlocked document serves, not only the first",
];

/// Issues tiles concurrently until the pool has had to grow, and reports what
/// came back.
///
/// **Forced rather than hoped for.** A pool grows in `checkout` only when a
/// request arrives and no idle worker is free, so a probe that fires tiles at a
/// tiny document one after another never grows it and would pass having
/// exercised exactly one worker --- a check that cannot fail. Each thread here
/// therefore issues several tiles back to back, so the requests overlap for long
/// enough that `checkout` finds nothing idle.
///
/// The returned count is printed whether or not the check passes, which is what
/// makes a run that did *not* grow readable as such instead of as a pass.
fn grow(service: &RenderService, doc: u32, at: Placement) -> (usize, Option<String>, String) {
    let threads = service.pool_size().max(2);
    let (tx, rx) = channel();
    std::thread::scope(|scope| {
        for t in 0..threads {
            let tx = tx.clone();
            scope.spawn(move || {
                for i in 0..8 {
                    let rid = (t * 8 + i + 100) as u64;
                    let _ = tx.send(tile_of(service, doc, rid, at).map(|_| ()));
                }
            });
        }
    });
    drop(tx);

    let mut served = 0;
    let mut failed = None;
    for outcome in rx {
        match outcome {
            Ok(()) => served += 1,
            Err(e) if failed.is_none() => failed = Some(e),
            Err(_) => {}
        }
    }
    (
        served,
        failed,
        format!("{threads} threads, pool capacity {}", service.pool_size()),
    )
}

/// Opens `path`, optionally with a password, through the render service.
fn open(
    service: &RenderService,
    path: &Path,
    password: Option<&str>,
) -> Result<DocumentInfo, Refusal> {
    let (tx, rx) = channel();
    service.open(
        path.to_path_buf(),
        true,
        password.map(str::to_string),
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    match rx.recv_timeout(ANSWER_BOUND) {
        Ok(result) => result,
        Err(_) => Err(Refusal {
            reason: format!("no answer within {} s", ANSWER_BOUND.as_secs()),
            locked: false,
        }),
    }
}

/// Renders one tile and returns its raw RGBA.
fn tile_of(service: &RenderService, doc: u32, rid: u64, at: Placement) -> Result<Vec<u8>, String> {
    let (tx, rx) = channel();
    service.tile(
        TileRequest {
            crop: None,
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
        },
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    match rx.recv_timeout(ANSWER_BOUND) {
        Ok(Ok(TileOutcome::Rendered(tile))) => Ok(tile.bytes),
        Ok(Ok(TileOutcome::Abandoned)) => Err("abandoned, and nothing withdrew it".into()),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(format!("no tile within {} s", ANSWER_BOUND.as_secs())),
    }
}

/// Pixels that are not paper, in an RGBA buffer.
fn ink(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|p| p[0] != 0xFF || p[1] != 0xFF || p[2] != 0xFF)
        .count()
}

/// A one-line account of an open, whichever way it went.
fn describe(result: &Result<DocumentInfo, Refusal>) -> String {
    match result {
        Ok(doc) => format!("opened, {} pages, id {}", doc.page_count, doc.id),
        Err(r) => format!("locked={} {:?}", r.locked, r.reason),
    }
}

fn library_dir() -> PathBuf {
    PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR)
}

/// Where in the page a tile is taken from.
#[derive(Clone, Copy)]
struct Placement {
    scale: f32,
    x: i32,
    y: i32,
    width: u16,
    height: u16,
}

impl Placement {
    fn inside(page: &PageSize) -> Self {
        let scale = 1.25_f32;
        let scaled_width = page.width_pt * scale;
        let scaled_height = page.height_pt * scale;
        Self {
            scale,
            x: 0,
            y: 0,
            width: (scaled_width as u32).clamp(16, 1024) as u16,
            height: (scaled_height as u32).clamp(16, 1024) as u16,
        }
    }
}

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
        let label = if ok { "[OK]" } else { "[FAIL]" };
        println!("{label:7}{name:64} {}", detail.as_ref());
    }

    fn skip(&mut self, name: &str, why: impl AsRef<str>) {
        self.checks += 1;
        self.skipped += 1;
        println!("{:7}{name:64} {}", "[SKIP]", why.as_ref());
    }

    fn finish(&self) {
        println!(
            "\n{} checks, {} failed, {} skipped",
            self.checks, self.failures, self.skipped
        );
        if self.failures > 0 {
            std::process::exit(1);
        }
    }
}
