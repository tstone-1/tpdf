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
#[cfg(target_os = "macos")]
use tpdf_lib::ocr_vision::Vision;
#[cfg(windows)]
use tpdf_lib::ocr_windows::WindowsOcr;
use tpdf_lib::ocr_worker::{OcrWorker, PIXELS_CAPACITY, REPLY_DEADLINE};
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
    tpdf_lib::ocr_worker::child_main_if_asked(&std::env::args().collect::<Vec<_>>());

    let mut file = PathBuf::new();
    // `PDFIUM_SUBDIR`, not `lib`. On Windows `lib/` exists and holds the *import*
    // library, so hardcoding it gives a directory that is there and a load that
    // fails much later pointing at a path that looks right ---
    // `only_the_macos_spikes_hardcode_the_library_directory` is the rule.
    let mut library = PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR);
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
    //
    // Through `ocr::Recogniser` since 2026-08-29, which is what makes this probe
    // portable: only the *construction* of the engine is per-platform, because
    // `Vision` is a unit struct and `WindowsOcr::new` asks the OS for a
    // recogniser and can fail. Everything after this line is the trait, so what
    // is measured is the worker rather than the engine behind it.
    let engine_here = match engine_here() {
        Ok(engine) => engine,
        Err(e) => {
            r.check(false, "in-process: the engine is available", e);
            r.finish();
        }
    };
    let here = match engine_here.recognise(sheet.pixels(), &opts) {
        Ok(items) => items,
        Err(e) => {
            r.check(
                false,
                "in-process: the engine runs on this page",
                format!("{e}"),
            );
            r.finish();
        }
    };

    // ---------------------------------------------------------------- the worker
    // **This comment used to say the spawn happens before the baseline,
    // deliberately, so that the scan below could ask whether running the
    // baseline is what mapped the engine.** It was false about the code sitting
    // thirteen lines beneath it --- the baseline is above --- and the ordering it
    // argued for would buy nothing anyway, because the rows further down measured
    // that `objc2-vision` links Vision and the framework is therefore mapped at
    // launch, called or not. The argument was live until that measurement landed
    // and outlived it. `docs/TRAPS.md` has the entry; the order here is not a
    // check of anything, and nothing should be built on it being one.
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
    // Against the in-process engine's own `id()` rather than a literal, which is
    // one fewer copy of the platform distinction and a stronger claim: the worker
    // has to report *this platform's* engine, not merely a non-empty one.
    //
    // The `build` is checked non-empty and deliberately **not** compared. On
    // macOS it comes from `sw_vers`, a subprocess spawned inside
    // `OCR_SANDBOX_PROFILE` --- so a difference there would be a finding about
    // what the sandbox still permits, which is `ocr-sandbox-probe`'s question and
    // not this one's.
    let expected = engine_here.id();
    r.check(
        engine.name == expected.name && !engine.build.is_empty(),
        "the engine identity comes back and is this platform's",
        format!(
            "{} ({}) against {} here",
            engine.name, engine.build, expected.name
        ),
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
    // **The control, and it asserts something known to be there rather than a
    // count.** This read `images.len() > 100` until 2026-08-29, which is a macOS
    // number wearing a portable check's clothes: a macOS process maps 600-odd
    // dylibs and a Windows debug binary maps 37, so widening this probe to
    // Windows turned a working enumeration into a red row and blamed the loader.
    // `backend-probe` had the right shape the whole time --- its control is that
    // the scan *finds* the library it knows is mapped, and the count is printed
    // as evidence and asserted about by nothing.
    //
    // A process maps its own executable on both platforms: it is dyld's image 0
    // and Toolhelp's first module. So this proves the table was read and that
    // what came back is *this* process's, which no threshold can say --- and it
    // is what makes the tesseract row below mean an absence rather than a scan
    // that matched nothing.
    let own = std::env::current_exe().ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    r.check(
        own.as_deref().is_some_and(|name| {
            images
                .iter()
                .any(|image| image.to_lowercase().ends_with(name))
        }),
        "the loader's table names this process's own executable",
        format!(
            "{} image(s), looking for {}",
            images.len(),
            own.as_deref().unwrap_or("(no path for this process)")
        ),
    );
    // macOS only, and not an omission on Windows. This row is a statement about
    // **static linkage**: `objc2-vision` links the framework, so it is in the
    // table before a call. `Windows.Media.Ocr` is WinRT, activated through
    // `combase` at the first call rather than linked, so what its images are and
    // when they arrive is a different question with a different answer --- and
    // asserting a name here without measuring one would be a guess wearing a
    // check's clothes. It is unmeasured, and saying so beats inventing a row.
    #[cfg(target_os = "macos")]
    {
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
    }
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
#[cfg(target_os = "macos")]
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
///
/// `workers::kill_pid` rather than a `libc::kill` of its own, which is what stood
/// here: that function already has both arms --- `SIGKILL` on unix, and
/// `TerminateProcess` on Windows, which has no signals --- and its Windows arm
/// carries a comment about a copy of it that degraded to a no-op off its platform
/// and left the deadline silently unenforced. A second copy of an `unsafe` FFI is
/// the shape this repository keeps finding drifted, and the sleep below is the
/// only thing this caller actually wants that the shared one does not do.
fn kill(pid: u32) {
    tpdf_lib::workers::kill_pid(pid);
    // The parent has not reaped it yet, so on unix it becomes a zombie rather
    // than vanishing; the pipe closing is what the next call sees either way.
    std::thread::sleep(Duration::from_millis(100));
}

/// The same engine the worker will run, constructed in this process.
///
/// Boxed rather than `impl Recogniser`, so that the three arms have one signature
/// and a platform with neither engine can return an error instead of needing a
/// type it does not have. The trait is object-safe, and a probe pays nothing for
/// the indirection.
#[cfg(target_os = "macos")]
fn engine_here() -> Result<Box<dyn Recogniser>, String> {
    Ok(Box::new(Vision))
}

#[cfg(windows)]
fn engine_here() -> Result<Box<dyn Recogniser>, String> {
    WindowsOcr::new()
        .map(|e| Box::new(e) as Box<dyn Recogniser>)
        .map_err(|e| e.to_string())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn engine_here() -> Result<Box<dyn Recogniser>, String> {
    Err(tpdf_lib::ocr_worker::NO_ENGINE.to_string())
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
