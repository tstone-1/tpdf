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
        }
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
    use super::{Fit, Place, Session, CAPACITY};
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
        };
        session.save(&dir.file()).expect("save");
        assert!(Session::load(&dir.file()).invert_pages);
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
