//! `docinfo::scan` on an arbitrary document --- the properties panel's reader.
//!
//! The widest target here by surface, because it is the only walker that reaches
//! four parsers rather than one. On a single document it can run `lopdf` over
//! the object graph, `quick-xml` over the catalog's `/Metadata` packet, the BER
//! walk over a signature's `/Contents`, and `cms`/`x509-cert`/`der` over what
//! the walk produced. Each of those has a target of its own where one exists;
//! this is the one that reaches them **in the arrangement a file chooses**,
//! which is the arrangement an attacker chooses.
//!
//! It also reads the encryption dictionary, and that ordering is itself a trap
//! this repository has an entry for --- `lopdf::decrypt` removes the `/Encrypt`
//! trailer entry, so the encryption has to be read before it. A fuzzer cannot
//! see an ordering, but it can reach the code with a document shaped so that
//! both readings are live at once.
//!
//! [`PAGES`] is fixed for the reason `annots_scan` states. `page_count` is `u32`
//! here rather than `usize`, which is `scan`'s own signature.
//!
//! # Invocation
//!
//! ```text
//! src-tauri/fuzz/run.py --target docinfo_scan --seconds 3600
//! ```
//!
//! `run.py` is where the toolchain, the linker flag the build does not link
//! without, and this bound live. A bare `cargo fuzz run` is not equivalent.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tpdf_lib::docinfo;

/// The page count handed to the walk. See `annots_scan`.
const PAGES: u32 = 4096;

fuzz_target!(|data: &[u8]| {
    let Ok(properties) = docinfo::scan(data, PAGES, None) else {
        return;
    };

    for field in &properties.fields {
        std::hint::black_box(field.value.len());
    }
    for signature in &properties.signatures {
        std::hint::black_box((
            signature.certificate.is_some(),
            signature.timestamp.is_some(),
        ));
    }
    std::hint::black_box(properties.encryption.is_some());
});
