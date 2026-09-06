//! The spike and check surface: what an automated run asks the app to tell it.
//!
//! None of it is reachable from the shipped UI, and all of it is gated on an
//! environment variable that only a harness sets --- see [`crate::spike_env`],
//! which is where that gate and the "webview alive" mark are written. The
//! commands are here rather than in `lib.rs` for the same reason the others
//! are: they are a group with one state, and that state is the environment.

use crate::{env_list, env_or, spike_env, startup};

/// Milliseconds since process exec, so the frontend can place its own marks on
/// the same timeline as the Rust side (spike 0.2).
#[tauri::command]
pub fn process_elapsed_ms() -> f64 {
    startup::since_process_start_ms()
}

/// Path to auto-benchmark on startup, from `TPDF_AUTOBENCH`.
///
/// The webview half of spike 0.1 has to run inside a real webview, but a
/// measurement that needs someone to click a button is a measurement that does
/// not get repeated. With this set, the app opens the document, runs the
/// transfer benchmark and exits, so the whole thing is one shell command.
#[tauri::command]
pub fn autobench_path() -> Option<String> {
    spike_env("TPDF_AUTOBENCH")
}

/// What the file-association check should assert, from `TPDF_OPENCHECK`.
///
/// Like the session check, this observes the real boot rather than replacing
/// it. Note the environment reaches the app even when Launch Services starts it:
/// `TPDF_OPENCHECK=... open -a tpdf.app file.pdf` does propagate, which is what
/// makes the actual double-click path testable rather than merely argued.
#[tauri::command]
pub fn opencheck_mode() -> Option<String> {
    spike_env("TPDF_OPENCHECK")
}

/// What the mark check should do this launch, from `TPDF_MARKCHECK`.
///
/// [`sessioncheck_mode`]'s posture rather than [`opencheck_mode`]'s, and for a
/// sharper version of the same reason: what it checks is the *wiring* between a
/// command, a gesture on the viewer, the edit model and the overlay --- all of
/// which lives in `App.svelte` and none of which exists in a harness that builds
/// its own `Viewer`. So the app boots normally and the check drives it through
/// the same handles a reader's pointer reaches.
///
/// It exists because a shape drawn on the last page of a document was silently
/// dropped for a fortnight while every gate stayed green: each side of that join
/// asserted its own half and was right about it. See `src/lib/markcheck.ts`.
#[tauri::command]
pub fn markcheck_mode() -> Option<String> {
    spike_env("TPDF_MARKCHECK")
}

/// What the session check should do this launch, from `TPDF_SESSIONCHECK`.
///
/// Unlike the other spike entry points this one does *not* replace the
/// application: session restore happens during the real boot, so a check that
/// bypassed it would be checking a second implementation. The mode says which
/// half of a two-launch run this is; the app boots normally either way and the
/// check observes it. See `src/lib/sessioncheck.ts`.
#[tauri::command]
pub fn sessioncheck_mode() -> Option<String> {
    spike_env("TPDF_SESSIONCHECK")
}

/// Everything the scroll benchmark needs to run without a human (spike 0.8).
///
/// Read from the environment rather than compiled in, so a variant sweep --- a
/// different scroll speed, a different tile size --- is a shell line rather than
/// a rebuild. Defaults are the shape docs/PLAN.md section 4 arrived at: the
/// fewest, largest tiles, and one screen of prefetch either way.
#[derive(serde::Serialize)]
pub struct ScrollBenchConfig {
    pub(crate) path: String,
    pub(crate) rounds: usize,
    pub(crate) frames: usize,
    pub(crate) warmup_frames: usize,
    pub(crate) px_per_frame: f64,
    pub(crate) tile_px: u32,
    pub(crate) zooms: Vec<f64>,
    pub(crate) layouts: Vec<String>,
    pub(crate) cache_tiles: usize,
    pub(crate) max_in_flight: usize,
    pub(crate) prefetch_screens: f64,
    /// Whether stale requests are withdrawn, as a variant dimension so the two
    /// behaviours can be interleaved rather than compared across runs.
    pub(crate) cancels: Vec<u8>,
}

/// The scroll benchmark's configuration, or `None` if none was requested.
#[tauri::command]
pub fn scrollbench_config() -> Option<ScrollBenchConfig> {
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
pub fn viewercheck_path() -> Option<String> {
    spike_env("TPDF_VIEWERCHECK")
}

/// A writable path a check may save to, from `TPDF_VIEWERCHECK_SCRATCH`.
///
/// The webview has no filesystem, so a phase that wants to compare what the
/// overlay draws against what the *file* renders has nowhere to put the file.
/// `viewer_check.py` makes a temporary path, binds it here and deletes it after
/// the run; a check that gets `None` says it had nowhere to write rather than
/// passing.
///
/// Deliberately a path and not a directory: a check writing wherever it liked
/// inside the app process is a wider authority than any of these need, and one
/// name is the smallest thing that makes the comparison possible.
#[tauri::command]
pub fn viewercheck_scratch() -> Option<String> {
    spike_env("TPDF_VIEWERCHECK_SCRATCH")
}

/// The reading-order expectations a check should assert against, if any.
///
/// Returns the *contents* of the file named by `TPDF_READING_MANIFEST`, because
/// the webview has no filesystem and the manifest is written by whatever
/// generated the fixture --- which is the point of it. A missing or unreadable
/// file is `None`, and the check then says it had nothing to compare against
/// rather than passing.
#[tauri::command]
pub fn reading_manifest() -> Option<String> {
    std::fs::read_to_string(spike_env("TPDF_READING_MANIFEST")?).ok()
}

/// The page geometry a check should assert the layout against, if any.
///
/// The same arrangement as [`reading_manifest`] and separate from it on purpose.
/// `viewer_check.py` binds any `<fixture>-manifest.json` to that variable and the
/// reading-order check then asserts it page by page, so a fixture that makes no
/// claim about reading order cannot use that name --- `testdata/mixed.pdf`
/// carries markers at its own corners rather than a sentence, and a manifest
/// under the other name would enrol it in a check it was not built for and
/// cannot pass. Its generator writes `mixed-geometry.json`, and this is the
/// variable that carries it.
#[tauri::command]
pub fn geometry_manifest() -> Option<String> {
    std::fs::read_to_string(spike_env("TPDF_GEOMETRY_MANIFEST")?).ok()
}

/// What a corpus's generator says is in it, if the fixture has such a sidecar.
///
/// The third of these, on the same arrangement and separate for the same reason.
/// `<fixture>-corpus.json` is written by `make_comments_pdf.py` and states, among
/// other things, the words the one bare mark in the corpus is drawn over --- so
/// the comments panel's covered-words check compares against a string a
/// different program wrote, rather than against anything derived from the reader
/// it is testing.
///
/// It carried that expectation for one commit with nothing reading it, which is
/// a claim written down and not enforced. The first hand-written version of it
/// named the wrong line.
///
/// Keyed here rather than in the webview, because the sidecar covers **several**
/// fixtures --- one generator writes `comments.pdf` and `comments-rotated.pdf`
/// --- and the process that knows which one is open is this one. The key is the
/// file name of [`viewercheck_path`], so a check reading this is looking at the
/// entry for the document it has in front of it and cannot silently assert one
/// fixture's expectations against another's.
///
/// `None` where there is no sidecar, no entry for this fixture, or nothing
/// readable --- all of which a check must report as "nothing to compare
/// against" rather than as a pass.
#[tauri::command]
pub fn corpus_manifest() -> Option<String> {
    let raw = std::fs::read_to_string(spike_env("TPDF_CORPUS_MANIFEST")?).ok()?;
    let all: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let path = viewercheck_path()?;
    let name = std::path::Path::new(&path).file_name()?.to_str()?;
    Some(all.get(name)?.to_string())
}

/// Path to time a cold open of on startup, from `TPDF_STARTUP` (spike 0.2).
#[tauri::command]
pub fn startup_path() -> Option<String> {
    spike_env("TPDF_STARTUP")
}

/// Records a webview-observed milestone on the process timeline.
///
/// `at_ms` is required rather than stamped here: every mark the webview cares
/// about happened before it could tell us, so stamping on arrival would measure
/// the IPC call instead of the event.
#[tauri::command]
pub fn startup_mark(name: String, at_ms: f64) {
    startup::mark_at(&name, at_ms);
}

/// The full startup timeline, Rust and webview marks merged.
#[tauri::command]
pub fn startup_timeline() -> Vec<(String, f64)> {
    startup::timeline()
}

/// Whether the pre-`main` interval could be measured on this platform.
///
/// The frontend needs to know, because a timeline that silently starts at
/// `main` would report a startup budget that excludes dyld.
#[tauri::command]
pub fn startup_pre_main_ms() -> Option<f64> {
    startup::pre_main_ms()
}

/// Prints spike output on the process's stdout.
///
/// Webview `console.log` does not reliably reach the terminal across platforms,
/// and the results need to land somewhere a script can read.
#[tauri::command]
pub fn spike_print(text: String) {
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
pub fn spike_exit(code: i32) {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code);
}
