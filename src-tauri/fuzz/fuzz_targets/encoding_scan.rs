//! `encoding::scan` on an arbitrary document.
//!
//! The narrowest of the four walkers and the one whose answer everything else
//! rests on: it builds each page's character mapping out of the font
//! dictionaries, which is what turns the bytes in a content stream into text a
//! reader can search. `annots.rs`, `links.rs` and `docinfo.rs` all borrow its
//! `resolve` and its `MAX_DECODE`, so a defect reached here is reached from all
//! four.
//!
//! Worth a target of its own rather than being left to `docinfo_scan` because
//! the shapes that stress it are font-shaped rather than document-shaped --- a
//! `/ToUnicode` CMap that disagrees with its own ranges, a composite font whose
//! descendant is missing, a `/Differences` array of the wrong type --- and a
//! fuzzer given a narrow target finds those far sooner than one given a wide
//! one.
//!
//! [`PAGES`] is fixed for the reason `annots_scan` states.
//!
//! # Invocation
//!
//! ```text
//! src-tauri/fuzz/run.py --target encoding_scan --seconds 3600
//! ```
//!
//! `run.py` is where the toolchain, the linker flag the build does not link
//! without, and this bound live. A bare `cargo fuzz run` is not equivalent.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tpdf_lib::encoding;

/// The page count handed to the walk. See `annots_scan`.
const PAGES: usize = 4096;

fuzz_target!(|data: &[u8]| {
    let Ok(pages) = encoding::scan(data, PAGES, None) else {
        return;
    };
    std::hint::black_box(pages.len());
});
