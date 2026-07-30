//! Proves that moving every document behind a process boundary changed nothing
//! the reader can see --- and that it really moved. macOS only; see [`imp`].
//!
//! The body lives in `../probes/backend_probe.rs` rather than here because it is
//! macOS-only and a `#![cfg]` at the crate root cannot express that for a
//! `[[bin]]`: it removes every item including `main`, and cargo then reports
//! "`main` function not found", which reads like a missing entry point rather
//! than a deliberately empty target. `fdpass_probe.rs` carries the same note and
//! the same shape. A module *file* is used instead of an inline `mod imp { .. }`
//! only to keep the gate off 1,800 lines that are otherwise unchanged.
//!
//! ```text
//! cargo run --release --bin backend-probe -- testdata/text-heavy.pdf
//! ```

// Not merely "does not build here": every claim this probe makes is about the
// worker backend, which `Worker::spawn` refuses to create on a platform with no
// process boundary. Running it there could only report the refusal, and a probe
// whose checks cannot run must say so rather than print a table nobody should
// read. Since 2026-07-29 that is neither macOS nor Windows.
//
// It fails to *link* on an unsupported platform rather than failing to compile,
// which is why a green `cargo clippy --all-targets` and `cargo test` said nothing
// about it: the two dyld symbols in `imp::mapped_images` are reachable only from
// `main`, and `cargo test` replaces `main` with the test harness's own, so the
// linker drops them as dead code. See `BUILD.md`.
#[cfg(not(any(target_os = "macos", windows)))]
fn main() {
    eprintln!(
        "backend-probe compares the worker backend against in-process; \
         macOS and Windows only"
    );
    std::process::exit(2);
}

#[cfg(any(target_os = "macos", windows))]
fn main() {
    imp::main();
}

#[cfg(any(target_os = "macos", windows))]
// `../probes/`, not `backend_probe/`, and the location is load-bearing rather
// than tidy: a **directory** under `src/bin/` has no `[[bin]]` path claiming it,
// and `tauri build` scans that directory and registers the first such entry as a
// binary of its own. The MSI was then generated with a component pointing at a
// `backend_probe.exe` that does not exist, and WiX refused the whole package.
// See `docs/TRAPS.md`; `src/bin/` must contain only declared bin sources.
#[path = "../probes/backend_probe.rs"]
mod imp;
