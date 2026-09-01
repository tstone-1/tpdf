//! `links::scan` and `links::outline_targets` on an arbitrary document.
//!
//! Both are in one target because they are two walks over the same structure ---
//! a destination array, resolved against the page tree --- reached from
//! different roots. `scan` starts at each page's `/Annots` and reads `/Link`
//! subtypes; `outline_targets` starts at the catalog's `/Outlines` and walks a
//! tree whose depth an attacker chooses and which the format permits to be
//! *infinite*, since a bookmark's `/Next` may point back up it. The bound on
//! that walk is the interesting thing here, and it is only reachable through
//! `outline_targets`.
//!
//! Fuzzing them together also puts pressure on a property the application
//! depends on and no unit test can state: they resolve destinations through two
//! different pieces of code --- `links.rs` reads the destination array itself,
//! `outline.rs` asks PDFium --- and `links-probe --mode agree` is what compares
//! them on real documents. PDFium is out of scope here, so this target cannot
//! make that comparison; what it can do is establish that the `lopdf` half never
//! panics on a destination no real document would write.
//!
//! [`PAGES`] is fixed for the reason `annots_scan` states.
//!
//! # Invocation
//!
//! ```text
//! src-tauri/fuzz/run.py --target links_scan --seconds 3600
//! ```
//!
//! `run.py` is where the toolchain, the linker flag the build does not link
//! without, and this bound live. A bare `cargo fuzz run` is not equivalent.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tpdf_lib::links;

/// The page count handed to both walks. See `annots_scan`.
const PAGES: usize = 4096;

fuzz_target!(|data: &[u8]| {
    if let Ok(found) = links::scan(data, PAGES, None) {
        for link in &found.items {
            std::hint::black_box((link.page, link.rect));
        }
    }

    // Not gated on the first walk succeeding: they load the document
    // independently, and a document one refuses is one the other still has to
    // refuse safely.
    if let Ok(targets) = links::outline_targets(data, PAGES) {
        std::hint::black_box(targets.len());
    }
});
