//! The whole signature-reading pipeline, from the bytes as the file holds them.
//!
//! `ber_definite` fuzzes the walk alone. This target fuzzes the composition the
//! application actually performs on a `/Contents` blob: the walk, then the two
//! ASN.1 readers behind it, exactly as `docinfo::signature_contents` chains
//! them. The composition is worth a target of its own because the walk's output
//! is `cms`'s input --- a rewrite that is self-consistent and wrong still
//! produces a value nobody else has to accept, and the seam between one parser
//! and the next is where a length that "looks fine" is spent.
//!
//! Both bounds are applied here in the shape production applies them, and in
//! that order, so the target reaches only inputs a document could actually get
//! to these parsers:
//!
//! * `2 x MAX_SIG_BLOB` on the walk's **input**, which `docinfo.rs` calls the
//!   load-bearing half --- without it a document chooses how much work the walk
//!   does;
//! * `MAX_SIG_BLOB` on its **output**, which is what `cms` and `der` then see.
//!
//! `MAX_SIG_BLOB` is `const` and private to `docinfo`, so the value is repeated
//! here rather than imported. That is a second copy of a constant and it is the
//! kind of thing this repository writes traps about, so it is bounded in the one
//! direction that matters: a fuzz target applying a *looser* bound than
//! production only ever explores inputs production refuses, which wastes
//! executions and cannot produce a false finding. It is stated rather than
//! hidden so that a bump to the real constant is a decision here too.
//!
//! # Invocation
//!
//! ```text
//! src-tauri/fuzz/run.py --target ber_certificate --seconds 3600
//! ```
//!
//! `run.py` is where the toolchain, the linker flag the build does not link
//! without, and this bound live. A bare `cargo fuzz run` is not equivalent.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tpdf_lib::{ber, docinfo};

/// `docinfo::MAX_SIG_BLOB`, which is private. See the module comment.
const MAX_SIG_BLOB: usize = 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    // All zeros is a reserved-but-unwritten placeholder rather than a
    // signature, and `signature_contents` answers it before the walk. Mirrored
    // so the corpus does not fill up with padding.
    if data.iter().all(|byte| *byte == 0) {
        return;
    }
    if data.len() > MAX_SIG_BLOB.saturating_mul(2) {
        return;
    }
    let Some(blob) = ber::to_definite_length(data) else {
        return;
    };
    if blob.len() > MAX_SIG_BLOB {
        return;
    }

    // Both readers, because they walk the same blob for different things: the
    // certificate is read out of `SignedData`, the timestamp out of the
    // unsigned attributes beside it. Neither may panic and neither may hang;
    // the answer itself is not asserted, because there is no independent oracle
    // for "what certificate is in these bytes" that is not a second copy of the
    // reader under test.
    let _ = docinfo::parse_certificate(&blob);
    let _ = docinfo::parse_timestamp_token(&blob);
});
