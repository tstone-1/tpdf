//! Body of `examples/ocr-sandbox-probe`. See that file for what this is and why.
//!
//! It lives here rather than in a directory beside the target for the reason
//! `src/probes/backend_probe.rs` does: a directory next to a target source has no
//! manifest entry claiming it, and a bundler that enumerates such a directory has
//! already cost this repository a failed Windows installer. See `docs/TRAPS.md`.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tpdf_lib::document::OpenDocument;
use tpdf_lib::ocr::{Options, Pixels, Recogniser};
use tpdf_lib::ocr_vision::Vision;
use tpdf_lib::ocr_worker::enter_boundary;
use tpdf_lib::progressive::{self, CancelToken, RawPage, TileSpec};
use tpdf_lib::worker::SANDBOX_PROFILE;
use tpdf_lib::worker_child::apply_sandbox;

/// What a rung reports back, one line of it.
///
/// Deliberately separate answers rather than a verdict: a rung that cannot
/// run Vision and a rung that cannot write a file are different findings, and a
/// single boolean would make the ladder unable to say which rung failed at what.
///
/// **Two readings, because the worker is asked two ways.** The text layer asks
/// plainly; the redaction gate asks for any script, which turns Vision's language
/// detection on --- a different request, whose models are a separate question
/// from the plain one's. On macOS 27 a model the process meets for the first time
/// inside the profile is a refused cache write and a failed recognition, so each
/// way of asking has to be shown to work after the boundary on its own.
struct Answers {
    wrote: String,
    connected: String,
    read: String,
    any_script: String,
}

impl Answers {
    fn line(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.wrote, self.connected, self.read, self.any_script
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let mut parts = line.trim().split('|');
        Some(Self {
            wrote: parts.next()?.to_string(),
            connected: parts.next()?.to_string(),
            read: parts.next()?.to_string(),
            any_script: parts.next()?.to_string(),
        })
    }
}

/// Verdicts, padded the way every other probe here pads them.
#[derive(Default)]
struct Report {
    passed: usize,
    failed: usize,
}

impl Report {
    fn check(&mut self, ok: bool, name: &str, detail: impl AsRef<str>) {
        if ok {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
        let label = if ok { "[OK]" } else { "[FAIL]" };
        println!("{label:7}{name:46} {}", detail.as_ref());
    }
    fn finish(&self) -> ! {
        println!();
        println!(
            "{}/{} checks passed",
            self.passed,
            self.passed + self.failed
        );
        std::process::exit(i32::from(self.failed != 0));
    }
}

pub fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(at) = args.iter().position(|a| a == "--rung") {
        let rung = args.get(at + 1).cloned().unwrap_or_default();
        let file = args.get(at + 2).cloned().unwrap_or_default();
        let library = args.get(at + 3).cloned().unwrap_or_default();
        let port: u16 = args.get(at + 4).and_then(|s| s.parse().ok()).unwrap_or(0);
        let page: u32 = args.get(at + 5).and_then(|s| s.parse().ok()).unwrap_or(0);
        child(&rung, Path::new(&file), Path::new(&library), port, page);
    }
    parent(&args);
}

// --------------------------------------------------------------------- child

/// One rung: render, apply the profile, then find out what is left.
///
/// **The render happens before the profile**, which is not a convenience: it is
/// what the parser worker does, and the whole question is what an engine can do
/// *after* a boundary comes down over a process that has already mapped what it
/// needs. Sandboxing first would measure a different program.
fn child(rung: &str, file: &Path, library: &Path, port: u16, page: u32) -> ! {
    let pixels = match render_page(file, library, page) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[rung {rung}] render failed: {e}");
            std::process::exit(3);
        }
    };

    // The `ocr` rung crosses the boundary through the worker's own function, warm-up and
    // all, rather than applying the profile here: a probe that carried its own copy of the
    // order stayed green on macOS 27 while the worker it vouched for failed every image.
    let entered = match rung {
        "bare" => Ok(()),
        "ocr" => enter_boundary().map(|_| ()),
        "parser" => apply_sandbox(SANDBOX_PROFILE),
        other => {
            eprintln!("unknown rung {other:?}");
            std::process::exit(3);
        }
    };
    if let Err(e) = entered {
        eprintln!("[rung {rung}] the kernel refused the profile: {e}");
        std::process::exit(3);
    }

    // The gate's way of asking first: it is the one a redaction verdict rests
    // on, and read second it would run in a process the plain reading had
    // already been through.
    let any_script = try_read(&pixels, true);
    let answers = Answers {
        wrote: try_write(rung),
        connected: try_connect(port),
        read: try_read(&pixels, false),
        any_script,
    };
    // One write, for the reason `worker_child::main` gives: several processes
    // share this stream and a formatted print is several writes.
    let line = format!("{}\n", answers.line());
    let _ = std::io::stdout().write_all(line.as_bytes());
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}

/// Creating a file where the profile is supposed to forbid it.
fn try_write(rung: &str) -> String {
    let path = std::env::temp_dir().join(format!("tpdf-ocr-sandbox-{rung}.probe"));
    match std::fs::write(&path, b"x") {
        Ok(()) => {
            let _ = std::fs::remove_file(&path);
            "ok".into()
        }
        Err(e) => format!("{:?}", e.kind()),
    }
}

/// Reaching a listener the parent is holding open.
///
/// The parent binds it and passes the port, so an unsandboxed rung **connects**
/// rather than being refused --- otherwise `ConnectionRefused` and a sandbox
/// denial would be indistinguishable, and the check would pass on every rung.
fn try_connect(port: u16) -> String {
    match TcpStream::connect(("127.0.0.1", port)) {
        Ok(_) => "ok".into(),
        Err(e) => format!("{:?}", e.kind()),
    }
}

/// Running the engine on the page this process rendered before the profile.
fn try_read(pixels: &Sheet, any_script: bool) -> String {
    let px = Pixels {
        rgba: &pixels.rgba,
        width: pixels.width,
        height: pixels.height,
        scale: pixels.scale,
    };
    let read = if any_script {
        Vision.recognise_any_script(px, &Options::default())
    } else {
        Vision.recognise(px, &Options::default())
    };
    match read {
        Ok(items) => format!("{}", items.len()),
        Err(e) => format!("err {e}"),
    }
}

// -------------------------------------------------------------------- parent

fn parent(args: &[String]) -> ! {
    let mut file = PathBuf::new();
    let mut library = PathBuf::from("vendor/pdfium/lib");
    // Zero-based. Which page matters since the worker is asked for any script:
    // what Vision loads then depends on what the page is written in, so a Latin
    // page passing says nothing about an Arabic one.
    let mut page = 0_u32;
    let mut rest = args.iter().skip(1);
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--lib" => library = PathBuf::from(rest.next().cloned().unwrap_or_default()),
            "--page" => page = rest.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            other => file = PathBuf::from(other),
        }
    }
    if file.as_os_str().is_empty() {
        eprintln!("[ERROR] usage: ocr-sandbox-probe <file.pdf> [--lib DIR] [--page N]");
        std::process::exit(2);
    }

    // Held for the whole run: a rung's connect has to reach something, or every
    // rung reports the same refusal and the ladder measures nothing.
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[ERROR] could not bind a control listener: {e}");
            std::process::exit(2);
        }
    };
    let port = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(e) => {
            eprintln!("[ERROR] the control listener has no port: {e}");
            std::process::exit(2);
        }
    };

    println!("document   {}, page {page}", file.display());
    println!("engine     {}", Vision.id());
    println!("listener   127.0.0.1:{port}");
    println!();

    let mut r = Report::default();

    let bare = run_rung("bare", &file, &library, port, page);
    let ocr = run_rung("ocr", &file, &library, port, page);
    let parser = run_rung("parser", &file, &library, port, page);

    // ------------------------------------------------------------ the control
    // Every assertion below is about a difference from this rung. Without it, a
    // machine where nothing works at all reports a perfectly contained ladder.
    match &bare {
        Rung::Answered(a) => {
            r.check(a.wrote == "ok", "bare: a file can be written", &a.wrote);
            r.check(
                a.connected == "ok",
                "bare: the listener can be reached",
                &a.connected,
            );
            r.check(
                a.read.parse::<usize>().is_ok_and(|n| n > 0),
                "bare: vision reads the page",
                format!("{} span(s)", a.read),
            );
            r.check(
                a.any_script.parse::<usize>().is_ok_and(|n| n > 0),
                "bare: and reads it asked for any script",
                format!("{} span(s)", a.any_script),
            );
        }
        other => {
            r.check(false, "bare: the control rung answered", other.describe());
            r.finish();
        }
    }

    // ------------------------------------------------------- the OCR boundary
    match &ocr {
        Rung::Answered(a) => {
            r.check(
                a.wrote == "PermissionDenied",
                "ocr: writing a file is denied",
                &a.wrote,
            );
            r.check(
                a.connected == "PermissionDenied",
                "ocr: the network is denied",
                &a.connected,
            );
            r.check(
                a.read.parse::<usize>().is_ok_and(|n| n > 0),
                "ocr: vision still reads the page",
                format!("{} span(s)", a.read),
            );
            r.check(
                a.any_script.parse::<usize>().is_ok_and(|n| n > 0),
                "ocr: and still reads it asked for any script",
                format!("{} span(s)", a.any_script),
            );
        }
        other => r.check(false, "ocr: the rung answered at all", other.describe()),
    }

    // -------------------------------------------- and why it needs its own one
    // `ocr.rs` records this as a measurement from 2026-07-31 and nothing has
    // re-run it since. It is the whole reason `OCR_SANDBOX_PROFILE` exists as a
    // separate constant rather than a flag on the parser's.
    r.check(
        !matches!(&parser, Rung::Answered(a) if a.read.parse::<usize>().is_ok_and(|n| n > 0)),
        "parser: vision does not read under it",
        parser.describe(),
    );

    r.finish();
}

/// What came back from one rung.
enum Rung {
    /// It answered, whatever the answers say.
    Answered(Answers),
    /// It died. The string is what killed it.
    Died(String),
    /// It could not be started, which is a fact about this harness.
    Unstarted(String),
}

impl Rung {
    fn describe(&self) -> String {
        match self {
            Self::Answered(a) => a.line(),
            Self::Died(why) => why.clone(),
            Self::Unstarted(why) => format!("not started: {why}"),
        }
    }
}

/// The variable Core Foundation takes the home directory from, which is where Vision's
/// compiled-model cache (`Library/Caches/<executable>/com.apple.e5rt.e5bundlecache`) lands.
///
/// `HOME` and `TMPDIR` do not move it; this does (measured on 26A428).
const FIXED_HOME: &str = "CFFIXED_USER_HOME";

/// Runs one rung in a fresh process **with a home of its own that nothing has used**.
///
/// That is the state a reader's first redaction is in, and it is the state this probe could
/// not see until 2026-09-27: every rung shared `~/Library/Caches/ocr-sandbox-probe`, the
/// unsandboxed `bare` rung ran first and filled it, and the `ocr` rung then read a page under
/// a profile that would have refused the very write it no longer needed --- 7/7 while every
/// worker the redaction gate spawned failed. A fresh home per rung makes each one pay the
/// cold compile (about 23 s on 26A428), which is the price of measuring the cold path.
fn run_rung(rung: &str, file: &Path, library: &Path, port: u16, page: u32) -> Rung {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return Rung::Unstarted(format!("current_exe: {e}")),
    };
    let home = std::env::temp_dir().join(format!(
        "tpdf-ocr-sandbox-home-{}-{rung}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&home);
    if let Err(e) = std::fs::create_dir_all(&home) {
        return Rung::Unstarted(format!("a fresh home at {}: {e}", home.display()));
    }
    let rung_result = run_rung_in(&exe, &home, rung, file, library, port, page);
    let _ = std::fs::remove_dir_all(&home);
    rung_result
}

fn run_rung_in(
    exe: &Path,
    home: &Path,
    rung: &str,
    file: &Path,
    library: &Path,
    port: u16,
    page: u32,
) -> Rung {
    let out = std::process::Command::new(exe)
        .env(FIXED_HOME, home)
        .arg("--rung")
        .arg(rung)
        .arg(file)
        .arg(library)
        .arg(port.to_string())
        .arg(page.to_string())
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => return Rung::Unstarted(format!("spawn: {e}")),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    if let Some(answers) = Answers::parse(&text) {
        return Rung::Answered(answers);
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let how = match out.status.signal() {
        Some(sig) => format!("killed by signal {sig}"),
        None => format!("exit {:?}", out.status.code()),
    };
    Rung::Died(if stderr.is_empty() {
        how
    } else {
        format!("{how}: {stderr}")
    })
}

// -------------------------------------------------------------------- render

struct Sheet {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    scale: f32,
}

fn render_page(file: &Path, library: &Path, index: u32) -> Result<Sheet, String> {
    use pdfium_render::prelude::Pdfium;
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = progressive::bind_library(&path)
        .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;
    let bindings = progressive::bindings_of(bound);
    let document = OpenDocument::open(bindings, file, None)?;
    let page = document.page(index)?;
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
