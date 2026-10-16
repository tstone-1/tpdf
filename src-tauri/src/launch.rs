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
//!
//! A command line can also say where to show the document: `--page 12`,
//! `--zoom fit-width` and `--forward-search file.tex:LINE` (`Place`). They travel with each path through the same
//! queue, and the window checks each value against the document, because only
//! the window knows how many pages it has and what a zoom may be.

use std::path::PathBuf;

use parking_lot::Mutex;

/// The event a path is delivered on once the frontend is listening.
pub const OPEN_EVENT: &str = "tpdf://open";

/// Where a command line asked for a document to be shown, as typed.
///
/// The values are not checked here. A page number means nothing until the
/// document says how many pages it has, and the zoom's rules are the window's
/// (`launchplace.ts`); a value refused there is reported in the window, which
/// is the only place a reader who started tpdf from an editor can see it. An
/// option given with no value is `Some("")`, so that it is reported rather than
/// dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Place {
    pub page: Option<String>,
    pub zoom: Option<String>,
    /// `file.tex:LINE`, the file made absolute against the folder the launch
    /// was made in (`resolved`).
    pub forward: Option<String>,
}

impl Place {
    /// Whether the command line asked for anything.
    pub fn is_empty(&self) -> bool {
        self.page.is_none() && self.zoom.is_none() && self.forward.is_none()
    }

    /// The place with `--forward-search`'s file joined to `cwd` when it is
    /// relative. An editor names the source as it sees it, and a second
    /// launch's folder is not the running window's. Split at the last colon,
    /// so a Windows path keeps its drive letter; a value with no colon is left
    /// for the window to refuse.
    #[must_use]
    pub fn resolved(mut self, cwd: &std::path::Path) -> Place {
        if let Some(value) = &self.forward {
            if let Some((source, line)) = value.rsplit_once(':') {
                let path = std::path::Path::new(source);
                if !source.is_empty() && path.is_relative() {
                    self.forward = Some(format!("{}:{line}", cwd.join(path).display()));
                }
            }
        }
        self
    }
}

/// A document handed over from outside, and where to show it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Opening {
    #[serde(serialize_with = "lossy")]
    pub path: PathBuf,
    #[serde(flatten)]
    pub place: Place,
}

impl From<PathBuf> for Opening {
    fn from(path: PathBuf) -> Self {
        Opening {
            path,
            place: Place::default(),
        }
    }
}

/// A path as the window takes it: a string, with any byte that is not UTF-8
/// replaced, as every other route into the window has always sent it.
fn lossy<S: serde::Serializer>(path: &std::path::Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&path.to_string_lossy())
}

/// Paths waiting for a frontend, and whether one has arrived.
#[derive(Default)]
struct State {
    queued: Vec<Opening>,
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
    Emit(Opening),
    /// Nothing can show it yet; it is queued.
    Queued,
}

impl Launch {
    /// Records a path, saying whether the caller should emit it.
    ///
    /// The decision is returned rather than acted on so that this module needs
    /// no `AppHandle` and can be tested without a running application.
    pub fn deliver(&self, opening: impl Into<Opening>) -> Delivery {
        let opening = opening.into();
        let mut state = self.state.lock();
        if state.listening {
            Delivery::Emit(opening)
        } else {
            state.queued.push(opening);
            Delivery::Queued
        }
    }

    /// Hands over everything queued, and starts emitting from now on.
    ///
    /// One lock for both halves. Draining and then separately setting the flag
    /// leaves a window in which a path is queued into a vector nobody will read
    /// again --- the same lost document as before, only harder to reproduce.
    pub fn take(&self) -> Vec<Opening> {
        let mut state = self.state.lock();
        state.listening = true;
        std::mem::take(&mut state.queued)
    }
}

/// The options that say where to show a document, each taking one value.
const PLACE_OPTIONS: [&str; 3] = ["--page", "--zoom", "--forward-search"];

/// Takes [`PLACE_OPTIONS`] and their values out of a command line, leaving the
/// rest, `argv[0]` included, in order.
///
/// Before anything looks for paths, because the value is a separate argument:
/// left in, `--page 12` would open a document called `12`. Both `--page 12` and
/// `--page=12` are read. An option repeated keeps its last value, as a later
/// argument usually overrides an earlier one. An option with nothing after it
/// is `Some("")`. Any other argument beginning with `-` is left for
/// [`paths_from_args`] to skip, as it always was.
pub fn split_place<I, S>(args: I) -> (Vec<String>, Place)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut rest = Vec::new();
    let mut place = Place::default();
    let mut args = args.into_iter().map(|arg| arg.as_ref().to_string());
    if let Some(first) = args.next() {
        rest.push(first);
    }
    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name.to_string(), Some(value.to_string())),
            None => (arg.clone(), None),
        };
        if !PLACE_OPTIONS.contains(&name.as_str()) {
            rest.push(arg);
            continue;
        }
        let value = inline.or_else(|| args.next()).unwrap_or_default();
        match name.as_str() {
            "--page" => place.page = Some(value),
            "--zoom" => place.zoom = Some(value),
            _ => place.forward = Some(value),
        }
    }
    (rest, place)
}

/// [`paths_from_launch`] with the command line's place given to each path.
pub fn openings_from_launch<I, S>(
    args: I,
    cwd: &std::path::Path,
    is_file: impl Fn(&std::path::Path) -> bool,
) -> Vec<Opening>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let (rest, place) = split_place(args);
    let place = place.resolved(cwd);
    paths_from_launch(rest, is_file)
        .into_iter()
        .map(|path| Opening {
            path,
            place: place.clone(),
        })
        .collect()
}

/// [`paths_from_second_launch`] with the command line's place given to each path.
pub fn openings_from_second_launch<I, S>(
    args: I,
    cwd: &str,
    is_file: impl Fn(&std::path::Path) -> bool,
) -> Vec<Opening>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let (rest, place) = split_place(args);
    let place = place.resolved(std::path::Path::new(cwd));
    paths_from_second_launch(rest, cwd, is_file)
        .into_iter()
        .map(|path| Opening {
            path,
            place: place.clone(),
        })
        .collect()
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

/// [`paths_from_launch`] for a second launch's command line, which the
/// single-instance plugin hands over with the second process's working folder.
///
/// A path typed in a terminal (`tpdf report.pdf`) is relative to the folder
/// the second process started in, not to this one's, so each candidate is
/// tested and returned joined to `cwd`. A file manager and Explorer pass
/// absolute paths, which `join` leaves as they are.
pub fn paths_from_second_launch<I, S>(
    args: I,
    cwd: &str,
    is_file: impl Fn(&std::path::Path) -> bool,
) -> Vec<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let cwd = std::path::Path::new(cwd);
    paths_from_launch(args, |path| is_file(&cwd.join(path)))
        .into_iter()
        .map(|path| cwd.join(path))
        .collect()
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
    use super::{
        openings_from_launch, openings_from_second_launch, path_from_url, paths_from_args,
        paths_from_launch, paths_from_second_launch, split_place, Delivery, Launch, Opening, Place,
    };
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
        #[cfg(unix)]
        let (text, want) = ("file:///Users/x/my%20report.pdf", "/Users/x/my report.pdf");

        let url = tauri::Url::parse(text).expect("url");
        assert_eq!(path_from_url(&url), Some(PathBuf::from(want)));
    }

    #[test]
    fn a_second_launch_resolves_its_paths_in_its_own_folder() {
        #[cfg(windows)]
        let (cwd, absolute) = (r"C:\work", r"D:\docs\b.pdf");
        #[cfg(unix)]
        let (cwd, absolute) = ("/work", "/docs/b.pdf");
        let here = std::path::Path::new(cwd).join("a.pdf");
        let spaced = std::path::Path::new(cwd).join("my report.pdf");
        let exists = |path: &std::path::Path| {
            path == here || path == spaced || path == std::path::Path::new(absolute)
        };
        // A relative name is the second process's file, not this one's.
        assert_eq!(
            paths_from_second_launch(["tpdf", "a.pdf"], cwd, exists),
            vec![here.clone()]
        );
        // An absolute path is left as it is.
        assert_eq!(
            paths_from_second_launch(["tpdf", absolute], cwd, exists),
            vec![PathBuf::from(absolute)]
        );
        // A name split at its space is put back together by testing it in the
        // second process's folder, where it is.
        assert_eq!(
            paths_from_second_launch(["tpdf", "my", "report.pdf"], cwd, exists),
            vec![spaced.clone()]
        );
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
        assert_eq!(
            launch.take(),
            vec![Opening::from(PathBuf::from("/tmp/a.pdf"))]
        );
    }

    #[test]
    fn a_path_arriving_after_the_frontend_is_emitted() {
        let launch = Launch::default();
        assert!(launch.take().is_empty());
        assert_eq!(
            launch.deliver(PathBuf::from("/tmp/a.pdf")),
            Delivery::Emit(Opening::from(PathBuf::from("/tmp/a.pdf")))
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
            vec![
                Opening::from(PathBuf::from("/tmp/a.pdf")),
                Opening::from(PathBuf::from("/tmp/b.pdf"))
            ]
        );
    }

    fn place(page: Option<&str>, zoom: Option<&str>) -> Place {
        Place {
            page: page.map(str::to_string),
            zoom: zoom.map(str::to_string),
            forward: None,
        }
    }

    #[test]
    fn a_page_number_is_not_a_document() {
        let openings =
            openings_from_launch(["tpdf", "a.pdf", "--page", "12"], Path::new("/"), |_| false);
        assert_eq!(
            openings,
            vec![Opening {
                path: PathBuf::from("a.pdf"),
                place: place(Some("12"), None)
            }]
        );
    }

    #[test]
    fn both_spellings_of_an_option_are_read_and_the_last_one_wins() {
        let (rest, read) = split_place([
            "tpdf",
            "--zoom=fit-width",
            "--page",
            "2",
            "a.pdf",
            "--zoom",
            "150",
        ]);
        assert_eq!(rest, vec!["tpdf", "a.pdf"]);
        assert_eq!(read, place(Some("2"), Some("150")));
    }

    #[test]
    fn an_option_with_nothing_after_it_is_kept_to_be_reported() {
        let (rest, read) = split_place(["tpdf", "a.pdf", "--page"]);
        assert_eq!(rest, vec!["tpdf", "a.pdf"]);
        assert_eq!(read, place(Some(""), None));
        assert!(!read.is_empty());
        assert!(split_place(["tpdf", "a.pdf", "--other", "x"]).1.is_empty());
    }

    #[test]
    fn the_executable_is_never_taken_as_an_option() {
        // `argv[0]` is skipped by name, so a binary renamed `--page` still
        // leaves the first argument as a document.
        let (rest, read) = split_place(["--page", "a.pdf"]);
        assert_eq!(rest, vec!["--page", "a.pdf"]);
        assert!(read.is_empty());
    }

    #[test]
    fn a_split_path_is_put_back_together_around_the_options() {
        let whole = PathBuf::from("Team Site - Documents/x.pdf");
        let openings = openings_from_launch(
            [
                "tpdf",
                "--page",
                "3",
                "Team",
                "Site",
                "-",
                "Documents/x.pdf",
            ],
            Path::new("/"),
            |path| path == whole,
        );
        assert_eq!(
            openings,
            vec![Opening {
                path: whole.clone(),
                place: place(Some("3"), None)
            }]
        );
    }

    #[test]
    fn a_second_launch_carries_its_place() {
        #[cfg(windows)]
        let cwd = r"C:\work";
        #[cfg(unix)]
        let cwd = "/work";
        let here = std::path::Path::new(cwd).join("a.pdf");
        let openings =
            openings_from_second_launch(["tpdf", "a.pdf", "--zoom", "fit-page"], cwd, |path| {
                path == here
            });
        assert_eq!(
            openings,
            vec![Opening {
                path: here.clone(),
                place: place(None, Some("fit-page"))
            }]
        );
    }

    #[test]
    fn an_opening_reaches_the_window_as_a_path_and_two_values() {
        let opening = Opening {
            path: PathBuf::from("/tmp/a.pdf"),
            place: place(Some("4"), None),
        };
        assert_eq!(
            serde_json::to_value(&opening).expect("json"),
            serde_json::json!({ "path": "/tmp/a.pdf", "page": "4", "zoom": null, "forward": null })
        );
    }

    #[test]
    fn a_forward_search_source_is_made_absolute_where_the_launch_was_made() {
        let forward = |value: &str| Place {
            forward: Some(value.to_string()),
            ..Place::default()
        };
        #[cfg(unix)]
        let (cwd, absolute) = ("/work", "/src/a.tex:12");
        #[cfg(windows)]
        let (cwd, absolute) = (r"C:\work", r"D:\src\a.tex:12");
        let joined = forward("ch/a.tex:12").resolved(Path::new(cwd));
        assert_eq!(
            joined.forward,
            Some(format!("{}:12", Path::new(cwd).join("ch/a.tex").display()))
        );
        assert_eq!(
            forward(absolute)
                .resolved(Path::new(cwd))
                .forward
                .as_deref(),
            Some(absolute)
        );
        // No colon, or nothing before it: left for the window to refuse.
        assert_eq!(
            forward("a.tex").resolved(Path::new(cwd)).forward.as_deref(),
            Some("a.tex")
        );
        assert_eq!(
            forward(":12").resolved(Path::new(cwd)).forward.as_deref(),
            Some(":12")
        );
        let (_, read) = split_place(["tpdf", "a.pdf", "--forward-search", "a.tex:3"]);
        assert_eq!(read.forward.as_deref(), Some("a.tex:3"));
        assert!(!read.is_empty());
    }

    #[test]
    fn a_second_launch_resolves_the_source_in_its_own_folder() {
        #[cfg(unix)]
        let cwd = "/work";
        #[cfg(windows)]
        let cwd = r"C:\work";
        let openings = openings_from_second_launch(
            ["tpdf", "a.pdf", "--forward-search", "a.tex:7"],
            cwd,
            |_| false,
        );
        assert_eq!(
            openings[0].place.forward,
            Some(format!("{}:7", Path::new(cwd).join("a.tex").display()))
        );
    }
}
