//! Helpers shared by this crate's own tests, compiled only under `cfg(test)`.
//!
//! One thing lives here so far: [`TempDir`]. It existed four times --- in
//! `diag.rs`, `print.rs`, `session.rs` and `worker_shm.rs` --- with the same
//! shape each time (create under the system temp directory, remove the tree on
//! drop) and three different naming rules, one of which is a real difference
//! rather than a stylistic one.
//!
//! **Two of the four omitted the process id**, so two test binaries running at
//! once shared a directory and each removed the other's. `cargo test` runs one
//! binary at a time per target, which is why nothing had gone wrong; a second
//! target's tests, or two checkouts building side by side, is all it would take.
//! The shared version always includes the pid, which is what `print.rs`'s copy
//! already did.
//!
//! What each module keeps is its own convenience accessor --- the file inside
//! the directory is `tpdf.log` in one and `session.json` in another, and those
//! names belong to the modules that mean them, not here. Same split
//! `rowline.ts` states for its fallback string on the frontend.

use std::path::PathBuf;

/// A directory that removes itself, so a failing test cannot leave litter.
///
/// The name is `tpdf-<tag>-<pid>` under the system temp directory. `tag` should
/// name the module and the case, so a directory left behind by a hard-killed
/// run says what it came from.
pub struct TempDir(PathBuf);

impl TempDir {
    /// Creates an empty directory, removing anything already at that name.
    ///
    /// Removing first rather than failing: a previous run killed hard enough to
    /// skip its `Drop` leaves the tree behind, and a test that then refuses to
    /// start reports a stale directory as a broken test.
    ///
    /// # Panics
    ///
    /// The directory cannot be created.
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("tpdf-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        Self(dir)
    }

    /// A path inside it. The file is not created.
    #[must_use]
    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Every entry's file name, sorted.
    ///
    /// Sorted because two callers assert on the whole list and `read_dir` makes
    /// no ordering promise --- an unsorted comparison passes or fails on what
    /// the filesystem happens to return.
    ///
    /// # Panics
    ///
    /// The directory cannot be read.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0)
            .expect("read dir")
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();
        names.sort();
        names
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
