//! How much of a redaction can be proved, over a corpus rather than a fixture.
//!
//! ```text
//! cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-reach-probe -- \
//!     ~/Downloads --pages 3 --regions 40
//! ```
//!
//! No `--lib` in that line, and not for brevity: this runs on both platforms, so
//! the default joins `PDFIUM_SUBDIR` --- which is `bin` on Windows, where the
//! `lib` directory exists, holds the *import* library, and binds to nothing.
//!
//! Counts and shapes only. It is pointed at the reader's own documents, and
//! nothing it read is printed.

#[path = "../src/probes/redact_reach_probe.rs"]
mod imp;

fn main() {
    imp::main();
}
