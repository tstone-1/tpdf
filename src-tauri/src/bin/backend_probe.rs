//! Proves that moving every document behind a process boundary changed nothing
//! the reader can see --- and that it really moved. macOS only; see [`imp`].
//!
//! The body lives in `backend_probe/imp.rs` rather than here because it is
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

// Not merely "does not build off macOS": every claim this probe makes is about
// the worker backend, which `Worker::spawn` refuses to create off macOS. Running
// it elsewhere could only report the refusal, and a probe whose checks cannot
// run must say so rather than print a table nobody should read.
//
// It fails to *link* off macOS rather than failing to compile, which is why a
// green `cargo clippy --all-targets` and `cargo test` said nothing about it: the
// two dyld symbols in `imp::mapped_images` are reachable only from `main`, and
// `cargo test` replaces `main` with the test harness's own, so the linker drops
// them as dead code. See `BUILD.md`.
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("backend-probe compares the worker backend against in-process; macOS only");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    imp::main();
}

#[cfg(target_os = "macos")]
#[path = "backend_probe/imp.rs"]
mod imp;
