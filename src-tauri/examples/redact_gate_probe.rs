//! Does the redaction gate certify a clean file and refuse a dirty one?
//!
//! ```text
//! cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-gate-probe -- \
//!     testdata/text-base14.pdf
//! ```
//!
//! No `--lib` in that line, and not for brevity: this runs on both platforms, so
//! the default joins `PDFIUM_SUBDIR` --- which is `bin` on Windows, where the
//! `lib` directory exists, holds the *import* library, and binds to nothing.
//! `only_the_macos_spikes_hardcode_the_library_directory` is what enforces it.
//!
//! The evidence for `docs/PLAN.md` §6 step 4 being *wired*, which no unit test
//! can be: `redact_copy` and `redact_document` are Tauri commands, and the join
//! between a command and `ocr_gate::run` is the layer `docs/TRAPS.md` records as
//! *a feature can be inert in the application while three layers of tests pass*.
//!
//! **The control is the same gate run against the file that was not redacted.**
//! A gate that certifies everything passes "the redacted file has no reasons"
//! perfectly, so that row alone is worth nothing.

// Portable on purpose: the render half runs on both platforms and the OCR half
// on one, so this reports what each platform actually does rather than refusing
// to start. A build with no engine checks that the gate says so exactly once.
#[path = "../src/probes/redact_gate_probe.rs"]
mod imp;

fn main() {
    imp::main();
}
