//! What the shell asks about itself: the launch queue and the version.
//!
//! The smallest of the groups and the only one that touches no document. It is
//! separate rather than folded into a neighbour because the state it reads is
//! its own --- `launch::Launch`, managed on the builder --- and grouping by the
//! state a command touches is what makes each of these files readable on its
//! own.

use crate::{launch, synctex};

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
pub fn launch_open_event() -> &'static str {
    launch::OPEN_EVENT
}

/// LaTeX forward search: where `line` of `source` went in `pdf`, by the
/// SyncTeX file beside it (`synctex.rs`).
///
/// On the blocking pool: the file is read and, usually, decompressed, which on
/// a long book is tens of megabytes. The window gets the page and the boxes,
/// or a sentence to say.
#[tauri::command]
pub async fn synctex_forward(
    pdf: String,
    source: String,
    line: u32,
) -> Result<synctex::Forward, String> {
    tauri::async_runtime::spawn_blocking(move || {
        synctex::forward(
            std::path::Path::new(&pdf),
            std::path::Path::new(&source),
            line,
        )
    })
    .await
    .map_err(|e| said!("the forward search did not finish: {e}"))?
}

/// The running version, so that a reader can find out which one they have.
///
/// **Nothing in the application said this until 2026-08-19, and the cost was a
/// bug report rather than a missing nicety.** A Windows reader on `26.8.4` hit
/// the defect where an app started with no console could open no document, and
/// could not tell whether the release that fixes it was the one they were
/// running --- so a two-second question became a report, a reproduction and a
/// bisect. `BUILD.md`'s release checklist has told anyone applying an update to
/// "confirm the new version in-app" since the updater landed, which means that
/// step was never performable and nothing said so.
///
/// It comes from `CARGO_PKG_VERSION` rather than from a constant of our own, so
/// it is `src-tauri/Cargo.toml` --- one of the four files a version bump has to
/// move together --- and cannot drift from what was built. Baked in at compile
/// time, so this reads nothing and can fail in no way.
#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Hands over documents that arrived from outside, and starts listening.
///
/// Called once by the frontend during boot. Everything queued before that ---
/// a double-click that launched the app, a path on the command line --- comes
/// back here; anything arriving afterwards is emitted on `launch::OPEN_EVENT`.
#[tauri::command]
pub fn take_launch_paths(launch: tauri::State<'_, launch::Launch>) -> Vec<launch::Opening> {
    launch.take()
}

/// *Install command-line tool…* (`install`) and *Uninstall command-line tool…*.
///
/// On macOS, links `/usr/local/bin/tpdf` to the tool inside this bundle, or
/// removes that link, asking the system for administrator rights only when the
/// directory needs them --- `clitool.rs` has the rules, among them that a file
/// tpdf did not put there is never replaced or removed. On Windows, says where
/// the installer put `tpdf-cli.exe`. The answer is a sentence for the reader,
/// read back from the filesystem after the change.
///
/// Takes no path: the link and the tool are both fixed, so the webview can ask
/// for this one change and nothing else.
#[tauri::command]
pub async fn command_line_tool(install: bool) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || crate::clitool::apply(install))
        .await
        .map_err(|e| said!("the command-line tool change did not run: {e}"))?
}

/// What the two commands above would find, so the window can grey the one
/// that has nothing to do.
///
/// Reads and changes nothing: on macOS what the two fixed paths in
/// `/usr/local/bin` hold, on Windows whether the tool's folder is on the
/// user's own `PATH`; `None` anywhere else, where there is nothing to read.
/// Takes no argument, so the webview cannot ask about another path. An error
/// is "not known", and the window then greys neither command --- the answer
/// that counts is still the one `command_line_tool` reads when it runs.
#[tauri::command]
pub async fn command_line_tool_state() -> Result<Option<crate::clitool::ToolState>, String> {
    tauri::async_runtime::spawn_blocking(crate::clitool::state)
        .await
        .map_err(|e| said!("the command-line tool could not be looked for: {e}"))?
}

/// Who answers Explorer for PDF thumbnails and the preview pane: tpdf,
/// nobody, or another program by name. `None` off Windows, where the two
/// commands that ask it are greyed. Reads only (`explorer.rs`).
#[tauri::command]
pub async fn explorer_state() -> Result<Option<crate::explorer::Handlers>, String> {
    if !cfg!(windows) {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(|| {
        crate::explorer::run(None).map(|(_, state)| Some(state))
    })
    .await
    .map_err(|e| said!("Explorer's handlers could not be read: {e}"))?
}

/// *Show PDFs in Explorer with tpdf* (`on`) and *Stop showing PDFs in Explorer
/// with tpdf*. `replace` is the reader's answer to replacing another
/// program's handler; the window asks it from [`explorer_state`]. The DLL
/// registered is the one beside this executable, never a path the webview
/// names. The answer is a sentence for the reader.
#[tauri::command]
pub async fn explorer_set(on: bool, replace: bool) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::explorer::run(Some((on, replace))).map(|(said, _)| said)
    })
    .await
    .map_err(|e| said!("Explorer's handlers could not be changed: {e}"))?
}

/// *Make tpdf the default PDF app*.
///
/// Run only when the reader chooses it; nothing asks at start (`defaultapp.rs`).
/// The application is named by this bundle's own identifier, so the webview
/// cannot name another one. The answer is a sentence for the reader.
#[tauri::command]
pub async fn default_pdf_app(app: tauri::AppHandle) -> Result<String, String> {
    let identifier = app.config().identifier.clone();
    tauri::async_runtime::spawn_blocking(move || crate::defaultapp::apply(&identifier))
        .await
        .map_err(|e| said!("the default application change did not run: {e}"))?
}
