//! Asks whether Windows can contain a PDFium child at all, and what it costs.
//!
//! macOS gets its process boundary from `sandbox_init` and an SBPL profile.
//! Windows has no counterpart: containment there is assembled from a job object
//! (resource limits), an integrity level (what the process may *write*) and a
//! restricted token (what it may reach at all). Which rungs of that ladder still
//! let PDFium render is not documented anywhere and cannot be reasoned out ---
//! `AGENTS.md` already records a sandboxed PDFium on macOS returning `ok` while
//! silently substituting a typeface, so the only honest test compares **pixels**.
//!
//! This is the spike `docs/PLAN.md` names as the prerequisite for a Windows
//! worker. It deliberately builds no worker: it re-execs *this* binary as the
//! contained child, hands it the document and its output as **inherited
//! handles**, and compares what comes back against an in-process render. That
//! also answers the transport question the real worker will face, since handing
//! over an already-open handle is the Windows analogue of the `dup2` the macOS
//! worker does and is the only way a contained child reaches anything.
//!
//! The body lives in `win_sandbox_probe/imp.rs` rather than here because a
//! `#![cfg]` at the crate root cannot express "Windows only" for a `[[bin]]`: it
//! removes every item including `main`, and cargo then reports "`main` function
//! not found", which reads like a missing entry point rather than a deliberately
//! empty target. `backend_probe.rs` and `fdpass_probe.rs` carry the same note.
//!
//! ```text
//! cargo run --release --bin win-sandbox-probe
//! cargo run --release --bin win-sandbox-probe -- testdata/vector-heavy.pdf
//! ```

// Every claim this probe makes is about Win32 containment primitives, which do
// not exist elsewhere. Running it on macOS could only report their absence, and
// a probe whose checks cannot run must say so rather than print a table nobody
// should read --- the same reasoning `backend_probe.rs` applies in the mirror.
#[cfg(not(windows))]
fn main() {
    eprintln!("win-sandbox-probe measures Win32 job objects and tokens; Windows only");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    imp::main();
}

#[cfg(windows)]
#[path = "win_sandbox_probe/imp.rs"]
mod imp;
