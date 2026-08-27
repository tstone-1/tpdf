//! Body of `examples/ocr-worker-probe`. See that file for what this is and why.
//!
//! It lives here rather than in a directory beside the target for the reason
//! `src/probes/backend_probe.rs` does: a directory next to a target source has no
//! manifest entry claiming it, and a bundler that enumerates such a directory has
//! already cost this repository a failed Windows installer. See `docs/TRAPS.md`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tpdf_lib::document::OpenDocument;
use tpdf_lib::ocr::{Options, Pixels, RecogniseError, RecognisedItem, Recogniser};
use tpdf_lib::ocr_vision::Vision;
use tpdf_lib::ocr_worker::{OcrWorker, OCR_WORKER_ARGV, PIXELS_CAPACITY, REPLY_DEADLINE};
use tpdf_lib::progressive::{self, CancelToken, RawPage, TileSpec};

/// Verdicts, padded the way every other probe here pads them.
#[derive(Default)]
struct Report {
    passed: usize,
    failed: usize,
    skipped: usize,
}

impl Report {
    fn check(&mut self, ok: bool, name: &str, detail: impl AsRef<str>) {
        if ok {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
        let label = if ok { "[OK]" } else { "[FAIL]" };
        println!("{label:7}{name:48} {}", detail.as_ref());
    }
    fn skip(&mut self, name: &str, why: impl AsRef<str>) {
        self.skipped += 1;
        println!("{:7}{name:48} {}", "[SKIP]", why.as_ref());
    }
    fn finish(&self) -> ! {
        println!();
        println!(
            "{}/{} checks passed, {} skipped",
            self.passed,
            self.passed + self.failed,
            self.skipped
        );
        std::process::exit(i32::from(self.failed != 0));
    }
}

pub fn main() {
    // This binary is its own worker. `OcrWorker::spawn` re-execs `current_exe`,
    // which for a probe is the probe --- the same arrangement `pool-bench` uses,
    // and the reason it is here rather than the probe carrying a private copy of
    // the child: what is under test has to be the shipped one.
    if std::env::args().any(|a| a == OCR_WORKER_ARGV) {
        tpdf_lib::ocr_worker::child_main();
    }

    let mut file = PathBuf::new();
    let mut library = PathBuf::from("vendor/pdfium/lib");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--lib" => library = PathBuf::from(args.next().unwrap_or_default()),
            other => file = PathBuf::from(other),
        }
    }
    if file.as_os_str().is_empty() {
        eprintln!("[ERROR] usage: ocr-worker-probe <file.pdf> [--lib DIR]");
        std::process::exit(2);
    }
    match run(&file, &library) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("[ERROR] {e}");
            std::process::exit(2);
        }
    }
}

fn run(file: &Path, library: &Path) -> Result<(), String> {
    let sheet = render_page(file, library)?;
    let opts = Options::default();

    println!("document   {}", file.display());
    println!(
        "page 0     {}x{} px at scale {}",
        sheet.width, sheet.height, sheet.scale
    );

    let mut r = Report::default();

    // A whole page is not what the gate hands over --- a redaction region plus a
    // control strip is a fraction of one --- and `vector-heavy.pdf` is A0, which
    // at scale 2 is 128 MB against a 16 MB buffer. Skipped with the size in it
    // rather than rendered smaller: shrinking the subject to fit the harness is
    // how a probe comes to measure something other than what it names.
    if sheet.rgba.len() > PIXELS_CAPACITY {
        r.skip(
            "this page can be handed over whole",
            format!(
                "{} bytes against a {PIXELS_CAPACITY} byte buffer",
                sheet.rgba.len()
            ),
        );
        r.finish();
    }

    // ------------------------------------------------------------- the baseline
    // In-process, in this same program, on the same bytes. A worker that reads
    // nothing and an engine that reads nothing produce identical output, and the
    // differential below is the only row that tells them apart.
    let here = match Vision.recognise(sheet.pixels(), &opts) {
        Ok(items) => items,
        Err(e) => {
            r.check(
                false,
                "in-process: vision runs on this page",
                format!("{e}"),
            );
            r.finish();
        }
    };

    // ---------------------------------------------------------------- the worker
    // **Before the in-process baseline, deliberately.** The scan below asks
    // whether this process ever mapped the engine, and running it here for a
    // baseline would map it --- so the order is the check.
    let mut worker = OcrWorker::spawn()?;
    let pid = worker.pid();
    let (engine, items) = match worker.recognise(sheet.pixels(), &opts) {
        Ok(v) => v,
        Err(e) => {
            r.check(false, "the worker answers at all", format!("{e}"));
            r.finish();
        }
    };
    // Two checks wearing one name, because what a correct answer looks like
    // depends on the page. `vector-heavy.pdf` is a pure vector drawing with no
    // text on it, and reading none there is the right answer --- a probe that
    // only knew the first would call that fixture a failure. `ocr-probe` has the
    // same shape for the same reason.
    if here.is_empty() {
        r.check(
            items.is_empty(),
            "nothing is read off a page that has none",
            format!(
                "this process read nothing and the worker read {} span(s)",
                items.len()
            ),
        );
    } else {
        r.check(
            !items.is_empty(),
            "the worker reads the page",
            format!("{} span(s) from pid {pid}", items.len()),
        );
    }
    r.check(
        engine.name == "vision" && !engine.build.is_empty(),
        "the engine identity comes back and resolves",
        format!("{} ({})", engine.name, engine.build),
    );
    r.check(
        pid != std::process::id(),
        "the engine ran in another process",
        format!("worker {pid}, us {}", std::process::id()),
    );

    // ------------------------------------------- what the loader can and cannot say
    // **`backend-probe`'s kind of evidence is not available here, and finding
    // that out is the point of these three rows.** That probe proves the app
    // process never maps `libpdfium`, and it can, because `pdfium-render`
    // `dlopen`s the library at runtime --- a process that never binds it never
    // maps it. `objc2-vision` links Vision the ordinary way, so **every binary
    // that links `ocr_vision` maps the framework at launch, called or not.**
    // Linking is not calling: what the worker buys is that Vision *code* runs
    // somewhere else, which is what the SIGTRAP measurement is about. An absent
    // image was never going to be the proof.
    let images = tpdf_lib::images::mapped();
    r.check(
        images.len() > 100,
        "the loader's table can be read at all",
        format!("{} image(s)", images.len()),
    );
    let engine_images = mapped_vision();
    r.check(
        !engine_images.is_empty(),
        "the engine is mapped from launch, because it is linked",
        format!(
            "{} image(s): {}",
            engine_images.len(),
            engine_images.first().map(String::as_str).unwrap_or("")
        ),
    );
    // The emptiness control. Without it, a filter that matches nothing reports a
    // clean process exactly as a clean process does --- and the row above would
    // be the only thing standing between this probe and that mistake.
    r.check(
        !images
            .iter()
            .any(|image| image.to_lowercase().contains("tesseract")),
        "the scan can report an absence",
        "no image here is named tesseract",
    );

    // The differential. Same engine, same pixels, one process apart -- so the
    // text has to be identical, not merely similar, and a mismatch is the
    // handover rather than the engine.
    r.check(
        joined(&items) == joined(&here),
        "what it read is what this process reads",
        format!(
            "{} span(s) across the boundary against {} here",
            items.len(),
            here.len()
        ),
    );

    // -------------------------------------------------------- the same process
    // A worker that respawned per call would pass every check above and cost a
    // process per region.
    let (_, again) = worker
        .recognise(sheet.pixels(), &opts)
        .map_err(|e| format!("the second call failed: {e}"))?;
    r.check(
        worker.pid() == pid && !again.is_empty(),
        "a second call reuses the same process",
        format!("pid {} still, {} span(s)", worker.pid(), again.len()),
    );

    // ----------------------------------------------------------- the refusals
    // Two of them, and they are checked apart on purpose: an image whose buffer
    // does not match its own dimensions and an image the mapping cannot hold are
    // different mistakes, and one outcome that two mechanisms produce cannot test
    // either one --- `docs/TRAPS.md` has that entry.
    let inconsistent = Pixels {
        rgba: &sheet.rgba,
        width: sheet.width + 1,
        height: sheet.height,
        scale: sheet.scale,
    };
    let refused = worker.recognise(inconsistent, &opts);
    r.check(
        matches!(refused, Err(RecogniseError::MalformedInput(_))),
        "a buffer that is not its own dimensions is refused",
        outcome(&refused),
    );

    // Consistent, and larger than the mapping: 2048 x 2560 x 4 is 20 MB against a
    // 16 MB buffer, so this reaches the size rule and nothing else.
    let big = vec![0u8; 2048 * 2560 * 4];
    let oversized = Pixels {
        rgba: &big,
        width: 2048,
        height: 2560,
        scale: 1.0,
    };
    let refused = worker.recognise(oversized, &opts);
    r.check(
        matches!(refused, Err(RecogniseError::MalformedInput(_))),
        "an image too big for the buffer is refused",
        outcome(&refused),
    );
    // **And the worker is still usable.** A refusal that killed the process would
    // turn one oversized region into a document that cannot be verified at all.
    let survived = worker.recognise(sheet.pixels(), &opts);
    r.check(
        survived.is_ok() && worker.pid() == pid,
        "the worker still answers after a refusal",
        format!("{} at pid {}", outcome(&survived), worker.pid()),
    );

    // ------------------------------------------------- and a bounded give-up
    // `docs/TRAPS.md` records a check whose failure mode is a wait, which cannot
    // fail. Kill the child from outside and the next call has to *report*, inside
    // its own deadline, rather than block for ever on a pipe nobody will write.
    kill(pid);
    let started = Instant::now();
    let after = worker.recognise(sheet.pixels(), &opts);
    let took = started.elapsed();
    r.check(
        after.is_err() && took < REPLY_DEADLINE,
        "a killed worker reports rather than hanging",
        match &after {
            Err(e) => format!("{e} after {:.2} s", took.as_secs_f32()),
            Ok(_) => "it answered anyway".into(),
        },
    );

    r.finish();
}

/// Which Vision images this process has mapped, by path.
fn mapped_vision() -> Vec<String> {
    tpdf_lib::images::mapped()
        .into_iter()
        .filter(|image| image.to_lowercase().contains("vision.framework"))
        .collect()
}

/// One line about a call, whichever way it went.
fn outcome(
    result: &Result<(tpdf_lib::ocr::EngineId, Vec<RecognisedItem>), RecogniseError>,
) -> String {
    match result {
        Ok((_, items)) => format!("{} span(s)", items.len()),
        Err(e) => format!("{e}"),
    }
}

/// Every span's text, joined, so a comparison is about content and not order.
fn joined(items: &[RecognisedItem]) -> String {
    let mut texts: Vec<&str> = items.iter().map(|i| i.text.as_str()).collect();
    texts.sort_unstable();
    texts.join("\u{1f}")
}

/// Kills a pid outright, so the next call meets a process that is gone.
fn kill(pid: u32) {
    // SAFETY: a pid this process spawned; `SIGKILL` to a pid that has already
    // exited is an error we ignore rather than undefined behaviour.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
    // The parent has not reaped it yet, so it becomes a zombie rather than
    // vanishing; the pipe closing is what the next call sees.
    std::thread::sleep(Duration::from_millis(100));
}

// -------------------------------------------------------------------- render

struct Sheet {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    scale: f32,
}

impl Sheet {
    fn pixels(&self) -> Pixels<'_> {
        Pixels {
            rgba: &self.rgba,
            width: self.width,
            height: self.height,
            scale: self.scale,
        }
    }
}

fn render_page(file: &Path, library: &Path) -> Result<Sheet, String> {
    use pdfium_render::prelude::Pdfium;
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = Pdfium::bind_to_library(&path)
        .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;
    let bindings = progressive::bindings_of(Box::leak(Box::new(Pdfium::new(bound))));
    let document = OpenDocument::open(bindings, file, None)?;
    let page = document.page(0)?;
    tile(bindings, &page, 2.0)
}

fn tile(bindings: progressive::Bindings, page: &RawPage<'_>, scale: f32) -> Result<Sheet, String> {
    let width = (page.width_pt() * scale).ceil() as u32;
    let height = (page.height_pt() * scale).ceil() as u32;
    let spec = TileSpec {
        scale,
        turns: 0,
        x: 0,
        y: 0,
        width: u16::try_from(width).map_err(|_| "page too wide for one tile".to_string())?,
        height: u16::try_from(height).map_err(|_| "page too tall for one tile".to_string())?,
    };
    let (rgba, _) = progressive::render_tile(
        bindings,
        page,
        spec,
        Some(Duration::from_millis(50)),
        &CancelToken::default(),
    )?;
    Ok(Sheet {
        rgba,
        width,
        height,
        scale,
    })
}
