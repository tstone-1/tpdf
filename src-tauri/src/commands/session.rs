//! What the reader was last looking at, and where it is kept.
//!
//! One file on disk and one lock over it. Every write goes through
//! [`with_session`], which is the read-modify-write that `SESSION_WRITE`
//! serialises: two windows closing at once would otherwise each write the file
//! they read, and the second would lose the first.

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use tauri::Manager;

use crate::session;

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

/// Reads the remembered places, most recently read first.
///
/// Synchronous on purpose: it is asked for during startup, where the whole
/// application budget is ~50 ms, and reading a few kilobytes costs microseconds
/// against the round trip that would be needed to hand it back later.
#[tauri::command]
pub fn session_load(app: tauri::AppHandle) -> session::Session {
    session::Session::load(&session_file(&app))
}

/// Serializes the session file's read-modify-write cycles.
///
/// Both writers below load, edit and save, which is only safe against a
/// concurrent writer if the three happen together. That used to be true by
/// accident: a synchronous `#[tauri::command]` runs on the thread the IPC
/// arrives on, so the main thread serialized them and nothing said so. Moving
/// the work to the blocking pool removes that and would leave a lost update ---
/// `session_set_invert_pages` is called directly rather than through the
/// frontend's write chain, so it really can overlap a throttled place write.
/// The lock is what the main thread used to be.
///
/// `parking_lot`'s, like every other lock here, so a panic mid-write cannot
/// poison it. That is the behaviour wanted rather than merely the one that
/// comes free: the guarded thing is a file, and `Session::save` is a
/// write-and-rename, so a write that panicked left the old file whole and the
/// next writer has nothing to recover from.
static SESSION_WRITE: Mutex<()> = Mutex::new(());

/// Loads, edits and saves the session file under [`SESSION_WRITE`].
pub(crate) fn with_session<F: FnOnce(&mut session::Session)>(
    path: &Path,
    edit: F,
) -> Result<(), String> {
    let _guard = SESSION_WRITE.lock();
    let mut session = session::Session::load(path);
    edit(&mut session);
    // Named, because this string crosses the IPC boundary and is the only thing
    // the reader is shown. `io::Error`'s own text is "permission denied" with no
    // subject, which is true of every path in the process.
    session
        .save(path)
        .map_err(|e| format!("could not write the session file {}: {e}", path.display()))
}

/// Records where a document was left.
///
/// Read-modify-write on every call rather than holding the session in managed
/// state: the file is the record, and a second window --- or a crash that skips
/// whatever teardown would have flushed it --- must not be able to roll back a
/// place already written.
///
/// **On the blocking pool, because this is on the scroll path.** The frontend
/// throttles to one write per second, but a write is a file read, a parse, a
/// serialize and a write-and-rename, and as a synchronous command all of that
/// ran on the thread the webview draws on. Measured release-profile on a full
/// 32-place session, 2,000 cycles: mean **0.911 ms**, p99 **1.381 ms**, max
/// **13.870 ms**. The mean is comfortably inside a frame and the maximum is not
/// --- 13.9 ms is past a 120 Hz frame at 8.3 ms --- so this was an occasional
/// visible hitch while scrolling rather than a steady cost. `async` alone would
/// only move the stall onto a runtime worker, which is the mistake
/// `print_document` records; the work is synchronous file I/O, so it belongs on
/// the pool built for it.
///
/// Returns `Result` so a failure to write is *visible* to the caller. Nothing
/// currently acts on it, and the frontend deliberately does not surface it: a
/// dialog because the position could not be saved would be worse than the lost
/// position.
#[tauri::command]
pub async fn session_remember(app: tauri::AppHandle, place: session::Place) -> Result<(), String> {
    let path = session_file(&app);
    tauri::async_runtime::spawn_blocking(move || {
        with_session(&path, |session| session.remember(place))
    })
    .await
    .map_err(|e| format!("the session write did not run: {e}"))?
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
///
/// On the pool for the same reason as [`session_remember`], though the case for
/// it is weaker --- a rare deliberate keypress can afford a stall a scroll
/// cannot. It goes there anyway because it is the *other* half of the pair
/// [`SESSION_WRITE`] exists for: bypassing the frontend's write chain is
/// exactly what makes it able to overlap a place write, and a writer that took
/// the lock on one thread while the other took it on another would be two
/// copies of one rule.
#[tauri::command]
pub async fn session_set_invert_pages(app: tauri::AppHandle, invert: bool) -> Result<(), String> {
    let path = session_file(&app);
    tauri::async_runtime::spawn_blocking(move || {
        with_session(&path, |session| session.invert_pages = invert)
    })
    .await
    .map_err(|e| format!("the session write did not run: {e}"))?
}
