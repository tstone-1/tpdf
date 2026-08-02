//! Does the Vision binding actually work, and is the coordinate flip right?
//!
//! ```text
//! cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-probe -- \
//!     testdata/text-base14.pdf --lib vendor/pdfium/lib
//! ```
//!
//! `ocr_vision.rs` has unit tests for [`normalised_to_points`], and they cannot fail for the
//! reason that matters: they assert the arithmetic against numbers this file also wrote. What
//! they cannot say is whether Vision's `boundingBox` means what the conversion assumes. Only a
//! real engine on a real page answers that, and the discriminator is **content at a position**
//! --- `docs/TRAPS.md` records the same lesson from the selection code, where a check that text
//! dragged out of a page appears in that page's text could not fail by construction.
//!
//! So the ordering check below does not ask whether Vision read the right words. It asks
//! whether the word Vision placed highest is the word the *embedded* text places highest. A
//! y-flip reverses that and nothing else here would notice.
//!
//! The gate checks are the other half, and they use no synthetic image: both the region under
//! test and the control band are strips of the same rendered page, stacked. That keeps every
//! pixel something PDFium actually drew, and it means the control is real text at the page's
//! own size rather than a token drawn large enough to be easy.

// macOS only, and it fails to *compile* off it rather than merely doing nothing:
// `tpdf_lib::ocr_vision` is `#[cfg(target_os = "macos")]` in `lib.rs`, so an
// unconditional `use` of it is an unresolved import on every other platform. That
// broke `clippy --all-targets`, `cargo test` and `cargo build --examples` on
// Windows for two days without anyone noticing, because the only machine that
// runs the gates is a Mac. The repository's first CI run found it in six minutes.
//
// The gate goes on a *module*, never on the crate root: `#![cfg(...)]` at the top
// of a target removes every item including `main`, and cargo then reports "`main`
// function not found", which reads like a missing entry point rather than a
// deliberately empty target. See `docs/TRAPS.md`.
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("ocr-probe exercises the macOS Vision binding; macOS only");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    imp::main();
}

#[cfg(target_os = "macos")]
#[path = "../src/probes/ocr_probe.rs"]
mod imp;
