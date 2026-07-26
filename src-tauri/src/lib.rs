//! tpdf --- Phase 0 spike harness.
//!
//! This is not the application. It exists to answer the feasibility questions in
//! docs/PLAN.md section 9 with numbers, and is expected to be thrown away.

mod protocol;
mod render;

use std::path::PathBuf;

use render::{DocumentInfo, RenderService};
use tauri::Manager;

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
#[tauri::command]
async fn open_document(
    service: tauri::State<'_, RenderService>,
    path: String,
) -> Result<DocumentInfo, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    service.open(
        PathBuf::from(path),
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    rx.recv().map_err(|_| "render thread stopped".to_string())?
}

/// Milliseconds since process start, so the frontend can place its own marks on
/// the same timeline as the Rust side (spike 0.2).
#[tauri::command]
fn process_elapsed_ms() -> f64 {
    render::since_process_start_ms()
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

/// Prints frontend benchmark output on the process's stdout.
///
/// Webview `console.log` does not reliably reach the terminal across platforms,
/// and the results need to land somewhere a script can read.
#[tauri::command]
fn autobench_report(text: String) {
    println!("{text}");
}

/// Ends an auto-benchmark run.
#[tauri::command]
fn autobench_done(app: tauri::AppHandle, code: i32) {
    app.exit(code);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    render::mark_process_start();

    tauri::Builder::default()
        .setup(|app| {
            let dir = pdfium_library_dir(app.handle());
            app.manage(RenderService::start(dir));
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
            autobench_report,
            autobench_done
        ])
        .run(tauri::generate_context!())
        .expect("error while running tpdf");
}
