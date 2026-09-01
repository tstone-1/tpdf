//! The text-string decoder and the body sanitiser, on arbitrary bytes.
//!
//! Both are `pub` functions taking bytes or a string and returning one, with no
//! document around them, which makes this the cheapest target here by a long
//! way --- a document target spends nearly every execution inside `lopdf`'s
//! loader, and this one spends all of it in the code under test. That is the
//! whole argument for having it: the same decoder is reached from
//! `annots_scan`, `docinfo_scan` and `links_scan`, but only ever through
//! whatever byte strings a *valid* document happened to carry.
//!
//! It is also the code most likely to be wrong in a way that is not a crash.
//! `decode_text_string` implements the PDF text-string encodings --- UTF-16BE
//! with a byte-order mark, UTF-8 with one, PDFDocEncoding otherwise --- over
//! bytes chosen by the file, and `sanitize_body` then clips the result to a
//! character budget. Clipping a string by characters while measuring it in bytes
//! is a defect this repository has already paid for once elsewhere, so the
//! budget is asserted here rather than assumed.
//!
//! # The chain, not the two functions
//!
//! They are called in composition because that is how `annots.rs` calls them
//! (`read_body`, line 660): the decoder's output is the sanitiser's input, so
//! feeding the sanitiser an arbitrary `&str` would fuzz a domain the decoder
//! cannot produce. The decoder is given the raw bytes and the sanitiser is given
//! what came out.
//!
//! # Invocation
//!
//! ```text
//! src-tauri/fuzz/run.py --target annots_text --seconds 3600
//! ```
//!
//! `run.py` is where the toolchain, the linker flag the build does not link
//! without, and this bound live. A bare `cargo fuzz run` is not equivalent.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tpdf_lib::annots;

/// `annots::MAX_BODY_CHARS`, which is private. Repeated for the reason
/// `ber_certificate` states about `MAX_SIG_BLOB`.
const MAX_BODY_CHARS: usize = 4_000;

fuzz_target!(|data: &[u8]| {
    let decoded = annots::decode_text_string(data);
    let (body, clipped) = annots::sanitize_body(&decoded, MAX_BODY_CHARS);

    // The budget is in **characters**, and a string clipped by bytes passes a
    // length check written in bytes while cutting a multi-byte character in
    // half. Counting `chars()` is the only form of this assertion that can fail
    // on the input that would prove it -- `"第".repeat(5_000)` is 15,000 bytes
    // and 5,000 characters, and `annots.rs` has a test using exactly that shape.
    let count = body.chars().count();
    assert!(
        count <= MAX_BODY_CHARS,
        "the sanitised body is {count} characters, past the {MAX_BODY_CHARS} budget"
    );

    // `clipped` is deliberately **not** asserted, and the reason is worth
    // recording: the obvious statement -- a body that fills the budget from a
    // longer source must be reported clipped -- is false. `sanitize_body` also
    // collapses whitespace, so a 4,100-character source can reach exactly the
    // budget without anything having been clipped, and an assertion saying
    // otherwise would fail on correct input. It is read rather than checked.
    std::hint::black_box(clipped);
});
