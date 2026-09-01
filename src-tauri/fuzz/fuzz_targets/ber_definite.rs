//! The BER walk, on arbitrary bytes.
//!
//! `ber::to_definite_length` is the highest-value target in this directory and
//! the reason it exists. It is about 150 lines this repository wrote, it runs on
//! a signature's `/Contents` --- bytes an attacker picks --- and it runs *in
//! front of* `cms` and `der`, so a defect here is reached before either of those
//! crates' own hardening applies. See `docs/THREAT-MODEL.md` §T6.8.
//!
//! # What the oracle is, beyond "it did not crash"
//!
//! Not crashing is the weakest thing a fuzz target can assert, and on a walk
//! written with `get` everywhere and bounds on every axis it is also the thing
//! least likely to fire. Two stronger properties are checked on every input the
//! walk accepts:
//!
//! * **Measured and written agree.** `to_definite_length` carries a
//!   `debug_assert_eq!(out.len(), span.output)` comparing what `measure` counted
//!   against what `emit` produced. Those are two independent walks of the same
//!   bytes, and a disagreement is the one defect class that would otherwise
//!   produce a blob whose header contradicts its body --- unparseable by
//!   everything downstream and diagnosable by nothing. `cargo fuzz` builds with
//!   debug assertions on, so that check is live here and is *not* live in a
//!   release build of the application.
//! * **Idempotence.** The module's own doc comment claims a blob that is already
//!   DER comes back byte-identical, and that claim is what makes it safe to put
//!   in front of every signature rather than only the ones that need it. Its own
//!   output is by construction already definite, so walking it a second time must
//!   return it unchanged. A rewrite that loses or duplicates a byte fails here
//!   even when both walks agree with each other.
//!
//! # Invocation
//!
//! ```text
//! src-tauri/fuzz/run.py --target ber_definite --seconds 3600
//! ```
//!
//! `run.py` is where the toolchain, the linker flag the build does not link
//! without, and this bound live. A bare `cargo fuzz run` is not equivalent.
//!
//! `-max_len` is above what a real `/Contents` needs for the structure to be
//! interesting and far below the 2 MiB `docinfo::signature_contents` admits: the
//! shapes that matter here are nesting and length forms, not size, and a large
//! corpus buys executions per second rather than coverage.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tpdf_lib::ber;

fuzz_target!(|data: &[u8]| {
    let Some(written) = ber::to_definite_length(data) else {
        // Refusing is the common answer and a correct one: truncated input, a
        // child overrunning its parent, nesting past the bound. Nothing to check
        // -- the property under test is about what the walk *accepts*.
        return;
    };

    // A value the module just wrote must be one it can read. This is not the
    // same statement as the assertion below it: an output the walk refuses
    // outright fails here, and one it accepts but rewrites fails there.
    let Some(again) = ber::to_definite_length(&written) else {
        panic!(
            "the walk refused its own output: {} bytes in, {} bytes out",
            data.len(),
            written.len()
        );
    };

    // Compared by slice rather than by `assert_eq!`, which would print two
    // megabyte-scale vectors into the crash report and bury the offsets that
    // say where they part.
    if again != written {
        let at = again
            .iter()
            .zip(written.iter())
            .position(|(l, r)| l != r)
            .unwrap_or_else(|| written.len().min(again.len()));
        panic!(
            "rewriting a definite encoding changed it: {} bytes became {} bytes, \
             first difference at offset {at}",
            written.len(),
            again.len()
        );
    }
});
