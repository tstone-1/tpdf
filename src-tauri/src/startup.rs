//! Startup timeline instrumentation (spike 0.2).
//!
//! The target in docs/PLAN.md is "cold start to first page painted under
//! 300 ms". A single end-to-end number cannot say *where* the budget went, so
//! this records a table of named milestones on one timeline and lets the run
//! print all of them.
//!
//! The timeline's origin is the kernel's process-creation time, not the first
//! instruction of `main`. Everything before `main` -- exec, dyld, linking
//! against the Tauri/WebKit frameworks -- is charged to the user's 300 ms just
//! the same, and on a framework-heavy app it is not a rounding error. Measuring
//! from `main` would silently exclude it and flatter every subsequent number.

use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Monotonic origin, stamped as early as `main` can.
static MAIN_ENTRY: OnceLock<Instant> = OnceLock::new();

/// Milliseconds between exec and `MAIN_ENTRY`. `None` where unmeasurable.
static PRE_MAIN_MS: OnceLock<Option<f64>> = OnceLock::new();

/// Named milestones, in the order they were first recorded.
static MARKS: Mutex<Vec<(String, f64)>> = Mutex::new(Vec::new());

/// Stamps the process start marker. Call first thing in `run`.
pub fn mark_process_start() {
    let _ = MAIN_ENTRY.set(Instant::now());
    let _ = PRE_MAIN_MS.set(measure_pre_main_ms());
    mark("main entry");
}

/// Milliseconds since process exec, the origin of the whole timeline.
///
/// Falls back to milliseconds since `main` when the pre-main interval could not
/// be measured, which understates the timeline rather than failing.
pub fn since_process_start_ms() -> f64 {
    let since_main = MAIN_ENTRY
        .get()
        .map(|t| t.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or(f64::NAN);

    since_main + pre_main_ms().unwrap_or(0.0)
}

/// Milliseconds spent before `main` -- exec, dyld, framework linking.
pub fn pre_main_ms() -> Option<f64> {
    PRE_MAIN_MS.get().copied().flatten()
}

/// Records a milestone at the current instant, keeping the first occurrence.
///
/// First-wins matters: `first tile decoded` is asked for on a path that runs
/// once per tile, and a last-wins table would report the last tile of the page.
pub fn mark(name: &str) {
    mark_at(name, since_process_start_ms());
}

/// Records a milestone at a caller-supplied time on the process timeline.
///
/// Used for marks the webview observed itself, which happened before it could
/// tell us about them.
pub fn mark_at(name: &str, at_ms: f64) {
    let Ok(mut marks) = MARKS.lock() else {
        return;
    };
    if marks.iter().any(|(existing, _)| existing == name) {
        return;
    }
    marks.push((name.to_string(), at_ms));
}

/// The recorded milestones, sorted onto the timeline.
///
/// Sorted by time rather than by insertion order, because the webview reports
/// its marks in a batch at the end and they interleave with the Rust ones.
pub fn timeline() -> Vec<(String, f64)> {
    let Ok(marks) = MARKS.lock() else {
        return Vec::new();
    };
    let mut out = marks.clone();
    out.sort_by(|a, b| a.1.total_cmp(&b.1));
    out
}

/// Milliseconds between process exec and now, measured against the wall clock.
///
/// Only called once, at `main` entry, so the wall clock's lower resolution and
/// jump risk do not accumulate: everything after this point is measured against
/// a monotonic `Instant`.
///
/// `libc` does not expose `kinfo_proc` on Apple targets, so this goes through
/// `proc_pidinfo` instead of the `sysctl(KERN_PROC_PID)` route the same query
/// takes on the BSDs.
#[cfg(target_os = "macos")]
fn measure_pre_main_ms() -> Option<f64> {
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };

    // SAFETY: `info` is a correctly sized, zero-initialised destination for the
    // flavour being requested, and `size` describes it truthfully.
    let written = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            std::ptr::addr_of_mut!(info).cast(),
            size as libc::c_int,
        )
    };
    // A short write means the kernel filled in less than the struct claims, so
    // the start-time fields at the end of it may be uninitialised.
    if written != size as libc::c_int {
        return None;
    }

    let started_epoch = info.pbi_start_tvsec as f64 + info.pbi_start_tvusec as f64 / 1e6;

    let now_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs_f64();

    let delta_ms = (now_epoch - started_epoch) * 1000.0;
    // A negative or absurd value means the two clocks disagree; report nothing
    // rather than a number that would quietly skew the whole timeline.
    (0.0..60_000.0).contains(&delta_ms).then_some(delta_ms)
}

/// Non-macOS placeholder.
///
/// Windows can do this with `GetProcessTimes`, but that needs a Win32 binding
/// this spike does not otherwise pull in. Until then the timeline on Windows
/// starts at `main` and says so, rather than guessing.
#[cfg(not(target_os = "macos"))]
fn measure_pre_main_ms() -> Option<f64> {
    let _ = (SystemTime::now(), UNIX_EPOCH);
    None
}
