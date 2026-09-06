//! What the shell asks about itself: the launch queue and the version.
//!
//! The smallest of the groups and the only one that touches no document. It is
//! separate rather than folded into a neighbour because the state it reads is
//! its own --- `launch::Launch`, managed on the builder --- and grouping by the
//! state a command touches is what makes each of these files readable on its
//! own.

use crate::launch;

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
pub fn take_launch_paths(launch: tauri::State<'_, launch::Launch>) -> Vec<String> {
    launch
        .take()
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}
