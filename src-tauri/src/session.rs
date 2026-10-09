//! Where the reader was, remembered across launches.
//!
//! A reader that opens on an empty window every morning is not the default
//! reader, whatever else it does --- so this is a Phase 1 item rather than
//! polish. It stores one *place* per document, most-recently-read first, and
//! nothing else: no window geometry, no scroll history, no per-page state.
//!
//! Two decisions here are deliberate and worth stating, because both look like
//! omissions.
//!
//! **A malformed or unreadable file is an empty session, never an error.** The
//! session is a convenience; refusing to start because it could not be parsed
//! would trade the whole application for the feature. Every path through
//! [`Session::load`] returns a `Session`.
//!
//! **A field out of range is repaired, not rejected.** This is the opposite of
//! what `protocol.rs` does with a `turns` query parameter, and the difference is
//! the caller: a tile request is a live instruction from code we wrote, so a
//! value it could not have produced is a bug worth surfacing. A session file is
//! a record that has been sitting on disk across upgrades, crashes and possibly
//! a text editor --- refusing it would discard every *other* document's place
//! over one bad number. See [`Place::sanitized`].

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Documents remembered, oldest dropped first.
///
/// Bounded because the file is read synchronously during startup, where the
/// whole application budget is about 50 ms (docs/PLAN.md §3). Thirty-two entries
/// is far more than "the document I was reading" needs and still parses in
/// microseconds.
const CAPACITY: usize = 32;

/// Zoom bounds, matching `MIN_ZOOM`/`MAX_ZOOM` in `src/lib/zoom.ts`.
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 16.0;

/// What the zoom was following, if anything.
///
/// The wire spelling of `FitMode` in `src/lib/zoom.ts`, and it replaced a
/// `fitting: bool` when fit-page arrived --- a boolean cannot hold three
/// answers, and keeping it beside this one would be two records of one fact.
///
/// Nothing has shipped, so there is no session file in anyone's hands written
/// with the old field. One written by an earlier build of this repository loses
/// only the distinction between a fixed zoom and fit-width, and reopens fitted
/// to the window, which is what [`Place::sanitized`] already does with a zoom it
/// cannot read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// A zoom the reader set, which stays where they put it.
    None,
    /// The page fills the window's width. The default a document opens at.
    #[default]
    Width,
    /// The whole page is visible at once.
    Page,
}

/// Where one document was left.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// Absolute path, as it was opened.
    pub path: String,
    /// Zero-based page at the top of the viewport.
    #[serde(default)]
    pub page: u32,
    /// Points down that page, or 0 --- which is all a rotated view reports.
    #[serde(default)]
    pub top_pt: f32,
    /// CSS pixels per PDF point.
    #[serde(default = "unit_zoom")]
    pub zoom: f32,
    /// What the zoom was following, if anything.
    #[serde(default)]
    pub fit: Fit,
    /// Quarter-turns clockwise, 0 to 3.
    #[serde(default)]
    pub turns: u8,
    /// Whether the sidebar was showing.
    #[serde(default)]
    pub sidebar: bool,
    /// Pages the document had when this was written.
    ///
    /// Kept so a restore can tell "the file has been replaced by a shorter one"
    /// from "the page number was always this". The clamp itself happens in the
    /// frontend, which is the side that knows what the document has *now*.
    #[serde(default)]
    pub page_count: u32,
}

fn unit_zoom() -> f32 {
    1.0
}

impl Place {
    /// Forces every field into a range the viewer can act on.
    ///
    /// Applied on load rather than on save, because the file may have been
    /// written by a version that allowed something this one does not --- and
    /// because a file edited by hand is exactly the case that must not be able
    /// to wedge the viewer.
    ///
    /// A zoom that is not a usable number falls back to fit-width rather than to
    /// 1.0: an unreadable zoom means the size is unknown, and fitting the window
    /// is the honest answer to that, where 1.0 is a guess wearing a number.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.turns %= 4;
        if !self.zoom.is_finite() || self.zoom <= 0.0 {
            self.zoom = 1.0;
            self.fit = Fit::Width;
        }
        self.zoom = self.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if !self.top_pt.is_finite() || self.top_pt < 0.0 {
            self.top_pt = 0.0;
        }
        self
    }
}

/// Every document with a remembered place, most recent first.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub places: Vec<Place>,
    /// Whether pages are shown with their lightness inverted.
    ///
    /// A preference rather than a place: it belongs to the reader, not to a
    /// document, so it sits beside the list rather than inside each entry. A
    /// reader who inverts one file has said how they want to read, not how they
    /// want to read that file.
    #[serde(default)]
    pub invert_pages: bool,
    /// The language *Recognise text* asks the engine to expect, as a BCP-47
    /// tag, or `None` for the engine's own choice.
    ///
    /// A preference like [`Session::invert_pages`]: a reader whose scans are
    /// German has said so once, not once a document. Whether the machine still
    /// offers it is asked when text is recognised, not here --- this file can
    /// be carried to another computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ocr_language: Option<String>,
    /// The documents that were open as tabs, in tab order.
    ///
    /// Beside `places` rather than derived from it: that list is every document
    /// read lately, most recent first, and says nothing about which of them
    /// were still open or in what order. Written whether or not
    /// [`Session::restore_tabs`] is set, so that turning the preference on, or
    /// asking once for last time's tabs, has a list to act on.
    #[serde(default)]
    pub tabs: Vec<String>,
    /// The tab that was showing, when it is one of [`Session::tabs`].
    #[serde(default)]
    pub active_tab: Option<String>,
    /// Whether a launch reopens every tab rather than the last document alone.
    ///
    /// Off unless the reader turns it on. Each tab is an open document with a
    /// worker behind it, and a reader who left twenty open has not thereby
    /// asked for twenty to be opened every morning.
    #[serde(default)]
    pub restore_tabs: bool,
    /// The two sides, when the window was showing two documents side by side.
    ///
    /// Only what [`Session::tabs`] does not say: which of them were on the
    /// right, which was in front of the side the reader was not in, and where
    /// the divider stood. `None` for a window showing one document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sides: Option<Sides>,
}

/// The narrowest a side is drawn, as a share of the width. `App.svelte`'s
/// divider stops at the same two shares.
const MIN_SHARE: f64 = 0.2;

/// How the tabs were divided between the two sides of the window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sides {
    /// The tabs on the right, in tab order. Every other tab was on the left.
    #[serde(default)]
    pub right: Vec<String>,
    /// The tab in front of the side the reader was not working in.
    #[serde(default)]
    pub beside: Option<String>,
    /// The share of the width the left side had.
    #[serde(default = "even_share")]
    pub share: f64,
}

fn even_share() -> f64 {
    0.5
}

impl Session {
    /// Reads the session, treating every failure as "there isn't one".
    ///
    /// Missing, unreadable, malformed and empty are all the same answer to the
    /// only question the caller has.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = fs::read(path) else {
            return Self::default();
        };
        let Ok(session) = serde_json::from_slice::<Self>(&raw) else {
            return Self::default();
        };
        Self {
            places: session
                .places
                .into_iter()
                .map(Place::sanitized)
                .take(CAPACITY)
                .collect(),
            // Carried through explicitly. This rebuilds the struct rather than
            // repairing it in place, so a field added later and not named here
            // is silently reset to its default on every load --- which for a
            // preference reads as "it does not remember", with nothing failing.
            invert_pages: session.invert_pages,
            restore_tabs: session.restore_tabs,
            ..Self::default()
        }
        .with_tabs(session.tabs, session.active_tab, session.sides)
        .with_ocr_language(session.ocr_language)
    }

    /// Records the language to recognise text in, or `None` for the engine's
    /// own choice.
    ///
    /// Anything that is not shaped like a language tag is `None`: the file is
    /// on disk where anything can edit it, and what it holds goes to an engine.
    pub fn set_ocr_language(&mut self, language: Option<String>) {
        self.ocr_language = language.filter(|tag| crate::ocr_layer::is_language_tag(tag));
    }

    fn with_ocr_language(mut self, language: Option<String>) -> Self {
        self.set_ocr_language(language);
        self
    }

    /// Records which documents are open as tabs, which of them is showing, and
    /// how they are divided between two sides.
    ///
    /// A path is listed once, at its first position. A repeat is ordinary
    /// and is not a damaged record: since one document can be shown on both
    /// sides, the window has two tabs on one file then and sends its path
    /// twice. Listing it once is what makes the document come back as one tab
    /// at the next launch. The active tab is kept only when it is in the
    /// list, because a launch opens it first and it must be one of the tabs
    /// that come back.
    ///
    /// The sides are kept only when they describe a split of these tabs: some
    /// of them on the right and some not. The tab in front of the other side
    /// has to be on the side the active tab is not on, and the divider is put
    /// back inside the range it can be dragged over.
    pub fn set_tabs(&mut self, paths: Vec<String>, active: Option<String>, sides: Option<Sides>) {
        let mut kept: Vec<String> = Vec::new();
        for path in paths {
            if kept.len() == CAPACITY {
                break;
            }
            if !path.is_empty() && !kept.contains(&path) {
                kept.push(path);
            }
        }
        self.active_tab = active.filter(|path| kept.contains(path));
        self.sides = sides.and_then(|sides| {
            let right: Vec<String> = kept
                .iter()
                .filter(|path| sides.right.contains(path))
                .cloned()
                .collect();
            if right.is_empty() || right.len() == kept.len() {
                return None;
            }
            let active_right = self.active_tab.as_ref().map(|path| right.contains(path));
            let beside = sides.beside.filter(|path| {
                kept.contains(path)
                    && active_right.is_some_and(|there| there != right.contains(path))
            });
            let share = if sides.share.is_finite() {
                sides.share.clamp(MIN_SHARE, 1.0 - MIN_SHARE)
            } else {
                even_share()
            };
            Some(Sides {
                right,
                beside,
                share,
            })
        });
        self.tabs = kept;
    }

    fn with_tabs(
        mut self,
        paths: Vec<String>,
        active: Option<String>,
        sides: Option<Sides>,
    ) -> Self {
        self.set_tabs(paths, active, sides);
        self
    }

    /// The document to reopen, if there is one.
    #[must_use]
    pub fn most_recent(&self) -> Option<&Place> {
        self.places.first()
    }

    /// Records a place, moving its document to the front.
    ///
    /// Keyed on the path, so re-reading a document updates its entry rather than
    /// adding a second one --- without which every scroll would append and the
    /// bound would evict the other documents within seconds.
    pub fn remember(&mut self, place: Place) {
        let place = place.sanitized();
        self.places.retain(|kept| kept.path != place.path);
        self.places.insert(0, place);
        self.places.truncate(CAPACITY);
    }

    /// Forgets where one document was left, and that it was read at all.
    ///
    /// The places and nothing else. [`Session::tabs`] is a different list with a
    /// different owner: a document that is open as a tab stays one, and is
    /// remembered again by the next [`Session::remember`] its reader causes.
    ///
    /// A path with no place is not an error. Two windows can ask for the same
    /// row to go, and the second finds it gone, which is what it asked for.
    pub fn forget(&mut self, path: &str) {
        self.places.retain(|kept| kept.path != path);
    }

    /// Forgets every place.
    ///
    /// What the reader prefers and which documents are open are not places, so
    /// both are left as they were: clearing a list of documents must not turn
    /// the pages back to their own colours or stop the tabs coming back.
    pub fn clear_places(&mut self) {
        self.places.clear();
    }

    /// Writes the session, replacing any previous one atomically.
    ///
    /// Through a temporary file and a rename, so a crash or a full disk during
    /// the write leaves the *old* session in place rather than a truncated file
    /// that the next launch would read as empty.
    ///
    /// The rename is not followed by a directory fsync. Losing the last few
    /// seconds of position to a power cut is not worth an `F_FULLFSYNC` --- a
    /// device-wide barrier, about 3 ms (docs/PLAN.md §6) --- on a file this is
    /// written to whenever someone stops scrolling.
    ///
    /// Two processes writing concurrently is last-writer-wins. The rename keeps
    /// each write whole, so the loser's places are dropped, never interleaved.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temp = temp_beside(path);
        fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(&temp, path)
    }
}

/// What the window is handed at launch: the session, and where home is.
///
/// The home folder is a fact about this machine and not about the session, so
/// it is not a field of [`Session`] and never reaches the file. It travels with
/// the session because the one thing that reads it is the list of remembered
/// documents, which shortens a folder under it to `~`, and because a second
/// round trip at launch for one string is a cost on the path a reader watches.
///
/// Flattened, so the reply has the session's own keys beside `home` and
/// everything that already read a session reads this unchanged.
#[derive(Clone, Debug, Serialize)]
pub struct Loaded {
    #[serde(flatten)]
    pub session: Session,
    /// The reader's home folder, when the platform names one.
    pub home: Option<String>,
}

/// A scratch name in the same directory as `path`.
///
/// The same directory because `rename` is only atomic within one filesystem, and
/// a temp file in `/tmp` may well be on another. Built by appending rather than
/// by `with_extension`, which would replace `.json` instead of adding to it and
/// so could collide with a real session file.
fn temp_beside(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map_or_else(|| OsString::from("session.json"), OsString::from);
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::{Fit, Place, Session, Sides, CAPACITY};
    use std::path::PathBuf;

    use crate::testutil::TempDir;

    /// The session file inside a scratch directory.
    ///
    /// An extension rather than a method on [`TempDir`], for `diag.rs`'s
    /// reason: `session.json` is this module's name for it.
    trait SessionFile {
        fn file(&self) -> PathBuf;
    }

    impl SessionFile for TempDir {
        fn file(&self) -> PathBuf {
            self.join("session.json")
        }
    }

    fn place(path: &str) -> Place {
        Place {
            path: path.to_string(),
            page: 3,
            top_pt: 12.5,
            zoom: 2.0,
            fit: Fit::None,
            turns: 1,
            sidebar: true,
            page_count: 10,
        }
    }

    #[test]
    fn a_missing_file_is_an_empty_session() {
        let dir = TempDir::new("missing");
        assert!(Session::load(&dir.file()).places.is_empty());
    }

    #[test]
    fn a_malformed_file_is_an_empty_session() {
        let dir = TempDir::new("malformed");
        std::fs::write(dir.file(), b"{not json at all").expect("write");
        assert!(Session::load(&dir.file()).places.is_empty());
    }

    #[test]
    fn the_inversion_preference_survives_a_round_trip() {
        // `load` rebuilds the struct field by field rather than repairing it in
        // place, so a field it forgets to name comes back as its default --- and
        // a preference that resets every launch has nothing that fails, it just
        // does not work.
        let dir = TempDir::new("preference");
        let session = Session {
            places: vec![place("/tmp/a.pdf")],
            invert_pages: true,
            ..Session::default()
        };
        session.save(&dir.file()).expect("save");
        assert!(Session::load(&dir.file()).invert_pages);
    }

    #[test]
    fn the_language_to_recognise_text_in_survives_a_round_trip() {
        // The same hazard as the inversion: `load` names what it carries.
        let dir = TempDir::new("ocr-language");
        let mut session = Session::default();
        session.set_ocr_language(Some("de-DE".into()));
        session.save(&dir.file()).expect("save");
        assert_eq!(
            Session::load(&dir.file()).ocr_language.as_deref(),
            Some("de-DE")
        );

        session.set_ocr_language(None);
        session.save(&dir.file()).expect("save");
        assert_eq!(Session::load(&dir.file()).ocr_language, None);
        let written = std::fs::read_to_string(dir.file()).expect("read");
        assert!(!written.contains("ocr_language"), "{written}");
    }

    #[test]
    fn a_stored_language_that_is_not_a_tag_is_the_engine_s_own_choice() {
        // Set through the command, and read from a file somebody edited.
        let mut session = Session::default();
        session.set_ocr_language(Some("de DE; rm".into()));
        assert_eq!(session.ocr_language, None);

        let dir = TempDir::new("ocr-language-edited");
        std::fs::write(dir.file(), br#"{"ocr_language":"../../etc"}"#).expect("write");
        assert_eq!(Session::load(&dir.file()).ocr_language, None);
    }

    fn paths(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn the_open_tabs_and_the_choice_to_reopen_them_survive_a_round_trip() {
        // `load` names every field it carries, so each of these three comes
        // back as its default if it is left out there.
        let dir = TempDir::new("tabs");
        let mut session = Session {
            restore_tabs: true,
            ..Session::default()
        };
        session.set_tabs(
            paths(&["/tmp/b.pdf", "/tmp/a.pdf", "/tmp/c.pdf"]),
            Some("/tmp/a.pdf".to_string()),
            None,
        );
        session.save(&dir.file()).expect("save");

        let loaded = Session::load(&dir.file());
        assert_eq!(
            loaded.tabs,
            paths(&["/tmp/b.pdf", "/tmp/a.pdf", "/tmp/c.pdf"]),
            "tab order is the reader's, not most-recent-first"
        );
        assert_eq!(loaded.active_tab.as_deref(), Some("/tmp/a.pdf"));
        assert!(loaded.restore_tabs);
    }

    #[test]
    fn reopening_every_tab_is_off_until_it_is_asked_for() {
        let dir = TempDir::new("tabs-default");
        std::fs::write(dir.file(), br#"{"places":[]}"#).expect("write");
        let loaded = Session::load(&dir.file());
        assert!(!loaded.restore_tabs);
        assert!(loaded.tabs.is_empty());
        assert_eq!(loaded.active_tab, None);
    }

    fn sides(right: &[&str], beside: Option<&str>, share: f64) -> Option<Sides> {
        Some(Sides {
            right: paths(right),
            beside: beside.map(str::to_string),
            share,
        })
    }

    #[test]
    fn the_two_sides_survive_a_round_trip() {
        let dir = TempDir::new("sides");
        let mut session = Session::default();
        session.set_tabs(
            paths(&["/tmp/a.pdf", "/tmp/b.pdf", "/tmp/c.pdf"]),
            Some("/tmp/a.pdf".to_string()),
            sides(&["/tmp/c.pdf", "/tmp/b.pdf"], Some("/tmp/c.pdf"), 0.3),
        );
        session.save(&dir.file()).expect("save");

        let loaded = Session::load(&dir.file());
        assert_eq!(
            loaded.sides,
            sides(&["/tmp/b.pdf", "/tmp/c.pdf"], Some("/tmp/c.pdf"), 0.3),
            "the right side is listed in tab order"
        );
    }

    #[test]
    fn sides_that_do_not_divide_the_tabs_are_no_split() {
        let open = paths(&["/tmp/a.pdf", "/tmp/b.pdf"]);
        let front = Some("/tmp/a.pdf".to_string());
        let mut session = Session::default();

        session.set_tabs(
            open.clone(),
            front.clone(),
            sides(&["/tmp/gone.pdf"], None, 0.5),
        );
        assert_eq!(session.sides, None, "nothing open is on the right");

        session.set_tabs(
            open.clone(),
            front.clone(),
            sides(&["/tmp/a.pdf", "/tmp/b.pdf"], None, 0.5),
        );
        assert_eq!(session.sides, None, "nothing is left on the left");

        session.set_tabs(
            open.clone(),
            front.clone(),
            sides(&["/tmp/b.pdf"], None, 0.5),
        );
        assert!(session.sides.is_some());
        session.set_tabs(open, front, None);
        assert_eq!(session.sides, None, "the split has ended");
    }

    #[test]
    fn the_tab_beside_is_on_the_other_side_and_the_divider_within_reach() {
        let open = paths(&["/tmp/a.pdf", "/tmp/b.pdf", "/tmp/c.pdf"]);
        let front = Some("/tmp/a.pdf".to_string());
        let mut session = Session::default();
        let mut beside_after = |beside: &str, active: Option<String>| {
            session.set_tabs(
                open.clone(),
                active,
                sides(&["/tmp/c.pdf"], Some(beside), 0.5),
            );
            session.sides.clone().expect("a split").beside
        };
        assert_eq!(
            beside_after("/tmp/c.pdf", front.clone()).as_deref(),
            Some("/tmp/c.pdf")
        );
        assert_eq!(
            beside_after("/tmp/b.pdf", front.clone()),
            None,
            "on the active tab's own side"
        );
        assert_eq!(
            beside_after("/tmp/gone.pdf", Some("/tmp/c.pdf".to_string())),
            None,
            "not open, and not on the left for that"
        );
        assert_eq!(
            beside_after("/tmp/gone.pdf", front.clone()),
            None,
            "not open"
        );
        assert_eq!(beside_after("/tmp/c.pdf", None), None, "beside nothing");

        let mut share_after = |share: f64| {
            session.set_tabs(
                open.clone(),
                front.clone(),
                sides(&["/tmp/c.pdf"], None, share),
            );
            session.sides.clone().expect("a split").share
        };
        assert!((share_after(0.05) - 0.2).abs() < 1e-9);
        assert!((share_after(7.0) - 0.8).abs() < 1e-9);
        assert!((share_after(f64::NAN) - 0.5).abs() < 1e-9);
        assert!((share_after(0.35) - 0.35).abs() < 1e-9);

        // A file that names the sides and no share reads as even halves.
        let dir = TempDir::new("sides-no-share");
        let raw = serde_json::json!({
            "tabs": ["/tmp/a.pdf", "/tmp/b.pdf"], "active_tab": "/tmp/a.pdf",
            "sides": { "right": ["/tmp/b.pdf"] },
        });
        std::fs::write(dir.file(), raw.to_string()).expect("write");
        assert_eq!(
            Session::load(&dir.file()).sides,
            sides(&["/tmp/b.pdf"], None, 0.5)
        );
    }

    #[test]
    fn a_tab_is_listed_once_and_the_active_one_is_among_them() {
        let mut session = Session::default();
        session.set_tabs(
            paths(&["/tmp/a.pdf", "", "/tmp/b.pdf", "/tmp/a.pdf"]),
            Some("/tmp/gone.pdf".to_string()),
            None,
        );
        assert_eq!(session.tabs, paths(&["/tmp/a.pdf", "/tmp/b.pdf"]));
        assert_eq!(
            session.active_tab, None,
            "a tab that is not open cannot be showing"
        );

        session.set_tabs(paths(&["/tmp/b.pdf"]), Some("/tmp/b.pdf".to_string()), None);
        assert_eq!(session.active_tab.as_deref(), Some("/tmp/b.pdf"));

        // Closing every tab leaves nothing to reopen.
        session.set_tabs(Vec::new(), None, None);
        assert!(session.tabs.is_empty());
    }

    #[test]
    fn the_tab_list_is_bounded_on_the_way_in_and_on_the_way_back() {
        let many: Vec<String> = (0..CAPACITY + 5).map(|n| format!("/tmp/{n}.pdf")).collect();
        let mut session = Session::default();
        session.set_tabs(many.clone(), None, None);
        assert_eq!(session.tabs.len(), CAPACITY);
        assert_eq!(
            session.tabs[0], "/tmp/0.pdf",
            "the first tabs are the ones kept"
        );

        // A file written by hand, or by a build with a larger bound.
        let dir = TempDir::new("tabs-bound");
        let raw = serde_json::json!({ "places": [], "tabs": many, "active_tab": "/tmp/36.pdf" });
        std::fs::write(dir.file(), raw.to_string()).expect("write");
        let loaded = Session::load(&dir.file());
        assert_eq!(loaded.tabs.len(), CAPACITY);
        assert_eq!(
            loaded.active_tab, None,
            "the active tab fell outside the bound"
        );
    }

    #[test]
    fn remembering_a_place_leaves_the_tabs_alone() {
        let mut session = Session::default();
        session.set_tabs(
            paths(&["/tmp/a.pdf", "/tmp/b.pdf"]),
            Some("/tmp/a.pdf".to_string()),
            None,
        );
        session.remember(place("/tmp/b.pdf"));
        assert_eq!(session.tabs, paths(&["/tmp/a.pdf", "/tmp/b.pdf"]));
        assert_eq!(session.active_tab.as_deref(), Some("/tmp/a.pdf"));
    }

    #[test]
    fn forgetting_a_document_removes_its_place_and_no_other() {
        let mut session = Session::default();
        session.remember(place("/Users/reader/Documents/report.pdf"));
        session.remember(place("/Users/reader/Documents/notes.pdf"));
        session.remember(place("C:\\Users\\reader\\spec.pdf"));

        session.forget("/Users/reader/Documents/notes.pdf");

        let kept: Vec<&str> = session.places.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(
            kept,
            [
                "C:\\Users\\reader\\spec.pdf",
                "/Users/reader/Documents/report.pdf"
            ],
            "the other two, in the order they had"
        );
    }

    #[test]
    fn forgetting_a_document_that_is_not_remembered_changes_nothing() {
        let mut session = Session::default();
        session.remember(place("/Users/reader/Documents/report.pdf"));
        let before = session.places.clone();

        session.forget("/Users/reader/Documents/absent.pdf");
        // A path that merely begins the same way is another document.
        session.forget("/Users/reader/Documents/report");

        assert_eq!(session.places, before);
    }

    #[test]
    fn forgetting_a_document_leaves_it_open_as_a_tab() {
        let mut session = Session::default();
        session.remember(place("/Users/reader/Documents/report.pdf"));
        session.set_tabs(
            paths(&["/Users/reader/Documents/report.pdf"]),
            Some("/Users/reader/Documents/report.pdf".into()),
            None,
        );

        session.forget("/Users/reader/Documents/report.pdf");

        assert!(session.places.is_empty());
        assert_eq!(session.tabs, paths(&["/Users/reader/Documents/report.pdf"]));
        assert_eq!(
            session.active_tab.as_deref(),
            Some("/Users/reader/Documents/report.pdf")
        );
    }

    #[test]
    fn clearing_the_places_keeps_the_preferences_and_the_tabs() {
        // Through the file, because that is where a cleared session has to
        // still say what the reader prefers: the next launch reads it there.
        let dir = TempDir::new("clear");
        let mut session = Session {
            invert_pages: true,
            restore_tabs: true,
            ..Session::default()
        };
        session.remember(place("/Users/reader/Documents/report.pdf"));
        session.remember(place("/Users/reader/Documents/notes.pdf"));
        let open = paths(&[
            "/Users/reader/Documents/notes.pdf",
            "/Users/reader/Documents/report.pdf",
        ]);
        session.set_tabs(
            open.clone(),
            Some("/Users/reader/Documents/report.pdf".into()),
            None,
        );

        session.clear_places();
        session.save(&dir.file()).expect("save");
        let loaded = Session::load(&dir.file());

        assert!(loaded.places.is_empty(), "every place is gone");
        assert!(loaded.invert_pages, "the inversion is the reader's");
        assert!(loaded.restore_tabs, "so is the choice to reopen tabs");
        assert_eq!(loaded.tabs, open);
        assert_eq!(
            loaded.active_tab.as_deref(),
            Some("/Users/reader/Documents/report.pdf")
        );
    }

    #[test]
    fn the_launch_reply_carries_home_beside_the_session_s_own_keys() {
        // Flattened: a reader of the old reply finds every key where it was.
        let mut session = Session::default();
        session.remember(place("/Users/reader/Documents/report.pdf"));
        let loaded = super::Loaded {
            session,
            home: Some("/Users/reader".into()),
        };

        let wire = serde_json::to_value(&loaded).expect("serialize");

        assert_eq!(wire["home"], "/Users/reader");
        assert_eq!(
            wire["places"][0]["path"],
            "/Users/reader/Documents/report.pdf"
        );
        assert!(wire.get("session").is_none(), "not nested");
        // And the file does not learn it: a session saved is a session only.
        let saved = serde_json::to_value(&loaded.session).expect("serialize");
        assert!(saved.get("home").is_none());
    }

    #[test]
    fn a_session_written_before_the_preference_existed_still_loads() {
        // The field is absent from every file written before today, and the
        // whole session must not be discarded over that.
        let dir = TempDir::new("older");
        std::fs::write(
            dir.file(),
            br#"{"places":[{"path":"/tmp/a.pdf","page":4,"zoom":1.0}]}"#,
        )
        .expect("write");
        let loaded = Session::load(&dir.file());
        assert_eq!(loaded.places.len(), 1);
        assert!(!loaded.invert_pages);
    }

    #[test]
    fn a_place_survives_a_round_trip() {
        let dir = TempDir::new("roundtrip");
        let mut session = Session::default();
        session.remember(place("/tmp/a.pdf"));
        session.save(&dir.file()).expect("save");

        let loaded = Session::load(&dir.file());
        assert_eq!(loaded.places, vec![place("/tmp/a.pdf")]);
    }

    #[test]
    fn a_fit_is_written_with_the_spelling_the_frontend_reads() {
        // The one field here whose two ends are in different languages, so
        // nothing but this asserts they agree: `FitMode` in `src/lib/zoom.ts` is
        // a union of these three strings, and a `rename_all` that produced
        // `"Width"` would deserialize on this side and be an unknown mode on
        // that one --- where TypeScript cannot see it either, since a value off
        // the IPC is whatever the annotation claims.
        for (fit, spelling) in [
            (Fit::None, "none"),
            (Fit::Width, "width"),
            (Fit::Page, "page"),
        ] {
            let json = serde_json::to_string(&fit).expect("serialize");
            assert_eq!(json, format!("\"{spelling}\""));
            let back: Fit = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, fit);
        }
    }

    #[test]
    fn the_most_recent_document_is_the_one_remembered_last() {
        let mut session = Session::default();
        session.remember(place("/tmp/a.pdf"));
        session.remember(place("/tmp/b.pdf"));
        assert_eq!(
            session.most_recent().map(|p| p.path.as_str()),
            Some("/tmp/b.pdf")
        );
    }

    #[test]
    fn remembering_a_document_again_moves_it_without_growing_the_list() {
        let mut session = Session::default();
        session.remember(place("/tmp/a.pdf"));
        session.remember(place("/tmp/b.pdf"));

        let mut moved = place("/tmp/a.pdf");
        moved.page = 99;
        session.remember(moved);

        assert_eq!(session.places.len(), 2, "a re-read must update, not append");
        assert_eq!(
            session.most_recent().map(|p| p.path.as_str()),
            Some("/tmp/a.pdf")
        );
        assert_eq!(session.most_recent().map(|p| p.page), Some(99));
    }

    #[test]
    fn the_list_is_bounded_and_drops_the_oldest() {
        let mut session = Session::default();
        for n in 0..CAPACITY + 5 {
            session.remember(place(&format!("/tmp/{n}.pdf")));
        }
        assert_eq!(session.places.len(), CAPACITY);
        assert!(
            !session.places.iter().any(|p| p.path == "/tmp/0.pdf"),
            "the first document read should have been evicted"
        );
        assert_eq!(
            session.most_recent().map(|p| p.path.as_str()),
            Some(format!("/tmp/{}.pdf", CAPACITY + 4).as_str())
        );
    }

    #[test]
    fn a_zoom_that_is_not_a_number_falls_back_to_fitting() {
        let mut broken = place("/tmp/a.pdf");
        broken.zoom = 0.0;
        broken.fit = Fit::None;

        let fixed = broken.sanitized();
        assert_eq!(
            fixed.fit,
            Fit::Width,
            "an unusable zoom means the size is unknown"
        );
        assert!(fixed.zoom > 0.0);
    }

    #[test]
    fn an_absurd_zoom_is_clamped_rather_than_discarded() {
        let mut wild = place("/tmp/a.pdf");
        wild.zoom = 5000.0;
        assert_eq!(wild.sanitized().zoom, super::MAX_ZOOM);
    }

    #[test]
    fn a_turn_out_of_range_is_reduced_not_refused() {
        let mut spun = place("/tmp/a.pdf");
        spun.turns = 7;
        // Unlike `protocol.rs`, which refuses one -- see the module comment.
        assert_eq!(spun.sanitized().turns, 3);
    }

    #[test]
    fn a_negative_offset_becomes_the_top_of_the_page() {
        let mut above = place("/tmp/a.pdf");
        above.top_pt = -40.0;
        assert_eq!(above.sanitized().top_pt, 0.0);
    }

    #[test]
    fn a_field_a_newer_version_wrote_is_ignored() {
        let dir = TempDir::new("forward");
        std::fs::write(
            dir.file(),
            br#"{"places":[{"path":"/tmp/a.pdf","page":4,"annotations":["future"]}],"mood":"cheerful"}"#,
        )
        .expect("write");

        let loaded = Session::load(&dir.file());
        assert_eq!(loaded.places.len(), 1);
        assert_eq!(loaded.places[0].page, 4);
    }

    #[test]
    fn a_field_an_older_version_omitted_takes_its_default() {
        let dir = TempDir::new("backward");
        std::fs::write(dir.file(), br#"{"places":[{"path":"/tmp/a.pdf"}]}"#).expect("write");

        let loaded = Session::load(&dir.file());
        let only = &loaded.places[0];
        assert_eq!(only.page, 0);
        assert_eq!(
            only.fit,
            Fit::Width,
            "a place with no zoom recorded should fit the window"
        );
        assert_eq!(only.zoom, 1.0);
    }

    #[test]
    fn a_file_longer_than_the_bound_is_truncated_on_load() {
        let dir = TempDir::new("overlong");
        let places: Vec<String> = (0..CAPACITY + 9)
            .map(|n| format!(r#"{{"path":"/tmp/{n}.pdf"}}"#))
            .collect();
        std::fs::write(
            dir.file(),
            format!(r#"{{"places":[{}]}}"#, places.join(",")).as_bytes(),
        )
        .expect("write");

        assert_eq!(Session::load(&dir.file()).places.len(), CAPACITY);
    }

    #[test]
    fn saving_goes_through_the_scratch_file_and_consumes_it() {
        // What pins the write to `rename`. A save that wrote the target
        // directly would satisfy every other test here -- it produces the right
        // bytes and leaves no scratch file *it* created. So the scratch file is
        // planted first: only a save that renames over it removes it.
        let dir = TempDir::new("atomic");
        let scratch = super::temp_beside(&dir.file());
        std::fs::write(&scratch, b"left over from a write that died").expect("plant");

        let mut session = Session::default();
        session.remember(place("/tmp/a.pdf"));
        session.save(&dir.file()).expect("save");

        assert!(
            !scratch.exists(),
            "a save that does not rename leaves the scratch file where it was"
        );
        assert_eq!(Session::load(&dir.file()).places.len(), 1);
    }

    #[test]
    fn saving_leaves_no_scratch_file_behind() {
        let dir = TempDir::new("scratch");
        let mut session = Session::default();
        session.remember(place("/tmp/a.pdf"));
        session.save(&dir.file()).expect("save");

        assert_eq!(dir.names(), vec!["session.json".to_string()]);
    }

    #[test]
    fn saving_over_a_previous_session_replaces_it_whole() {
        let dir = TempDir::new("replace");
        let mut first = Session::default();
        first.remember(place("/tmp/a.pdf"));
        first.remember(place("/tmp/b.pdf"));
        first.save(&dir.file()).expect("save");

        let mut second = Session::default();
        second.remember(place("/tmp/c.pdf"));
        second.save(&dir.file()).expect("save");

        let loaded = Session::load(&dir.file());
        assert_eq!(loaded.places.len(), 1, "a stale place must not survive");
        assert_eq!(
            loaded.most_recent().map(|p| p.path.as_str()),
            Some("/tmp/c.pdf")
        );
    }

    #[test]
    fn the_scratch_file_sits_beside_the_target() {
        let target = std::path::Path::new("/some/where/session.json");
        assert_eq!(
            super::temp_beside(target),
            std::path::Path::new("/some/where/session.json.tmp"),
            "a rename is only atomic within one filesystem"
        );
    }
}
