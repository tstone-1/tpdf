//! The command line the parent writes, and the flags the child reads back off it.
//!
//! Split out of `worker.rs` when that file had grown to 2,861 lines and four
//! concerns. Nothing changed in the move: `worker.rs` re-exports every getter
//! here, so `crate::worker::doc_len_arg` still resolves, which is the path
//! `worker_child.rs` reaches it by.
//!
//! Both ends of one agreement, deliberately in one file. `command_line` builds
//! the single string a Windows child is given, and the getters read flags back
//! out of what a child received --- on either platform, since a macOS worker is
//! handed the same flags through a real `argv`. A change to one end is a change
//! to both.
//!
//! Not to be confused with `protocol.rs`, which is the tile URI the webview asks
//! through.

use std::path::PathBuf;

#[cfg(windows)]
use crate::worker::{DOC_HANDLE_ARGV, OUT_HANDLE_ARGV, TILE_HANDLE_ARGV};

/// Where the worker's PDFium library lives, given the parent's own.
#[must_use]
pub fn library_dir_arg(args: &[String]) -> Option<PathBuf> {
    value_of(args, "--lib").map(PathBuf::from)
}

/// The document length the parent passed.
#[must_use]
pub fn doc_len_arg(args: &[String]) -> Option<usize> {
    value_of(args, "--doc-len").and_then(|v| v.parse().ok())
}

/// The document section handle the parent passed, on Windows.
///
/// `usize` rather than `i32`, for the reason [`crate::worker_shm::Shm::raw_handle`]
/// gives: a handle
/// is pointer-sized, and an `i32` would truncate one silently into a value that
/// still looks like a plausible handle.
#[cfg(windows)]
#[must_use]
pub fn doc_handle_arg(args: &[String]) -> Option<usize> {
    value_of(args, DOC_HANDLE_ARGV).and_then(|v| v.parse().ok())
}

/// The tile section handle the parent passed, on Windows.
#[cfg(windows)]
#[must_use]
pub fn tile_handle_arg(args: &[String]) -> Option<usize> {
    value_of(args, TILE_HANDLE_ARGV).and_then(|v| v.parse().ok())
}

/// The output file handle the parent passed, on Windows.
///
/// `None` for every worker but one spawned to write --- see
/// [`crate::worker::OUT_FD`].
#[cfg(windows)]
#[must_use]
pub fn out_handle_arg(args: &[String]) -> Option<usize> {
    value_of(args, OUT_HANDLE_ARGV).and_then(|v| v.parse().ok())
}

/// Joins arguments into the single command line `CreateProcess` takes.
///
/// Windows has no `argv`. A process is given one string and **the child** splits
/// it, so quoting is the parent's job --- `std::process::Command` does this and
/// `spawn_contained` cannot use `Command`, so it is done here.
///
/// The rule is the one `CommandLineToArgvW` and the MSVC runtime implement, and
/// it is not "wrap in quotes if it has a space". A backslash is ordinary *except*
/// immediately before a quote, where it escapes; so a run of backslashes that
/// ends the argument must be doubled, or the closing quote we add becomes an
/// escaped quote and the argument swallows the next one. That case is not
/// exotic here: `--lib C:\Program Files\tpdf\` is a directory with a space and a
/// trailing separator, which is exactly the input that breaks a naive quoter.
///
/// The executable is passed through the same way even though argv[0] obeys a
/// simpler rule (quotes delimit, backslashes never escape). The two agree on
/// every string that can be a Windows path, since `"` is not a legal filename
/// character --- so the only divergence is unreachable.
#[cfg(windows)]
pub(crate) fn command_line(parts: &[&str]) -> String {
    let mut line = String::new();
    for part in parts {
        if !line.is_empty() {
            line.push(' ');
        }
        quote_arg(part, &mut line);
    }
    line
}

/// Appends one argument to a command line, quoted if it needs to be.
#[cfg(windows)]
fn quote_arg(arg: &str, out: &mut String) {
    // An empty argument still needs quotes, or it disappears entirely rather
    // than arriving as an empty string.
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        out.push_str(arg);
        return;
    }
    out.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => {
                backslashes += 1;
                out.push(c);
            }
            // The run before a quote is doubled and the quote escaped: one extra
            // backslash per backslash already written, plus one for the quote.
            '"' => {
                for _ in 0..=backslashes {
                    out.push('\\');
                }
                out.push('"');
                backslashes = 0;
            }
            _ => {
                backslashes = 0;
                out.push(c);
            }
        }
    }
    // And the run before the *closing* quote, for the same reason.
    for _ in 0..backslashes {
        out.push('\\');
    }
    out.push('"');
}

/// The value following a flag.
fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::{doc_len_arg, library_dir_arg, value_of};

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_flag_reads_the_value_after_it() {
        let a = args(&["--render-worker", "--doc-len", "42", "--lib", "/x"]);
        assert_eq!(value_of(&a, "--doc-len"), Some("42"));
        assert_eq!(doc_len_arg(&a), Some(42));
        assert_eq!(
            library_dir_arg(&a).as_deref(),
            Some(std::path::Path::new("/x"))
        );
    }

    #[test]
    fn a_flag_with_nothing_after_it_is_absent_rather_than_panicking() {
        // The worker parses its own argv after `exec`, where a panic is a
        // process that dies before it can say why.
        let a = args(&["--render-worker", "--doc-len"]);
        assert_eq!(value_of(&a, "--doc-len"), None);
        assert_eq!(doc_len_arg(&a), None);
    }

    #[test]
    fn a_flag_that_is_not_there_is_absent() {
        let a = args(&["--render-worker"]);
        assert_eq!(doc_len_arg(&a), None);
        assert_eq!(library_dir_arg(&a), None);
        // And a value is not mistaken for a flag: `--lib` as the *value* of
        // `--doc-len` must not then satisfy a lookup for `--lib`.
        let confusing = args(&["--doc-len", "--lib"]);
        assert_eq!(value_of(&confusing, "--doc-len"), Some("--lib"));
        assert_eq!(doc_len_arg(&confusing), None);
    }

    /// A handle survives argv, including one that does not fit an `i32`.
    ///
    /// The value that matters is the large one. A handle is pointer-sized, and
    /// parsing it into anything narrower is the defect this is aimed at --- it
    /// would not fail, it would produce a *different* handle, and mapping a
    /// wrong-but-valid handle is a far worse outcome than mapping none. The two
    /// flags are also checked not to answer each other's lookups, since they
    /// differ by one word and are passed adjacently.
    #[cfg(windows)]
    #[test]
    fn a_section_handle_survives_argv_at_full_width() {
        use super::{doc_handle_arg, tile_handle_arg};

        let wide = u32::MAX as usize + 4096;
        let a = args(&[
            "--doc-handle",
            &wide.to_string(),
            "--tile-handle",
            "512",
            "--lib",
            "C:\\lib",
        ]);
        assert_eq!(doc_handle_arg(&a), Some(wide));
        assert_eq!(tile_handle_arg(&a), Some(512));

        let neither = args(&["--render-worker"]);
        assert_eq!(doc_handle_arg(&neither), None);
        assert_eq!(tile_handle_arg(&neither), None);

        // A handle is unsigned: a negative value is a parse failure, not a
        // wraparound into a plausible one.
        let negative = args(&["--doc-handle", "-1"]);
        assert_eq!(doc_handle_arg(&negative), None);
    }

    /// The command line is read back by the parser Windows itself uses.
    ///
    /// `CommandLineToArgvW` rather than a table of expected strings, because a
    /// table would only restate the algorithm above and agree with it about
    /// output that is wrong --- `AGENTS.md` records exactly that failure, where
    /// every check on a generated file went through the library that wrote it.
    /// This is the *consumer's* parser: the same rules the child's own
    /// `std::env::args` implements.
    ///
    /// The awkward argument is the real one. `--lib C:\Program Files\tpdf\` has
    /// a space *and* a trailing separator, so a quoter that handles spaces but
    /// not the backslash run escapes its own closing quote, and the library path
    /// silently swallows the flag that follows it.
    #[cfg(windows)]
    #[test]
    fn a_command_line_survives_the_parser_windows_actually_uses() {
        let parts = [
            r"C:\Program Files\tpdf\tpdf.exe",
            crate::worker::WORKER_ARGV,
            "--doc-len",
            "4096",
            "--lib",
            r"C:\Program Files\tpdf\",
            super::DOC_HANDLE_ARGV,
            "312",
        ];
        assert_eq!(parse_command_line(&super::command_line(&parts)), parts);
    }

    /// The control, and it is not optional: it shows the oracle can fail.
    ///
    /// A round trip through a *lenient* parser would pass on any joining rule at
    /// all, and the check above would then be decoration. Joining the same parts
    /// with plain spaces must therefore come back wrong --- which is the naive
    /// implementation, so this also names what the quoting is for.
    #[cfg(windows)]
    #[test]
    fn a_command_line_joined_naively_does_not_survive_it() {
        let parts = [
            r"C:\Program Files\tpdf\tpdf.exe",
            "--lib",
            r"C:\Program Files\tpdf\",
        ];
        let naive = parts.join(" ");
        assert_ne!(parse_command_line(&naive), parts);
    }

    /// Splits a command line the way the child will, using Win32 itself.
    #[cfg(windows)]
    fn parse_command_line(line: &str) -> Vec<String> {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};

        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

        let wide: Vec<u16> = std::ffi::OsStr::new(line)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut count: i32 = 0;
        // SAFETY: `wide` is NUL-terminated and outlives the call; `count` is
        // written by it. The returned array is owned by us until `LocalFree`.
        let argv = unsafe { CommandLineToArgvW(wide.as_ptr(), &raw mut count) };
        assert!(!argv.is_null(), "CommandLineToArgvW rejected {line:?}");
        let mut out = Vec::new();
        for i in 0..count as isize {
            // SAFETY: `i` is below the count the call reported, and each entry
            // is a NUL-terminated wide string it allocated.
            unsafe {
                let arg = *argv.offset(i);
                let len = (0..).take_while(|n| *arg.offset(*n) != 0).count();
                out.push(
                    std::ffi::OsString::from_wide(std::slice::from_raw_parts(arg, len))
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        // SAFETY: the one allocation the call made, freed once.
        unsafe { LocalFree(argv.cast()) };
        out
    }
}
