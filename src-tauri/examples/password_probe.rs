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
//! - **The questions PDFium cannot answer are answered too.** Comments, links,
//!   properties and the character mapping are each a second parse of the same
//!   bytes with `lopdf`, in the worker, and `lopdf` reads **no objects at all**
//!   without the key. So a password that reached PDFium and stopped there gives
//!   a document that opens, renders and searches while its panels come back
//!   empty --- and empty is the reassuring answer, which is why this is checked
//!   against the count PDFium reports rather than against zero.
//! - **A mark can be saved onto it, and the file is still encrypted after.** The
//!   whole of `save.rs`'s append runs inside the worker, so this is the only
//!   place the production path is exercised end to end. `lopdf` re-encrypts each
//!   appended object with the key the load recorded; the check reopens the file
//!   afterwards and asks whether it still needs the same password.
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

use tpdf_lib::docmodel::{MarkKind, Quad};
use tpdf_lib::edits::{PageView, Plan, PlannedMark};
use tpdf_lib::fingerprint::Fingerprint;
use tpdf_lib::progressive::Refusal;
use tpdf_lib::render::{
    Backend, DocumentInfo, PageSize, RenderService, TileFormat, TileOutcome, TileRequest,
};
use tpdf_lib::{save, worker, worker_child};

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
        // Named rather than omitted, and it is worth being loud: a silent pass
        // here would be a password path with no coverage anywhere, which is the
        // defect 5850cf2 was about.
        //
        // **This used to say "it needs qpdf", and that was the reason this
        // whole probe printed twelve `[SKIP]`s on every hosted runner.** The
        // fixture is written with pyhanko since 2026-08-23, which the signed
        // group already installs, so `scripts/ci_fixtures.py --signed` produces
        // it and CI runs these checks for real.
        for name in CHECKS {
            report.skip(name, format!("{LOCKED} is not generated"));
        }
        return report.finish();
    }

    let service = RenderService::start_with(library_dir(), Backend::Worker);
    // The save writes, so it works on a copy beside the fixture rather than on
    // it. Named here so the cleanup at the end can find it whichever way the
    // run goes.
    let scratch =
        std::env::temp_dir().join(format!("tpdf-password-probe-{}.pdf", std::process::id()));

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

            // ------------------------------------ what PDFium cannot answer
            //
            // Each of these is a second parse of the same bytes with `lopdf`,
            // inside the worker. Without the key that parse reads no objects at
            // all --- and every one of them then answers something a reader
            // cannot tell from the truth: no properties, no links, no mapping.
            // So each is checked against what PDFium says about the same
            // document rather than against zero.
            let properties = ask(|reply| service.properties(doc.id, reply));
            report.check(
                CHECKS[7],
                properties
                    .as_ref()
                    .is_ok_and(|p| p.pages as usize == doc.page_count && !p.limits.locked),
                match &properties {
                    Ok(p) => format!(
                        "{} pages, locked={}, encryption={:?}",
                        p.pages,
                        p.limits.locked,
                        p.encryption.as_ref().map(|e| e.method.clone())
                    ),
                    Err(e) => e.clone(),
                },
            );

            // `pages_missed` is the module's own count of pages PDFium
            // paginated and `lopdf` could not see, which is exactly what a
            // parse with no key produces --- and it is a better observable than
            // the number of links, because a document may legitimately have
            // none.
            let links = ask(|reply| service.links(doc.id, reply));
            report.check(
                CHECKS[8],
                links.as_ref().is_ok_and(|l| l.limits.pages_missed == 0),
                match &links {
                    Ok(l) => format!(
                        "{} link(s), {} page(s) unaccounted for",
                        l.items.len(),
                        l.limits.pages_missed
                    ),
                    Err(e) => e.clone(),
                },
            );

            // Same shape one module over: a page `encoding::scan` could not
            // read comes back `truncated`, which is how a reader is stopped
            // from being told "no matches" on a page nobody searched.
            let mapping = ask(|reply| service.mapping(doc.id, reply));
            report.check(
                CHECKS[9],
                mapping.as_ref().is_ok_and(|pages| {
                    pages.len() == doc.page_count && !pages.iter().any(|page| page.truncated)
                }),
                match &mapping {
                    Ok(pages) => format!(
                        "{} page(s), {} truncated",
                        pages.len(),
                        pages.iter().filter(|page| page.truncated).count()
                    ),
                    Err(e) => e.clone(),
                },
            );

            // The fourth `lopdf` reader, and the one with the weakest
            // observable: this fixture carries no comments, so a count of them
            // cannot tell "none" from "could not look". `pages_missed` is the
            // module's own answer to exactly that, and it is why this check
            // exists at all --- without it, dropping the password in
            // `annots::scan` reddened nothing here, measured 2026-08-23.
            let comments = ask(|reply| service.comments(doc.id, reply));
            report.check(
                CHECKS[10],
                comments.as_ref().is_ok_and(|c| c.limits.pages_missed == 0),
                match &comments {
                    Ok(c) => format!(
                        "{} comment(s), {} page(s) unaccounted for",
                        c.items.len(),
                        c.limits.pages_missed
                    ),
                    Err(e) => e.clone(),
                },
            );

            // ------------------------------------------------------- the save
            let saved = save_a_mark(&service, &scratch);
            report.check(
                CHECKS[11],
                saved.is_ok(),
                match &saved {
                    Ok(detail) => detail.clone(),
                    Err(e) => e.clone(),
                },
            );
        }
    }

    let _ = std::fs::remove_file(&scratch);
    report.finish();
}

/// Puts a mark on the unlocked document and checks the file afterwards.
///
/// **The only end-to-end run of the production save path there is.** The update
/// section is built by `save::append_update` inside the worker --- which is
/// where the document and its password are --- and `save::append_in_place`
/// writes it here, re-reading the result to check the cross-reference chained.
/// Both halves need the key and neither is reachable from a unit test in this
/// process, which is the same argument the module note makes about the open.
///
/// The file is a **copy**, because this writes to it. Answering from the
/// original would leave a modified fixture behind and make the next run's
/// baseline wrong.
fn save_a_mark(service: &RenderService, scratch: &Path) -> Result<String, String> {
    std::fs::copy(LOCKED, scratch).map_err(|e| format!("could not copy the fixture: {e}"))?;
    let doc = open(service, scratch, Some(PASSWORD)).map_err(|r| r.reason)?;

    let before = std::fs::metadata(scratch)
        .map_err(|e| format!("could not measure the copy: {e}"))?
        .len();
    let plan = mark_plan(scratch, doc.page_count)?;

    let ready = save::append_ready(scratch, &plan).map_err(|why| why.message)?;
    let (tx, rx) = channel();
    service.append(
        doc.id,
        plan,
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    let update = rx
        .recv_timeout(ANSWER_BOUND)
        .map_err(|_| format!("no update within {} s", ANSWER_BOUND.as_secs()))??;

    // The password the *parent* holds, which is the value `save_document` asks
    // for and hands to the read-back. Asking here is what makes this the same
    // path rather than a probe that happens to know the answer.
    let held = ask(|reply| service.password(doc.id, reply))?;
    if held.as_deref() != Some(PASSWORD) {
        return Err(format!("the service holds {held:?}, not the password"));
    }

    let appended = save::appended(ready, update).map_err(|why| why.message)?;
    // The document is closed first for the reason `save_document` closes it:
    // the file is about to change under a mapping the worker still holds.
    let _ = ask(|reply| service.close(doc.id, reply));
    save::append_in_place(&appended, scratch, held.as_deref(), &save::Here)?;

    // The claim, and it is about the file rather than about what we wrote: it
    // still needs the same key, it still has its pages, and the first one lists
    // the mark.
    let after = std::fs::read(scratch).map_err(|e| format!("could not read it back: {e}"))?;
    let reopened = lopdf::Document::load_mem_with_options(
        &after,
        lopdf::LoadOptions {
            password: Some(PASSWORD.to_string()),
            ..Default::default()
        },
    )
    .map_err(|e| format!("the saved file will not reopen with its password: {e}"))?;
    if !reopened.was_encrypted() {
        return Err("the saved file is no longer encrypted".into());
    }
    if reopened.get_pages().len() != doc.page_count {
        return Err(format!(
            "the saved file has {} page(s), not {}",
            reopened.get_pages().len(),
            doc.page_count
        ));
    }
    // Read with no password at all, which is what a reader without the key has.
    // It must not open --- an append that wrote its objects in the clear would
    // still fail this, because the previous revision is untouched and stays
    // encrypted, so what this catches is the trailer losing `/Encrypt`.
    if lopdf::Document::load_mem(&after).is_ok_and(|d| !d.get_pages().is_empty()) {
        return Err("the saved file opens with no password at all".into());
    }

    Ok(format!(
        "{} bytes appended to {before}, still AES-256, {} pages",
        after.len() as u64 - before,
        reopened.get_pages().len()
    ))
}

/// A plan that adds one highlight to page 1 and changes nothing else.
fn mark_plan(at: &Path, pages: usize) -> Result<Plan, String> {
    Ok(Plan {
        baseline: pages as u32,
        opened_as: Some(Fingerprint::of(at)?),
        pages: (0..pages as u32)
            .map(|source| PageView {
                id: u64::from(source) + 1,
                source,
                turns: 0,
                crop: None,
            })
            .collect(),
        redactions: Vec::new(),
        notes: Vec::new(),
        marks: vec![PlannedMark {
            kind: MarkKind::Highlight,
            stamp: None,
            source: 0,
            // Display space, which is y-down: `top` is the smaller number. The
            // same shape `save.rs`'s own tests use, because a quad the writer
            // maps to no area is refused with a message about the mark rather
            // than about the encryption, and that reads as a broken save path.
            quads: vec![Quad {
                left: 72.0,
                top: 100.0,
                right: 300.0,
                bottom: 118.0,
            }],
            strokes: Vec::new(),
            color: [1.0, 0.9, 0.2],
            author: "password-probe".to_string(),
            note: String::new(),
            made: "D:20260823120000Z".to_string(),
        }],
    })
}

/// Runs one service call that answers through a `Reply` and waits for it.
fn ask<T: Send + 'static>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, String>) + Send>),
) -> Result<T, String> {
    let (tx, rx) = channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    match rx.recv_timeout(ANSWER_BOUND) {
        Ok(result) => result,
        Err(_) => Err(format!("no answer within {} s", ANSWER_BOUND.as_secs())),
    }
}

/// Every check name, in order, so a phase that cannot run still prints them.
const CHECKS: [&str; 12] = [
    "an encrypted document is refused as locked rather than as broken",
    "a document that is not encrypted opens with no password",
    "a wrong password is refused as locked",
    "the second refusal is worded differently from the first",
    "the right password opens the document",
    "an unlocked document renders a tile with ink in it",
    "every worker of an unlocked document serves, not only the first",
    "the properties of an unlocked document are read, not reported empty",
    "the links of an unlocked document are accounted for on every page",
    "an unlocked document's character mapping covers every page",
    "the comments of an unlocked document are accounted for on every page",
    "a mark saves onto an unlocked document and it stays encrypted",
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
