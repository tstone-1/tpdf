//! What is left of a process once the OCR profile is in force?
//!
//! ```text
//! cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-sandbox-probe -- \
//!     testdata/text-base14.pdf --lib vendor/pdfium/lib
//! ```
//!
//! `ocr.rs` carries a three-row table measured by hand on 2026-07-31: under the parser
//! worker's profile Vision is killed by SIGTRAP, with `/System/Library` readable it fails
//! with `nilError`, and with reads allowed entirely it works. That table is the argument for
//! `OCR_SANDBOX_PROFILE` being a separate constant rather than a relaxation flag, and for the
//! engine getting a process of its own --- and nothing has re-run it since, in a repository
//! whose own index says a safety net that has never fired looks exactly like one that keeps
//! passing.
//!
//! It also does not measure the constant that shipped. The rung that worked allowed reads and
//! said nothing about writes; `OCR_SANDBOX_PROFILE` denies `file-write*` and `network*`, and
//! whether Vision runs under *that* has never been established.
//!
//! Three rungs, each in a re-exec'd child that renders a page **before** the profile comes
//! down --- the parser worker maps PDFium first too, and sandboxing before the render would
//! measure a different program. Each then tries three things and prints what happened:
//!
//! | rung | writes a file | reaches the listener | runs Vision |
//! |---|---|---|---|
//! | `bare` --- the control | must succeed | must succeed | must read |
//! | `ocr` --- `OCR_SANDBOX_PROFILE` | must be denied | must be denied | must still read |
//! | `parser` --- the render worker's | --- | --- | must **not** read |
//!
//! **The control rung is what makes the other two mean anything.** A machine where nothing
//! works reports a perfectly contained ladder, and a `ConnectionRefused` reads exactly like a
//! sandbox denial --- which is why the parent holds a real listener open and passes its port,
//! so that an unsandboxed rung connects rather than being refused.

// macOS only, and it fails to *compile* off it rather than doing nothing: `ocr_vision` and
// `apply_sandbox` are both macOS-gated. The gate goes on a module and never on the crate
// root, because `#![cfg(...)]` there removes `main` and cargo then reports a missing entry
// point. See `docs/TRAPS.md`.
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("ocr-sandbox-probe measures the macOS sandbox profiles; macOS only");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    imp::main();
}

#[cfg(target_os = "macos")]
#[path = "../src/probes/ocr_sandbox_probe.rs"]
mod imp;
