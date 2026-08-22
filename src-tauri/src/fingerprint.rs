//! What the file looked like when the reader opened it.
//!
//! ## Why this exists
//!
//! `docs/PLAN.md` §5 states the requirement and nothing implemented it: *retain
//! file identity plus size, mtime and baseline digest; recheck immediately
//! before save*. Until 2026-08-19 the save path had exactly one guard against a
//! file that changed underneath it --- the **page count** against the plan's
//! baseline --- and that catches a single shape of a much wider problem.
//!
//! Every modification that keeps the page count is invisible to it. A colleague
//! re-exporting the same report over the top, a sync client landing a newer
//! copy, a signing tool rewriting the file in place: the reader's edits then
//! replay onto an object graph they were never made against, and because the
//! write is atomic the result is a confidently wrong file rather than a visibly
//! broken one. That became a live hazard rather than a theoretical one when Save
//! in place shipped in `26.8.5`; before it, the worst case was a bad copy beside
//! an intact original.
//!
//! ## What is compared, and what each part is worth
//!
//! Three things, and they are deliberately not equivalent:
//!
//!  - **Length.** Free, and catches most of it. Conclusive when it differs.
//!  - **Modification time.** Free, and **evidence of nothing on its own.** It is
//!    wrong in both directions: `cp -p` and `rsync --times` preserve it across a
//!    rewrite, a backup tool or a bare `touch` moves it without a byte changing,
//!    and filesystem resolution is coarse enough (FAT is 2 s) that two writes can
//!    share one. It is what you compare when you cannot afford to read.
//!  - **A digest of the whole file.** The one that actually answers the
//!    question, and the only one that costs anything.
//!
//! **So the two checks are not nested, and that is the design rather than an
//! oversight.** [`Fingerprint::agrees_with`] compares length and digest and does
//! *not* consult the timestamp --- with the bytes in hand a timestamp adds no
//! evidence and subtracts a real save. [`Fingerprint::agrees_shallowly`] compares
//! length and timestamp, for the one moment a third full read is the wrong price.
//! Writing the deep check as the shallow one plus a digest, which was the first
//! design, made the digest comparison unfalsifiable *and* refused saves it should
//! have allowed. `docs/TRAPS.md` carries it.
//!
//! **This is a change detector, not a security boundary, and the difference is
//! worth stating rather than leaving to be assumed.** SHA-256 is used because it
//! is already in the dependency graph, not because a crafted collision is in the
//! threat model --- `docs/THREAT-MODEL.md` is about hostile *documents* reaching
//! the parser, and an adversary who can write to the reader's file at the moment
//! they save has better things to do than defeat this. What it is built to catch
//! is the ordinary case: another program wrote to the file.
//!
//! ## Fail closed
//!
//! A fingerprint that could not be taken is [`None`], and a save with no
//! fingerprint is **refused**. That is the repository's own rule about a check
//! that cannot answer --- `docs/TRAPS.md`, *When a check cannot answer, make the
//! failure path the SAFE one* --- and it matters here because the alternative
//! collapses "checked, unchanged" and "could not look" into one silent success.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// How much is read at a time when hashing.
///
/// The whole file is never held in memory: `make_incremental_pdf.py` writes a
/// **550 MB** fixture on purpose, and a save path that doubled its own peak
/// footprint to check a timestamp would be a poor trade for the thing it
/// catches.
const CHUNK: usize = 64 * 1024;

/// A file as it was at one moment.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fingerprint {
    /// Bytes on disk.
    pub len: u64,
    /// Nanoseconds since the Unix epoch, or `None` where the platform has no
    /// modification time for this file.
    ///
    /// `None` rather than zero, and the difference is the usual one: a file
    /// whose mtime cannot be read is not a file modified in 1970, and comparing
    /// two `None`s as equal would make an unreadable timestamp look like an
    /// unchanged one on every platform that has that gap.
    pub modified_ns: Option<u128>,
    /// SHA-256 of every byte.
    pub digest: [u8; 32],
}

impl Fingerprint {
    /// Reads `path` and records what it finds.
    ///
    /// # Errors
    ///
    /// The file cannot be opened, its metadata cannot be read, or a read fails
    /// part way through.
    pub fn of(path: &Path) -> Result<Fingerprint, String> {
        let mut file = File::open(path)
            .map_err(|e| format!("could not open {} to fingerprint it: {e}", path.display()))?;
        let meta = file
            .metadata()
            .map_err(|e| format!("could not read {}'s metadata: {e}", path.display()))?;

        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; CHUNK];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|e| format!("could not read {} to fingerprint it: {e}", path.display()))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }

        Ok(Fingerprint {
            len: meta.len(),
            modified_ns: modified_ns(&meta),
            digest: hasher.finalize().into(),
        })
    }

    /// Compares length and modification time, without reading the file.
    ///
    /// The guard for a moment where a full read is the wrong cost --- between
    /// staging and the rename, where the window is short and the reader is
    /// already mid-save. It cannot see a change that preserved both, which is
    /// exactly why [`Self::agrees_with`] exists and runs earlier.
    ///
    /// The two do **not** nest: `agrees_with` does not call this, because the
    /// mtime comparison below is the one part of it that a digest supersedes.
    /// See that method for why deferring to a timestamp you can afford to
    /// ignore is a false refusal rather than caution.
    ///
    /// # Errors
    ///
    /// The metadata cannot be read, or either field differs. The message is the
    /// **fact only** --- what a reader can do about it depends on where this was
    /// called from, and the caller is the only one that knows. This doc comment
    /// promised the opposite until 2026-08-19, and the shared advice it referred
    /// to was untrue at one of the two call sites.
    pub fn agrees_shallowly(&self, path: &Path) -> Result<(), String> {
        let meta = std::fs::metadata(path)
            .map_err(|e| format!("{} could not be checked before saving: {e}", path.display()))?;
        self.agrees_with_metadata(&meta, path)
    }

    /// [`Self::agrees_shallowly`] against metadata the caller already holds.
    ///
    /// **Split out so that a caller holding an open handle can ask the question
    /// about the file it is holding**, rather than about whatever the pathname
    /// names by the time it looks. `File::metadata` reads through the descriptor;
    /// `std::fs::metadata` resolves the path again, and between the two a rename
    /// can put a different file there. That is the whole difference, and it is
    /// why `save::append_in_place` calls this one: it writes through a handle, so
    /// a check against the path would be a check on a different file from the one
    /// it is about to modify.
    ///
    /// Not a second copy of the comparison --- [`Self::agrees_shallowly`] is
    /// this, with a `stat` in front. `docs/TRAPS.md`, *Two copies of a
    /// distinction drift, and a mutation of one survives*.
    ///
    /// # Errors
    ///
    /// Either field differs. The message is the fact only, for the reason
    /// [`Self::agrees_shallowly`] gives.
    pub fn agrees_with_metadata(
        &self,
        meta: &std::fs::Metadata,
        path: &Path,
    ) -> Result<(), String> {
        self.len_matches(meta, path)?;
        let now = modified_ns(meta);
        // Both sides unknown is *not* agreement: see `modified_ns`. A platform
        // that cannot report the time gives no evidence either way, and the
        // digest comparison is what carries the answer there.
        if now.is_some() && self.modified_ns.is_some() && now != self.modified_ns {
            return Err(changed(path, "it was modified"));
        }
        Ok(())
    }

    /// Compares length and contents, reading the file. Returns what it read.
    ///
    /// **Deliberately not [`Self::agrees_shallowly`] plus a digest**, and the
    /// difference is the whole reason both exist. A modification time is a
    /// *hint*: it is what you compare when you cannot afford to read, and it is
    /// wrong in both directions --- `cp -p` preserves it across a rewrite, and a
    /// backup tool, a sync client or a bare `touch` moves it without changing a
    /// byte. Here the bytes are in hand, so the digest is the answer and the
    /// timestamp has no vote. Consulting it anyway would refuse a save whose
    /// file is byte-for-byte what the reader opened, which is a false refusal at
    /// the one moment a reader least wants an argument.
    ///
    /// That deference also made the digest comparison unfalsifiable: both tests
    /// named for it passed with the comparison deleted, because the mtime branch
    /// fired first and produces a message they could not tell apart. See
    /// `docs/TRAPS.md`, *An outcome two mechanisms can produce cannot test
    /// either one*.
    ///
    /// The returned fingerprint is the file **as of this call**, which is what a
    /// caller staging a save should carry forward to any later check: comparing
    /// a second time against the open-time value would re-ask a question this
    /// one has already answered more thoroughly.
    ///
    /// # Errors
    ///
    /// The metadata cannot be read, the length differs, the file cannot be read,
    /// or its contents differ.
    pub fn agrees_with(&self, path: &Path) -> Result<Fingerprint, String> {
        // The advice is added here rather than inside the comparisons, because
        // this is the call site where it is true: nothing has been staged, the
        // document is open, and the journal holds every edit.
        self.compare_deeply(path)
            .map_err(|fact| format!("{fact}. {WAY_OUT}"))
    }

    /// [`Self::agrees_with`] without the advice, so the advice has one home.
    fn compare_deeply(&self, path: &Path) -> Result<Fingerprint, String> {
        // Cheap and conclusive when it differs, so the common failure costs no
        // read at all and its message names the size rather than the bytes.
        self.len_agrees(path)?;
        let now = Fingerprint::of(path)?;
        if now.digest != self.digest {
            return Err(changed(
                path,
                "its contents changed while keeping the same length",
            ));
        }
        Ok(now)
    }

    /// The length comparison, given a path rather than metadata.
    ///
    /// It handed the metadata back until 2026-08-22, so that
    /// [`Self::agrees_shallowly`] could read two fields from one `stat`. That
    /// caller now takes its own metadata --- or is handed it, which is the whole
    /// point of [`Self::agrees_with_metadata`] --- so the only reader left
    /// discards it.
    fn len_agrees(&self, path: &Path) -> Result<(), String> {
        let meta = std::fs::metadata(path)
            .map_err(|e| format!("{} could not be checked before saving: {e}", path.display()))?;
        self.len_matches(&meta, path)
    }

    /// The length comparison itself, against metadata already in hand.
    ///
    /// One body, so the two entry points cannot come to disagree about what
    /// "the same length" means or about how the difference is worded.
    fn len_matches(&self, meta: &std::fs::Metadata, path: &Path) -> Result<(), String> {
        if meta.len() != self.len {
            return Err(changed(
                path,
                &format!("its length went from {} to {}", self.len, meta.len()),
            ));
        }
        Ok(())
    }
}

/// Which file a handle or a pathname refers to, as the filesystem sees it.
///
/// **A pathname is a lookup, not a file**, and the difference is what a save
/// that writes through a handle has to be able to see. Between the moment
/// `save::append_in_place` checks the file and the moment it finishes writing
/// to it, another program can rename a different file over that name. Every
/// byte this process writes still goes to the file it opened --- which is the
/// point of holding the handle --- but that file no longer has the name the
/// reader typed, so the save has not landed where they asked and saying it
/// succeeded would be a lie.
///
/// A [`Fingerprint`] cannot answer this. It compares what a file *contains*;
/// this compares *which file it is*, and the two questions come apart exactly
/// when a rename is involved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileId {
    /// The volume: `st_dev` on Unix, the volume serial number on Windows.
    volume: u64,
    /// The file on it: the inode number on Unix, the file index on Windows.
    file: u64,
}

impl FileId {
    /// Reads the identity of an already-open file.
    ///
    /// [`None`] where the platform could not answer, which is a failed system
    /// call rather than a platform without the concept: both of the two this
    /// runs on have one. Callers treat it as "could not tell", never as
    /// "different" or as "the same" --- see `docs/TRAPS.md`, *When a check
    /// cannot answer, make the failure path the SAFE one*.
    #[must_use]
    pub fn of(file: &File) -> Option<FileId> {
        identity(file)
    }

    /// Reads the identity of whatever `path` names at this moment.
    ///
    /// Opens it, because the answer has to come from a handle on Windows and
    /// going through one on both platforms keeps this a single body. The open
    /// is read-only and the handle is dropped before this returns.
    #[must_use]
    pub fn at(path: &Path) -> Option<FileId> {
        File::open(path).ok().as_ref().and_then(FileId::of)
    }
}

#[cfg(unix)]
fn identity(file: &File) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt as _;
    let meta = file.metadata().ok()?;
    Some(FileId {
        volume: meta.dev(),
        file: meta.ino(),
    })
}

#[cfg(windows)]
fn identity(file: &File) -> Option<FileId> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    // The Windows counterpart of `st_dev`/`st_ino`, and the only route to it:
    // `std::os::windows::fs::MetadataExt::file_index` exists but is unstable
    // behind `windows_by_handle`, and this repository pins a stable toolchain.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `file` owns the handle for the whole call, and `info` is a live,
    // correctly sized, correctly aligned structure this thread alone can reach.
    let read = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut info) };
    if read == 0 {
        return None;
    }
    Some(FileId {
        volume: u64::from(info.dwVolumeSerialNumber),
        file: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}

/// The bare fact, with no advice about what to do next.
///
/// **Split from the advice on purpose, and the reason is a message a reader was
/// shown.** The two checks in this module run at moments where different things
/// are true. Before the parse the document is open and the journal intact, so
/// "save them under another name" is a real instruction. Between staging and the
/// rename the document has already been closed, and the same sentence tells
/// somebody to do something they can no longer do --- while the caller appends
/// *"the document has been closed"* to the end of it, so the message contradicts
/// itself in two clauses.
///
/// So [`Fingerprint::agrees_shallowly`] returns facts and its caller supplies the
/// advice that is true at its own call site. See `docs/TRAPS.md`.
fn changed(path: &Path, how: &str) -> String {
    format!(
        "{} changed on disk since you opened it --- {how}",
        path.display()
    )
}

/// What a reader can do while their document is still open.
///
/// Appended by [`Fingerprint::agrees_with`], which is the check that runs before
/// anything is disturbed. A reader told their file changed and offered nothing
/// has to guess whether their edits still exist, and here they do.
const WAY_OUT: &str = "Your edits are still here: save them under another name, \
                       or open the file again to start from what is on disk now.";

/// A file's modification time as nanoseconds since the epoch.
///
/// `None` for a platform or filesystem with no modification time, and also for
/// the pre-1970 case, which is unreachable in practice and would otherwise need
/// a signed representation for no benefit.
fn modified_ns(meta: &std::fs::Metadata) -> Option<u128> {
    meta.modified()
        .ok()
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_nanos())
}

/// The moment a fingerprint was taken, for a message a reader reads.
///
/// Unused by the guards themselves --- they compare, they do not narrate --- and
/// kept because `SystemTime` is what a future "changed at 14:02" message needs
/// and deriving it later from `modified_ns` would mean re-deciding the epoch.
#[must_use]
pub fn at_nanos(ns: u128) -> Option<SystemTime> {
    u64::try_from(ns / 1_000_000_000)
        .ok()
        .map(|secs| UNIX_EPOCH + std::time::Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A scratch file that removes itself.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str, bytes: &[u8]) -> Scratch {
            let path = std::env::temp_dir().join(format!(
                "tpdf-fingerprint-{name}-{}.bin",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            let mut file = File::create(&path).expect("create scratch");
            file.write_all(bytes).expect("write scratch");
            Scratch(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn rewrite(&self, bytes: &[u8]) {
            let mut file = File::create(&self.0).expect("rewrite scratch");
            file.write_all(bytes).expect("write scratch");
        }

        /// Moves the modification time without touching a byte.
        ///
        /// Set explicitly and well into the future rather than by writing and
        /// hoping: filesystem timestamp resolution is coarse enough (FAT is 2 s)
        /// that a rewrite can land inside the same tick as the original, and a
        /// test whose precondition is "these two timestamps differ" must not be
        /// left to chance. The tests using this assert the move happened.
        fn touch(&self) {
            let file = File::options()
                .write(true)
                .open(&self.0)
                .expect("open scratch to touch");
            let later = SystemTime::now() + std::time::Duration::from_secs(120);
            file.set_times(std::fs::FileTimes::new().set_modified(later))
                .expect("move the scratch file's timestamp");
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn an_untouched_file_agrees_with_its_own_fingerprint() {
        // The control, and it is the half that fails if the comparison is made
        // too strict --- a guard that refuses everything protects nothing and
        // looks exactly like one that works until somebody tries to save.
        let file = Scratch::new("untouched", b"one two three");
        let taken = Fingerprint::of(file.path()).expect("fingerprint");
        assert_eq!(taken.agrees_shallowly(file.path()), Ok(()));
        assert_eq!(taken.agrees_with(file.path()), Ok(taken.clone()));
    }

    #[test]
    fn a_file_whose_timestamp_moved_but_whose_bytes_did_not_still_saves() {
        // The false refusal the deep check must not have. `cp -p`, `rsync`, a
        // sync client and a bare `touch` all move an mtime without changing a
        // byte, and the file the reader opened is still, byte for byte, the file
        // in front of them. Refusing here would send somebody to Save a copy
        // over a backup tool having run.
        //
        // The shallow check is asserted to refuse the *same* file in the same
        // breath, because that is what says the two are deliberately different
        // rather than that this one is simply not looking.
        let file = Scratch::new("touched", b"one two three");
        let taken = Fingerprint::of(file.path()).expect("fingerprint");
        file.touch();
        assert_ne!(
            modified_ns(&std::fs::metadata(file.path()).expect("metadata")),
            taken.modified_ns,
            "the touch did not move the timestamp, so this proves nothing",
        );
        assert!(taken.agrees_shallowly(file.path()).is_err());
        let verified = taken.agrees_with(file.path()).expect("bytes are unchanged");
        // What comes back is the file as of the check, which carries the new
        // timestamp -- that is what makes it usable for a later shallow look.
        assert_eq!(verified.digest, taken.digest);
        assert_ne!(verified.modified_ns, taken.modified_ns);
        assert_eq!(verified.agrees_shallowly(file.path()), Ok(()));
    }

    #[test]
    fn a_file_that_grew_is_refused_without_reading_it() {
        let file = Scratch::new("grew", b"one two three");
        let taken = Fingerprint::of(file.path()).expect("fingerprint");
        file.rewrite(b"one two three four");
        let why = taken
            .agrees_shallowly(file.path())
            .expect_err("must refuse");
        assert!(why.contains("length"), "{why}");
        // And **no** advice. This check runs at a moment where the caller knows
        // what the reader can do and this function does not --- between staging
        // and the rename the document is already closed, and "save them under
        // another name" is then an instruction nobody can follow.
        assert!(!why.contains("another name"), "{why}");
        assert!(!why.contains("still here"), "{why}");
    }

    #[test]
    fn the_deep_check_tells_the_reader_what_they_can_still_do() {
        // The other half of the split: the fact alone is a diagnosis, and a
        // reader stopped mid-save needs the way out. This check runs before
        // anything is staged, so the journal really does still hold their edits.
        let file = Scratch::new("way-out", b"one two three");
        let taken = Fingerprint::of(file.path()).expect("fingerprint");
        file.rewrite(b"one two three four");
        let why = taken.agrees_with(file.path()).expect_err("must refuse");
        assert!(why.contains("changed on disk"), "{why}");
        assert!(why.contains("still here"), "{why}");
        assert!(why.contains("another name"), "{why}");
    }

    #[test]
    fn a_rewrite_of_the_same_length_is_caught_by_the_digest_and_not_by_the_length() {
        // The case the page-count guard and the length guard both miss, and the
        // reason a digest is worth reading a file for. Same size, different
        // bytes: `agrees_shallowly` may well pass, and `agrees_with` must not.
        let file = Scratch::new("same-length", b"one two three");
        let taken = Fingerprint::of(file.path()).expect("fingerprint");
        file.rewrite(b"ONE TWO THREE");
        assert_eq!(taken.len, 13);
        assert_eq!(Fingerprint::of(file.path()).expect("re-read").len, 13);
        let why = taken.agrees_with(file.path()).expect_err("must refuse");
        assert!(why.contains("changed on disk"), "{why}");
        // Named specifically, not merely "changed": until 2026-08-19 this
        // assertion read only the line above, and passed with the digest
        // comparison deleted --- the mtime branch inside `agrees_shallowly` fired
        // first and said "it was modified", which contains the same words. The
        // deep check no longer consults an mtime at all, and this is what says
        // the digest is what refused.
        assert!(why.contains("contents changed"), "{why}");
    }

    #[test]
    fn two_files_of_the_same_length_and_different_bytes_have_different_digests() {
        // The property everything above rests on, asserted directly rather than
        // through a guard that could be passing for another reason.
        let a = Scratch::new("digest-a", b"aaaaaaaa");
        let b = Scratch::new("digest-b", b"bbbbbbbb");
        let one = Fingerprint::of(a.path()).expect("a");
        let two = Fingerprint::of(b.path()).expect("b");
        assert_eq!(one.len, two.len);
        assert_ne!(one.digest, two.digest);
    }

    #[test]
    fn a_file_that_is_gone_is_refused_rather_than_treated_as_unchanged() {
        // Fail closed. A missing file cannot be compared, and the branch that
        // matters is that this is an error rather than an `Ok(())` reached by a
        // metadata call nobody checked.
        let file = Scratch::new("gone", b"here");
        let taken = Fingerprint::of(file.path()).expect("fingerprint");
        let path = file.path().to_path_buf();
        drop(file);
        assert!(taken.agrees_shallowly(&path).is_err());
        assert!(taken.agrees_with(&path).is_err());
    }

    #[test]
    fn an_empty_file_fingerprints_rather_than_failing() {
        // The chunked read's boundary case: the loop must terminate on the first
        // zero-length read rather than needing at least one byte.
        let file = Scratch::new("empty", b"");
        let taken = Fingerprint::of(file.path()).expect("fingerprint");
        assert_eq!(taken.len, 0);
        assert_eq!(taken.agrees_with(file.path()), Ok(taken.clone()));
    }

    #[test]
    fn a_file_larger_than_one_chunk_hashes_every_chunk() {
        // A hasher fed only its first chunk would agree with itself here and
        // differ from nothing, so the assertion is that a change *past* the
        // first chunk is seen.
        let mut bytes = vec![b'x'; CHUNK * 2 + 7];
        let file = Scratch::new("chunked", &bytes);
        let taken = Fingerprint::of(file.path()).expect("fingerprint");
        let last = bytes.len() - 1;
        bytes[last] = b'y';
        file.rewrite(&bytes);
        let why = taken.agrees_with(file.path()).expect_err("must refuse");
        assert!(why.contains("changed on disk"), "{why}");
        // The digest, for the reason the same-length test gives: a hasher fed
        // only its first chunk still produces *a* refusal here if anything else
        // is looking, and this names which mechanism answered.
        assert!(why.contains("contents changed"), "{why}");
    }

    #[test]
    fn an_empty_file_that_gained_bytes_is_refused() {
        // The other side of the empty-file boundary. A zero-length file hashes
        // without reading anything, and a comparison that special-cased that
        // would treat every later write as no change at all.
        let file = Scratch::new("empty-grew", b"");
        let taken = Fingerprint::of(file.path()).expect("fingerprint");
        file.rewrite(b"something");
        assert!(taken.agrees_with(file.path()).is_err());
    }
}
