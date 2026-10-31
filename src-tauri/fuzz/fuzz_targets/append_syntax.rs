//! `docinfo::append_syntax::reads_plainly` on arbitrary bytes --- the check that
//! decides whether what follows a signature can be compared at all.
//!
//! Its own target, although `docinfo_scan` reaches it too, because there it runs
//! only behind two `lopdf` parses that succeed, on a signature with an append.
//! Hardly any mutated document gets that far, and a table entry's offset can be
//! anything at all, which is how a panic got past its first version.
//!
//! The first two bytes choose where the signed range ends, so that an offset can
//! point on either side of it. The oracle is a panic, and one thing it promises:
//! an append it takes, unless it is nothing but white space, ends in `%%EOF`, which
//! is what leaves no `startxref` for a parser to find after the last one.
//!
//! # Invocation
//!
//! ```text
//! src-tauri/fuzz/run.py --target append_syntax --seconds 3600
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use tpdf_lib::docinfo::append_syntax::reads_plainly;

fuzz_target!(|data: &[u8]| {
    let Some((end, bytes)) = data.split_first_chunk::<2>() else {
        return;
    };
    let end = usize::from(u16::from_be_bytes(*end));
    if end > bytes.len() {
        return;
    }
    if !reads_plainly(bytes, end, |id| Some(i64::from(id.0 % 8))) {
        return;
    }
    let rest = bytes[end..]
        .iter()
        .rposition(|byte| !matches!(byte, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' '))
        .map(|last| &bytes[end..=end + last]);
    if let Some(rest) = rest {
        assert!(rest.ends_with(b"%%EOF"), "taken without an end: {rest:?}");
    }
});
