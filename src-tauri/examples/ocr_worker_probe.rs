//! Does the OCR engine work from a process of its own?
//!
//! ```text
//! cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-worker-probe -- \
//!     testdata/text-base14.pdf --lib vendor/pdfium/lib
//! ```
//!
//! `ocr-sandbox-probe` measures the *profile*: what a process can still do once
//! `OCR_SANDBOX_PROFILE` is in force. This measures the *worker*: whether the engine
//! reads the same page from behind that boundary, whether the identity of the engine that
//! read it survives the wire, and whether the process that asked ever maps the engine at all.
//!
//! **The baseline is the same program reading the same bytes in-process**, because a worker
//! that reads nothing and an engine that reads nothing produce identical output --- the
//! failure this whole subsystem exists to refuse. Everything else is stated as a difference
//! from that row.
//!
//! Two properties are here because they are the ones a caller cannot recover from. An image
//! larger than the shared buffer must be refused *and leave the worker usable*, or one
//! oversized region costs a whole document its verification. And a worker killed from outside
//! must **report** inside its own deadline rather than block on a pipe nobody will write to:
//! `docs/TRAPS.md` has *a check whose failure mode is a wait cannot fail*, and the engine
//! ignores the deadline it is handed, so the parent is the only place that bound can live.

// macOS only, and it fails to *compile* off it rather than doing nothing: `ocr_vision` is
// macOS-gated, and so is the worker's child half. The gate goes on a module and never on the
// crate root, because `#![cfg(...)]` there removes `main` and cargo then reports a missing
// entry point. See `docs/TRAPS.md`.
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("ocr-worker-probe exercises the macOS OCR worker; macOS only");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    imp::main();
}

#[cfg(target_os = "macos")]
#[path = "../src/probes/ocr_worker_probe.rs"]
mod imp;
