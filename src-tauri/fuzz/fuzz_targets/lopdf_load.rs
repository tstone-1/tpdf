//! `lopdf` document load, with the options this application passes it.
//!
//! Not the crate in isolation. Every walker here loads with the same three
//! settings --- `max_decompressed_size`, a password, and defaults for the rest
//! --- and it is that combination that decides what a hostile document can make
//! `lopdf` do. A load fuzzed with the crate's own defaults would be a statement
//! about a configuration tpdf never uses.
//!
//! # What this covers that the walker targets do not
//!
//! The walkers stop at the first `Err` from the load, so an input that fails to
//! parse exercises the loader and nothing else --- which is most inputs, and is
//! fine, because the loader is the part running on the raw file. What they never
//! reach is the work *after* a successful load that they have no reason to do:
//! this target resolves and decompresses every object the document holds, which
//! is the path a decompression bomb takes and the path an object stream takes.
//! `MAX_DECODE` is the bound that is supposed to make that safe, and this is
//! where it is put under pressure.
//!
//! The object walk is bounded here too, at [`MAX_OBJECTS`], for the reason the
//! production walkers bound theirs: without it one input decides how long every
//! later input waits, and a fuzzer that spends a minute on one document is a
//! fuzzer that has stopped.
//!
//! # Invocation
//!
//! ```text
//! src-tauri/fuzz/run.py --target lopdf_load --seconds 3600
//! ```
//!
//! `run.py` is where the toolchain, the linker flag the build does not link
//! without, and this bound live. A bare `cargo fuzz run` is not equivalent.
#![no_main]

use libfuzzer_sys::fuzz_target;
use lopdf::{Document, LoadOptions};

/// `encoding::MAX_DECODE`, which is `pub(crate)`. Repeated rather than imported,
/// and equal to it deliberately: a different value here would fuzz a bound the
/// application does not have.
const MAX_DECODE: usize = 64 * 1024 * 1024;

/// How many objects one input may have resolved before the target moves on.
const MAX_OBJECTS: usize = 4096;

fuzz_target!(|data: &[u8]| {
    let Ok(document) = Document::load_mem_with_options(
        data,
        LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            password: None,
            ..Default::default()
        },
    ) else {
        return;
    };

    // The question every walker asks first, and the one a damaged page tree can
    // answer with a cycle.
    let pages = document.get_pages();

    // Then the objects themselves. `get_object` is where a reference chain is
    // followed and where a stream is decompressed, so this is the call that
    // reaches the filters.
    for (index, id) in document.objects.keys().copied().enumerate() {
        if index >= MAX_OBJECTS {
            break;
        }
        if let Ok(object) = document.get_object(id) {
            if let Ok(stream) = object.as_stream() {
                // **`_with_limit`, and the unlimited twin is a finding rather
                // than an alternative.** `stream.decompressed_content()` ignores
                // the `max_decompressed_size` this document was loaded with --
                // measured, not assumed: a 2,289-byte PDF whose catalog
                // `/Metadata` declares a gigabyte took `docinfo::scan` to
                // 1,081 MiB resident, and 64 MiB of declared payload -- exactly
                // `MAX_DECODE` -- passed too. The bound applies to what the
                // loader expands for itself and to nothing a caller asks for
                // afterwards.
                //
                // This target called the unlimited one for its first run and
                // libFuzzer stopped it on an out-of-memory at input #290, which
                // is how the finding was made. Keeping it that way would spend
                // every later run re-finding the same thing and reaching
                // nothing behind it, so the bound is applied here and the
                // finding lives in the report and in the fix.
                let _ = stream.decompressed_content_with_limit(MAX_DECODE);
            }
        }
    }

    // Page content is a second decompression path -- a page's `/Contents` may be
    // an array of streams, which `get_object` above never assembles.
    //
    // `_with_limit` again, to match `redact.rs`, which uses the bounded form
    // throughout -- every unbounded `get_page_content` left in the tree is
    // inside a `#[cfg(test)]` module, checked rather than assumed, because
    // "there is another caller like this" is the sort of claim that sends the
    // next reader to fix test code.
    //
    // It is **not** what stopped this target reaching libFuzzer's memory
    // ceiling, and the first version of this comment said it was. The ceiling
    // was two oversized seeds: `incr-scan-40p.pdf` costs 1,019 MB resident on
    // its own and `incr-scan-20p.pdf` 537 MB, against 188 MB for the heaviest
    // of the other 3,243 corpus entries. `seed.py`'s `MAX_SEED_BYTES` is the
    // fix. The reason that took a while to see is worth carrying: process RSS
    // is a high-water mark, so it looked exactly like a leak, and libFuzzer
    // without a sanitizer cannot attribute an out-of-memory to an input -- one
    // of these runs blamed `da39a3ee...`, which is the SHA-1 of the empty
    // string.
    for id in pages.values().take(64) {
        let _ = document.get_page_content_with_limit(*id, MAX_DECODE);
    }
});
