//! Telling the operating system which documents were opened.
//!
//! ## Why this exists
//!
//! Right-clicking tpdf's taskbar icon showed no recent documents, and the cause
//! is not a broken registration --- it is that nothing had ever told Windows a
//! document was opened. The two lists are unrelated despite the name:
//!
//!  - **`src/lib/recents.ts`** is tpdf's own, shown in the command palette. It
//!    lives in the session file and the OS knows nothing about it.
//!  - **The Jump List** is the shell's, shown on the taskbar and Start menu. It
//!    is populated by `SHAddToRecentDocs`, and by nothing else an application
//!    does.
//!
//! Having one has never implied the other, which is why the absence reads as a
//! bug in a feature that exists rather than a feature that was never built.
//!
//! ## What has to be true for the entry to appear
//!
//! Three things, and only the last was missing:
//!
//!  1. **The file type is registered to this application.** `tauri.conf.json`
//!     declares a `pdf` file association, so the installer writes it. The shell
//!     files a recent document under the application registered for its
//!     extension, so without this the call is accepted and the entry lands
//!     somewhere else.
//!  2. **The application has a Start Menu shortcut**, which is what gives it an
//!     AppUserModelID for the shell to hang a Jump List on. Both the MSI and the
//!     NSIS installer write one. **A binary run straight out of `target/release`
//!     has neither, so this will look like it does nothing there** --- which is a
//!     property of the checkout, not of the code, and is worth knowing before
//!     spending an hour on it.
//!  3. **Something calls `SHAddToRecentDocs`.** That is this module.
//!
//! ## How to tell whether it worked, without installing anything
//!
//! The Jump List itself needs an installed build, so it is the wrong thing to
//! check first. The shell's acceptance of the call is visible immediately and
//! from outside the process: `SHAddToRecentDocs` writes a shortcut into
//! `%APPDATA%\Microsoft\Windows\Recent\<name>.lnk`. Open a document and look.
//!
//! ```text
//! ls "$APPDATA/Microsoft/Windows/Recent/"*.pdf.lnk
//! ```
//!
//! And resolve one, because an entry existing is not an entry that opens:
//!
//! ```text
//! $s = New-Object -ComObject WScript.Shell
//! $s.CreateShortcut("$env:APPDATA\Microsoft\Windows\Recent\x.pdf.lnk").TargetPath
//! ```
//!
//! Measured 2026-08-19 while `viewer_sweep.py` ran: one `.lnk` per corpus it
//! opened, each resolving to the real fixture, absolute and without the verbatim
//! prefix. That is the shell's own record rather than a claim of ours, which is
//! the standard this repository holds evidence to.
//!
//! ## Why not simply let the file dialog do it
//!
//! `IFileOpenDialog` files a recent document by itself, so opening through the
//! panel would half-work today. Every other way in would not: a drag onto the
//! window, a double-click in Explorer, a path in `argv`, and the single-instance
//! forward are the routes a reader actually uses, and none of them goes through
//! a dialog. Doing it once at the point where a document has genuinely opened
//! covers all of them, and covers them identically.
//!
//! ## macOS is not done here, and the reason is a constraint rather than a plan
//!
//! The counterpart is `NSDocumentController`'s `noteNewRecentDocumentURL:`,
//! which fills the Dock icon's Recent Documents and *File ▸ Open Recent*. It is
//! AppKit, so it must be called on the main thread, and `open_document` is an
//! async command that carries no such guarantee --- it would need a
//! `run_on_main_thread` hop. That is a small piece of work and it is not a
//! guess-and-ship one: calling AppKit off the main thread is undefined rather
//! than merely wrong, and this machine cannot run it even once.

#[cfg(windows)]
use std::path::Path;

/// Files `path` with the shell as a document this application just opened.
///
/// Best-effort by design: a failure means a Jump List entry is missing, which is
/// not a reason to refuse a document that opened perfectly well. There is
/// nothing to report and nothing a reader could do about it, so the call is made
/// and the result is not consulted --- `SHAddToRecentDocs` returns nothing at
/// all, and has no error to consult even in principle.
///
/// Called once per successful open, after the document exists. Before it would
/// file documents that failed to parse.
#[cfg(windows)]
pub fn note_opened(path: &Path) {
    let Some(wide) = shell_path(path) else {
        return;
    };

    // SAFETY: `wide` is a NUL-terminated UTF-16 sequence that outlives the call,
    // and `SHARD_PATHW` is precisely the flag that says the pointer is one.
    unsafe {
        windows_sys::Win32::UI::Shell::SHAddToRecentDocs(
            // `windows-sys` types this constant `i32` and the parameter `u32`,
            // so the cast is the binding's, not a reinterpretation of ours.
            windows_sys::Win32::UI::Shell::SHARD_PATHW as u32,
            wide.as_ptr().cast(),
        );
    }
}

/// The buffer handed to the shell, or `None` when the file cannot be resolved.
///
/// **A separate function so that a test can read its result.** The FFI call is
/// the one part nothing here can observe --- `SHAddToRecentDocs` returns
/// nothing, and what it did is a Jump List a person looks at --- so the seam is
/// put where the mistakes actually are. A first draft of this module tested a
/// *copy* of the conversion living in the test module, which is the writer
/// agreeing with its own reader: every assertion passed and a change to the real
/// code could not have moved one of them.
#[cfg(windows)]
fn shell_path(path: &Path) -> Option<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;

    // The shell wants an absolute path. A relative one is filed against the
    // *shell's* idea of the current directory rather than ours, which is a
    // different file or no file --- and the entry that results points somewhere
    // a reader's click cannot follow.
    //
    // It also refuses a file that is not there, which is the fail-closed half:
    // filing an unresolvable path puts an entry in the reader's Jump List that
    // opens nothing, and that is worse than the absence this module fixes,
    // because it looks like the feature working.
    let absolute = std::fs::canonicalize(path).ok()?;

    // `\\?\` is what `canonicalize` returns on Windows and the shell does not
    // understand it: an entry filed under the verbatim form appears with the
    // prefix visible in its label and does not resolve when clicked. Stripped
    // rather than avoided, because the canonicalisation is what makes the path
    // absolute in the first place.
    let text = absolute.as_os_str().to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);

    Some(
        std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect(),
    )
}

/// Does nothing off Windows. See the module docs for what macOS would need.
#[cfg(not(windows))]
pub fn note_opened(_path: &std::path::Path) {}

#[cfg(all(test, windows))]
mod tests {
    use super::shell_path;
    use std::path::{Path, PathBuf};

    fn scratch(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("tpdf-recentdocs-{name}-{}.pdf", std::process::id()));
        std::fs::write(&path, b"%PDF-1.7\n").expect("write scratch");
        path
    }

    #[test]
    fn a_path_the_shell_is_given_is_absolute_nul_terminated_and_not_verbatim() {
        let path = scratch("plain");
        let wide = shell_path(&path).expect("convert");
        let text = String::from_utf16(&wide[..wide.len() - 1]).expect("utf-16");

        assert_eq!(wide.last(), Some(&0), "NUL-terminated: {text}");
        assert!(!text.contains('\0'), "one NUL and it is the last: {text}");
        assert!(
            !text.starts_with(r"\\?\"),
            "the shell does not understand the verbatim prefix: {text}"
        );
        assert!(
            Path::new(&text).is_absolute(),
            "filed against the shell's cwd otherwise: {text}"
        );
        // And it is still the file, which is what says the stripping above took
        // a prefix off rather than a character out of the path.
        assert!(Path::new(&text).exists(), "{text}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_relative_path_is_made_absolute_rather_than_passed_through() {
        // The control for the clause above: a relative path must not survive as
        // one. `canonicalize` is what does it, and a version of this that merely
        // stripped a prefix would pass every other assertion in the test above.
        //
        // **The relative path is built against the cwd rather than by changing
        // it**, and that is not a stylistic choice. The first version of this
        // test moved the process's working directory to the scratch folder and
        // put it back --- and `cargo test` runs tests on several threads, so for
        // the duration of that window every other test in the crate had a
        // different cwd. `save.rs`'s `fixture()` is `Path::new("../testdata")`,
        // a relative path, and dozens of tests resolve it: during the window they
        // either panic on a file that is suddenly not there, or --- much worse ---
        // report `[SKIP] not generated` and pass. It cost one unexplained exit
        // 101 out of four runs before it was found, and the quiet failure is the
        // one that would have survived.
        //
        // `target/` because it is gitignored, so a run killed mid-test leaves
        // nothing in `git status` to mistake for real work.
        let name = format!("target/tpdf-recentdocs-rel-{}.pdf", std::process::id());
        let relative = Path::new(&name);
        assert!(!relative.is_absolute(), "the input really is relative");
        std::fs::create_dir_all("target").expect("target exists");
        std::fs::write(relative, b"%PDF-1.7\n").expect("write scratch");

        let wide = shell_path(relative).expect("convert");
        let text = String::from_utf16(&wide[..wide.len() - 1]).expect("utf-16");

        assert!(Path::new(&text).is_absolute(), "{text}");
        assert!(text.ends_with(".pdf"), "{text}");
        assert!(
            Path::new(&text).exists(),
            "and it is still the file: {text}"
        );
        let _ = std::fs::remove_file(relative);
    }

    #[test]
    fn a_file_that_is_not_there_is_not_filed() {
        // Fail closed, in the small way this module can. A missing file cannot
        // be canonicalised, and filing the un-canonicalised path would put an
        // entry in the reader's Jump List that opens nothing when clicked ---
        // worse than the absence this module exists to fix, because it looks
        // like the feature working.
        let missing = std::env::temp_dir().join("tpdf-recentdocs-not-here.pdf");
        let _ = std::fs::remove_file(&missing);
        assert!(shell_path(&missing).is_none());
    }

    #[test]
    fn a_path_with_a_space_and_a_non_ascii_name_survives_the_round_trip() {
        // UTF-16 is where a byte-oriented conversion would look right and be
        // wrong, and a reader's Documents folder is full of both of these.
        let path = std::env::temp_dir().join(format!(
            "tpdf-recentdocs Prüfbericht {}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, b"%PDF-1.7\n").expect("write scratch");
        let wide = shell_path(&path).expect("convert");
        let text = String::from_utf16(&wide[..wide.len() - 1]).expect("utf-16");
        assert!(text.contains("Prüfbericht"), "{text}");
        assert!(text.contains(' '), "{text}");
        assert!(Path::new(&text).exists(), "{text}");
        let _ = std::fs::remove_file(&path);
    }
}
