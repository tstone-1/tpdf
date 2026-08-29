//! Does the OCR engine work from a process of its own?
//!
//! ```text
//! cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-worker-probe -- \
//!     testdata/text-base14.pdf
//! ```
//!
//! No `--lib`, and not for brevity: the default joins `PDFIUM_SUBDIR`, which is
//! `bin` on Windows, where `lib` exists, holds the *import* library and binds to
//! nothing.
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

// Portable since 2026-08-29, and it was the last OCR instrument that was not.
// It was macOS-only because its in-process baseline named `ocr_vision::Vision`
// directly; `WindowsOcr` is behind the same `ocr::Recogniser`, so only the
// engine's *construction* is per-platform now and the probe measures the worker
// on whichever platform it is run. That mattered more than it sounds: the worker
// this measures is newest on Windows, and Windows was the one platform that
// could not measure it.
#[path = "../src/probes/ocr_worker_probe.rs"]
mod imp;

fn main() {
    imp::main();
}
