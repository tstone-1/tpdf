//! Documents handed to tpdf by something other than its own file dialog.
//!
//! Three routes, and they do not look alike:
//!
//! - **macOS, double-click or "Open With".** Launch Services sends an Apple
//!   Event, which Tauri surfaces as `RunEvent::Opened { urls }`. Nothing arrives
//!   in `argv` at all.
//! - **Windows, double-click.** The path arrives as `argv[1]`.
//! - **A terminal, either platform.** `tpdf file.pdf`, also `argv[1]`.
//!
//! The awkward part is *when* the first of those arrives. The Apple Event can be
//! delivered before the webview exists, so a handler that emits an event to the
//! frontend and nothing else drops the document silently --- the app opens on an
//! empty window and there is no error anywhere, which is exactly the failure a
//! user reads as "it does not work with double-click".
//!
//! So paths are queued until the frontend says it is listening, and the handoff
//! happens under one lock: draining the queue and setting the flag have to be a
//! single step, or a path arriving between the two is lost by the same mechanism
//! in a smaller window.

use std::path::PathBuf;

use parking_lot::Mutex;

/// The event a path is delivered on once the frontend is listening.
pub const OPEN_EVENT: &str = "tpdf://open";

/// Paths waiting for a frontend, and whether one has arrived.
#[derive(Default)]
struct State {
    queued: Vec<PathBuf>,
    listening: bool,
}

/// Documents that arrived from outside, queued until someone can show them.
#[derive(Default)]
pub struct Launch {
    state: Mutex<State>,
}

/// What should happen to a path that has just arrived.
#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    /// The frontend is listening; emit it.
    Emit(PathBuf),
    /// Nothing can show it yet; it is queued.
    Queued,
}

impl Launch {
    /// Records a path, saying whether the caller should emit it.
    ///
    /// The decision is returned rather than acted on so that this module needs
    /// no `AppHandle` and can be tested without a running application.
    pub fn deliver(&self, path: PathBuf) -> Delivery {
        let mut state = self.state.lock();
        if state.listening {
            Delivery::Emit(path)
        } else {
            state.queued.push(path);
            Delivery::Queued
        }
    }

    /// Hands over everything queued, and starts emitting from now on.
    ///
    /// One lock for both halves. Draining and then separately setting the flag
    /// leaves a window in which a path is queued into a vector nobody will read
    /// again --- the same lost document as before, only harder to reproduce.
    pub fn take(&self) -> Vec<PathBuf> {
        let mut state = self.state.lock();
        state.listening = true;
        std::mem::take(&mut state.queued)
    }
}

/// Paths named on the command line.
///
/// `argv[0]` is the executable. Anything beginning with `-` is a flag: macOS has
/// historically passed `-psn_0_...` to bundled applications, and a stray
/// `-NSDocumentRevisionsDebugMode` would otherwise be opened as a document.
///
/// Nothing here checks that the file exists. A path that does not is still what
/// the user asked for, and reporting "no such file" is the opener's job --- a
/// filter here would turn a typo into an empty window with no explanation.
pub fn paths_from_args<I, S>(args: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .skip(1)
        .map(|arg| arg.as_ref().to_string())
        .filter(|arg| !arg.starts_with('-'))
        .map(PathBuf::from)
        .collect()
}

/// Paths named on the command line, with one path put back together when the
/// command line arrived without its quotes.
///
/// After an update on Windows the installer starts tpdf again with the
/// arguments the old process had, so that the document comes back. The updater
/// quotes a path with spaces in it, and the installer reads that value with
/// NSIS `GetOptions`, which returns the inside of a value that is one quoted
/// string. Measured 2026-10-08: `/ARGS "C:\a b\x.pdf"` comes back as
/// `C:\a b\x.pdf`, and tpdf was started with `C:\a` and `b\x.pdf`. A
/// document under `Team Site - Documents` came back as three arguments, the
/// middle one `-`, and the message named a path that was the tail of the
/// real one.
///
/// So when there are two arguments or more, none of the ones that could be a
/// document is a file, and all of them joined by one space are a file, that
/// file is what was asked for. `-` pieces are part of the join: a flag never
/// was. Two things this does not do. A path with two spaces in a row is
/// not put back, because the split kept no count of them. And arguments that
/// are not a file together are left as they are, so that a mistyped name is
/// still reported by whatever opens it.
///
/// `is_file` is a parameter so that this needs no disk to test.
pub fn paths_from_launch<I, S>(args: I, is_file: impl Fn(&std::path::Path) -> bool) -> Vec<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect();
    let pieces = args.get(1..).unwrap_or_default();
    if pieces.len() > 1 {
        let whole = PathBuf::from(pieces.join(" "));
        let one_is_a_file = pieces
            .iter()
            .any(|piece| !piece.starts_with('-') && is_file(std::path::Path::new(piece)));
        if !one_is_a_file && is_file(&whole) {
            return vec![whole];
        }
    }
    paths_from_args(args)
}

/// Turns a `file://` URL from an Apple Event into a path.
///
/// `Url::to_file_path` rather than `Url::path`, because the URL is
/// percent-encoded: a document called `my report.pdf` arrives as
/// `file:///Users/x/my%20report.pdf`, and the raw path opens nothing. Any URL
/// that is not a local file --- an `http://` handed over by another application
/// --- returns `None` rather than being coerced into a path that does not exist.
///
/// The scheme check is **not** redundant with `to_file_path`, however much it
/// looks it. That function maps a `localhost` host to *no host* whatever the
/// scheme, so `https://localhost/a.pdf` is a path it will happily build; only a
/// host that is some other domain makes it refuse. This guard is the whole of
/// what stops a URL from another application naming a local file it should not.
pub fn path_from_url(url: &tauri::Url) -> Option<PathBuf> {
    if url.scheme() != "file" {
        return None;
    }
    url.to_file_path().ok()
}

#[cfg(test)]
mod tests {
    use super::{path_from_url, paths_from_args, paths_from_launch, Delivery, Launch};
    use std::path::{Path, PathBuf};

    #[test]
    fn the_executable_is_not_a_document() {
        assert!(paths_from_args(["/Applications/tpdf.app/Contents/MacOS/tpdf"]).is_empty());
    }

    #[test]
    fn a_path_after_the_executable_is_a_document() {
        assert_eq!(
            paths_from_args(["tpdf", "/tmp/a.pdf"]),
            vec![PathBuf::from("/tmp/a.pdf")]
        );
    }

    #[test]
    fn several_documents_keep_their_order() {
        assert_eq!(
            paths_from_args(["tpdf", "/tmp/a.pdf", "/tmp/b.pdf"]),
            vec![PathBuf::from("/tmp/a.pdf"), PathBuf::from("/tmp/b.pdf")]
        );
    }

    #[test]
    fn a_flag_is_not_a_document() {
        // macOS has passed `-psn_0_...` to bundled apps, and opening that as a
        // document is an empty window with no explanation.
        assert_eq!(
            paths_from_args(["tpdf", "-psn_0_1234", "/tmp/a.pdf"]),
            vec![PathBuf::from("/tmp/a.pdf")]
        );
    }

    #[test]
    fn a_path_that_does_not_exist_is_still_a_document() {
        // Deliberately: reporting "no such file" belongs to whatever opens it.
        // Filtering here turns a typo into an empty window and no message.
        assert_eq!(
            paths_from_args(["tpdf", "/tmp/definitely-not-here.pdf"]),
            vec![PathBuf::from("/tmp/definitely-not-here.pdf")]
        );
    }

    /// The arguments tpdf gets after an update when the document's folder has
    /// spaces and a lone hyphen in its name.
    const SPLIT: [&str; 5] = [
        "tpdf",
        r"C:\Users\x\Team",
        "-",
        r"Documents\my",
        "report.pdf",
    ];
    const WHOLE: &str = r"C:\Users\x\Team - Documents\my report.pdf";

    #[test]
    fn a_path_split_at_its_spaces_is_put_back_together() {
        assert_eq!(
            paths_from_launch(SPLIT, |path| path == Path::new(WHOLE)),
            vec![PathBuf::from(WHOLE)]
        );
    }

    #[test]
    fn pieces_that_are_no_file_together_stay_apart() {
        // A mistyped name has to reach the opener as it was typed, flags left
        // out as before, so that the message names what was asked for.
        assert_eq!(
            paths_from_launch(SPLIT, |_| false),
            vec![
                PathBuf::from(r"C:\Users\x\Team"),
                PathBuf::from(r"Documents\my"),
                PathBuf::from("report.pdf")
            ]
        );
    }

    #[test]
    fn two_documents_that_exist_are_two_documents() {
        // Also when their names joined by a space happen to be a file.
        assert_eq!(
            paths_from_launch(["tpdf", "a.pdf", "b.pdf"], |_| true),
            vec![PathBuf::from("a.pdf"), PathBuf::from("b.pdf")]
        );
    }

    #[test]
    fn one_argument_is_never_joined_with_anything() {
        assert_eq!(
            paths_from_launch(["tpdf", "/tmp/not here.pdf"], |_| false),
            vec![PathBuf::from("/tmp/not here.pdf")]
        );
        assert!(paths_from_launch(["tpdf"], |_| true).is_empty());
    }

    #[test]
    fn a_percent_encoded_url_becomes_the_name_it_stands_for() {
        // The *fixture* is platform-shaped because `Url::to_file_path` is:
        // on Windows it requires a drive letter and refuses `file:///Users/x/...`
        // outright, so the macOS spelling asserts nothing there except that
        // refusal --- which is a different claim, and one this test is not about.
        //
        // Written this way rather than gated off Windows on purpose. The property
        // under test is the percent-decoding, it holds on both platforms, and a
        // check that quietly stops existing on one of them is exactly what
        // `BUILD.md` warns is worse than a check that skips out loud.
        #[cfg(windows)]
        let (text, want) = (
            "file:///C:/Users/x/my%20report.pdf",
            r"C:\Users\x\my report.pdf",
        );
        #[cfg(not(windows))]
        let (text, want) = ("file:///Users/x/my%20report.pdf", "/Users/x/my report.pdf");

        let url = tauri::Url::parse(text).expect("url");
        assert_eq!(path_from_url(&url), Some(PathBuf::from(want)));
    }

    #[test]
    fn a_url_that_is_not_a_file_is_refused() {
        let url = tauri::Url::parse("https://example.com/a.pdf").expect("url");
        assert_eq!(path_from_url(&url), None);
    }

    #[test]
    fn a_url_that_is_not_a_file_is_refused_even_when_it_looks_local() {
        // The case above cannot fail if the scheme check is deleted: a domain
        // host makes `to_file_path` refuse on its own, so it probes a direction
        // the guard does not defend. `localhost` is the direction that does ---
        // `to_file_path` treats it as *no host at all*, whatever the scheme, and
        // builds `/a.pdf` from the segments. Found by mutation, 2026-07-28.
        let url = tauri::Url::parse("https://localhost/a.pdf").expect("url");
        assert_eq!(path_from_url(&url), None);
    }

    #[test]
    fn a_path_arriving_before_the_frontend_is_queued() {
        let launch = Launch::default();
        assert_eq!(
            launch.deliver(PathBuf::from("/tmp/a.pdf")),
            Delivery::Queued
        );
        assert_eq!(launch.take(), vec![PathBuf::from("/tmp/a.pdf")]);
    }

    #[test]
    fn a_path_arriving_after_the_frontend_is_emitted() {
        let launch = Launch::default();
        assert!(launch.take().is_empty());
        assert_eq!(
            launch.deliver(PathBuf::from("/tmp/a.pdf")),
            Delivery::Emit(PathBuf::from("/tmp/a.pdf"))
        );
    }

    #[test]
    fn taking_twice_does_not_hand_the_same_document_over_again() {
        // A second window, or a frontend that reloads, must not reopen what the
        // first one already showed.
        let launch = Launch::default();
        launch.deliver(PathBuf::from("/tmp/a.pdf"));
        assert_eq!(launch.take().len(), 1);
        assert!(launch.take().is_empty());
    }

    #[test]
    fn everything_queued_before_the_handoff_survives_it() {
        let launch = Launch::default();
        launch.deliver(PathBuf::from("/tmp/a.pdf"));
        launch.deliver(PathBuf::from("/tmp/b.pdf"));
        assert_eq!(
            launch.take(),
            vec![PathBuf::from("/tmp/a.pdf"), PathBuf::from("/tmp/b.pdf")]
        );
    }
}
