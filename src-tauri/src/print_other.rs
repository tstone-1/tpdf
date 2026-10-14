//! The operating system's PDF parser, on a platform that has none tpdf uses.
//!
//! The print and save tests read what tpdf wrote back with a parser that did
//! not write it: PDFKit on macOS (`print_macos.rs`), `Windows.Data.Pdf` on
//! Windows (`print_win.rs`). Linux has no such parser in the system that fits
//! the licensing rule in `AGENTS.md`: the usual one, Poppler, is GPL. Until
//! one is chosen, [`read`] answers `None`, and every test that needs it says
//! `[SKIP]` instead of passing or vanishing (`BUILD.md`: a check that quietly
//! stops existing on one platform is worse than one that skips out loud).
//!
//! Test-only: nothing in the application calls it.

/// One page as a parser saw it. The same shape as on the other platforms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageReading {
    /// The page's rotation in degrees.
    pub rotation: i64,
    /// The page's text, and `None` when it was not asked for.
    pub text: Option<String>,
}

/// A document as a parser saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    pub pages: Vec<PageReading>,
}

/// Always `None`: there is no parser here to ask.
pub fn read(_bytes: &[u8]) -> Option<Reading> {
    None
}

/// What a test prints when it stops at this parser.
pub const SKIP: &str = "[SKIP] no independent PDF parser on this platform yet";
