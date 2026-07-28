//! tpdf --- the application shell, and the harness that proved it could exist.
//!
//! It began as the second of those: everything here was written to answer the
//! feasibility questions in docs/PLAN.md section 9 with numbers. Phase 0 closed
//! and the viewer now runs on the same pieces, so the file is no longer
//! throwaway --- but the spike entry points are still here, still reachable by
//! their `TPDF_*` environment variables, and are still how every number in
//! `AGENTS.md` is reproduced. Do not delete one because nothing calls it: the
//! caller is a shell command in `BUILD.md`.

pub mod invert;
pub mod launch;
pub mod outline;
pub mod print;
#[cfg(target_os = "macos")]
pub mod print_macos;
pub mod progressive;
mod protocol;
mod queue;
pub mod render;
pub mod search;
pub mod session;
pub mod startup;
pub mod sweep;
pub mod text;
pub mod worker;
pub mod worker_child;

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use parking_lot::Mutex;
use render::{DocumentInfo, RenderService};
use tauri::Manager;

/// Who creates the window, and what it points at (spike 0.7).
///
/// Spike 0.2 left 142 ms warm between `main` and the setup hook unattributed.
/// Tauri creates the windows listed in `tauri.conf.json` *before* calling that
/// hook, so webview creation is inside the interval rather than after it, and no
/// mark can be placed between the two. Moving creation into the hook splits it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellMode {
    /// Tauri creates the window from the config, before the setup hook.
    Config,
    /// The setup hook creates it, so its cost gets its own line.
    Manual,
    /// As `Manual`, but pointing at a page that does the same work with no
    /// framework: no module graph, no Svelte, no `@tauri-apps/api`.
    Blank,
}

impl ShellMode {
    /// Reads `TPDF_SHELL_MODE`, defaulting to the shape the app ships with.
    fn from_env() -> Self {
        match std::env::var("TPDF_SHELL_MODE")
            .unwrap_or_default()
            .as_str()
        {
            "manual" => Self::Manual,
            "blank" => Self::Blank,
            _ => Self::Config,
        }
    }

    /// The page this variant loads.
    fn page(self) -> &'static str {
        match self {
            Self::Blank => "shell.html",
            _ => "index.html",
        }
    }
}

/// A document open that was started before the webview asked for it.
///
/// The path is known at launch --- from a file association, an argument, or
/// `TPDF_STARTUP` here --- and the shell then spends ~95 ms booting a webview
/// that cannot ask for anything. The open can run inside that interval instead
/// of after it. Holding the receiver rather than the result means the frontend
/// blocks only if it beat the render thread to the finish.
struct EagerOpen(Mutex<Option<Receiver<Result<DocumentInfo, String>>>>);

/// Whether page geometry should be collected lazily rather than up front.
///
/// Lazy is the default, and it is the reason the Phase 0 startup criterion is
/// met: enumerating every page of the 775-page corpus costs 86 ms on the
/// critical path to buy a scrollbar exactness the scroller estimates anyway
/// (docs/PLAN.md §4). `TPDF_EAGER_GEOMETRY` restores the walk, so the variant
/// that measurement compared against is still reachable.
fn lazy_geometry() -> bool {
    std::env::var_os("TPDF_EAGER_GEOMETRY").is_none()
}

/// The event name a document handed over later will arrive on.
///
/// Asked for rather than agreed in two places. A constant duplicated on both
/// sides fails by *silence* when the two drift --- the app keeps working, and
/// simply stops noticing documents opened while it is already running, which is
/// the half of file associations nobody tests by hand.
///
/// It has to be a separate call from `take_launch_paths`, and in that order: the
/// listener must be registered before the queue is drained, because a path
/// delivered between the drain and the listen is emitted to nobody.
#[tauri::command]
fn launch_open_event() -> &'static str {
    launch::OPEN_EVENT
}

/// Hands over documents that arrived from outside, and starts listening.
///
/// Called once by the frontend during boot. Everything queued before that ---
/// a double-click that launched the app, a path on the command line --- comes
/// back here; anything arriving afterwards is emitted on `launch::OPEN_EVENT`.
#[tauri::command]
fn take_launch_paths(launch: tauri::State<'_, launch::Launch>) -> Vec<String> {
    launch
        .take()
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// Where the remembered places are kept.
///
/// `TPDF_SESSION_FILE` overrides it, and every automated run sets it. Without
/// that the session check would read and overwrite whatever the person using
/// this machine was last reading --- and a check that can destroy the state it
/// is checking is not one that can be run twice.
fn session_file(app: &tauri::AppHandle) -> PathBuf {
    if let Some(override_path) = std::env::var_os("TPDF_SESSION_FILE") {
        return PathBuf::from(override_path);
    }
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("session.json")
}

/// Locates the Pdfium dynamic library.
///
/// In development it sits in `vendor/pdfium/lib` at the repo root. In a bundled
/// app it will sit alongside the executable. Both are tried, dev first, because
/// `cargo tauri dev` runs from `src-tauri`.
fn pdfium_library_dir(app: &tauri::AppHandle) -> PathBuf {
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("vendor/pdfium/lib"));

    if let Some(dev) = dev {
        if dev.exists() {
            return dev;
        }
    }

    app.path()
        .resource_dir()
        .map(|d| d.join("pdfium"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Opens a document and returns its page geometry.
///
/// Collects an eager open if one is outstanding, which is why this takes the
/// app handle: the pending receiver is managed state that only exists in that
/// variant.
#[tauri::command]
async fn open_document(
    app: tauri::AppHandle,
    service: tauri::State<'_, RenderService>,
    path: String,
) -> Result<DocumentInfo, String> {
    if let Some(pending) = app.try_state::<EagerOpen>() {
        if let Some(rx) = pending.0.lock().take() {
            startup::mark("eager open collected");
            return rx.recv().map_err(|_| "render thread stopped".to_string())?;
        }
    }

    let (tx, rx) = std::sync::mpsc::channel();
    service.open(
        PathBuf::from(path),
        lazy_geometry(),
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    rx.recv().map_err(|_| "render thread stopped".to_string())?
}

/// Releases a document the reader has finished with.
///
/// Called when the window moves to another file, and it matters more than it
/// looks: under the worker backend an unreleased document is a sandboxed
/// process, not a heap allocation, so a session that opens a dozen files would
/// otherwise be holding a dozen of them.
///
/// It waits for the render thread's reply rather than returning as soon as the
/// job is posted, so the promise resolving means the process is really gone and
/// a refusal has somewhere to be reported. Whether the *caller* waits on that
/// promise is its own decision, and `App.svelte` does not --- the render thread
/// is FIFO, so this is already queued behind everything the outgoing document
/// had outstanding, and holding the reader there would put a process teardown on
/// the path to the first page of the file they asked for.
#[tauri::command]
async fn close_document(service: tauri::State<'_, RenderService>, doc: u32) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    service.close(
        doc,
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    rx.recv().map_err(|_| "render thread stopped".to_string())?
}

/// Extracts one page's characters and their positions.
///
/// Selection, search and the accessibility tree all read this, and they read
/// the same one deliberately --- three extractions would disagree in ways no
/// test catches, each being self-consistent. Cached on the frontend rather than
/// here: what a page's text costs to *re-request* is an IPC round trip, and what
/// it costs to re-extract is measured in `bin/text_probe.rs`.
#[tauri::command]
async fn page_text(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
) -> Result<text::PageText, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    service.text(
        doc,
        page,
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    rx.recv().map_err(|_| "render thread stopped".to_string())?
}

/// Finds a query in one page, returning character ranges.
///
/// One page per call, because the render thread is FIFO and a whole-document
/// scan would sit in front of every tile --- see `RenderService::search`. The
/// caller walks the document and stops when it wants to cancel.
#[tauri::command]
async fn search_page(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
    query: String,
) -> Result<search::PageMatches, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    service.search(
        doc,
        page,
        query,
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    rx.recv().map_err(|_| "render thread stopped".to_string())?
}

/// Reads a document's outline --- its bookmarks --- as a bounded tree.
///
/// Bounded is the operative word: the outline of a malformed document can be
/// infinite, and PDFium documents that it is our job to notice. See
/// `outline.rs`.
#[tauri::command]
async fn document_outline(
    service: tauri::State<'_, RenderService>,
    doc: u32,
) -> Result<outline::Outline, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    service.outline(
        doc,
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    rx.recv().map_err(|_| "render thread stopped".to_string())?
}

/// Reads the remembered places, most recently read first.
///
/// Synchronous on purpose: it is asked for during startup, where the whole
/// application budget is ~50 ms, and reading a few kilobytes costs microseconds
/// against the round trip that would be needed to hand it back later.
#[tauri::command]
fn session_load(app: tauri::AppHandle) -> session::Session {
    session::Session::load(&session_file(&app))
}

/// Records where a document was left.
///
/// Read-modify-write on every call rather than holding the session in managed
/// state: the file is the record, and a second window --- or a crash that skips
/// whatever teardown would have flushed it --- must not be able to roll back a
/// place already written.
///
/// Returns `Result` so a failure to write is *visible* to the caller. Nothing
/// currently acts on it, and the frontend deliberately does not surface it: a
/// dialog because the position could not be saved would be worse than the lost
/// position.
#[tauri::command]
fn session_remember(app: tauri::AppHandle, place: session::Place) -> Result<(), String> {
    let path = session_file(&app);
    let mut session = session::Session::load(&path);
    session.remember(place);
    session.save(&path).map_err(|e| e.to_string())
}

/// Records whether pages are shown inverted.
///
/// Its own command rather than a field on `session_remember`, because it is a
/// preference and not a place. Folding it into the place payload would also make
/// it invisible to the writer's own de-duplication: that compares consecutive
/// places, so toggling the mode without moving would compare equal and never be
/// written at all.
///
/// Called directly instead of through the throttle, since a reader inverts the
/// page deliberately and rarely, where a place changes on every frame.
#[tauri::command]
fn session_set_invert_pages(app: tauri::AppHandle, invert: bool) -> Result<(), String> {
    let path = session_file(&app);
    let mut session = session::Session::load(&path);
    session.invert_pages = invert;
    session.save(&path).map_err(|e| e.to_string())
}

/// Builds a print job and opens the platform print dialog for it.
///
/// `async` so the build happens off the main thread: `print::build` parses the
/// whole document, and on a 337 MB scan that is not something to do on the
/// thread the webview draws on. Only the panel is dispatched back.
///
/// Returns as soon as the panel has been *asked for*, not when it closes. The
/// outcome is deliberately not reported: `runOperation` answers one boolean for
/// both "printed" and "cancelled" (see `print_macos::present`), so a caller
/// waiting for it could only turn a Cancel into an error message.
#[tauri::command]
async fn print_document(
    app: tauri::AppHandle,
    path: String,
    pages: Option<Vec<u32>>,
    turns: u8,
) -> Result<(), String> {
    let source = PathBuf::from(&path);
    let job = print::Job {
        pages: pages.map_or(print::Pages::All, print::Pages::Only),
        turns,
    };
    let expected = match &job.pages {
        print::Pages::Only(wanted) => Some(wanted.len()),
        print::Pages::All => None,
    };
    let bytes = print::build(&source, &job)?;
    let title = source.file_name().map_or_else(
        || "Document".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    present_job(&app, bytes, title, expected)
}

/// Hands built bytes to the platform, having first read them back.
#[cfg(target_os = "macos")]
fn present_job(
    app: &tauri::AppHandle,
    bytes: Vec<u8>,
    title: String,
    expected: Option<usize>,
) -> Result<(), String> {
    // Re-parsed by PDFKit before anything is offered to a printer --- a third
    // parser, and the one the print system will use itself. Refusing here costs
    // a dialog; not refusing costs paper.
    let reading = print_macos::read(&bytes)
        .ok_or("the print job could not be read back, so it will not be printed")?;
    print::expect_pages(reading.pages.len(), expected)?;

    app.run_on_main_thread(move || {
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            // Unreachable by construction, and silence here would be a print
            // command that does nothing and says nothing.
            eprintln!("[print] dispatched off the main thread; no panel shown");
            return;
        };
        if let Err(e) = print_macos::present(&bytes, &title, mtm) {
            eprintln!("[print] {e}");
        }
    })
    .map_err(|e| e.to_string())
}

/// The Windows side, which is not written.
///
/// An error rather than a no-op: everything in this repository is macOS-only
/// until a Windows build has actually run, and a print command that quietly
/// does nothing is the worse of the two failures.
#[cfg(not(target_os = "macos"))]
fn present_job(
    _app: &tauri::AppHandle,
    _bytes: Vec<u8>,
    _title: String,
    _expected: Option<usize>,
) -> Result<(), String> {
    Err("printing is implemented on macOS only".into())
}

/// Milliseconds since process exec, so the frontend can place its own marks on
/// the same timeline as the Rust side (spike 0.2).
#[tauri::command]
fn process_elapsed_ms() -> f64 {
    startup::since_process_start_ms()
}

/// The mark that says the webview executed a line of JavaScript.
const WEBVIEW_ALIVE: &str = "webview alive";

/// Reads a spike's environment variable, recording that the webview asked.
///
/// Every spike entry point begins by asking Rust for its path or config, so the
/// *first* of these calls is proof that the page loaded and ran. That matters
/// because the alternative failure --- WebKit suspending a page whose window is
/// occluded --- produces no output at all, and is otherwise indistinguishable
/// from a run that is merely slow. The watchdog keys its diagnosis on this mark;
/// `mark` is first-wins, so the four callers leave one entry between them.
fn spike_env(key: &str) -> Option<String> {
    startup::mark(WEBVIEW_ALIVE);
    std::env::var(key).ok()
}

/// Path to auto-benchmark on startup, from `TPDF_AUTOBENCH`.
///
/// The webview half of spike 0.1 has to run inside a real webview, but a
/// measurement that needs someone to click a button is a measurement that does
/// not get repeated. With this set, the app opens the document, runs the
/// transfer benchmark and exits, so the whole thing is one shell command.
#[tauri::command]
fn autobench_path() -> Option<String> {
    spike_env("TPDF_AUTOBENCH")
}

/// What the file-association check should assert, from `TPDF_OPENCHECK`.
///
/// Like the session check, this observes the real boot rather than replacing
/// it. Note the environment reaches the app even when Launch Services starts it:
/// `TPDF_OPENCHECK=... open -a tpdf.app file.pdf` does propagate, which is what
/// makes the actual double-click path testable rather than merely argued.
#[tauri::command]
fn opencheck_mode() -> Option<String> {
    spike_env("TPDF_OPENCHECK")
}

/// What the session check should do this launch, from `TPDF_SESSIONCHECK`.
///
/// Unlike the other spike entry points this one does *not* replace the
/// application: session restore happens during the real boot, so a check that
/// bypassed it would be checking a second implementation. The mode says which
/// half of a two-launch run this is; the app boots normally either way and the
/// check observes it. See `src/lib/sessioncheck.ts`.
#[tauri::command]
fn sessioncheck_mode() -> Option<String> {
    spike_env("TPDF_SESSIONCHECK")
}

/// Everything the scroll benchmark needs to run without a human (spike 0.8).
///
/// Read from the environment rather than compiled in, so a variant sweep --- a
/// different scroll speed, a different tile size --- is a shell line rather than
/// a rebuild. Defaults are the shape docs/PLAN.md section 4 arrived at: the
/// fewest, largest tiles, and one screen of prefetch either way.
#[derive(serde::Serialize)]
struct ScrollBenchConfig {
    path: String,
    rounds: usize,
    frames: usize,
    warmup_frames: usize,
    px_per_frame: f64,
    tile_px: u32,
    zooms: Vec<f64>,
    layouts: Vec<String>,
    cache_tiles: usize,
    max_in_flight: usize,
    prefetch_screens: f64,
    /// Whether stale requests are withdrawn, as a variant dimension so the two
    /// behaviours can be interleaved rather than compared across runs.
    cancels: Vec<u8>,
}

/// Reads a `TPDF_`-prefixed environment variable, falling back to `default`.
fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(default)
}

/// Reads a comma-separated list, falling back to `default`.
fn env_list<T: std::str::FromStr>(name: &str, default: Vec<T>) -> Vec<T> {
    let Ok(raw) = std::env::var(name) else {
        return default;
    };
    let parsed: Vec<T> = raw
        .split(',')
        .filter_map(|item| item.trim().parse().ok())
        .collect();
    if parsed.is_empty() {
        default
    } else {
        parsed
    }
}

/// The scroll benchmark's configuration, or `None` if none was requested.
#[tauri::command]
fn scrollbench_config() -> Option<ScrollBenchConfig> {
    let path = spike_env("TPDF_SCROLLBENCH")?;

    Some(ScrollBenchConfig {
        path,
        rounds: env_or("TPDF_SCROLL_ROUNDS", 5),
        frames: env_or("TPDF_SCROLL_FRAMES", 300),
        warmup_frames: env_or("TPDF_SCROLL_WARMUP", 180),
        // A brisk flick rather than a reading scroll: ~3600 css px/s at 120 Hz.
        // The demanding case is the one the criterion is about.
        px_per_frame: env_or("TPDF_SCROLL_PX", 30.0),
        tile_px: env_or("TPDF_SCROLL_TILE", 1024),
        zooms: env_list("TPDF_SCROLL_ZOOMS", vec![1.0, 4.0]),
        layouts: env_list(
            "TPDF_SCROLL_LAYOUTS",
            vec!["tiles".to_string(), "viewport".to_string()],
        ),
        cache_tiles: env_or("TPDF_SCROLL_CACHE", 32),
        max_in_flight: env_or("TPDF_SCROLL_INFLIGHT", 4),
        prefetch_screens: env_or("TPDF_SCROLL_PREFETCH", 1.0),
        // One value by default, so an ordinary run is not twice the size. Pass
        // `0,1` to measure what withdrawal is worth.
        cancels: env_list("TPDF_SCROLL_CANCEL", vec![1]),
    })
}

/// Path to run the viewer's functional check against, from `TPDF_VIEWERCHECK`.
///
/// Unlike the benchmarks either side of it this one asserts rather than
/// measures --- see `src/lib/viewercheck.ts` --- and it needs a real webview for
/// the same reason they do: the frame loop, the input handlers and the layout it
/// checks do not exist anywhere else.
#[tauri::command]
fn viewercheck_path() -> Option<String> {
    spike_env("TPDF_VIEWERCHECK")
}

/// Path to time a cold open of on startup, from `TPDF_STARTUP` (spike 0.2).
#[tauri::command]
fn startup_path() -> Option<String> {
    spike_env("TPDF_STARTUP")
}

/// Records a webview-observed milestone on the process timeline.
///
/// `at_ms` is required rather than stamped here: every mark the webview cares
/// about happened before it could tell us, so stamping on arrival would measure
/// the IPC call instead of the event.
#[tauri::command]
fn startup_mark(name: String, at_ms: f64) {
    startup::mark_at(&name, at_ms);
}

/// The full startup timeline, Rust and webview marks merged.
#[tauri::command]
fn startup_timeline() -> Vec<(String, f64)> {
    startup::timeline()
}

/// Whether the pre-`main` interval could be measured on this platform.
///
/// The frontend needs to know, because a timeline that silently starts at
/// `main` would report a startup budget that excludes dyld.
#[tauri::command]
fn startup_pre_main_ms() -> Option<f64> {
    startup::pre_main_ms()
}

/// Prints spike output on the process's stdout.
///
/// Webview `console.log` does not reliably reach the terminal across platforms,
/// and the results need to land somewhere a script can read.
#[tauri::command]
fn spike_print(text: String) {
    println!("{text}");
}

/// Ends an automated spike run, with the code the run asked for.
///
/// **`AppHandle::exit` does not set the process's exit code.** It ends the event
/// loop, `App::run` then returns normally, `run()` returns, `main` returns unit
/// --- and the process exits 0 whatever was asked for. Every automated run here
/// therefore reported success for its whole existence, including
/// `scripts/viewer_check.py`, whose `return completed.returncode` could not fail.
/// Found 2026-07-27 by a session-check phase that printed `[FAIL]` and `0/1
/// checks passed` above a harness verdict of `[OK]`.
///
/// `process::exit` skips destructors, which is right here rather than merely
/// acceptable: the render thread owns PDFium handles and a spike that has
/// printed its results has nothing left to tear down. Stdout is flushed first
/// because that is the entire product of the run.
#[tauri::command]
fn spike_exit(code: i32) {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code);
}

/// Kills the process if an automated spike run has not finished in time.
///
/// Every automated run ends by calling `spike_exit` from the webview, so a
/// webview that never gets there leaves the app sitting in its event loop with
/// no output at all --- indistinguishable from a slow run, and the harness's own
/// timeout reports only that something took too long. Printing the marks that
/// *were* reached says where it stopped.
fn start_watchdog() {
    // The scroll benchmark is frame-driven, which is exactly the thing WebKit
    // stops doing when the window stops being visible, so it needs the watchdog
    // more than the others do --- and it needs far longer, since it runs every
    // variant in one launch rather than one launch per sample.
    let seconds: u64 = if std::env::var_os("TPDF_SCROLLBENCH").is_some() {
        env_or("TPDF_SCROLL_TIMEOUT", 900)
    } else if std::env::var_os("TPDF_VIEWERCHECK").is_some() {
        // Frame-driven like the scroll benchmark, and so exposed to the same
        // suspension, but it waits on renders rather than counting frames.
        env_or("TPDF_VIEWERCHECK_TIMEOUT", 300)
    } else if std::env::var_os("TPDF_OPENCHECK").is_some() {
        // One of its phases deliberately waits for a document that another
        // process sends it, so it outlives a plain boot by design.
        env_or("TPDF_OPENCHECK_TIMEOUT", 120)
    } else if std::env::var_os("TPDF_SESSIONCHECK").is_some() {
        // Opens a document and waits for one screen, twice per two-launch run.
        env_or("TPDF_SESSIONCHECK_TIMEOUT", 120)
    } else if std::env::var_os("TPDF_STARTUP").is_some()
        || std::env::var_os("TPDF_AUTOBENCH").is_some()
    {
        30
    } else {
        return;
    };

    std::thread::Builder::new()
        .name("tpdf-watchdog".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            eprintln!("[FAIL] spike run did not finish within {seconds} s. Reached:");
            let marks = startup::timeline();
            for (name, at) in &marks {
                eprintln!("  {name:<30} {at:>9.1}");
            }

            // The difference between "slow" and "never started" is one mark, and
            // without saying so out loud this reads as a hang in whatever was
            // most recently changed. It is usually not: WebKit suspends a page
            // whose window is fully covered, and an occluded window is not a
            // locked screen, so `webview_guard.py` passes and nothing runs.
            if !marks.iter().any(|(name, _)| name == WEBVIEW_ALIVE) {
                for line in [
                    format!("No `{WEBVIEW_ALIVE}` mark: the page never ran a line of JavaScript,"),
                    "so this is not a slow run. WebKit suspends a page whose window is".into(),
                    "occluded --- covered by another window, or on another Space --- and".into(),
                    "an unlocked screen is not a visible one.".into(),
                    String::new(),
                    "Re-run with TPDF_RAISE=1, or with nothing covering the window.".into(),
                    "See BUILD.md.".into(),
                ] {
                    eprintln!("       {line}");
                }
            }
            // Straight out, not through the app handle: the point of this path
            // is that the event loop may be the thing that is stuck.
            std::process::exit(2);
        })
        .expect("failed to spawn watchdog thread");
}

/// Starts the document open now, before anything can ask for it.
///
/// Returns `None` unless both a path and the opt-in are set, so the variant is
/// off by default and the baseline stays the baseline.
fn start_eager_open(service: &RenderService) -> Option<EagerOpen> {
    std::env::var_os("TPDF_EAGER_OPEN")?;
    let path = std::env::var("TPDF_STARTUP").ok()?;

    let (tx, rx) = std::sync::mpsc::channel();
    service.open(
        PathBuf::from(path),
        lazy_geometry(),
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    startup::mark("eager open requested");
    Some(EagerOpen(Mutex::new(Some(rx))))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    startup::mark_process_start();

    // Before anything else, and before the watchdog: this process may not be the
    // app at all. A worker is this executable re-exec'd with a marker argument,
    // and everything below --- the watchdog, the Tauri context, a window ---
    // would be wrong for it. It never returns.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == worker::WORKER_ARGV) {
        worker_child::main(&args);
    }

    // Also before the watchdog, and for a reason the panic in `RenderService::start`
    // cannot serve. That call happens in the setup hook, which `App::run` invokes
    // from inside AppKit's frames --- so a panic there is *non-unwinding*, aborts
    // through a backtrace with no symbols, and races the watchdog's own 30-second
    // report about an occluded webview. A misspelt environment variable would then
    // be diagnosed as a suspended page. Read it here, where there is no event loop
    // to lose the message in and no window to be occluded.
    if let Err(e) = render::Backend::from_env() {
        eprintln!("[FAIL] {e}");
        std::process::exit(2);
    }

    start_watchdog();
    let mode = ShellMode::from_env();

    let mut context = tauri::generate_context!();
    // Everything before this is ours: reading the embedded config and building
    // the asset table. Everything after it, up to the setup hook, is Tauri's.
    startup::mark("context built");
    if mode != ShellMode::Config {
        context.config_mut().app.windows.clear();
    }

    // Managed on the builder rather than in the setup hook, and the difference
    // is not stylistic. **`RunEvent::Opened` fires before setup runs**, so with
    // this registered there `state::<Launch>()` panics inside the run callback
    // on exactly the path it exists to serve: a cold double-click. The window
    // appears, nothing else happens, and the last startup mark is `app built`.
    //
    // Queued here for the same reason: on Windows a double-click arrives in
    // `argv`, long before there is a webview to tell about it.
    let launch = launch::Launch::default();
    for path in launch::paths_from_args(std::env::args()) {
        launch.deliver(path);
    }

    let mut builder = tauri::Builder::default()
        .manage(launch)
        .plugin(tauri_plugin_dialog::init());
    if std::env::var_os("TPDF_EMPTY_MENU").is_some() {
        // Tauri installs a full default application menu on macOS. Building it
        // means constructing every item and submenu through AppKit, which is
        // not obviously free at 37 ms of builder time --- so it gets measured
        // rather than assumed.
        builder = builder.menu(tauri::menu::Menu::new);
    }

    let app = builder
        .setup(move |app| {
            startup::mark("tauri setup");
            let dir = pdfium_library_dir(app.handle());
            let service = RenderService::start(dir);
            if let Some(pending) = start_eager_open(&service) {
                app.manage(pending);
            }
            app.manage(service);

            // A frame-rate measurement in an unfocused window measures the
            // throttle, not the platform. The app is launched from a script, so
            // nothing else would raise it, and the resulting cadence would look
            // exactly like a ceiling WebKit had imposed on us.
            //
            // The viewer *check* does not do this by default: it asserts
            // behaviour rather than timing it, so an unfocused window costs it
            // nothing --- and raising a window over whatever someone is doing,
            // every time a check runs, is its own bug.
            //
            // But unfocused and *occluded* are different things, and the
            // difference is not cosmetic: WebKit suspends a page whose window
            // is fully covered, so a check launched from a shell behind a
            // full-screen terminal never runs a single line of frontend code.
            // It does not fail --- it produces nothing, which is why
            // `TPDF_RAISE` exists. Opt-in, so the default stays polite and a
            // run that has nowhere visible to put a window can still say what
            // it needs.
            if std::env::var_os("TPDF_SCROLLBENCH").is_some()
                || std::env::var_os("TPDF_RAISE").is_some()
            {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_focus();
                }
            }

            if mode != ShellMode::Config {
                startup::mark("window build start");
                tauri::WebviewWindowBuilder::new(
                    app,
                    "main",
                    tauri::WebviewUrl::App(mode.page().into()),
                )
                .title("tpdf")
                .inner_size(1200.0, 900.0)
                .build()?;
                // `build()` returns once the webview exists and has been told
                // what to load, not once it has loaded it.
                startup::mark("window built");
            }
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("tile", |ctx, request, responder| {
            let service = ctx.app_handle().state::<RenderService>();
            protocol::handle(&service, request, responder);
        })
        .invoke_handler(tauri::generate_handler![
            open_document,
            close_document,
            page_text,
            search_page,
            document_outline,
            launch_open_event,
            take_launch_paths,
            session_load,
            session_remember,
            session_set_invert_pages,
            print_document,
            process_elapsed_ms,
            autobench_path,
            viewercheck_path,
            sessioncheck_mode,
            opencheck_mode,
            startup_path,
            scrollbench_config,
            startup_mark,
            startup_timeline,
            startup_pre_main_ms,
            spike_print,
            spike_exit
        ])
        .build(context)
        .expect("error while building tpdf");

    // Distinct from the setup hook: everything the builder does after it ---
    // menus, tray, remaining runtime wiring --- lands here.
    startup::mark("app built");

    app.run(|_handle, event| {
        if matches!(event, tauri::RunEvent::Ready) {
            startup::mark("event loop ready");
        }

        // How a double-click reaches tpdf on macOS. Launch Services sends an
        // Apple Event and nothing appears in `argv` at all, so this arm is the
        // *only* route for the way most people will open a document --- and it
        // can fire before the webview exists, which is why it queues rather
        // than emitting unconditionally.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Opened { urls } = &event {
            use tauri::{Emitter, Manager};
            // `try_state`, not `state`: the latter panics on unmanaged state,
            // and this arm runs before the setup hook. It is managed on the
            // builder now so this cannot be `None`, but a panic here is
            // invisible --- a window with nothing in it --- and the degradation
            // is one document not opening.
            let Some(launch) = _handle.try_state::<launch::Launch>() else {
                return;
            };
            for url in urls {
                let Some(path) = launch::path_from_url(url) else {
                    continue;
                };
                if let launch::Delivery::Emit(path) = launch.deliver(path) {
                    let _ = _handle.emit(launch::OPEN_EVENT, path.to_string_lossy().into_owned());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{spike_env, WEBVIEW_ALIVE};
    use crate::startup;

    fn alive() -> bool {
        startup::timeline()
            .iter()
            .any(|(name, _)| name == WEBVIEW_ALIVE)
    }

    /// The watchdog's diagnosis is gated on this mark's *absence*, so the mark
    /// has to be produced by something the page cannot reach without running.
    ///
    /// The first assertion is the control and is the point of the test: without
    /// it, a mark that was somehow always present would pass the second one, and
    /// the diagnosis would then never fire --- which is indistinguishable from a
    /// harness that simply never hits the failure.
    ///
    /// Note this is the only test in the crate that touches the global mark
    /// table, which is what makes asserting its emptiness first safe under
    /// `cargo test`'s parallelism.
    #[test]
    fn asking_for_a_spike_path_marks_the_webview_alive() {
        assert!(!alive(), "the mark exists before anything asked for it");
        // Unset on purpose: the mark records that the *page asked*, which it
        // does on every launch, not that the spike was requested.
        assert_eq!(spike_env("TPDF_NO_SUCH_VARIABLE_4711"), None);
        assert!(alive());
    }
}
