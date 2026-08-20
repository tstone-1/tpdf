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
//!    does. Its macOS counterpart is the Dock icon's Recent Documents.
//!
//! Having one has never implied the other, which is why the absence reads as a
//! bug in a feature that exists rather than a feature that was never built.
//!
//! macOS has the same split and the same absence: the Dock icon's Recent
//! Documents is AppKit's list, filled by `NSDocumentController`'s
//! `noteNewRecentDocumentURL:` and by nothing else. (*File ▸ Open Recent* would
//! read the same list, and tpdf has no such submenu --- see `note_opened`.)
//!
//! ## What has to be true for the entry to appear
//!
//! Three things on Windows, and only the last was missing:
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
//! macOS asks for less: the list is the application's own, keyed by bundle
//! identifier, so a bundle is the only precondition. **A bare `cargo run`
//! binary is not a bundle**, has no identifier to key a list to, and will look
//! exactly like this module doing nothing --- the same property-of-the-checkout
//! trap as the Start Menu shortcut above, arriving on the other platform.
//!
//! ## How to tell whether it worked, from outside the process
//!
//! On Windows the Jump List itself needs an installed build, so it is the wrong
//! thing to check first. The shell's acceptance of the call is visible
//! immediately: `SHAddToRecentDocs` writes a shortcut into
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
//! **macOS has no such file, and the three obvious places to look all answer
//! "nothing here" for a feature that is working.** Measured 2026-08-20, in this
//! order, and each answer reads exactly like the call doing nothing:
//!
//!  - `defaults read com.timostein.tpdf NSRecentDocumentRecords` --- *does not
//!    exist*, and stays that way through 75 s of running and a clean quit. That
//!    key is the pre-Sierra location; nothing writes it now.
//!  - `sfltool list-info com.apple.LSSharedFileList.ApplicationRecentDocuments`
//!    --- hangs. Not an instrument.
//!  - `ls ~/Library/Application Support/com.apple.sharedfilelist/` --- reports
//!    `Operation not permitted`. It is TCC-protected, so what is in it is *not
//!    established*; with `2>/dev/null` it prints `total 0` and reads as empty,
//!    which is how that non-answer first got written down as one.
//!
//! So there is no file to check, and `TPDF_RECENTDOCS_PROBE` exists because of
//! it: set it and each open prints AppKit's own list before and after filing.
//!
//! **The evidence is two launches, and it is the feature rather than a proxy for
//! it.** Launch tpdf on one document, quit, launch it on another: the second
//! process --- which never filed the first document --- starts with the first
//! document already in the list, and after filing its own the list is ordered
//! most-recent-first. Measured 2026-08-20, `text-heavy.pdf` then `rotated.pdf`:
//! `BEFORE filing, AppKit holds 0` then `1`, carrying `text-heavy.pdf` over.
//! That is a different process reading state this one left with the operating
//! system, which is the standard the Windows `.lnk` sweep meets. `BUILD.md` has
//! the procedure.
//!
//! ## Why not simply let the file dialog do it
//!
//! `IFileOpenDialog` files a recent document by itself, so opening through the
//! panel would half-work today. Every other way in would not: a drag onto the
//! window, a double-click in Explorer or the Dock, a path in `argv`, and the
//! single-instance forward are the routes a reader actually uses, and none of
//! them goes through a dialog. Doing it once at the point where a document has
//! genuinely opened covers all of them, and covers them identically.

#[cfg(any(windows, target_os = "macos"))]
use std::path::{Path, PathBuf};

/// The absolute, existing file both platforms have to be given, or `None`.
///
/// **One rule in one place, because it is one rule.** Windows files a recent
/// document against the *shell's* current directory when handed a relative path,
/// and AppKit's `fileURLWithPath:` resolves one against the *process's* --- two
/// different wrong files, from the same mistake. Both are fixed by resolving
/// before either platform sees the path, and a copy of that per platform is the
/// shape `docs/TRAPS.md` records under *"two copies of a distinction drift, and
/// a mutation of one survives"*.
///
/// It is also the fail-closed half. `canonicalize` refuses a file that is not
/// there, and filing an unresolvable path puts an entry in the reader's list
/// that opens nothing when clicked --- worse than the absence this module fixes,
/// because it looks like the feature working.
#[cfg(any(windows, target_os = "macos"))]
fn resolved(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Files `path` with the shell as a document this application just opened.
///
/// Best-effort by design: a failure means a menu entry is missing, which is not
/// a reason to refuse a document that opened perfectly well. There is nothing to
/// report and nothing a reader could do about it, so the call is made and the
/// result is not consulted --- `SHAddToRecentDocs` returns nothing at all, and
/// has no error to consult even in principle.
///
/// Called once per successful open, after the document exists. Before it would
/// file documents that failed to parse.
///
/// The `AppHandle` is unused here and load-bearing on macOS, where the call has
/// to reach the main thread.
#[cfg(windows)]
pub fn note_opened(_app: &tauri::AppHandle, path: &Path) {
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

    let absolute = resolved(path)?;

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

/// Files `path` with AppKit as a document this application just opened.
///
/// Best-effort, for the same reason the Windows arm is: what this buys is an
/// entry in the Dock icon's Recent Documents, and a document that opened
/// correctly must not be refused because a menu could not be updated.
///
/// **Not *File ▸ Open Recent*, which tpdf does not have.** The menu bar is built
/// from `menubar.ts`'s own spec, and its `NOT_IN_MENU` table records why the
/// recent list is absent from it: the list is rebuilt whenever a file is opened,
/// so a menu following it has to be rebuilt with it. This fills the list that
/// submenu will read when somebody builds it.
/// `noteNewRecentDocumentURL:` returns nothing, so there is no error to consult
/// even in principle --- what can fail is getting to the thread it must run on,
/// and that is reported to the diagnostic log rather than to the reader.
///
/// **The main-thread hop is the whole reason this arm was not written with the
/// Windows one.** `NSDocumentController` is `MainThreadOnly`, and
/// `open_document` is an async command with no such guarantee. Calling AppKit
/// off the main thread is undefined rather than merely wrong, so this goes
/// through `run_on_main_thread` --- and the requirement is carried by the type
/// rather than by this paragraph: `sharedDocumentController` takes a
/// `MainThreadMarker`, which cannot be forged.
///
/// The path is resolved *before* the hop and the closure carries a `String`,
/// because `Retained<NSURL>` is not `Send` and the closure must be. That is not
/// a workaround: it puts the fallible half on the calling thread, where it can
/// return, and leaves the main thread holding two infallible calls.
#[cfg(target_os = "macos")]
pub fn note_opened(app: &tauri::AppHandle, path: &Path) {
    let Some(absolute) = resolved(path) else {
        return;
    };
    let text = absolute.to_string_lossy().into_owned();

    let hop = app.run_on_main_thread(move || {
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            // Unreachable by construction --- this closure runs on the main
            // thread by definition --- and silence here would be a menu that
            // never fills with no way to find out why.
            crate::diag::note("[recentdocs] dispatched off the main thread; nothing filed");
            return;
        };
        let controller = objc2_app_kit::NSDocumentController::sharedDocumentController(mtm);
        report(&controller, "before");
        controller.noteNewRecentDocumentURL(&document_url(&text));
        report(&controller, "after");
    });
    if let Err(e) = hop {
        crate::diag::note(&format!("[recentdocs] {e}"));
    }
}

/// Prints AppKit's own recent-document list, when `TPDF_RECENTDOCS_PROBE` is set.
///
/// **The only observable this module has, and it had to be earned.** There is no
/// file to check. `NSRecentDocumentRecords` in the application's defaults is the
/// pre-Sierra location and is never written --- measured, and it reads exactly
/// like the feature doing nothing; `sfltool list-info` hangs rather than
/// printing the list; and `~/Library/Application Support/com.apple.sharedfilelist/`
/// refuses to be listed at all, which an `ls` with `2>/dev/null` reports as an
/// empty directory. See the module docs above and `docs/TRAPS.md`.
///
/// What *is* checkable, and is the whole feature rather than a proxy for it, is
/// that a **second launch starts with the first launch's document already in the
/// list**. That is a different process reading state this one left with the
/// operating system, which is the same standard the Windows `.lnk` sweep meets.
/// `BUILD.md` has the two-launch procedure.
///
/// Off unless asked for: this runs on every open, and a shipped application that
/// narrates its own menu bookkeeping is noise in the one log a reader sends back.
#[cfg(target_os = "macos")]
fn report(controller: &objc2_app_kit::NSDocumentController, when: &str) {
    if std::env::var_os("TPDF_RECENTDOCS_PROBE").is_none() {
        return;
    }
    let listed = controller.recentDocumentURLs();
    crate::diag::note(&format!(
        "[recentdocs] {when} filing, AppKit holds {} document(s)",
        listed.len()
    ));
    for url in listed.iter() {
        crate::diag::note(&format!(
            "[recentdocs]   {}",
            url.path().map(|s| s.to_string()).unwrap_or_default()
        ));
    }
}

/// The file URL AppKit is given for an already-resolved absolute path.
///
/// **A separate function so that a test can read its result**, for the reason
/// the Windows `shell_path` is one: `noteNewRecentDocumentURL:` returns nothing
/// and what it did is a menu a person looks at, so the seam goes where the
/// mistake actually is.
///
/// And the mistake is a specific one. `NSURL::URLWithString` is the constructor
/// that looks right and is wrong for a path: it parses its argument as a URL, so
/// `/Users/x/a b.pdf` either comes back with no scheme at all or, once a space
/// is in it, comes back **nil**. `fileURLWithPath:` is the one that means "this
/// text is a filesystem path", and it percent-encodes on the way out. A reader's
/// Documents folder is full of spaces and umlauts, so the failing case is the
/// ordinary one rather than an edge.
#[cfg(target_os = "macos")]
fn document_url(text: &str) -> objc2::rc::Retained<objc2_foundation::NSURL> {
    objc2_foundation::NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(text))
}

/// Does nothing on a platform with neither list.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn note_opened(_app: &tauri::AppHandle, _path: &std::path::Path) {}

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

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::{document_url, resolved};
    use std::path::{Path, PathBuf};

    fn scratch(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("tpdf-recentdocs-{name}-{}.pdf", std::process::id()));
        std::fs::write(&path, b"%PDF-1.7\n").expect("write scratch");
        path
    }

    /// Reads the URL back the way AppKit will: its own accessors, not ours.
    fn parts(text: &str) -> (bool, String, String) {
        let url = document_url(text);
        (
            url.isFileURL(),
            url.path().map(|s| s.to_string()).unwrap_or_default(),
            url.absoluteString()
                .map(|s| s.to_string())
                .unwrap_or_default(),
        )
    }

    #[test]
    fn a_url_the_menu_is_given_is_an_absolute_file_url() {
        let path = scratch("plain");
        let absolute = resolved(&path).expect("resolve");
        let (is_file, url_path, absolute_string) = parts(&absolute.to_string_lossy());

        assert!(is_file, "a path URL, not a string URL: {absolute_string}");
        assert!(absolute_string.starts_with("file://"), "{absolute_string}");
        assert!(Path::new(&url_path).is_absolute(), "{url_path}");
        // Resolved rather than compared byte for byte, and the reason is the
        // whole of the fourth test below: `fileURLWithPath:` hands the path
        // back **decomposed**, so an equality assertion holds here only because
        // this fixture's name is ASCII. Written that way it would be a rule that
        // is false the first time a reader opens a file with an umlaut in it,
        // asserted on the one fixture that cannot tell.
        assert_eq!(
            std::fs::canonicalize(&url_path).ok().as_deref(),
            Some(absolute.as_path()),
            "and it names the same file: {url_path}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_relative_path_is_made_absolute_rather_than_passed_through() {
        // The control for the clause above: a relative path must not survive as
        // one. `fileURLWithPath:` accepts one and resolves it against the
        // *process's* working directory, so the entry names whatever file sits
        // at that name wherever the reader happened to launch tpdf from --- and
        // the URL that results is absolute and looks entirely correct.
        //
        // **The relative path is built against the cwd rather than by changing
        // it**, and that is not a stylistic choice --- `cargo test` runs tests on
        // several threads, so moving the process's working directory makes every
        // other test in the crate read a different `../testdata`. The Windows
        // twin of this test carries the full account of what that cost.
        //
        // `target/` because it is gitignored, so a run killed mid-test leaves
        // nothing in `git status` to mistake for real work.
        let name = format!("target/tpdf-recentdocs-rel-{}.pdf", std::process::id());
        let relative = Path::new(&name);
        assert!(!relative.is_absolute(), "the input really is relative");
        std::fs::create_dir_all("target").expect("target exists");
        std::fs::write(relative, b"%PDF-1.7\n").expect("write scratch");

        let absolute = resolved(relative).expect("resolve");
        assert!(absolute.is_absolute(), "{}", absolute.display());
        let (_, url_path, _) = parts(&absolute.to_string_lossy());
        assert!(Path::new(&url_path).is_absolute(), "{url_path}");
        assert!(url_path.ends_with(".pdf"), "{url_path}");
        assert!(
            Path::new(&url_path).exists(),
            "and it is still the file: {url_path}"
        );
        let _ = std::fs::remove_file(relative);
    }

    #[test]
    fn a_file_that_is_not_there_is_not_filed() {
        // Fail closed, in the small way this module can. A missing file cannot
        // be canonicalised, and filing the un-canonicalised path would put an
        // entry in the Dock's Recent Documents that opens nothing --- worse
        // than the absence this module exists to fix, because it looks like the
        // feature working.
        let missing = std::env::temp_dir().join("tpdf-recentdocs-not-here-macos.pdf");
        let _ = std::fs::remove_file(&missing);
        assert!(resolved(&missing).is_none());
    }

    #[test]
    fn a_path_with_a_space_and_a_non_ascii_name_survives_the_round_trip() {
        // The case that separates the two constructors, and it is the ordinary
        // one rather than an edge: a reader's Documents folder is full of both.
        // `URLWithString:` parses its argument as a URL, so a space makes it
        // return nil outright; `fileURLWithPath:` treats the text as a path and
        // percent-encodes on the way out.
        //
        // So the two assertions are a pair and neither is redundant. The
        // encoded form proves the constructor escaped rather than concatenated;
        // `path` proves the escaping is reversible, which is what AppKit will do
        // to it when a reader clicks the entry.
        let path = std::env::temp_dir().join(format!(
            "tpdf-recentdocs Prüfbericht {}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, b"%PDF-1.7\n").expect("write scratch");
        let absolute = resolved(&path).expect("resolve");
        let (is_file, url_path, absolute_string) = parts(&absolute.to_string_lossy());

        assert!(is_file, "{absolute_string}");
        assert!(
            absolute_string.contains("%20"),
            "a space is escaped, not carried: {absolute_string}"
        );
        assert!(
            !absolute_string.contains(' '),
            "and no raw space survives: {absolute_string}"
        );
        assert!(url_path.contains(' '), "{url_path}");
        assert!(
            !url_path.is_ascii(),
            "the umlaut survived in some form: {url_path}"
        );
        // **The assertion that matters, and the one an equality check gets
        // wrong.** `fileURLWithPath:` decomposes: the file on disk is NFC
        // (`c3 bc`, measured --- APFS preserves the bytes it is given) and
        // `path` comes back NFD (`75 cc 88`), so `url_path == absolute` is false
        // for a name no reader would call unusual, and the difference is
        // invisible when printed. It is not a mangled name: APFS looks a
        // filename up normalisation-insensitively, so the decomposed path opens
        // the same file --- which is exactly what this asserts, by resolving it
        // and getting the composed form back.
        assert_eq!(
            std::fs::canonicalize(&url_path).ok().as_deref(),
            Some(absolute.as_path()),
            "the entry opens the reader's file: {url_path}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
