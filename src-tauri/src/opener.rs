//! Handing a web address to the operating system, and to nothing else.
//!
//! ## The whole point is that no string is ever interpolated into a command line
//!
//! `docs/PLAN.md` §11 states it as a constraint rather than a preference: the
//! URL is handed to the OS opener **as a URL**. Both platforms have an API that
//! takes one as a value --- `NSWorkspace openURL:` takes an `NSURL` object,
//! `ShellExecuteW` takes a wide string in its own parameter --- so there is no
//! shell, no argument vector and no quoting anywhere in this file. That closes
//! the second injection surface, after the display one that [`crate::weburl`]
//! closes.
//!
//! `std::process::Command::new("open").arg(url)` would also pass the URL as an
//! argv element rather than through a shell, and it is deliberately not what
//! this does: it spawns a process, and a URL beginning with `-` is then an
//! option to that process rather than a URL. The platform APIs have no such
//! seam.
//!
//! ## Taking a `Web` rather than a `&str` is the guard
//!
//! Nothing here re-checks the scheme, because nothing here *can* be reached
//! with an unchecked one: [`Web`] is only constructible through
//! [`Web::parse`], which is where the allowlist lives. `AGENTS.md` records the
//! shape --- an unreachable guard is worth keeping when the type can carry it
//! instead, and this is the type carrying it.

use crate::weburl::Web;

/// Opens a web address in whatever the reader has set as their browser.
///
/// Blocking on Windows, where `ShellExecuteW` can take a noticeable moment to
/// start a cold browser, so callers run it off the async runtime's threads.
///
/// The error is ours in every arm: a failure here is the operating system
/// declining, and nothing it returns carries text from the document.
pub fn open(web: &Web) -> Result<(), String> {
    open_url(&web.url)
}

#[cfg(target_os = "macos")]
fn open_url(url: &str) -> Result<(), String> {
    use objc2_foundation::{NSString, NSURL};

    let text = NSString::from_str(url);
    // `URLWithString:` is nullable and returns nil for a string it cannot read.
    // It cannot happen for a URL `url::Url` has already serialized, and the
    // branch is here rather than an `expect` because a panic on the main thread
    // inside AppKit's own frames aborts without a usable backtrace --- the
    // reason `render.rs` gives for not panicking in the setup hook.
    //
    // No `unsafe`: `objc2-foundation` types this one safe, and wrapping it
    // anyway is `unused_unsafe`, which `-D warnings` makes a build failure on
    // the macOS leg. Written with the block first and caught by compiling on a
    // Mac rather than by reading --- from Windows this whole function is
    // invisible to every gate, which is `check_windows.py`'s trap with the
    // platforms swapped.
    let Some(target) = NSURL::URLWithString(&text) else {
        return Err("this link is not an address the system could read".into());
    };
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    if workspace.openURL(&target) {
        Ok(())
    } else {
        Err("the system could not open this link".into())
    }
}

#[cfg(windows)]
fn open_url(url: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide: Vec<u16> = std::ffi::OsStr::new(url)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = "open\0".encode_utf16().collect();

    // SAFETY: both buffers are NUL-terminated UTF-16 sequences that outlive the
    // call, and every pointer this does not use is passed null, which is what
    // the API documents for "no parameters, no working directory, no parent
    // window". `lpParameters` in particular is null rather than empty: a URL is
    // the *file* argument here, so there is no argument string at all.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    // The return is an `HINSTANCE` for source compatibility with 16-bit Windows
    // and is not a handle: any value **above 32** is success and anything at or
    // below it is an error code. Reading it as a pointer, or testing it against
    // zero the way every other Win32 call here is tested, would call a failure a
    // success for 32 of the 33 ways this can fail.
    if result as usize > 32 {
        Ok(())
    } else {
        Err("the system could not open this link".into())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn open_url(_url: &str) -> Result<(), String> {
    // Neither shipped platform, so this is the arm a `cargo test` on a Linux CI
    // box would take. A refusal rather than a `Command::new("xdg-open")` that
    // nobody has run: `AGENTS.md` records that a guard degrading to a no-op off
    // its platform stops being a guard, and an untested opener is worse than an
    // honest absence.
    Err("opening web links is not supported on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The signature is the assertion.
    ///
    /// There is nothing to test here that does not open a browser window: every
    /// arm ends in a call to the operating system, and a fake in front of it
    /// would be this module's own reader agreeing with its writer --- the shape
    /// `recentdocs.rs` records having gotten wrong once. What *is* checkable is
    /// that the only way in takes a parsed [`Web`], so a caller cannot reach the
    /// opener with a string the allowlist has not seen. That is a compile-time
    /// fact, and this is what fails to compile if it stops being true.
    #[test]
    fn the_only_way_in_is_a_parsed_web() {
        let taker: fn(&Web) -> Result<(), String> = open;
        let web = Web::parse("https://example.invalid/").expect("a legal https URL");
        // Not called: `open` reaches the window server. The value exists so the
        // type above is checked against a real `Web` rather than only inferred.
        let _ = (taker, web);
    }
}
