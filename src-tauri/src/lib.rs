//! tpdf --- Phase 0 spike harness.
//!
//! This is not the application. It exists to answer the feasibility questions in
//! docs/PLAN.md section 9 with numbers, and is expected to be thrown away.

mod protocol;
mod render;
mod startup;

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

/// Whether page geometry should be collected for the whole document up front.
fn lazy_geometry() -> bool {
    std::env::var_os("TPDF_LAZY_GEOMETRY").is_some()
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

/// Milliseconds since process exec, so the frontend can place its own marks on
/// the same timeline as the Rust side (spike 0.2).
#[tauri::command]
fn process_elapsed_ms() -> f64 {
    startup::since_process_start_ms()
}

/// Path to auto-benchmark on startup, from `TPDF_AUTOBENCH`.
///
/// The webview half of spike 0.1 has to run inside a real webview, but a
/// measurement that needs someone to click a button is a measurement that does
/// not get repeated. With this set, the app opens the document, runs the
/// transfer benchmark and exits, so the whole thing is one shell command.
#[tauri::command]
fn autobench_path() -> Option<String> {
    std::env::var("TPDF_AUTOBENCH").ok()
}

/// Path to time a cold open of on startup, from `TPDF_STARTUP` (spike 0.2).
#[tauri::command]
fn startup_path() -> Option<String> {
    std::env::var("TPDF_STARTUP").ok()
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

/// Ends an automated spike run.
#[tauri::command]
fn spike_exit(app: tauri::AppHandle, code: i32) {
    app.exit(code);
}

/// Kills the process if an automated spike run has not finished in time.
///
/// Every automated run ends by calling `spike_exit` from the webview, so a
/// webview that never gets there leaves the app sitting in its event loop with
/// no output at all --- indistinguishable from a slow run, and the harness's own
/// timeout reports only that something took too long. Printing the marks that
/// *were* reached says where it stopped.
fn start_watchdog(seconds: u64) {
    if std::env::var_os("TPDF_STARTUP").is_none() && std::env::var_os("TPDF_AUTOBENCH").is_none() {
        return;
    }

    std::thread::Builder::new()
        .name("tpdf-watchdog".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            eprintln!("[FAIL] spike run did not finish within {seconds} s. Reached:");
            for (name, at) in startup::timeline() {
                eprintln!("  {name:<30} {at:>9.1}");
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
    start_watchdog(30);
    let mode = ShellMode::from_env();

    let mut context = tauri::generate_context!();
    // Everything before this is ours: reading the embedded config and building
    // the asset table. Everything after it, up to the setup hook, is Tauri's.
    startup::mark("context built");
    if mode != ShellMode::Config {
        context.config_mut().app.windows.clear();
    }

    let mut builder = tauri::Builder::default();
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
            process_elapsed_ms,
            autobench_path,
            startup_path,
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
    });
}
