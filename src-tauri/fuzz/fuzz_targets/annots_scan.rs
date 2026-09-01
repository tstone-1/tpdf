//! `annots::scan` on an arbitrary document --- the comments panel's whole reader.
//!
//! This is the largest of the `lopdf` walkers and the one that touches the most
//! attacker-chosen structure: every page's `/Annots`, each annotation's
//! dictionary, its `/Contents` text string in whatever encoding the file
//! declares, its `/M` date, its `/IRT` reply chain and its `/F` flags. Nearly
//! all of that is decoded rather than merely read, which is what distinguishes
//! it from `encoding_scan` next door.
//!
//! # The page count, and why it is a constant
//!
//! `scan` takes the page count PDFium reported, and uses it for two things: a
//! `take` that bounds the walk, and `limits.pages_missed`, which is how a
//! reader is told the object graph saw fewer pages than the renderer did. A
//! fuzz target has no PDFium answer to hand it, and deriving one from the input
//! bytes would make every seed something other than a PDF --- so the corpus
//! could not be shared with the other document targets or seeded from
//! `testdata/`.
//!
//! [`PAGES`] is therefore a fixed, generous number. It is generous on purpose:
//! a small one would make the `take` the binding constraint and hide every page
//! after it, which is the shape of a bound that quietly stops a check from
//! reaching what it is aimed at.
//!
//! # Invocation
//!
//! ```text
//! src-tauri/fuzz/run.py --target annots_scan --seconds 3600
//! ```
//!
//! `run.py` is where the toolchain, the linker flag the build does not link
//! without, and this bound live. A bare `cargo fuzz run` is not equivalent.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tpdf_lib::annots;

/// The page count handed to the walk. See the module comment.
const PAGES: usize = 4096;

fuzz_target!(|data: &[u8]| {
    let Ok(comments) = annots::scan(data, PAGES, None) else {
        return;
    };

    // Reading every field is the point rather than an afterthought: a walk that
    // built a `Comment` holding an unreachable index or a string it cannot
    // render has not failed until something looks. `black_box` is what stops
    // the optimiser deciding none of this is observable.
    for comment in &comments.items {
        std::hint::black_box((
            comment.page,
            comment.id,
            comment.body.len(),
            comment.author.len(),
        ));
    }
    std::hint::black_box(comments.limits.pages_missed);
});
