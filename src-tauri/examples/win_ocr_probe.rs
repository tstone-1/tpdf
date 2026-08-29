//! Asks whether `Windows.Media.Ocr` can be the Windows engine behind
//! [`ocr::Recogniser`], and answers the one question that decides it.
//!
//! `docs/PLAN.md` §9.10 ranks the in-box engine first, on the strength of adding
//! no package: two features on a `windows` crate already here for the print path.
//! That argument is about cost and says nothing about whether it *works* on a
//! machine nobody configured. `OcrEngine` needs an installed recogniser language
//! pack, and `ocr::RecogniseError::Unavailable`'s doc comment has said so since
//! the interface was written --- without anyone measuring how often that state is
//! the normal one. If a stock Windows install carries no pack, the in-box engine
//! is not a feature that ships, it is one that works on machines somebody set up,
//! and the ranking changes.
//!
//! Four readings, and the last two are what make this more than an enumeration:
//!
//! 1. `AvailableRecognizerLanguages` --- the installed set, which is the gating
//!    answer.
//! 2. `TryCreateFromUserProfileLanguages` --- whether the call an implementation
//!    would actually make comes back with an engine.
//! 3. A **positive control**: a word drawn into a bitmap here and read back. An
//!    enumeration that lists a language says the pack is installed; it does not
//!    say the engine reads. `docs/TRAPS.md` records what an empty answer from a
//!    scan that never looked is worth.
//! 4. The **same reading for a non-word**. `ocr::Options::language_correction` is
//!    documented as off for verification *always*, because a corrector turns
//!    marks it cannot read into plausible words --- and `Windows.Media.Ocr`
//!    exposes no switch for it where Vision does
//!    (`ocr_vision.rs`'s `setUsesLanguageCorrection`). Whether that contract can
//!    be honoured here is a question about this engine's behaviour, not about its
//!    API surface, so it is measured rather than argued.
//!
//! **Both strings are read at two sizes, and the second one is the point.** The
//! first run of this probe drew at 44 px and both came back verbatim, which is a
//! reading taken about 3x above `ocr_gate::MIN_CONTROL_PX` --- a control easier
//! than the check, which `docs/TRAPS.md` has an entry about. A corrector's effect
//! is largest on marginal input, and marginal is exactly what the gate hands an
//! engine: a control sized from the smallest box a redaction covered. So the
//! second size is that floor.
//!
//! **What it cannot say.** It runs at whatever integrity the shell gave it. The
//! parser worker runs at low integrity inside a job object, and macOS already
//! taught this lesson in the mirror: Vision is killed by SIGTRAP under
//! `SANDBOX_PROFILE` and needs general `file-read`, which is why OCR is a
//! separate process under `OCR_SANDBOX_PROFILE`. Whether `Windows.Media.Ocr`
//! survives containment is a second measurement and wants a rung ladder like
//! `win_sandbox_probe.rs`, not a line here.
//!
//! **Exit codes are the reading, not a verdict on the platform.** 0 means the
//! probe measured something --- including "no language packs", which is an
//! answer and not a failure. 2 means it could not measure. A probe that reddened
//! CI for reporting an inconvenient truth would be one nobody leaves switched on.
//!
//! ```text
//! cargo run --release --example win-ocr-probe
//! ```

// The body is Win32 and WinRT throughout; on any other platform it could only
// report their absence. Same shape as `win_sandbox_probe.rs`, and for the same
// reason a crate-root `#![cfg]` cannot express it -- that removes `main` too, and
// cargo then reports a missing entry point rather than an empty target.
#[cfg(not(windows))]
fn main() {
    eprintln!("win-ocr-probe measures Windows.Media.Ocr; Windows only");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    imp::main();
}

#[cfg(windows)]
// Out of `src/bin/`, for the trap about a directory there becoming a phantom
// binary in the Windows installer.
#[path = "../src/probes/win_ocr_probe.rs"]
mod imp;
