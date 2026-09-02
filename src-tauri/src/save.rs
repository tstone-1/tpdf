//! Writing the working document to a file.
//!
//! **Save a copy: the pages the reader kept, each with its turn applied.** The
//! plan is [`edits::Plan`](crate::edits::Plan) --- the same answer the viewer is
//! drawing from --- so a saved copy and a rendered page cannot disagree about
//! what the reader was looking at. Two readings of one answer rather than two
//! derivations of one rule.
//!
//! **What the plan says.** The kept pages, in the order the reader put them,
//! each with the quarter turns to add. A page nobody kept is deleted from the
//! page tree; a page nobody turned is written back byte for byte; and a plan
//! whose pages have *moved* is written as a new page tree rather than refused,
//! which is what changed when `Command::Move` was wired.
//!
//! **A move is paid for by the whole tree, and a plan in document order must not
//! pay it.** `pagetree::reorder_pages` flattens the page tree so that every page
//! carries what it used to inherit; run over a document nobody rearranged, that
//! would rewrite pages nobody touched. So the order is compared against the
//! file's before anything is written, and the common case goes nowhere near it.
//!
//! **Three refusals, and none of them is defensive.**
//!
//!  - An **encrypted** document. `docs/TRAPS.md` records that `lopdf` silently
//!    drops encryption on save, so writing one produces a file whose restrictions
//!    are gone and whose reader has no way to know. 3 of the 39 PDFs in a real
//!    Downloads folder carry `/Encrypt` (measured for `progressive::open_failure`),
//!    so this is a case a reader meets, not a hypothetical.
//!  - A **page count that disagrees** with the plan's baseline. That is the
//!    external modification §5 of `docs/PLAN.md` is about: the file changed under
//!    the open document, and the edits the reader applied no longer name the pages
//!    they were applied to. Compared against the *baseline* rather than against
//!    the plan's length, which is what makes it survive a deletion --- a plan of
//!    three pages for a five-page file is what deleting two of them looks like.
//!  - Writing **over the source**. A copy is never the file it was copied from,
//!    and a reader who types the open document's own name into the panel means
//!    Save rather than Save a copy. Saving in place is the operation below, and
//!    it is not this one with a different destination.
//!
//! **The write is atomic**: the bytes go to a sibling temporary file and are
//! renamed over the destination, so an interrupted save leaves either the old
//! file or the new one. A partially written PDF is the worst of the three
//! outcomes --- it opens, and it is missing pages.
//!
//! **Save, in place, is that write split in two.** [`stage_in_place`] does
//! everything up to and including the temporary file; [`commit_in_place`] does
//! the rename. Between them the caller closes the document, and that is not
//! tidiness: a `rename` over a memory-mapped file succeeds on macOS and leaves
//! the mapping serving the inode that is no longer there, so a worker would
//! render the document as it was before the save for as long as it stayed open.
//! Windows refuses the rename instead. One order is correct on both.
//!
//! What the caller does *not* get back is a rebase in §5's full sense. The
//! journal is spent rather than compacted, and the document is reopened from the
//! file that was just written --- which is the same answer for a save that
//! succeeded, and is why a save that fails after the close costs the reader
//! their unsaved commands. Carrying a journal across a reopen is what §5 calls
//! rebasing, and nothing here does it.
//!
//! The page-tree surgery itself is `pagetree.rs`, shared with the print path,
//! which needs every one of the same operations for the same reasons.
//!
//! **Where the parse happens is a seam here and an implementation elsewhere.**
//! [`Reread`], [`Rewriter`] and [`Outside`] are declared below, with [`Here`] ---
//! the in-process fallback --- beside them, because which of the two a save picks
//! is part of what a save is. What [`InWorker`] does about that choice is
//! `save_outside.rs`: spawning a process, mapping a shared segment and speaking
//! the worker protocol is four modules of the process boundary, and this file's
//! import block below is meant to be read as the whole of what it depends on.

use std::path::{Path, PathBuf};

use lopdf::{Document, IncrementalDocument};

use lopdf::{Object, ObjectId};

use crate::docmodel::{PageSource, Size};
use crate::edits::Plan;
use crate::encoding::MAX_DECODE;
use crate::fingerprint::{FileId, Fingerprint};
use crate::pagetree::{agreed_turns, ordered_pages};

/// The middle of the name of the file bytes are written to before the rename.
///
/// Sibling rather than in the system temp directory, because a rename across
/// filesystems is not atomic and the temp directory is routinely on another
/// one. The full name is `<destination>.tpdf-partial-<pid>-<attempt>`: see
/// [`stage`] for why every part of that is load-bearing.
const PARTIAL: &str = "tpdf-partial";

/// How many names [`stage`] will try before giving up.
///
/// A collision means another save of the same destination is in flight in this
/// process, or a stale leftover from a run whose pid has since been reused.
/// Either way the next attempt index is free. Sixteen in a row is a directory
/// somebody is fighting us for, and reporting that beats looping.
const STAGING_ATTEMPTS: u32 = 16;

/// Why a save was refused, and whether the file having changed is the reason.
///
/// **`changed` is a field rather than a wording**, for exactly the reason
/// `SaveFailure::reopen` in `lib.rs` is one: a caller that decided by matching
/// on the message would be parsing a string this end is free to reword, and the
/// message here is deliberately reworded whenever a reader is served badly by
/// it. See `docs/TRAPS.md` on the two-moments message, which is this same file
/// getting that wrong within the week.
///
/// What it buys is one decision the window cannot otherwise make safely.
/// Reloading throws away every edit in the journal, so offering it is right for
/// *this* refusal --- the file underneath is a different file, and there is
/// nothing else the reader can do with their edits but write them elsewhere ---
/// and wrong for "a document must keep at least one page", where reloading would
/// discard the reader's work in exchange for nothing at all.
///
/// Every other refusal reaches this through `From<String>`, which is what keeps
/// the `?` in the middle of `planned_bytes` unchanged: the flag is set at the
/// one call site that knows, and defaults to false everywhere else.
///
/// **The serialised form is the contract `print_document` rejects with**, and
/// the two field names are the whole of it: `message` and `changed`, lowercase,
/// with nothing else beside them. The window reads them exactly as it reads
/// `SaveFailure`'s, so a rename here is a change to what the frontend parses and
/// not a refactor --- which is why
/// `the_wire_shape_of_a_refusal_is_a_message_and_a_changed_flag` asserts the
/// names against literals rather than round-tripping through this same derive,
/// where a writer and its own reader would agree about anything.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Refusal {
    /// What to tell the reader.
    pub message: String,
    /// The source changed on disk since the reader opened it.
    pub changed: bool,
}

impl Refusal {
    /// A refusal because the file is not the file that was opened.
    #[must_use]
    pub fn changed(message: impl Into<String>) -> Refusal {
        Refusal {
            message: message.into(),
            changed: true,
        }
    }
}

impl From<String> for Refusal {
    fn from(message: String) -> Refusal {
        Refusal {
            message,
            changed: false,
        }
    }
}

impl From<&str> for Refusal {
    fn from(message: &str) -> Refusal {
        Refusal::from(message.to_string())
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What a source that changed under the reader means for the operation.
///
/// The two save paths answer this differently and always have --- `stage_in_place`
/// refuses what `write_copy` tolerates --- and until 2026-08-19 that was true of a
/// missing fingerprint and **not** of a changed file, which stranded the reader in
/// the exact way the comment in `stage_in_place` warns about. A save in place was
/// refused with a message naming Save a copy, and Save a copy was refused by the
/// same guard one function down. The escape hatch was closed and the message
/// pointed at it anyway.
///
/// The asymmetry is the same one, for the same reason. A copy writes a new file
/// and leaves the original exactly as it is, so the worst case is a file the
/// reader can look at and delete. Writing in place spends the original, and there
/// is no looking at it afterwards.
///
/// What still protects the copy is everything else `planned_bytes` checks: the
/// page count against the plan's baseline, every page the plan names existing,
/// and the shared-page refusals. A changed file that also changed shape is
/// refused by those, whichever value this carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OnChange {
    /// Refuse, because what is at stake is the reader's only copy.
    Refuse,
    /// Write it, and say so. `changed` on the result is how the reader is told.
    Proceed,
}

/// A copy that was written, and whether its source had changed underneath.
///
/// `changed` is a fact the reader has to be told rather than a failure: the file
/// is on disk and is the best tpdf can produce, and it was built from a document
/// that is no longer the one they opened. Silence here would be the worst of the
/// three options --- a copy that is quietly built from different pages reads as a
/// copy that is right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Copied {
    /// The source changed since it was opened, and the copy was written anyway.
    pub changed: bool,
}

/// A staged save: the file to rename, and what the source was when it was built.
///
/// `verified` is not an `Option`, and that is the point of the type. The caller
/// takes its last look before the rename through this field, and an `Option`
/// there would give that look a `None` arm --- which could only be written as
/// "skip the check", i.e. the one save that most needs it proceeding unchecked.
/// A missing fingerprint is already refused by [`stage_in_place`] several lines
/// above; making it unsayable here is what stops that refusal being undone by a
/// later `if let`.
///
/// It is the fingerprint taken **during** staging, not the one taken at open.
/// That is the whole reason this struct exists rather than a bare path: the
/// caller's last look before the rename should ask whether anything moved since
/// the file was read and rewritten, not whether anything moved since the reader
/// opened it an hour ago. The second question refuses a `touch` that the deep
/// check already forgave.
#[derive(Debug)]
pub struct Staged {
    /// The sibling temporary file, which nothing has renamed yet.
    pub path: PathBuf,
    /// The source as it was when its bytes were read.
    pub verified: Fingerprint,
}

/// What a rewrite is being produced for.
///
/// **Two facts that always move together, so they are one value.** A rewrite
/// differs between a save and a print job in exactly two ways: the reader's own
/// view rotation is part of the paper and not part of the document, and an
/// encrypted document may be saved and may not be printed in part. Carrying
/// those as a `u8` and a `bool` was the first shape of this and is the one
/// `docs/TRAPS.md` records as *two copies of a distinction drift* --- a caller
/// setting one and forgetting the other gets a print job written in the clear,
/// or a save whose pages come out turned the way the reader happened to be
/// holding them.
///
/// It replaced `NO_VIEW_TURN`, which was a named zero for the same reason
/// stated the other way round: a bare `0` in a quarter-turn parameter reads as
/// *no rotation at all*, which is wrong, since the plan's own per-page turns
/// still apply. [`Job::Save`] says *the view adds nothing*, which is the fact.
///
/// **It crosses the worker boundary**, which is why it is `serde`-derived: it is
/// the field [`crate::worker_proto::Request::Rewrite`] carries, and the whole
/// point of it being a value rather than a rule in the coordinator is that the
/// coordinator no longer does the parse the rule is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Job {
    /// The document, stored. Its encryption is kept and the view adds nothing.
    Save,
    /// Paper, turned by the reader's own rotation in quarter turns clockwise.
    ///
    /// **Refused for an encrypted document**, and neither answer a rewrite can
    /// give is right: re-encrypting hands `NSPrintOperation` or
    /// `Windows.Data.Pdf` a document they cannot read, and not re-encrypting
    /// hands the platform --- and Print to PDF, and a printer's spool --- a
    /// decrypted copy of a document somebody encrypted deliberately.
    Print {
        /// The reader's own rotation, in quarter turns clockwise.
        view: u8,
    },
}

impl Job {
    /// The quarter turns the reader's own view adds, which only a print job has.
    #[must_use]
    pub fn view(self) -> u8 {
        match self {
            Self::Save => 0,
            Self::Print { view } => view,
        }
    }

    /// Whether this is paper, which is what the encryption refusal turns on.
    #[must_use]
    fn is_print(self) -> bool {
        matches!(self, Self::Print { .. })
    }
}

/// Asks whether the file a print job is about to be built from is the one the
/// reader opened.
///
/// **All three print routes read the source by name, and until 2026-08-31 none
/// of them asked.** `Passthrough` hands the file over byte for byte, `Working`
/// goes through [`print_bytes`], `Range` through `print::build`; every one of
/// them resolves the pathname afresh, so a sync client landing a newer copy over
/// it is printed with the reader's plan applied to content the plan was never
/// made against. The identical file state one keystroke earlier is refused by
/// `stage_in_place`, and `docs/PLAN.md`'s external-modification section counted
/// the print path as paying for the fingerprint already --- it did, in
/// `Edits::plan`, and then never read the answer.
///
/// **The deep comparison, and that is the same one every other reader of
/// [`Plan::opened_as`] makes.** [`Fingerprint::agrees_shallowly`] is used at
/// exactly one moment --- between staging and the rename, against the *staging*
/// fingerprint, over a window measured in milliseconds --- because a modification
/// time is a hint that is wrong in both directions. An hour can pass between
/// opening a document and printing it, and a `touch` in that hour would refuse a
/// print of a file that is byte for byte what the reader opened. The escape from
/// that false refusal is reopening, which spends their edits, so it is not a
/// cheap mistake to make.
///
/// What it costs is one SHA-256 of the file, on a path that already reads the
/// whole of it and then hands it to PDFKit or `Windows.Data.Pdf` to be parsed a
/// second time before a panel opens. The passthrough is the one route where this
/// is the larger share of the work, and it is still a fraction of a print.
///
/// **A plan with no fingerprint prints.** [`stage_in_place`] refuses one, because
/// what is at stake there is the reader's only copy and a check that cannot
/// answer must fail closed. Here the worst case of proceeding is a sheet of
/// paper, and refusing every print of a document tpdf could not fingerprint at
/// open would take away an operation that costs nothing to repeat. The two
/// answers differ because the stakes do, and this is the one place that is
/// written down.
///
/// **The refusal carries `changed`, and since 2026-09-01 `print_document`
/// carries it out.** That command rejects with a serialised [`Refusal`] rather
/// than with its message, so the window branches on the same flag a save's
/// refusal gives it and can put the actions that answer this one beside the
/// sentence --- `docs/TRAPS.md`, *A refusal flattened to a string across a
/// process boundary loses the action that answers it*, whose shape this was for
/// the day between the guard landing and the wire carrying it. The message
/// still names the escape, because a reader who can act on the sentence unaided
/// is not depending on a button being drawn.
///
/// # Errors
///
/// The file changed since it was opened.
pub fn print_ready(source: &Path, plan: &Plan) -> Result<(), Refusal> {
    rewrite_ready(source, plan, OnChange::Refuse)
        .map(|_ready| ())
        // `agrees_with`'s own sentence names the fact and two escapes that are
        // both real here --- save the edits elsewhere, or reopen --- and neither
        // of them is what a reader pressing Print expects to be told, so the
        // clause that says why a print is the operation being refused is added
        // where it is known.
        .map_err(|why| {
            Refusal::changed(format!(
                "{} A print job is built from the file on disk, so the paper would carry \
                 that document with your edits placed on the pages of the one you opened.",
                why.message
            ))
        })
}

/// The bytes of a print job built from the working document.
///
/// **The same writer a save uses, and that is the whole of this function.**
/// `print::build` grew its own page walk --- selection, deletion, reorder, turns
/// --- because printing came first and needed a subset of what saving does. The
/// two then drifted in the way `docs/TRAPS.md` records as two copies of a
/// distinction: this one gained marks and crops and that one did not, so a
/// reader who highlighted a paragraph and pressed Print got paper with no
/// highlight on it, and a page they had cropped printed at its full size.
/// Measured before it was fixed: a job built from a plan carrying one mark and
/// one crop came back with **no page carrying `/Annots` and none carrying
/// `/CropBox`**.
///
/// `view` is the reader's own rotation, in quarter turns clockwise, which is the
/// one input a print job has and a save does not.
///
/// **A changed file is refused, through [`print_ready`], before anything is
/// read.** This paragraph argued the opposite until 2026-08-31 --- *printing is
/// worth doing from what the reader is looking at, and refusing would take away
/// the one operation that cannot lose anything* --- and both halves were wrong
/// about this function. The line below reads `source` by name, so what would go
/// on paper is the file that is on disk *now*, not the one the reader is looking
/// at; the mapping serving their screen is of the inode that was there at open.
/// And what cannot be lost is the *file*. The artifact can be: the reader's
/// marks land at coordinates measured on the document they opened, over content
/// that is no longer it, and paper does not say so.
///
/// **The parse is `rewriter`'s, and since 2026-09-01 that is what this function
/// is.** It read the source into this process and parsed it here, which made
/// every print of an edited document a coordinator-side parse of
/// attacker-controlled bytes --- `docs/THREAT-MODEL.md` residual risk 18. It now
/// takes the seam [`stage_in_place`] and [`write_copy`] take, with the one
/// difference printing forces: the answer has to come **back**, because
/// `NSPrintOperation` and `Windows.Data.Pdf` are handed bytes and not a
/// pathname. So the worker writes into a scratch file this process created and
/// this process reads it, which is a read of bytes tpdf itself wrote a moment
/// ago and not a parse of the reader's document. The parse that was here is
/// gone; what the platform then does with the job is the readback, which is
/// disclosed and is the whole point of it being an independent parser.
///
/// `view` is the reader's own rotation, in quarter turns clockwise, which is the
/// one input a print job has and a save does not --- see [`Job`].
///
/// # Errors
///
/// The source is not the file the plan was made against ([`print_ready`]), the
/// scratch file cannot be created or read back, or anything [`rewrite_update`]
/// refuses --- including an encrypted source, which printing reaches only when
/// the reader has edited it, since an untouched one is handed over byte for byte
/// and never parsed at all. `password` is the reader's, and it is what makes
/// that refusal the encryption one rather than the locked-document one; see
/// [`rewrite_update`], which is where both now live.
pub fn print_bytes(
    source: &Path,
    plan: &Plan,
    view: u8,
    password: Option<&str>,
    rewriter: &dyn Rewriter,
) -> Result<Vec<u8>, Refusal> {
    // Before anything is read, for the reason `rewrite_ready` sits before the
    // parse in `stage_in_place`: a guard asked after the surgery is a true
    // sentence about the wrong document. Asked here rather than by the caller
    // because `print::route`'s `Working` arm calls this function *directly*.
    print_ready(source, plan)?;
    into_scratch(source, |reading, len, into| {
        staged_rewrite(
            rewriter,
            reading,
            len,
            into,
            plan,
            Job::Print { view },
            password,
        )
    })
}

/// The bytes of a print job for the page range a reader typed.
///
/// **[`print_bytes`]'s counterpart for the route that carries no plan, and the
/// last parse of the reader's document this process did.** `print::build` took a
/// path and ran here; `docs/THREAT-MODEL.md` residual risk 18 never listed it,
/// because that risk names the paths that *write* a document and a page range
/// writes nothing --- it builds bytes and hands them to a printer. It is reached
/// by opening a file and typing two numbers.
///
/// Everything else about it is [`print_bytes`]: the source goes in as a handle,
/// the answer is written into a scratch file this process created, and this
/// process reads that file back through the handle it created it with.
///
/// **No [`print_ready`] here, and the caller is why.** All three print routes
/// ask it, and the two that do not go through [`print_bytes`] are asked together
/// in `lib.rs`'s `print_job` --- a second call would hash the file twice.
///
/// **No password either**, and that is not an omission. A range job carries no
/// plan, so there is nothing to decrypt *for*: `print::build_update` refuses an
/// encrypted document outright, because its writer emits every object in the
/// clear and a selection cannot be appended. See `print::build_update`.
///
/// # Errors
///
/// The source cannot be opened, the scratch file cannot be created or read back,
/// or anything [`crate::print::build_update`] refuses.
pub fn print_range_bytes(
    source: &Path,
    job: &crate::print::Job,
    rewriter: &dyn Rewriter,
) -> Result<Vec<u8>, Refusal> {
    into_scratch(source, |reading, len, into| {
        staged_range(rewriter, reading, len, into, job)
    })
}

/// Runs a builder against a scratch file and hands back what it wrote.
///
/// **The dance both print routes do**, in one place because it is the same file
/// with the same lifetime and the same reason for existing: `NSPrintOperation`
/// and `Windows.Data.Pdf` are handed bytes rather than a pathname, so a print
/// job --- unlike a save --- has to come *back*.
///
/// Read back through the same handle the writer wrote down, never by re-opening
/// the name: the file is unlinked below and the name is this process's own, but
/// reading a handle rather than a name is what the whole boundary is written in
/// and there is no reason for this one call to be the exception.
///
/// # Errors
///
/// The source cannot be opened, the scratch file cannot be created, `build`
/// refuses, or the bytes cannot be read back.
fn into_scratch(
    source: &Path,
    build: impl FnOnce(&mut std::fs::File, usize, &mut std::fs::File) -> Result<usize, Refusal>,
) -> Result<Vec<u8>, Refusal> {
    // The handle, never the pathname handed on --- see `opened_to_rewrite`.
    let (mut reading, len) = opened_to_rewrite(source)?;
    let (at, mut into) = job_scratch()?;
    let built = build(&mut reading, len, &mut into).and_then(|wrote| {
        read_whole(&mut into, wrote)
            .map_err(|e| Refusal::from(format!("the print job could not be read back: {e}")))
    });
    // Whatever happened. The scratch file holds a decrypted, reordered copy of
    // the reader's document, so leaving one behind on a refusal would be worse
    // than the refusal.
    drop(into);
    let _ = std::fs::remove_file(&at);
    built
}

/// Serialises every test in the crate that builds a print job.
///
/// **[`job_scratch`] names its file in a directory the whole suite shares**, and
/// `cargo test` runs its tests in parallel --- so a scratch file another print
/// test had open when this one counted them is a difference the counting test
/// reports as a leftover. Measured rather than reasoned about: the mutation
/// harness's control run went red on
/// `a_print_job_whose_writer_refuses_leaves_no_scratch_file_behind` on a tree
/// whose whole suite had passed a minute earlier, which is the shape this
/// repository files under *flake* and which is not one.
///
/// **Here rather than in either test module, because the tests that reach
/// [`print_bytes`] are in two of them** --- this file's and `print.rs`'s --- and
/// a lock each would serialise two groups against themselves and neither
/// against the other, which is the second-copy shape `docs/TRAPS.md` records.
///
/// Poisoning is stepped over deliberately: a panicking neighbour has already
/// failed and reported, and turning that into a second failure here would name
/// the wrong test.
#[cfg(test)]
pub(crate) fn print_lock() -> std::sync::MutexGuard<'static, ()> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

/// A private, empty file for a worker to write a print job into.
///
/// **Not [`stage`], and the difference is the destination.** Staging exists to
/// put a file beside the one it will replace so the rename is atomic; a print
/// job replaces nothing and is never renamed anywhere --- it is read back into
/// this process and the file is removed. What it shares with staging is the part
/// that matters: the name is fresh per call and the file is created with
/// `create_new`, so nothing existing is truncated and a symlink at that path is
/// refused outright.
///
/// **Created `0600` on Unix, rather than with the process umask.** A staged copy
/// lands beside the reader's own document, in a directory they chose; this one
/// lands in the temporary directory, which on both platforms is per-user but is
/// not the reader's own. What it holds for the length of a print is their
/// document with the encryption off and the pages in the order they asked for,
/// which is not a thing to write `0644` anywhere. Windows has no counterpart to
/// copy a mode from and no umask to widen it: a file there inherits the ACL of
/// the directory it is created in, and `%TEMP%` is inside the user's profile.
///
/// # Errors
///
/// No unused name after [`STAGING_ATTEMPTS`] tries, or the file cannot be
/// created.
fn job_scratch() -> Result<(PathBuf, std::fs::File), Refusal> {
    let dir = std::env::temp_dir();
    for attempt in 0..STAGING_ATTEMPTS {
        let at = dir.join(staging_name(
            std::ffi::OsStr::new("tpdf-print-job"),
            attempt,
        ));
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&at) {
            Ok(file) => return Ok((at, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(Refusal::from(format!(
                    "the print job needs somewhere to be built and {at:?} could not be \
                     created: {e}"
                )))
            }
        }
    }
    Err(Refusal::from(format!(
        "could not find an unused temporary name in {dir:?} after {STAGING_ATTEMPTS} tries"
    )))
}

/// Writes the pages `plan` keeps, each with its own turn, from `source` to `out`.
///
/// **The parse is `rewriter`'s, and since 2026-09-01 that is what this function
/// is.** It read the source into this process and applied the plan here, through
/// [`planned_bytes`], which made every Save a copy, every Extract and every
/// Redact to a copy a coordinator-side parse of attacker-controlled bytes ---
/// `docs/THREAT-MODEL.md` residual risk 18, reached by pressing the one button a
/// reader is told to press when a save in place refuses. It now takes the seam
/// [`stage_in_place`] already took, and the shape is the same one: the file's
/// **handle** goes in, the staging file's handle goes in, and a length comes
/// back.
///
/// **The destination is not the file the rewriter is handed**, which is what
/// made this reachable at all and is worth stating rather than leaving to be
/// read out of the code. A copy's destination is a path the reader chose in a
/// dialog, and handing a sandboxed child a descriptor to that would be handing
/// it something it did not create. What it is handed is the staging file
/// [`stage`] creates beside that destination --- the same file, created the same
/// way, as the one an in-place save stages --- and the rename onto the reader's
/// chosen name happens here, in the process that has the authority for it.
///
/// # Errors
///
/// Everything [`rewrite_ready`] and the rewriter refuse; `out` is the source; the
/// source cannot be opened or measured; a rewriter that wrote a different number
/// of bytes than it reported; or the write fails. The temporary file is removed
/// on every failing path that created one.
pub fn write_copy(
    source: &Path,
    plan: &Plan,
    out: &Path,
    password: Option<&str>,
    rewriter: &dyn Rewriter,
) -> Result<Copied, Refusal> {
    if same_file(source, out) {
        return Err(
            "tpdf cannot save over the document it is reading --- choose another name".into(),
        );
    }
    // `Proceed` rather than `Refuse`, which is the one place a copy and a save
    // in place differ on purpose: a copy IS the fallback the in-place refusal
    // points at, so refusing a changed source here would leave a reader whose
    // file moved under them with nowhere at all to put their work. What comes
    // back is the fact, and `Copied` carries it to them.
    let ready = rewrite_ready(source, plan, OnChange::Proceed)?;
    // Named differently from `stage_in_place`'s, deliberately, and the reason is
    // the mutation harness rather than taste: two character-for-character
    // identical calls make one anchor ambiguous, an ambiguous anchor is refused,
    // and the mutation then stops being able to fail. Distinct bindings are the
    // fix; a longer anchor is only the workaround.
    let (mut reading, len) = opened_to_rewrite(source)?;
    let staged = stage(out, |writing| {
        staged_rewrite(
            rewriter,
            &mut reading,
            len,
            writing,
            plan,
            Job::Save,
            password,
        )
        .map(|_| ())
    })?;
    commit(&staged, out)?;
    Ok(Copied {
        changed: ready.changed,
    })
}

/// A split that was written: the files, and whether the source had changed.
///
/// `paths` rather than a count, because the reader chose **one** name and got
/// several --- the numbering rule is this module's and naming the files is the
/// only way the reader learns where they went. `changed` is [`Copied`]'s field
/// carrying that type's whole argument.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Split {
    /// The source changed since it was opened, and the split was written anyway.
    pub changed: bool,
    /// Every file written, in order, as the reader will find them on disk.
    pub paths: Vec<String>,
}

/// The names a split of `count` files writes, derived from the chosen `out`.
///
/// `report.pdf` and 3 gives `report-1.pdf`, `report-2.pdf`, `report-3.pdf`.
///
/// **The chosen name is never one of them**, and that is deliberate rather than
/// an oversight in the numbering. A reader choosing `report.pdf` for a three-way
/// split has not asked for a file called `report.pdf`; writing the first part
/// there would make the set inconsistent --- one unnumbered file and two
/// numbered ones --- and the part that is *not* numbered is the one that reads
/// as the whole document. The cost is that the save dialog may have asked about
/// replacing a file this never writes.
///
/// A `count` of zero gives no names, which the caller refuses before reaching
/// here; the function has no opinion about it because "no files" is not a
/// naming question.
pub fn split_paths(out: &Path, count: usize) -> Vec<PathBuf> {
    // `file_stem` drops the last extension only, so `report.v2.pdf` becomes
    // `report.v2-1.pdf` rather than `report-1.pdf`. That is what a reader who
    // put a dot in a name meant by it.
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".to_string());
    let extension = out
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "pdf".to_string());
    let parent = out.parent().unwrap_or_else(|| Path::new(""));
    (1..=count)
        .map(|n| parent.join(format!("{stem}-{n}.{extension}")))
        .collect()
}

/// Writes one file per plan, numbered from the name the reader chose.
///
/// # Errors
///
/// Fewer than two plans; any destination is the source; **any destination
/// already exists**; or everything [`planned_bytes`] refuses.
///
/// **The existence check covers every file before any is written**, and it is
/// the one refusal here that does not exist for `write_copy`. A single save
/// goes to the path the reader picked in a dialog, so the platform has already
/// asked them about replacing it. A split derives `count - 1` further paths
/// that no dialog ever showed, and [`write_atomically`] finishes with a rename,
/// which replaces. Without this, splitting `report.pdf` into three in a
/// directory that already holds `report-2.pdf` destroys it silently.
///
/// It is a check and not a guarantee: a file appearing between the check and
/// the rename is still replaced. Closing that needs the shared write path to
/// commit with `create_new`, which would change every save in this module, and
/// the value here is turning "destroys files without saying so" into "refuses",
/// not winning the race.
///
/// A failure part-way through leaves the files already written, and the message
/// says which one failed and how many stand --- deleting them would be a second
/// destructive act on a reader who has just been told something went wrong.
pub fn write_split(
    source: &Path,
    plans: &[Plan],
    out: &Path,
    password: Option<&str>,
    rewriter: &dyn Rewriter,
) -> Result<Split, Refusal> {
    if plans.len() < 2 {
        return Err("a split writes at least two files".into());
    }
    let targets = split_paths(out, plans.len());
    for target in &targets {
        if same_file(source, target) {
            return Err(
                "tpdf cannot save over the document it is reading --- choose another name".into(),
            );
        }
        if target.exists() {
            return Err(format!(
                "{} already exists, and a split would replace it --- choose another name",
                target.display()
            )
            .into());
        }
    }

    // **Asked once for the whole split, not once per part.** It is a question
    // about the *file* --- is this still the one the plans were made against ---
    // and the answer cannot differ between two parts of one run without the
    // source having been replaced mid-split, which is a race no number of
    // fingerprints closes. Once is also what stops a 337 MB source being hashed
    // as many times as the reader asked for files.
    let ready = rewrite_ready(source, &plans[0], OnChange::Proceed)?;
    // **Opened once, and every part is rewritten from that one handle.** A part
    // is a rewrite of the same document under a different plan, so re-opening
    // per part would be re-opening a name --- and a source replaced between part
    // two and part three would give the reader a numbered set built from two
    // different documents, with nothing saying so.
    // Named `opened` and `into`, where `write_copy`'s are `reading` and
    // `writing`, and that is the mutation harness rather than taste: two
    // character-for-character identical calls make one anchor ambiguous, an
    // ambiguous anchor is refused, and the mutation stops being able to fail.
    let (mut opened, len) = opened_to_rewrite(source)?;

    let mut written: Vec<String> = Vec::new();
    for (plan, target) in plans.iter().zip(&targets) {
        // The count is what a reader needs to know which files stand, and it is
        // added to whatever refused --- the rewriter's refusal about the document
        // or this side's about the disk, both of which reach here as a
        // `Refusal` and keep the field a window branches on.
        let landed = stage(target, |into| {
            staged_rewrite(rewriter, &mut opened, len, into, plan, Job::Save, password).map(|_| ())
        })
        .and_then(|staged| Ok(commit(&staged, target)?))
        .map_err(|why| Refusal {
            message: format!(
                "{} ({} of {} written)",
                why.message,
                written.len(),
                plans.len()
            ),
            changed: why.changed,
        });
        landed?;
        written.push(target.display().to_string());
    }
    let changed = ready.changed;
    Ok(Split {
        changed,
        paths: written,
    })
}

/// A merge that was written: what it holds, and whether its source had changed.
///
/// `changed` is [`Copied`]'s field and carries that type's whole argument ---
/// the file is on disk and was built from a document that is no longer the one
/// the reader opened, which is a fact to be told rather than a failure.
///
/// `pages` and `files` are here because a merge is the one write path whose
/// result a reader cannot check by looking at what they asked for. An extract of
/// "1-3" produces three pages and they know it; a merge of four documents
/// produces however many pages those documents had, and the number is the only
/// evidence that each of them was really read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Merged {
    /// The source changed since it was opened, and the merge was written anyway.
    pub changed: bool,
    /// How many pages the written file holds, the reader's own document
    /// included.
    pub pages: u32,
    /// How many documents were merged in, not counting the open one.
    pub files: u32,
}

/// Writes the working document followed by every page of `others`, to `out`.
///
/// The open document goes in **as the reader has it** --- turned, cropped,
/// reordered, marked up, with deleted pages gone --- because it is built through
/// [`planned_bytes`], the same function `write_copy` and the print path use. The
/// other files go in **as they are on disk**: they are not open, so there is no
/// working document for them and nothing to apply.
///
/// That asymmetry is worth stating because it is the whole shape of the command.
/// A merge is not an edit: the working document is untouched, nothing is
/// journalled, and undo has nothing to undo --- which is `plan_subset`'s
/// argument for extract, arriving at the other end of the same write path.
///
/// **Every refusal `write_copy` states applies to the open document**, and is
/// not restated: an encrypted source, a page count that disagrees with the
/// baseline, a page the plan names that the file does not have. What is added
/// here is the same encryption refusal for each incoming file, and it is the
/// same refusal for the same reason --- a merged file is one file, and there is
/// no way to write it that preserves two documents' encryption, so silently
/// dropping one document's restrictions is the outcome to refuse.
///
/// # Errors
///
/// `others` is empty; `out` is the source or any file being merged in; anything
/// [`planned_bytes`] refuses about the open document; an incoming file that
/// cannot be parsed, is encrypted, or has no pages; or the write fails.
pub fn write_merged(
    source: &Path,
    plan: &Plan,
    others: &[PathBuf],
    out: &Path,
    password: Option<&str>,
    merger: &dyn Rewriter,
) -> Result<Merged, Refusal> {
    if others.is_empty() {
        return Err("choose at least one document to merge in".into());
    }
    // The open document and every file going in, against the one destination.
    // A merge reads several files and writes one, so "do not write over an
    // input" is a claim about a *set* rather than about a single path --- and
    // written as a loop rather than as `write_copy`'s check plus a copy of it,
    // which is the second reason: a second `if same_file(source, out) {` in
    // this file makes the mutation aimed at that line ambiguous, and an
    // ambiguous anchor is refused, so the mutation stops being able to fail.
    // `docs/TRAPS.md` has the entry, and it says the fix is to stop having two
    // near-copies rather than to lengthen the anchor.
    //
    // Before anything is parsed, so a reader who picked the destination by
    // mistake is told rather than kept waiting for work that will be thrown
    // away.
    for input in [source]
        .into_iter()
        .chain(others.iter().map(PathBuf::as_path))
    {
        if !same_file(input, out) {
            continue;
        }
        return Err(if input == source {
            // `write_copy`'s wording, deliberately: it is the same refusal
            // about the same file, and a reader who meets it in two commands
            // should not have to work out whether they are the same rule.
            Refusal::from(
                "tpdf cannot save over the document it is reading --- choose another name",
            )
        } else {
            Refusal::from(format!(
                "tpdf cannot write the merge over {}, which is one of the documents going into it",
                name_of(input)
            ))
        });
    }

    // **The base takes the reader's password and the incoming files do not, and
    // that asymmetry is the rule rather than an oversight.** `source` is the
    // document on screen, so a rewrite of it can keep its own encryption. An
    // incoming file is refused a few lines below because there is no way to
    // write one file that preserves *two* documents' encryption --- and tpdf
    // holds no key for those anyway, having never opened them.
    // **Read, not parsed**, and that distinction is the whole of what moved on
    // 2026-09-01. Filling a buffer with a file's bytes asks nothing about what
    // they mean; every `lopdf` load that used to happen here now happens in a
    // sandboxed worker. See `crate::worker_proto::Request::Merge`.
    let (whole, incoming) = concatenated(others)?;

    // The fingerprint, which is a question about the *file* and needs filesystem
    // authority no worker has. `OnChange::Proceed`, because a merge writes
    // somewhere new: a source that changed under the reader is recorded and
    // reported rather than refused.
    //
    // **Bound as `base` rather than `ready`, which is `write_copy`'s word.** Two
    // identical lines make one anchor ambiguous, an ambiguous anchor is refused,
    // and the mutation aimed at it then stops being able to fail --- the trap
    // this function's own comment above cites, arriving here the moment the
    // merge started staging the way the copy does. Distinct bindings are the
    // fix; a longer anchor is only the workaround.
    let base = rewrite_ready(source, plan, OnChange::Proceed)?;
    // The handle, never the pathname handed on --- see `opened_to_rewrite`.
    let (mut reading, len) = opened_to_rewrite(source)?;
    let mut pages = 0;
    let staged = stage(out, |writing| {
        staged_merge(
            merger,
            &mut reading,
            len,
            writing,
            plan,
            Inputs {
                whole: &whole,
                each: &incoming,
            },
            password,
        )
        .map(|counted| pages = counted)
    })?;
    commit(&staged, out)?;
    Ok(Merged {
        changed: base.changed,
        pages,
        files: others.len() as u32,
    })
}

/// Reads every incoming document into one buffer, and says where each begins.
///
/// **The coordinator's whole part in a merge's inputs.** It opens the files the
/// reader chose and copies their bytes; it does not parse them, and after
/// 2026-09-01 nothing in this process does. The buffer and the spans are what
/// cross to the worker --- one mapping rather than one per file, for the reason
/// [`crate::worker::IN_FD`] gives.
///
/// # Errors
///
/// A file that cannot be read, named by the name the reader saw.
fn concatenated(others: &[PathBuf]) -> Result<(Vec<u8>, Vec<Incoming>), Refusal> {
    let mut inputs = Vec::new();
    let mut incoming = Vec::with_capacity(others.len());
    for other in others {
        let bytes = std::fs::read(other)
            .map_err(|e| Refusal::from(format!("could not read {}: {e}", name_of(other))))?;
        incoming.push(Incoming {
            at: inputs.len(),
            len: bytes.len(),
            // The name the reader saw in the dialog, resolved here because this
            // is where the path is. See `Incoming::label`.
            label: name_of(other),
        });
        inputs.extend_from_slice(&bytes);
    }
    Ok((inputs, incoming))
}

/// A path as it should appear in a message to the reader.
///
/// The file name rather than the whole path. Two files with one name are
/// possible and the reader chose both of them a moment ago; a message carrying
/// two absolute paths is one nobody reads.
fn name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// Writes the working document beside `source`, ready to be put in its place.
///
/// The first half of saving over the open file. What comes back is the path of
/// the sibling temporary file, which nothing has renamed yet.
///
/// **Two kinds of refusal, and only one of them is still free.** This paragraph
/// used to say that every guard runs before anything the reader has is
/// disturbed, and half of that stopped being true when the rewrite moved into a
/// worker. The questions about the *file* --- is this still the one the edits
/// were made against, and can tpdf tell --- are [`rewrite_ready`]'s, and they
/// still run before anything is created. The questions about the *document* are
/// [`rewrite_update`]'s, and they cannot be asked until there is somewhere for
/// the answer to be written, so they arrive after the temporary file exists.
///
/// What makes that safe is narrower than what the old sentence claimed, and it
/// is the whole of it: [`stage`] removes the partial on every failing path, and
/// nothing has been renamed either way.
///
/// **The split exists because the document has to be closed in between.** A
/// `rename` over a file some process has memory-mapped succeeds on macOS and
/// leaves the mapping serving the inode that is no longer at that path ---
/// measured, and the reason it is worth splitting a function over: the reader's
/// worker would go on rendering the document as it was before the save,
/// indefinitely and without anything looking wrong. Windows refuses the rename
/// outright while a section is open, so the two platforms fail differently and
/// only one of them fails visibly.
///
/// So the caller stages, closes the document, and only then commits. See
/// `lib.rs`'s `save_document`, which is the only caller and holds the ordering.
///
/// # Errors
///
/// Everything [`rewrite_ready`] and [`rewrite_update`] refuse; a source that
/// cannot be opened or measured; a rewriter that wrote a different number of
/// bytes than it reported; and a temporary file that cannot be written. The
/// temporary file is removed on every failing path that created one.
pub fn stage_in_place(
    source: &Path,
    plan: &Plan,
    password: Option<&str>,
    rewriter: &dyn Rewriter,
) -> Result<Staged, Refusal> {
    // Refused here and tolerated by `write_copy`, which is the one place the two
    // paths differ on purpose. A fingerprint that could not be taken means tpdf
    // cannot tell whether this file is still the file -- and the two operations
    // have different stakes: writing a copy risks a bad new file beside an intact
    // original, writing in place risks the original. So the fallback the message
    // names has to keep working, or the refusal strands the reader.
    if plan.opened_as.is_none() {
        return Err(
            "tpdf could not record what this file looked like when it was opened, \
                    so it cannot tell whether saving over it is safe --- use Save a copy"
                .into(),
        );
    }
    let ready = rewrite_ready(source, plan, OnChange::Refuse)?;
    // Refused rather than unwrapped. It cannot fire --- `rewrite_ready` derives
    // this from the same `plan.opened_as` the guard above just proved present ---
    // but the two are eight lines and one function call apart, and the failure a
    // panic would replace is a *refused save*, which is the outcome every other
    // branch here already produces. A guard that turns an internal inconsistency
    // into the safe answer costs three lines and is not the unreachable guard the
    // repository has a rule about deleting.
    let verified = ready.verified.ok_or_else(|| {
        "tpdf could not confirm what this file was when it read it --- use Save a copy".to_string()
    })?;

    // The document, not the name it was reached by --- see `resolved`. Staging
    // beside the target is also what puts the temporary file on the same
    // filesystem as the thing it will replace, which is what makes the rename
    // atomic rather than a copy.
    let target = resolved(source);
    // Opened here and read through, never re-opened by name --- the reason is
    // on `opened_to_rewrite`, which is where the three paths that do it share it.
    let (mut file, len) = opened_to_rewrite(&target)?;

    // **The document refusals now arrive after the temporary file exists**, and
    // that is a change worth naming rather than leaving to be noticed. They used
    // to be made before anything was created, which the comment on this function
    // still gives as the reason for the split; what makes it safe is that
    // `stage` removes the partial file on every failing path, including this
    // one, and that nothing has been renamed either way. What is *not* given up
    // is the free half: `rewrite_ready` above still refuses a changed file
    // before a byte is written anywhere.
    let path = stage(&target, |out| {
        staged_rewrite(rewriter, &mut file, len, out, plan, Job::Save, password).map(|_| ())
    })?;
    Ok(Staged { path, verified })
}

/// Has `rewriter` fill a staging file, and checks that what landed is what it said.
///
/// **The one statement of what a delegated rewrite has to satisfy**, read by
/// every path that has one: [`stage_in_place`], [`write_copy`] and
/// [`write_split`]. It was inline in the first of those while it was the only
/// one, and a second copy of it is how the paths would come to disagree about
/// what a completed write is --- `docs/TRAPS.md` has that under *two copies of a
/// distinction drift*, and this module has already paid for it once with two
/// spellings of the same `same_file` guard.
///
/// **The check is here rather than in the rewriter because only this side can
/// make it**: the writer says how many bytes it wrote and this file says how
/// many it has, and the two are independent statements about the same write. A
/// short write, a second rewrite appending to the first, or a reply built for
/// another request all disagree here. Neither number is derived from the other,
/// which is what makes it a check rather than a restatement --- and it is the
/// only one there can be, because on the worker path the bytes never enter this
/// process.
///
/// # Errors
///
/// Everything the rewriter refuses, the staged file not being measurable, and a
/// length that disagrees with the one reported.
fn staged_rewrite(
    rewriter: &dyn Rewriter,
    source: &mut std::fs::File,
    len: usize,
    out: &mut std::fs::File,
    plan: &Plan,
    job: Job,
    password: Option<&str>,
) -> Result<usize, Refusal> {
    let wrote = rewriter.write(source, len, out, plan, job, password)?;
    landed_is(out, wrote)
}

/// Checks a writer's reported length against the file's own size.
///
/// **The whole of what stands between a lying or truncated write and a
/// destination**, on both routes that hand a document to a writer this process
/// does not contain: [`staged_rewrite`] and [`staged_range`]. The bytes never
/// reach this process, so its own size is the only reading available.
///
/// One function rather than one per caller, because two copies of it would be
/// two statements of what a completed write looks like --- and a mutation of one
/// would survive, which `docs/TRAPS.md` records under *Two copies of a
/// distinction drift*.
///
/// # Errors
///
/// The file cannot be measured, or its size is not the length reported.
fn landed_is(out: &std::fs::File, wrote: usize) -> Result<usize, Refusal> {
    let landed = out
        .metadata()
        .map_err(|e| format!("could not measure the staged file: {e}"))?
        .len();
    if landed != wrote as u64 {
        return Err(format!(
            "the rewritten document was reported as {wrote} bytes and the staged file is \
             {landed} --- the save was not completed"
        )
        .into());
    }
    Ok(wrote)
}

/// Builds a print job for the page range `job` names, into `out`.
///
/// [`staged_rewrite`]'s counterpart for the one route that carries no plan. The
/// cross-check below is the same one and is the same function, because what it
/// asserts --- that the writer's reported length is the file's size --- is a
/// property of the channel rather than of the instruction sent down it.
///
/// # Errors
///
/// Anything [`crate::print::build_update`] refuses, the write failing, or a
/// length that disagrees with the file.
fn staged_range(
    rewriter: &dyn Rewriter,
    source: &mut std::fs::File,
    len: usize,
    out: &mut std::fs::File,
    job: &crate::print::Job,
) -> Result<usize, Refusal> {
    let wrote = rewriter.write_range(source, len, out, job)?;
    landed_is(out, wrote)
}

/// Merges `source` and `inputs` into `out`, and checks what landed.
///
/// [`staged_rewrite`]'s counterpart for the widest instruction. The same
/// cross-check for the same reason: the bytes never reach this process, so the
/// staged file's own size is the only reading of what was written.
///
/// # Errors
///
/// Anything [`merge_update`] refuses, the write failing, or a length that
/// disagrees with the file.
fn staged_merge(
    rewriter: &dyn Rewriter,
    source: &mut std::fs::File,
    len: usize,
    out: &mut std::fs::File,
    plan: &Plan,
    inputs: Inputs<'_>,
    password: Option<&str>,
) -> Result<u32, Refusal> {
    let (wrote, pages) = rewriter.merge(source, len, out, plan, inputs, password)?;
    landed_is(out, wrote)?;
    Ok(pages)
}

/// Opens a document to be rewritten, and measures it.
///
/// **A handle and a length, never a pathname handed on**, which is the whole
/// reason it exists as a step of its own on three paths. Everything the caller
/// established about this file --- that it is still the one the plan was made
/// against, or that it is not --- is about *this* file, and a rewriter that
/// opened the path again would apply the plan to whatever has that name by
/// then. It is the same race [`Reread`] closes on the way out, closed here on
/// the way in.
///
/// # Errors
///
/// The file cannot be opened or measured.
fn opened_to_rewrite(source: &Path) -> Result<(std::fs::File, usize), Refusal> {
    let file = std::fs::File::open(source)
        .map_err(|e| format!("could not open {source:?} to read it: {e}"))?;
    let len = file
        .metadata()
        .map_err(|e| format!("could not measure {source:?}: {e}"))?
        .len() as usize;
    Ok((file, len))
}

/// How a save should be written.
///
/// **The strictest mode present wins**, which `docs/PLAN.md` §5 states as the
/// rule and which today has two answers rather than three: `Forbidden` is not
/// here, because nothing yet refuses a save on a certified document --- §5 says
/// plainly that a signature cannot survive an edit whatever the DocMDP level
/// permits, and what to *do* about that is a decision about the product rather
/// than a mode.
///
/// Chosen from the plan and the file's size, so it can be exercised without a
/// document open, and named rather than decided inside the command for the
/// reason every guard in this module is out here: a branch inside a Tauri
/// command has no failing case a test can reach.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Append an update section. Fast on a large file, and only for an edit that
    /// touches nothing but annotations --- see [`Plan::is_appendable`].
    Append,
    /// Reserialise the whole document. Correct for every plan, and what every
    /// save did until 2026-08-22.
    Rewrite,
}

/// The largest document whose save may be prepared inside a worker.
///
/// **A bound on the input, because the parse is what costs.** [`append_update`]
/// re-parses the document with `lopdf` and carries a discarded copy of the
/// previous revision, which measures at roughly three times the file's size in
/// private memory: 336.6 MB of scanned PDF takes a worker to **980.3 MiB**.
///
/// A Windows worker is capped at `sandbox_win::WORKER_MEMORY_CAP`, 1 GiB of
/// commit, so past that the allocation is refused and the worker aborts.
/// Measured 2026-08-22 on `MOTHERSHIP`: a 345.0 MB scan prepares at 98.1% of the
/// cap, a 361.9 MB scan does not prepare at all, and the largest fixture in the
/// repository sits at 95.7%. `BUILD.md` has the table.
///
/// **Applied on both platforms, and macOS is the reason rather than the
/// exception.** It has no cap, and that is the worse case: `docs/THREAT-MODEL.md`
/// T3 records that its worker has no memory bound at the kernel at all, so an
/// unbounded parse there is bounded by the machine. One rule with one reason ---
/// the parse is too large to do in a worker --- rather than a Windows constant
/// leaking into a decision the other platform also needs.
///
/// 256 MiB leaves a worker at roughly 780 MiB of the 1 GiB. **A judgement, not a
/// measurement**: the ceiling is one machine, one PDFium build and one
/// document's mix of content, so this sits well under it rather than at it.
pub const APPEND_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// The bound stays under the largest document measured to prepare successfully.
///
/// 345,040,737 bytes is a 41-page synthetic scan that reached 98.1% of a Windows
/// worker's commit cap on 2026-08-22; the 43-page one above it aborted. Checked
/// when the crate is built rather than in a test, because it relates two
/// constants and a run-time assertion between two constants is one that cannot
/// fail --- which is a thing this repository refuses elsewhere and should refuse
/// here. Raising `APPEND_MAX_BYTES` past a measured ceiling stops the build.
const _: () = assert!(APPEND_MAX_BYTES < 345_040_737);

/// Which writer a plan needs.
///
/// The append is not merely a faster rewrite and the difference is not
/// performance: an update section leaves the previous revision **byte for byte
/// intact** inside the new file, so what was signed stays exactly where it was
/// and a validator can still show it. A rewrite renumbers every object in the
/// document.
///
/// The narrowness is deliberate and is bounded by evidence rather than by
/// caution: spike 0.6 put an appended annotation to PDFium, QPDF, poppler and
/// CoreGraphics across twelve fixtures. It never put an appended page deletion,
/// reorder, rotation or crop to any of them. Those take the rewrite until
/// somebody measures them.
///
/// **Size is the second condition, and it costs something real.** Above
/// [`APPEND_MAX_BYTES`] a marks-only save is reserialised, which means the
/// previous revision does *not* survive byte for byte --- so a signed revision
/// that an append would have left intact for a validator to show is gone. That
/// is a genuine loss and it is the better of the two outcomes available: the
/// alternative is not an append, it is a save that cannot be completed at all,
/// because the worker is refused the memory to prepare one. Saving a large
/// document differently is worth more than failing to save it identically.
///
/// What would remove the trade rather than choose a side is making the parse
/// cheaper --- `docs/PLAN.md` §3 --- at which point this bound rises and the
/// question stops being asked of documents this size.
#[must_use]
pub fn mode_for(plan: &Plan, source_bytes: u64) -> Mode {
    if plan.is_appendable() && source_bytes <= APPEND_MAX_BYTES {
        Mode::Append
    } else {
        Mode::Rewrite
    }
}

/// [`mode_for`], measuring the file itself.
///
/// **A file that cannot be measured takes the rewrite.** That is the arm with no
/// memory bound over it and it is correct for every plan, so an unreadable
/// `metadata` costs a slower save rather than a worker refused the memory to
/// prepare a fast one. `AGENTS.md` records a migration whose
/// `if (checked -and safe) {stop}` collapsed "checked, fine to proceed" with
/// "could not check at all" and force-pushed on the second; the same shape with
/// the branches the other way round is this one.
///
/// Out here rather than in the command for the module's usual reason, and it is
/// load-bearing rather than habitual: the `u64::MAX` below is the whole guard,
/// and inside a Tauri command nothing could reach it to prove it still points
/// the safe way.
#[must_use]
pub fn mode_for_source(plan: &Plan, source: &Path) -> Mode {
    mode_for(
        plan,
        std::fs::metadata(source).map_or(u64::MAX, |m| m.len()),
    )
}

/// A save that has been prepared as an append: the bytes to add, and the length
/// they go after.
///
/// [`Staged`]'s counterpart for the other mode, and it carries a *length* where
/// that carries a path. The distinction is the whole difference between the two
/// modes: a rewrite produces a new file and renames it over the old one, and an
/// append adds to the file that is already there --- so what has to be recorded
/// is where the previous revision ended, both to write after it and to cut back
/// to if anything goes wrong.
///
/// `verified` is not an `Option`, for exactly [`Staged`]'s reason: the caller's
/// last look before it writes goes through this field, and a `None` arm could
/// only be written as "skip the check".
#[derive(Debug)]
pub struct Appended {
    /// The update section: objects, cross-reference and trailer.
    update: Vec<u8>,
    /// How long the file was when the update was built against it.
    was: u64,
    /// How many pages it has, which an append does not change: it adds
    /// annotations. Carried rather than recomputed, because the verification
    /// runs after the file has been written and re-deriving it would mean
    /// parsing the document again to check the first parse.
    pages: usize,
    /// The source as it was when its bytes were read.
    pub verified: Fingerprint,
}

impl Appended {
    /// How many bytes the save will add. For a report, and for the tests.
    #[must_use]
    pub fn len(&self) -> usize {
        self.update.len()
    }

    /// Whether the update section is empty, which nothing produces.
    ///
    /// Present because clippy asks for it beside `len`, and answering honestly
    /// is cheaper than an allow: `lopdf` always writes at least a cross-reference
    /// and a trailer, so this is false for every value this type ever holds.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.update.is_empty()
    }
}

/// Builds the update section for a plan that only adds marks.
///
/// **The whole of what makes an append fast is that the previous revision is
/// never rewritten**, so nothing here reads or copies the file's existing bytes
/// beyond the parse every edit has to pay. `docs/PLAN.md` §5 measured the
/// difference where it shows: 29 ms against 239 ms on a 337 MB scan, and 723
/// bytes written against 336,623,496.
///
/// Only the objects that actually change are written. Rewriting an object to
/// hold the value it already had is still a change to a structural object, which
/// matters to anything comparing revisions --- so a page whose `/Annots` is its
/// own object never has its dictionary touched. Which shape a page has is
/// [`AnnotsSite`]'s answer, read from the previous revision.
///
/// # Errors
///
/// The plan is empty or is not append-shaped; `source` changed since it was
/// opened, or cannot be read, parsed or measured; it is encrypted; its page
/// count is not the plan's baseline; or a mark maps to nothing.
pub fn append_bytes(
    source: &Path,
    plan: &Plan,
    password: Option<&str>,
) -> Result<Appended, Refusal> {
    let ready = append_ready(source, plan)?;
    let original = std::fs::read(source).map_err(|e| format!("could not read {source:?}: {e}"))?;
    appended(ready, append_update(original, plan, password)?)
}

/// What the caller established about the file before anything parsed it.
///
/// The parent's half of an append, and the split is the boundary: everything in
/// here is a question about a *path* --- has this file changed, how long is it ---
/// and answering it needs filesystem authority and no parser. Everything in
/// [`append_update`] is a parse of attacker-controlled bytes and needs no
/// filesystem at all. `docs/THREAT-MODEL.md` §T6 and residual risk 18.
#[derive(Debug)]
pub struct Ready {
    was: u64,
    verified: Fingerprint,
}

impl Ready {
    /// How long the file was when it was checked. For a caller reporting.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.was
    }

    /// Whether that length is zero, which no PDF is.
    ///
    /// Present because clippy asks for it beside `len`; a file of no bytes has
    /// already been refused as unparseable long before this.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.was == 0
    }
}

/// Checks the file an append is about to be built against, without parsing it.
///
/// # Errors
///
/// The plan is not append-shaped or has no fingerprint; the file changed since
/// it was opened; or it cannot be measured.
pub fn append_ready(source: &Path, plan: &Plan) -> Result<Ready, Refusal> {
    let was = std::fs::metadata(source)
        .map_err(|e| format!("could not measure {source:?}: {e}"))?
        .len();
    // **Asks [`mode_for`] rather than repeating its rule.** This used to test
    // `is_appendable` itself, which was one condition and therefore harmless
    // to say twice; with a size bound beside it, two copies of the rule is two
    // places for it to drift, and `docs/TRAPS.md` records what a second copy
    // does to a differential. The measurement moved above the guard for the
    // same reason --- the guard needs the number.
    if mode_for(plan, was) != Mode::Append {
        return Err("this document needs a full rewrite rather than an append".into());
    }
    let opened_as = plan.opened_as.as_ref().ok_or_else(|| {
        "tpdf could not record what this file looked like when it was opened, \
         so it cannot tell whether saving over it is safe --- use Save a copy"
            .to_string()
    })?;
    let verified = opened_as.agrees_with(source).map_err(Refusal::changed)?;
    Ok(Ready { was, verified })
}

/// Puts a builder's answer together with what the caller checked itself.
///
/// **The one place the two halves meet, and it is a comparison rather than a
/// hand-off.** The update section's byte offsets and `/Prev` are measured from
/// the length of the bytes it was built against; the caller separately measured
/// and hashed a file. If those two lengths differ, the builder was looking at
/// something other than the file the caller is about to write to --- a worker
/// holding a stale mapping, or a file that changed between the two --- and the
/// resulting cross-reference would point at the wrong bytes in a file that still
/// opens. Neither half can see that alone, which is exactly why it is checked
/// here.
///
/// Until 2026-08-22 there was nothing to compare: one function read the file and
/// built the update from what it had read, so the two lengths were the same
/// number by construction. That is no longer true once the parse happens in
/// another process, and a property that used to hold by construction is the kind
/// that needs an assertion the moment it stops.
///
/// # Errors
///
/// The builder worked from a different number of bytes than the caller checked.
pub fn appended(ready: Ready, update: Update) -> Result<Appended, Refusal> {
    if update.built_against as u64 != ready.was {
        return Err(Refusal::changed(format!(
            "these edits were built against {} bytes and the file is {} --- \
             reopen it before saving",
            update.built_against, ready.was
        )));
    }
    Ok(Appended {
        update: update.update,
        was: ready.was,
        pages: update.pages,
        verified: ready.verified,
    })
}

/// What [`append_update`] produces: the bytes to add, and what they were built
/// against.
///
/// **Crosses the worker boundary**, which is why it is a type of its own rather
/// than an [`Appended`] with the file facts left blank. `Appended` carries a
/// [`Fingerprint`] and a file length --- facts about a *path*, which the worker
/// has no access to and no business asserting. What comes back from a process
/// holding a hostile document is only what it built and what it says it built
/// against, and the caller checks the second against what it fingerprinted
/// itself.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Update {
    /// The update section: objects, cross-reference and trailer.
    pub update: Vec<u8>,
    /// How many pages the document had. An append does not change it.
    pub pages: usize,
    /// How long the bytes this was built against were.
    ///
    /// **Stated by the builder and checked by the caller**, which is the point
    /// of carrying it: the update's byte offsets and `/Prev` are measured from
    /// this, so a caller that fingerprinted a file of a different length is
    /// holding an update for a document it does not have.
    pub built_against: usize,
}

/// Builds the update section for a plan that only adds marks, over bytes.
///
/// **Pure, and that is the whole point of it existing separately.** Everything
/// here is parsing and serialising attacker-controlled bytes; nothing here opens
/// a file, names a path, or knows one exists. That is what lets it run in the
/// worker that already holds the document --- which parses it with `lopdf`
/// already, for comments, links and properties --- instead of in the process
/// holding the window and the reader's filesystem authority.
/// `docs/THREAT-MODEL.md` §T6 and residual risk 18 carry what that changes.
///
/// **`password` is the reader's, when the document needed one.** It is a key to
/// bytes this process already holds rather than a new authority --- the same
/// argument [`crate::progressive::OpenDocument::open_bytes`] makes for handing it
/// to PDFium --- and it is what makes an append the *only* save an encrypted
/// document can have: `lopdf` re-encrypts every appended object with the
/// original key and restores the trailer's `/Encrypt`, where its full serialiser
/// writes plaintext and drops the dictionary. Measured by spike 0.6, and by
/// `examples/password_probe.rs` through the production path.
///
/// # Errors
///
/// The plan is not append-shaped; the bytes cannot be parsed; the document is
/// encrypted and no password opened it; its page count is not the plan's
/// baseline; or a mark maps to nothing.
pub fn append_update(
    original: Vec<u8>,
    plan: &Plan,
    password: Option<&str>,
) -> Result<Update, Refusal> {
    if !plan.is_appendable() {
        // Not a reader-facing refusal: nothing offers this mode, `mode_for`
        // chooses it, and a caller reaching here with the wrong plan has a
        // defect rather than a document problem. Refused rather than
        // debug-asserted, because the safe answer exists and is the rewrite.
        return Err("this document needs a full rewrite rather than an append".into());
    }
    let was = original.len() as u64;
    let built_against = original.len();

    let prev = Document::load_mem_with_options(
        &original,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            password: password.map(str::to_string),
            ..Default::default()
        },
    )
    .map_err(|e| format!("this document could not be parsed: {e}"))?;

    // **Where the two save paths part company, and the only place in this file
    // they do.** The rewrite refuses an encrypted document outright because
    // `lopdf`'s full serialiser writes every object in the clear and drops the
    // `/Encrypt` dictionary with it. An append does the opposite: the previous
    // revision's bytes are never rewritten, and `IncrementalDocument::save_to`
    // encrypts each appended object with the state the load recorded and puts
    // `/Encrypt` back in the appended trailer. So `was_encrypted` --- there was
    // encryption and the load holds its key --- goes *through* here and is
    // refused by `planned_bytes`.
    //
    // What is refused is a document still locked, which is what `is_encrypted`
    // reports: no password opened it, so `lopdf` parsed no objects at all and
    // the page walk below would see an empty document. `lopdf` refuses this
    // itself in `check_incremental_save_supported`, which is a second reader of
    // the same fact and not a reason to skip the first --- its message names an
    // issue number.
    if prev.is_encrypted() {
        return Err(
            "This document is encrypted and tpdf could not unlock it, so it cannot be \
             saved. Open it with its password first."
                .into(),
        );
    }

    let pages = ordered_pages(&prev);
    if pages.len() != plan.baseline as usize {
        return Err(Refusal::changed(format!(
            "the document on disk has {} page(s) and the edits were made against {} --- it has \
             changed since it was opened, so reopen it before saving",
            pages.len(),
            plan.baseline
        )));
    }

    // **Built from the plan rather than taken to be `pages`, which it is.**
    // `is_appendable` requires `pages_are_the_file`, so position *n* of the plan
    // is baseline page *n* and this loop is the identity today. It is written
    // out because a mark is addressed by its position and the resolution is
    // what makes that address mean a page object --- an append that ever
    // carried something other than the file's own pages, in the file's own
    // order, would otherwise put every mark on the wrong page while every
    // existing test went on passing. The refusal is the guard for that, not a
    // case a reader can reach: a plan with a page tpdf made in it is not
    // appendable, and this is the second reader of that fact.
    let sheet: Vec<ObjectId> = plan
        .pages
        .iter()
        .map(|page| match page.source {
            PageSource::Baseline(number) => pages.get(number as usize).copied().ok_or_else(|| {
                Refusal::from(format!(
                    "a mark names page {}, which this document does not have",
                    number + 1
                ))
            }),
            PageSource::Blank(_) => Err(Refusal::from(
                "a save that only adds marks cannot carry a page tpdf made".to_string(),
            )),
        })
        .collect::<Result<_, Refusal>>()?;
    let sites = mark_sites(&prev, &sheet, &plan.marks)?;

    // **Moved rather than copied, and this buys nothing --- it is written this
    // way so that the one copy is visible at the caller instead of hidden here.**
    //
    // `IncrementalDocument` keeps the previous revision's bytes because
    // `save_to` writes them through ahead of the update. `Tail` below throws
    // every one of them away, so the buffer exists to be discarded, and reading
    // `lopdf`'s writer shows it needs only two facts about it: the **length**,
    // which is what advances `bytes_written` and therefore makes the update's
    // offsets correct, and the **last byte**, which decides whether a newline is
    // emitted before the appended revision. A 337 MB buffer is carried to supply
    // a number and a byte, and `create_from` takes `Vec<u8>` so there is no way
    // to hand it less.
    //
    // The signature took `&[u8]` and called `to_vec()` for a day. Changing it to
    // move an owned buffer looked like removing that copy and did not: the
    // worker's document is a read-only mapping, so `into_owned()` at the call
    // site costs exactly what `to_vec()` cost here. `worker-probe` measured both
    // and reported **+667.0 MB either way** on the 337 MB scan, to four
    // significant figures --- which is what an edit that changed nothing looks
    // like. The 667 is the sum of this buffer and the parsed object graph, and
    // both are `lopdf`'s rather than ours.
    //
    // It matters because a Windows worker is capped at 1 GiB of *commit* by its
    // job object, and **that reasoning was wrong, measured on Windows
    // 2026-08-22.** What stood here compared the 667 rather than the 1029.8
    // macOS footprint, on the grounds that the document's mapping is file-backed
    // and not commit. The mapping half is right and the conclusion is not: macOS
    // `phys_footprint` excludes *clean* file-backed pages, so the mapping is
    // absent from the 1029.8 too, and the 362.7 MB baseline it was taken for is
    // PDFium's own allocation --- private commit on Windows exactly as it is
    // anonymous memory on macOS. The two metrics measure the same thing here and
    // agree to 0.2%: `worker-probe` on this fixture peaks at **980.3 MiB of
    // commit (1027.9 MB)** against the macOS 1029.8. So the whole footprint was
    // the term to compare, and the margin is **43.7 MiB, 4.3%**, not the 35% the
    // 667 suggested.
    //
    // Bracketed rather than extrapolated: a 345.0 MB scan saves at 98.1% of the
    // cap, a 361.9 MB scan dies. Above roughly **350 MB an append cannot be
    // built on Windows** --- the allocation fails, the worker aborts, and
    // `save_document` refuses before it closes the document, so nothing is
    // written and the reader keeps their edits. `BUILD.md` and `docs/PLAN.md`
    // §3 carry the run.
    // **Checked against the previous revision, which is where a reply's parent
    // lives, and before `prev` is moved into the incremental document.** The new
    // document is empty from here on --- an append writes only what changed ---
    // so asking *it* about an annotation the file already had would refuse every
    // honest reply. See [`RepliesChecked`].
    let replies = check_replies(&prev, &plan.marks)?;
    let mut incremental = IncrementalDocument::create_from(original, prev);
    // **Brought across before anything is written, and only what changes.** A
    // page whose `/Annots` is its own object contributes that array and nothing
    // else; a page with an inline list or none contributes its dictionary.
    // `opt_clone_object_to_new_document` is clone-*if-absent*, which is what
    // makes two marks on one page safe: the second finds the first's work rather
    // than replacing it with a fresh copy of the original.
    for site in &sites {
        match &site.annots {
            AnnotsSite::ArrayObject(array) => incremental
                .opt_clone_object_to_new_document(*array)
                .map_err(|e| format!("could not bring this page's /Annots across: {e}"))?,
            AnnotsSite::Inline(_) | AnnotsSite::Absent => incremental
                .opt_clone_object_to_new_document(site.page)
                .map_err(|e| format!("could not bring this page across: {e}"))?,
        }
    }

    // The proof is discarded here, and that is the honest thing to do rather
    // than a leak: an append writes marks and *nothing else*. There is no
    // rotation and no crop on this path --- `append_ready` refuses a plan
    // carrying either --- so there is no later step for the token to gate. The
    // binding says so out loud, because `#[must_use]` is right to ask.
    let _marks_are_all_this_path_writes =
        write_marks(&mut incremental.new_document, &plan.marks, &sites, replies)
            .map_err(Refusal::from)?;

    // **Comments that were already in the file, overridden in place.** An
    // incremental update writes a *new version of an object*, which is exactly
    // what changing somebody else's note is --- so this needs no new machinery
    // beyond bringing the object across, the way each page's `/Annots` is
    // brought across above. `opt_clone_object_to_new_document` is clone-if-
    // absent, so two edits to one annotation find each other's work.
    write_note_edits(&mut incremental, &plan.notes)?;

    // **The previous revision is thrown away as it is written**, which is the
    // point: `IncrementalDocument::save_to` writes the whole prior file through
    // to the target before appending, and materialising that is exactly the copy
    // an append exists not to make. On a 337 MB scan it is the entire cost.
    let mut sink = Tail {
        skip: usize::try_from(was).unwrap_or(usize::MAX),
        seen: 0,
        tail: Vec::with_capacity(4096),
    };
    incremental
        .save_to(&mut sink)
        .map_err(|e| format!("could not build the update section: {e}"))?;

    Ok(Update {
        update: sink.tail,
        pages: pages.len(),
        built_against,
    })
}

/// A sink that discards the first `skip` bytes and keeps the rest.
///
/// Spike 0.6's `TailSink`, and it is here for the same reason it was there: an
/// append that first materialised a copy of the document would cost what a
/// rewrite costs, which is the whole thing being avoided.
struct Tail {
    skip: usize,
    seen: usize,
    tail: Vec<u8>,
}

impl std::io::Write for Tail {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let start = self.skip.saturating_sub(self.seen).min(buf.len());
        self.tail.extend_from_slice(&buf[start..]);
        self.seen += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The last look before the rename, and the cleanup when it refuses.
///
/// Split out of `save_document` rather than written inline there for the reason
/// the other two guards are: a check inside a Tauri command has no failing case
/// a test can reach, and `docs/TRAPS.md` records that costing real defects twice
/// over. The comment in `lib.rs` cited that rule for the deep check while this
/// one sat inline three lines below it.
///
/// Only length and modification time, and only against what [`stage_in_place`]
/// read. Staging rewrites the whole document and closing it is a round trip to
/// the worker, so a window exists; a third full read to narrow one measured in
/// milliseconds is the wrong trade, and the timestamp is the best evidence
/// available at that price.
///
/// The staged file is removed before returning, because nothing else will: the
/// caller is past the point where it tracks temporary files, and a refusal that
/// leaves one behind puts a file the reader never named beside their document.
///
/// # Errors
///
/// `source` changed since staging read it, or its metadata cannot be read. The
/// message says that nothing was written and that the document is closed, which
/// are the two facts the reader needs --- and stops there. What to *do* is the
/// caller's, because the caller is the only one that knows what it did next.
pub fn verify_before_commit(staged: &Staged, source: &Path) -> Result<(), Refusal> {
    if let Err(why) = staged.verified.agrees_shallowly(source) {
        let _ = std::fs::remove_file(&staged.path);
        // Its own advice, not `agrees_with`'s. That one says the reader's edits
        // are still here and to save them under another name, which was true
        // where it is written and is false here: the close two statements up in
        // `save_document` has already spent the journal. The two sentences used
        // to arrive in one message, contradicting each other.
        // The fact, and no instruction. It ended "open the file again to see what
        // is there now" until 2026-08-19, and `save_document`'s caller reopens
        // the file by itself on every `after_close` --- so the advice was
        // addressed to somebody it had already been carried out for. Same shape
        // as the two-moments message this file was caught on the same week, one
        // layer up: the producer states what happened, and the caller, which is
        // the only one that knows what it did next, owns what to do about it.
        return Err(Refusal::changed(format!(
            "{why} --- nothing was written, so the file on disk is untouched. \
             Your document has been closed and the edits you had made are gone."
        )));
    }
    Ok(())
}

/// Adds a prepared update section to the end of the file it was built against.
///
/// **The one write in this module that is not a rename**, and the difference is
/// worth stating plainly rather than discovering: a rewrite builds a whole new
/// file beside the old one and swaps it in, so until the rename the reader's
/// document is untouched and afterwards it is complete. An append has no such
/// instant. It puts bytes on the end of the file the reader has.
///
/// Four things bound that, and none of them is a promise that a crash is
/// impossible.
///
/// **The file is identified first, through the handle it will be written to.**
/// The update names byte offsets into the previous revision and chains its
/// `/Prev` to that revision's `startxref`, so appending it to any other file
/// produces a cross-reference table pointing at the wrong bytes. The length has
/// to match --- it is what those offsets are measured from --- but a length is
/// not evidence that the file is the same one, so the check is the whole of
/// [`Fingerprint::agrees_with_metadata`]: length and modification time, read
/// through the open handle rather than by looking the pathname up again.
///
/// It said "the length is checked first" until 2026-08-22, and so did the code.
/// `Appended::verified` carried a full fingerprint the whole time and nothing
/// read it, while the caller's comment claimed a length was a *sharper* answer
/// than a length and a timestamp. See `docs/TRAPS.md`.
///
/// **The trailer goes in its own write.** The update ends with `startxref`, an
/// offset and `%%EOF`, and a reader looking for the current revision scans
/// backwards for the last `startxref` --- so a partial write that stopped inside
/// the body leaves the previous revision's trailer as the last complete one, and
/// the file still opens as it was. Splitting there means the moment the file
/// becomes the new revision is a write of a few dozen bytes, which lands in one
/// sector on any filesystem this runs on. It is not an atomic rename and it is
/// not claimed to be.
///
/// **Anything that fails cuts the file back.** The length before the write is
/// what it is truncated to, so a refused or failed append leaves the file
/// exactly as it found it --- including the case where the *verification* below
/// refuses, which is a file this function wrote and then took back. The
/// truncation goes through the same handle as the writes, so it cannot land on
/// a file that merely acquired the name in between.
///
/// **And the name is checked last, without a roll-back.** Everything above is
/// about the file the handle holds; this is the one question about the pathname.
/// If another program renamed something over it mid-save, the edits are complete
/// and correct in the file that was there when the save began --- unreachable
/// now, or living under another name, and truncating it in that second case
/// would destroy the only copy of work the reader asked to keep. So it reports
/// that the save did not land where it was asked and touches neither file.
///
/// The verification is a genuine re-read: the file is parsed again and asked for
/// its page count. A rewrite gets that for free by verifying the staged copy
/// before the rename; an append has to ask afterwards, and asking is what makes
/// the rollback reachable rather than theoretical.
///
/// # Errors
///
/// The file changed length since the update was built; the write, the flush or
/// the truncation fails; or the appended file cannot be parsed or has the wrong
/// number of pages.
pub fn append_in_place(
    appended: &Appended,
    source: &Path,
    password: Option<&str>,
    reread: &dyn Reread,
) -> Result<(), String> {
    // **Opened once, and everything below goes through this handle.** The check,
    // the writes, the read-back and the roll-back are all about *this* file, not
    // about whatever the pathname resolves to at the moment each of them runs.
    // Reopening by name between them is how a rename lands the roll-back on a
    // file that was never ours to truncate.
    // **`write`, not `append`, and that is a portability fix rather than a
    // preference.** Rust maps `append(true)` on Windows to
    // `FILE_GENERIC_WRITE & !FILE_WRITE_DATA` (`std::sys::fs::windows`,
    // `get_access_mode`), and `File::set_len` there is
    // `SetFileInformationByHandle(FileEndOfFileInfo)`, which needs exactly the
    // right that mode removes. An append-mode handle would therefore write
    // happily and fail every roll-back with *access denied* --- on the platform
    // this cannot be tested from, which is where such a thing survives.
    //
    // It is also the more correct semantics for what this does: the trailer has
    // to land immediately after the body, and the file offset says that, where
    // `O_APPEND` says "wherever the end is now".
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(source)
        .map_err(|e| format!("could not open {source:?} to save it: {e}"))?;
    append_through(&mut file, appended, source, password, reread)
}

/// [`append_in_place`] with the handle supplied.
///
/// **A seam, not a convenience.** Everything this function guarantees is about
/// the difference between a handle and a pathname, and the window where they can
/// disagree opens *inside* [`append_in_place`] --- between its `open` and its
/// first write --- where no test can plant anything. Taking the handle as an
/// argument moves that window to the caller, so a test can open the file, let
/// something else rename a different file over the pathname, and then ask this
/// function what it does. `docs/TRAPS.md`, *A guard written inline with an FFI
/// call is reachable by nothing --- the fix is a seam, not a harness*.
fn append_through(
    file: &mut std::fs::File,
    appended: &Appended,
    source: &Path,
    password: Option<&str>,
    reread: &dyn Reread,
) -> Result<(), String> {
    // Which file this is, taken before a byte is written and compared after the
    // last one. See `FileId`, and the failure it reports at the end.
    let writing_to = FileId::of(file).ok_or_else(|| {
        format!(
            "tpdf could not tell which file {source:?} is, so it cannot promise a save would \
             land there --- nothing was written. Use Save a copy."
        )
    })?;

    let meta = file
        .metadata()
        .map_err(|e| format!("could not measure {source:?}: {e}"))?;
    // **Length *and* modification time, against the file as it was when the
    // update section was built against it.** The length is what the update's
    // byte offsets and `/Prev` depend on, so it is the one that has to match ---
    // but it is not evidence of identity on its own, and the repository has a
    // trap of its own about believing that it is. A file replaced by a distinct
    // revision of the same length would take this update's offsets into an
    // object graph they were never computed for.
    //
    // This is the consumer of `Appended::verified`, which had none until
    // 2026-08-22: the field was populated, documented as the caller's last look,
    // and read by nobody, so the guard was a length comparison wearing a
    // fingerprint's clothes.
    appended
        .verified
        .agrees_with_metadata(&meta, source)
        .map_err(|why| {
            format!("{why} --- nothing was written, so the file on disk is untouched")
        })?;

    // The split point: the last `startxref` in the update section. Found in the
    // bytes rather than computed from a length, because what has to be in the
    // second write is the trailer and nothing else. An update with no
    // `startxref` is not one `lopdf` produces; the whole thing then goes in one
    // write, which is the conservative answer rather than a refusal.
    let split =
        find_last(&appended.update, b"\nstartxref").map_or(appended.update.len(), |at| at + 1);
    let (body, trailer) = appended.update.split_at(split);

    let cut_back = |file: &std::fs::File, why: String| -> String {
        match file.set_len(appended.was) {
            Ok(()) => format!("{why} --- the file has been put back as it was"),
            Err(also) => format!("{why} --- and it could not be put back: {also}"),
        }
    };

    if let Err(e) = write_in_two(file, body, trailer) {
        return Err(cut_back(
            file,
            format!("could not append to {source:?}: {e}"),
        ));
    }

    // The re-read, and the reason the roll-back above is reachable rather than
    // theoretical. `lopdf` is the writer's own reader, which is weaker than the
    // rewrite path's third parser and is what is available here --- and what it
    // can see is a file that no longer parses or has lost pages, which is
    // exactly what a mis-chained cross-reference produces.
    //
    // Read through the handle rather than from the path, for this function's
    // one reason: a read by name would parse whichever file has that name now,
    // so a replacement would be checked in place of the file we wrote.
    let expected = usize::try_from(appended.was).unwrap_or(0) + appended.update.len();
    // **Asked, never computed here**, and the bytes never enter this process ---
    // see [`Reread`], which is where that guarantee is written down. The handle
    // is what is handed over, not `source`: a read by name would parse whichever
    // file has that name now, so a replacement would be checked in place of the
    // file we wrote.
    match reread.pages(file, expected, password) {
        Ok(pages) if pages == appended.pages => {}
        Ok(pages) => {
            return Err(cut_back(
                file,
                format!(
                    "the saved file has {pages} page(s) and should have {}",
                    appended.pages
                ),
            ))
        }
        Err(e) => {
            return Err(cut_back(
                file,
                format!("the saved file could not be read back: {e}"),
            ))
        }
    }

    // **Last, and deliberately not rolled back.** Everything above is about the
    // file this handle holds; this is the one question about the *name*. If
    // another program renamed something over it while the writes were in flight,
    // the edits are complete and correct in the file that was there when the
    // save began --- which is either unreachable now or living under some other
    // name, and in the second case truncating it would destroy the only copy of
    // work the reader asked to keep. So it says what happened and leaves both
    // files alone.
    if FileId::at(source) != Some(writing_to) {
        // Covers a deletion as well as a rename: `at` answers `None` for a name
        // that resolves to nothing, and both mean the same thing here --- this
        // handle's file is no longer what that name reaches.
        return Err(format!(
            "{source:?} stopped being the file it was while it was being saved --- \
             something else renamed or removed it. The edits were written to the file that \
             had that name when the save began, and nothing that has the name now was touched."
        ));
    }
    Ok(())
}

/// Who re-reads a file that has just been written, and says how many pages it has.
///
/// **A seam, and the guarantee is what it takes away.** [`append_through`] used
/// to read the whole written file into this process and parse it here. The
/// previous revision of that file is the document the reader opened --- attacker
/// bytes, verbatim --- so every in-place append parsed untrusted input in the
/// coordinator, which is the case `docs/THREAT-MODEL.md` residual risk 17 reads
/// as having been closed when only the *preparation* had moved.
///
/// Passing the answer in rather than computing it here means the coordinator no
/// longer holds the bytes at all. That is carried by the type: there is nothing
/// to parse, so no later edit can quietly reintroduce a parse. A source-level
/// assertion that the call had moved would prove a shape rather than an
/// ordering, which this repository has a trap about.
///
/// **The handle, never the pathname.** Everything [`append_through`] guarantees
/// is about the difference between the two, so an implementation that reopens by
/// name reintroduces exactly the race that function exists to close --- it would
/// check whichever file has that name now, rather than the one just written.
///
/// **`Send`, because the save runs on the blocking pool.** Every arm of the
/// coordinator's save does real file work on a file the size of the reader's
/// document, so the whole match crossed into `spawn_blocking` on 2026-08-23 ---
/// and a verifier chosen before that call has to travel with it. The bound is
/// stated here rather than at the one call site so that an implementation which
/// cannot cross is a compile error where it is written, not where it is used.
pub trait Reread: Send {
    /// How many pages the written file has, or why it could not be read.
    ///
    /// `len` is how long the file should now be: a capacity hint to
    /// [`read_whole`] for [`Here`], and the length to map for a worker, which
    /// cannot ask a handle how long its file is. It is passed rather than
    /// measured so that the answer is about the file this save produced.
    ///
    /// **`&mut`, because reading through a handle moves it.** [`Here`] seeks to
    /// the start, so this is not the read-only borrow it looks like it could be
    /// --- and saying so in the signature is better than a worker
    /// implementation that happens not to need it making the requirement look
    /// gratuitous.
    ///
    /// # Errors
    ///
    /// The file could not be read back, or the parser refused it --- which is
    /// what a mis-chained cross-reference produces and is the answer this is
    /// here for.
    fn pages(
        &self,
        file: &mut std::fs::File,
        len: usize,
        password: Option<&str>,
    ) -> Result<usize, String>;
}

/// Who scans a file this build just wrote for the words a redaction removed.
///
/// **The third seam, and the last parse to leave the coordinator.**
/// [`Reread`] asks whether a save chained and [`Rewriter`] produces the new
/// file; this asks whether the words are gone, which `docs/PLAN.md` §6 makes
/// mandatory after a redaction. It is a parse of the same attacker-controlled
/// bytes as the other two --- the file's previous revision is the reader's
/// document verbatim --- and it was the one `docs/THREAT-MODEL.md` residual risk
/// 18 kept naming, because that risk and `scripts/check_writers.py` both
/// enumerate what *writes* and this only reads.
///
/// **A trait of its own rather than a method on [`Reread`]**, because the two
/// answer different questions of the same file and a caller wants exactly one of
/// them: `save_document` re-reads and never scans, and a redaction scans a file
/// whose page count `write_copy` has already checked. Bundling them would give
/// each caller a method it must not call.
///
/// **`Send`, for [`Reread`]'s reason** --- the scan runs on the blocking pool.
pub trait Verifier: Send {
    /// What the scan found, or why the file could not be read at all.
    ///
    /// `len` is how long the file should now be: a capacity hint for [`Here`]
    /// and the length to map for a worker, which cannot ask a handle how long
    /// its file is. `&mut`, because reading through a handle moves it --- both
    /// of these are [`Reread::pages`]'s and mean the same.
    ///
    /// # Errors
    ///
    /// The file could not be read. A file that cannot be *parsed* is not an
    /// error: it is a blind spot, reported inside the [`crate::verify::Report`],
    /// which is the whole point of that type.
    fn scan(
        &self,
        file: &mut std::fs::File,
        len: usize,
        needles: &[String],
        password: Option<&str>,
    ) -> Result<crate::verify::Report, String>;
}

/// Re-reads in the coordinator, which is the process that just did the writing.
///
/// **The fallback rather than the shipped path**, and named so it cannot be
/// mistaken for either. It is what a platform with no sandbox gets, the same way
/// `render::Backend::InProcess` is, and it is what the tests in this module use
/// --- a `cargo test` cannot spawn a contained worker holding a real document,
/// so the worker path is proved by `worker-probe` instead.
///
/// Using it is not silent. See `crate::render::UNSANDBOXED_MARK`, which exists so
/// that an uncontained run stays distinguishable from a contained one.
pub struct Here;

impl Reread for Here {
    fn pages(
        &self,
        file: &mut std::fs::File,
        len: usize,
        password: Option<&str>,
    ) -> Result<usize, String> {
        let bytes = read_whole(file, len).map_err(|e| e.to_string())?;
        reread_pages(&bytes, password)
    }
}

impl Verifier for Here {
    fn scan(
        &self,
        file: &mut std::fs::File,
        len: usize,
        needles: &[String],
        password: Option<&str>,
    ) -> Result<crate::verify::Report, String> {
        let bytes = read_whole(file, len).map_err(|e| e.to_string())?;
        Ok(crate::verify::scan(&bytes, needles, password))
    }
}

/// Re-reads in a sandboxed worker spawned for the purpose, and dropped after it.
///
/// **The shipped path.** The file it maps is the one just written, and its
/// previous revision is the document the reader opened --- attacker bytes,
/// verbatim --- so the parse belongs behind the same boundary as every other
/// parse of them.
///
/// **A worker of its own, because there is none left to ask.** `save_document`
/// closes the document before the write happens, and every question put to the
/// document's own worker --- `append_ready`, `Request::Append`, the password ---
/// is asked before that close. `docs/PLAN.md` recorded the obstacle as the
/// worker "holding a mapping of the file as it was"; there is no such mapping by
/// this point, and the real constraint is simply that a child has to be started.
///
/// It costs one spawn per in-place append. That child pays PDFium's
/// initialisation for a job that only needs `lopdf`, which is a real cost and
/// not a large one against a save that has already written the file and waited
/// for the platter twice.
///
/// **Declared here and implemented in [`crate::save_outside`]**, which is the
/// one thing about this type a reader of this file has to be told. Doing what
/// the paragraphs above describe means spawning a process, mapping a shared
/// segment, speaking the worker protocol and ending a pid on a deadline --- four
/// modules of the process boundary, none of which this module's header names,
/// and all four of which used to be reached from function bodies down the page.
/// The declaration stays beside [`Here`] so that the pair a save picks between
/// is named in one place; the behaviour lives with the dependency it carries.
pub struct InWorker {
    /// Where `libpdfium` lives, which is all a worker needs to be started.
    ///
    /// `pub(crate)` for the module that implements this type rather than for
    /// anyone else: the field is what [`crate::save_outside`] hands to
    /// `Worker::spawn_shared`, and [`InWorker::at`] is still the only way to
    /// set it.
    pub(crate) library_dir: std::path::PathBuf,
}

impl InWorker {
    /// A verifier that will spawn its workers against this library directory.
    #[must_use]
    pub fn at(library_dir: std::path::PathBuf) -> Self {
        Self { library_dir }
    }
}

/// Who applies the reader's edits to a document and produces the new file.
///
/// **A seam, and the guarantee is what it takes away.** [`stage_in_place`] used
/// to parse the reader's document with `lopdf` in this process, apply the plan
/// and serialise it here. Those bytes are the attacker's verbatim, so every
/// rewriting save --- a deleted page, a move, a turn, a crop --- was a
/// coordinator-side parse of untrusted input, which is
/// `docs/THREAT-MODEL.md` residual risk 18 and is reached by deleting a page and
/// pressing the save key.
///
/// Passing the writing in rather than doing it here means the coordinator never
/// holds the document's bytes and never holds the new file's. That is carried by
/// the type: there is nothing here to parse, so no later edit can quietly
/// reintroduce a parse.
///
/// **Two handles, never two pathnames**, for [`Reread`]'s reason in both
/// directions. `source` is opened by the caller and read through, so what is
/// rewritten is the file that was fingerprinted rather than whatever has that
/// name now; `out` is the staging file the caller created, so the worker writes
/// one file it was handed and has no name it could be made to write anywhere
/// else. The second half is why this could not move until now: an append's
/// answer is kilobytes and fits in a reply, and a rewrite's is the document.
///
/// **`Send`, because the save runs on the blocking pool** --- see [`Reread`],
/// whose bound is here for the same reason and is stated in the same place.
pub trait Rewriter: Send {
    /// Writes `plan` applied to `source` into `out`, and says how many bytes.
    ///
    /// `len` is how long `source` is: a capacity hint for [`Here`] and the
    /// length to map for a worker, which cannot ask a handle how long its file
    /// is.
    ///
    /// **`&mut` on `source` because reading through a handle moves it**, and on
    /// `out` because writing does --- [`Here`] does both in this process.
    ///
    /// # Errors
    ///
    /// Everything [`rewrite_update`] refuses, and the write failing.
    fn write(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        plan: &Plan,
        job: Job,
        password: Option<&str>,
    ) -> Result<usize, Refusal>;

    /// Writes the pages `job` names, each turned by the reader's view, into
    /// `out`, and says how many bytes.
    ///
    /// **A second method rather than a second trait, because it is the same
    /// act.** What crosses this seam either way is *produce a document from
    /// these bytes and write it down this channel*; only the instruction
    /// differs, and it differs because a page range a reader typed is not an
    /// edit --- it carries no marks, no crops and no plan, which
    /// `crate::print::select` has documented since it was written. Expressing
    /// that as a [`Plan`] would mean the coordinator resolving page numbers
    /// against a page table, which is the parse this exists to move.
    ///
    /// **No password**, for the reason `crate::print::build_update` refuses an
    /// encrypted document rather than unlocking one: its writer emits every
    /// object in the clear, so a key would produce a decrypted copy rather than
    /// a printable one.
    ///
    /// `len`, `&mut` and the handles are [`Rewriter::write`]'s and mean the
    /// same.
    ///
    /// # Errors
    ///
    /// Everything [`crate::print::build_update`] refuses, and the write failing.
    fn write_range(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        job: &crate::print::Job,
    ) -> Result<usize, Refusal>;

    /// Writes `source` under `plan`, with `inputs` appended, into `out`.
    ///
    /// **The third instruction across this seam, and the widest.** The other two
    /// are about the document the reader opened; this one appends documents tpdf
    /// has never seen --- a reader picked them in a dialog, and every one of them
    /// is parsed whole. `inputs` is their bytes concatenated and `spans` names
    /// where each begins, which is [`crate::worker_proto::Request::Merge`]'s
    /// shape and is explained there.
    ///
    /// **The coordinator reads those files and does not parse them.** Filling a
    /// buffer with bytes asks nothing about what they mean; handing them to
    /// `lopdf` is what this takes away.
    ///
    /// It answers with two numbers because a merge does: the length, for the
    /// same check every other write gets, and the page count, which can only be
    /// taken where the merged document is.
    ///
    /// # Errors
    ///
    /// Everything [`merge_update`] refuses, and the write failing.
    fn merge(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        plan: &Plan,
        inputs: Inputs<'_>,
        password: Option<&str>,
    ) -> Result<(usize, u32), Refusal>;
}

/// Where a parse of the reader's own document happens.
///
/// **One choice, three seams.** [`Reread`] checks the file an append wrote,
/// [`Rewriter`] produces the file a rewrite writes, and [`Verifier`] scans the
/// file a redaction wrote for the words it removed; all three are parses of
/// attacker-controlled bytes, and all three belong in a sandboxed child wherever
/// there can be one. Naming them together is what keeps the rule --- ask
/// `render::Backend`, take a worker where there is one, mark the run where there
/// is not --- stated once instead of at each seam, which is the second copy
/// `docs/TRAPS.md` records drifting.
///
/// It adds no method of its own, deliberately: the thing being named is the
/// *choice*, and a member here would be a fourth seam nobody asked for.
pub trait Outside: Reread + Rewriter + Verifier {}

impl Outside for Here {}

impl Rewriter for Here {
    fn write(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        plan: &Plan,
        job: Job,
        password: Option<&str>,
    ) -> Result<usize, Refusal> {
        use std::io::Write as _;

        let original = read_whole(source, len).map_err(|e| e.to_string())?;
        let bytes = rewrite_update(&original, plan, job, password)?;
        out.write_all(&bytes)
            .and_then(|()| out.flush())
            .map_err(|e| format!("the rewritten document could not be written: {e}"))?;
        Ok(bytes.len())
    }

    fn write_range(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        job: &crate::print::Job,
    ) -> Result<usize, Refusal> {
        use std::io::Write as _;

        let original = read_whole(source, len).map_err(|e| e.to_string())?;
        let bytes = crate::print::build_update(&original, job)?;
        out.write_all(&bytes)
            .and_then(|()| out.flush())
            .map_err(|e| format!("the print job could not be written: {e}"))?;
        Ok(bytes.len())
    }

    fn merge(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        plan: &Plan,
        inputs: Inputs<'_>,
        password: Option<&str>,
    ) -> Result<(usize, u32), Refusal> {
        use std::io::Write as _;

        let original = read_whole(source, len).map_err(|e| e.to_string())?;
        let (bytes, pages) = merge_update(&original, plan, inputs, password)?;
        out.write_all(&bytes)
            .and_then(|()| out.flush())
            .map_err(|e| format!("the merged document could not be written: {e}"))?;
        Ok((bytes.len(), pages))
    }
}

/// How many pages `lopdf` finds in a file that has just been appended to.
///
/// **The one question the read-back asks**, in one place, because it is asked
/// from two processes now: here, and in the worker that
/// [`Request::Reread`](crate::worker_proto::Request::Reread) answers. Two copies
/// of it would be two statements of what a valid save looks like, and this
/// repository has the trap about a second copy drifting.
///
/// **`lopdf` and deliberately not PDFium**, which is the finding that decided
/// the shape of this. The worker protocol already answers a page count ---
/// `Request::Open` replies with one --- and reusing it would have made the move
/// three lines. It would also have replaced this check with a parser that
/// repairs the exact defect it exists to catch: PDFium is deliberately lenient,
/// and `docs/TRAPS.md` records it rendering a structurally broken file
/// pixel-identically to a correct one while `qpdf --check` named the defect at
/// once. What is being tested here is whether the cross-reference *chained*, and
/// a parser that reconstructs a broken table answers yes either way. `lopdf`
/// refuses, and its refusal is the whole instrument.
///
/// **The password is not optional, and forgetting it would roll every encrypted
/// save back.** `lopdf` parses no objects at all for a document it cannot
/// authenticate, so the read-back would report 0 pages against the 2 it expects,
/// decide the cross-reference was mis-chained, and truncate a file that is in
/// fact correct. A refusal rather than a corruption, which is the safe direction
/// and still wrong.
///
/// # Errors
///
/// `lopdf` refusing the bytes, which is what a mis-chained cross-reference
/// produces and is the answer this is here for.
pub fn reread_pages(bytes: &[u8], password: Option<&str>) -> Result<usize, String> {
    Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            password: password.map(str::to_string),
            ..Default::default()
        },
    )
    .map(|after| after.get_pages().len())
    .map_err(|e| e.to_string())
}

/// Writes the body, gets it on the platter, then writes the trailer.
///
/// The two-write split is [`append_in_place`]'s second bound and the reason it
/// is a separate function is the same as ever: a closure capturing the handle
/// mutably cannot coexist with the roll-back that needs to read it.
///
/// The `sync_data` between the halves is what makes the ordering a statement
/// about the file rather than about this process's buffers.
fn write_in_two(file: &mut std::fs::File, body: &[u8], trailer: &[u8]) -> std::io::Result<()> {
    use std::io::{Seek as _, Write as _};
    // The handle is not in append mode --- see `append_in_place` for why --- so
    // where the body goes is this seek and nothing else. It is also what makes
    // the second write land against the first: after `write_all` the offset is
    // exactly the end of the body, wherever the file's end has since moved to.
    file.seek(std::io::SeekFrom::End(0))?;
    file.write_all(body)?;
    file.flush()?;
    file.sync_data()?;
    file.write_all(trailer)?;
    file.flush()?;
    file.sync_data()
}

/// Reads a whole open file from the beginning, through the handle it is given.
///
/// `capacity` is a hint, not a bound: `lopdf`'s own path-based loader passes the
/// file's length for the same reason, and getting it wrong costs a reallocation
/// rather than a wrong answer.
fn read_whole(file: &mut std::fs::File, capacity: usize) -> std::io::Result<Vec<u8>> {
    use std::io::{Read as _, Seek as _};
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The last position of `needle` in `hay`, or `None`.
///
/// Written out because there is no `rfind` for byte slices, and the alternative
/// is a dependency or converting a PDF's bytes to a `String`, which they are not.
fn find_last(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len())
        .rev()
        .find(|&at| &hay[at..at + needle.len()] == needle)
}

/// Puts a file [`stage_in_place`] wrote where `source` is.
///
/// The second half, and the only step that is not reversible: after it the file
/// the reader opened holds the edits they made, and the journal that describes
/// them is spent. `staged` is removed if the rename fails, so a refused save
/// leaves the directory as it found it.
pub fn commit_in_place(staged: &Path, source: &Path) -> Result<(), String> {
    // The same resolution `stage_in_place` made, and it has to be the same one:
    // the staged file was written beside the *target*, so renaming it onto the
    // link would move it across directories and replace the wrong entry. See
    // `resolved`.
    commit(staged, &resolved(source))
}

/// A plan checked against the document on disk, and everything the rewrite needs.
///
/// The output of [`checked`] and the whole input to [`rewrite`], which is why it
/// exists: those two used to be one 217-line function whose first half refuses
/// and whose second half mutates, with nothing but blank lines between them. An
/// outside review scored it *"surgery whose correctness is sentence order"*.
///
/// Splitting on that line is worth more than the length is. Everything above it
/// can refuse and has written nothing; everything below it has changed the object
/// graph, so a refusal there costs a cleanup. That is a real property of the code
/// and it was readable only by reading all 217 lines in order.
/// What supplies one page of the output.
///
/// [`crate::docmodel::PageSource`]'s counterpart on this side of the plan, and
/// deliberately not that type: the model says *which baseline page*, and by the
/// time a rewrite has parsed the file the answer worth carrying is *which
/// object*. Resolving it once, in [`checked`], is what keeps the refusal for a
/// page the file does not have in the half that has written nothing.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Slot {
    /// A page of the file, at this object.
    Kept(lopdf::ObjectId),
    /// A page tpdf makes, at this size. It has no object yet ---
    /// [`make_blank_pages`] gives it one.
    Made(Size),
}

struct Checked {
    /// The parsed document, not yet touched.
    doc: Document,
    /// Every page object, in document order, as the file has them.
    pages: Vec<lopdf::ObjectId>,
    /// One-based numbers of the pages the plan drops, empty for a plan that
    /// keeps everything --- which is what the rewrite branches on.
    dropped: Vec<u32>,
    /// What each output page is and the quarter turns it should end up with, in
    /// the reader's order.
    ///
    /// **One entry per page the reader sees**, which since 2026-08-30 is the
    /// only such list this struct carries. There was a `kept` beside it holding
    /// one-based baseline numbers, and it survived the arrival of inserted pages
    /// as a second, shorter list of the same thing --- read by exactly one
    /// caller, to resolve a mark's page. That caller reads this instead now, so
    /// the second list is gone rather than carried unread.
    slots: Vec<(Slot, u8)>,
    /// Whether the reader moved anything, so the page tree has to be rebuilt.
    moved: bool,
    /// The encryption the source had, when a password opened it, so [`rewrite`]
    /// can put it back.
    ///
    /// **It travels in this struct rather than beside it**, because the document
    /// and the key to it are one fact: a `Checked` whose `doc` was decrypted and
    /// whose state went missing serialises perfectly and writes the reader's
    /// document in the clear. Two values a caller must remember to keep together
    /// is the shape `docs/TRAPS.md` records as *two copies of a distinction
    /// drift*, and here the drift is a silent loss of encryption.
    ///
    /// `None` for a document that never had any --- and also for one still
    /// locked, which never reaches [`rewrite`] because `checked` refuses it.
    encryption: Option<lopdf::EncryptionState>,
}

/// Proof that everything expressed in the document's *opened* geometry has been
/// written, so the geometry may now move.
///
/// **The ordering constraint, as a value.** Two steps in [`rewrite`] must precede
/// two others, for one fact stated twice: a mark's quads are in the space the
/// file had when the reader made them. `mark_sites` reads the rotation the file
/// has *now*, so turning first puts every quad a quarter turn out --- on exactly
/// the pages a reader rotated. Cropping first moves the origin those quads were
/// measured from.
///
/// Both were held by comments, and those comments say so themselves: *"the order
/// is load-bearing rather than tidy"*. A comment cannot be violated in a way
/// anything notices, and `docmodel.rs` records that page insertion is the next
/// step to be added here --- by somebody deciding where it goes.
///
/// So [`write_marks`] hands this back and the geometry steps require it. Calling
/// them first stops being a mistake to catch in review and becomes a value that
/// does not exist yet.
///
/// It carries nothing, deliberately: anything it carried would be a second reason
/// to hold it, and the reason to hold it is the ordering.
#[must_use]
struct MarksWritten;

/// What the caller established about the file before anything parsed it.
///
/// **The coordinator's half of a rewrite**, and the split is the same boundary
/// [`Ready`] draws for an append: everything in here is a question about a
/// *path* --- is this still the file the reader opened --- and answering it needs
/// filesystem authority and no parser. Everything in [`rewrite_update`] is a
/// parse of attacker-controlled bytes and needs no filesystem at all.
/// `docs/THREAT-MODEL.md` §T6 and residual risk 18.
///
/// It carries no bytes and no length, which is the one place it differs from
/// [`Ready`] and is worth stating rather than reading as an omission. An
/// append's answer is offsets *measured from* a length, so the two halves have a
/// number to compare and [`appended`] compares it; a rewrite's answer is a whole
/// document that names nothing about what it was built from, so there is nothing
/// of that shape to check. What plays that part here is the second fingerprint
/// [`Staged`] carries, taken during staging and read again before the rename.
#[derive(Debug)]
pub struct RewriteReady {
    verified: Option<Fingerprint>,
    changed: bool,
}

/// Asks whether the file is still the one the plan was made against.
///
/// **No parser, and that is the point of it being separate.** It reads `source`
/// only through [`Fingerprint`], which hashes bytes and never interprets them,
/// so nothing here is a parse of attacker-controlled input.
///
/// # Errors
///
/// The file changed since it was opened, under [`OnChange::Refuse`].
fn rewrite_ready(source: &Path, plan: &Plan, on_change: OnChange) -> Result<RewriteReady, Refusal> {
    // Before the parse, and before anything is written anywhere. Every operation
    // below rewrites the object graph this plan was made against, so a `source`
    // that changed since the reader opened it is a different graph and the edits
    // no longer name what they were made on.
    //
    // This is the general form of the page-count refusal in `checked`, which
    // shipped first and catches exactly one shape of the same problem: a file
    // whose page count changed. Everything that keeps the count -- a re-export
    // over the top, a sync client landing a newer copy, a signing tool rewriting
    // in place -- was invisible to it. See `docs/PLAN.md` §5 and `fingerprint.rs`.
    let mut changed = false;
    let verified = match (&plan.opened_as, on_change) {
        (Some(opened_as), OnChange::Refuse) => {
            Some(opened_as.agrees_with(source).map_err(Refusal::changed)?)
        }
        // Written anyway, and recorded. `verified` stays `None` because there is
        // nothing verified about it -- which is also what stops this result being
        // committed in place by a caller that changed its mind, since
        // `stage_in_place` refuses a `None`.
        (Some(opened_as), OnChange::Proceed) => match opened_as.agrees_with(source) {
            Ok(now) => Some(now),
            Err(_) => {
                changed = true;
                None
            }
        },
        (None, _) => None,
    };
    Ok(RewriteReady { verified, changed })
}

/// Applies a plan to a document's bytes and serialises the result.
///
/// **Pure, and that is the whole point of it existing separately.** Everything
/// here is parsing and serialising attacker-controlled bytes; nothing here opens
/// a file, names a path, or knows one exists --- which is what lets it run in a
/// sandboxed worker instead of in the process holding the window and the
/// reader's filesystem authority. The same split [`append_update`] has, for the
/// same reason, and `docs/THREAT-MODEL.md` residual risk 18 is what it narrows.
///
/// `job` says what the answer is for --- see [`Job`], which carries both of the
/// ways a print job differs from a save.
///
/// **The print refusal is here, and it moved with the parse.** It used to sit in
/// [`print_bytes`], between the two phases, which was the right place while the
/// two phases ran in the coordinator: `Checked` is where the answer is, and
/// asking the file again would have been a second parse deciding the same fact.
/// The phases are in a worker now, so the refusal followed them --- staying
/// behind would have meant either shipping the reader's decrypted document to
/// this process to be refused, or not refusing at all.
///
/// # Errors
///
/// As [`planned_bytes`], minus the file being readable; and, for a print job, an
/// encrypted document.
pub fn rewrite_update(
    original: &[u8],
    plan: &Plan,
    job: Job,
    password: Option<&str>,
) -> Result<Vec<u8>, Refusal> {
    let checked = checked(original, plan, job.view(), password)?;
    // **Between the phases, which is where the answer is.** `checked` holds the
    // encryption state it took off the document, so this reads what has already
    // been established rather than asking the bytes a second question.
    //
    // **And it is only reachable once the parse in front of it succeeds**, which
    // is why the reader's password reaches a function whose job here is to
    // refuse: without the key `checked` refuses first, with *"tpdf could not
    // unlock it ... Open it with its password first"* --- said to a reader who
    // has the document open with its password, naming an escape they have
    // already taken. A guard whose neighbour refuses the same input cannot be
    // reached by it.
    if job.is_print() && checked.encryption.is_some() {
        return Err(
            "This document is encrypted, and printing part of it would have to write a copy \
             the printer could not read. Print the whole document instead --- that is handed \
             over unchanged."
                .into(),
        );
    }
    rewrite(plan, checked)
}

/// One document going into a merge: where its bytes are, and what to call it.
///
/// **A struct rather than a tuple beside a list of names**, because two parallel
/// lists are two things to keep in step and `docs/TRAPS.md` records what happens
/// when they drift. Everything a refusal has to say about one incoming file is
/// here.
///
/// **`label` is a display name, never a path**, and that is the whole of why a
/// request may carry it: `crate::worker_proto::Request`'s standing property is
/// that it names nothing the worker could act on, and a bare file name with no
/// directory is not something a sandboxed child can open --- it is the string a
/// refusal puts in front of the reader. Without it the worker can only say
/// *"document 2 of the merge"* about a file the reader chose by name, which is
/// the message they would have to work backwards from.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Incoming {
    /// Where this document begins in the handed-over bytes.
    pub at: usize,
    /// How long it is.
    pub len: usize,
    /// What to call it in a message to the reader.
    pub label: String,
}

/// The documents going into a merge: their bytes, and where each one is.
///
/// **One type rather than two parameters**, and clippy's argument count is the
/// smaller half of the reason. The bytes and the spans into them are one fact
/// --- a span is meaningless beside a different buffer --- and passing them
/// separately is what makes a mismatch expressible at every call site. Here the
/// only way to read one document is [`Inputs::bytes_of`], which checks the span
/// against the buffer it actually belongs to.
#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    /// Every incoming document's bytes, concatenated.
    pub whole: &'a [u8],
    /// Where each of them begins, how long it is, and what to call it.
    pub each: &'a [Incoming],
}

impl<'a> Inputs<'a> {
    /// One incoming document's bytes.
    ///
    /// **Checked, not trusted.** The spans come from the coordinator, so a wrong
    /// one is a defect on this side of the pipe rather than an attack --- and a
    /// slice past the end of a mapping is a fault rather than a message.
    /// `checked_add` because `at + len` can wrap.
    ///
    /// # Errors
    ///
    /// A span that does not lie inside [`Inputs::whole`].
    pub fn bytes_of(&self, one: &Incoming) -> Result<&'a [u8], Refusal> {
        let end = one
            .at
            .checked_add(one.len)
            .filter(|end| *end <= self.whole.len())
            .ok_or_else(|| {
                Refusal::from(format!(
                    "{} is named as {} bytes at {} in the merge, and only {} were handed over",
                    one.label,
                    one.len,
                    one.at,
                    self.whole.len()
                ))
            })?;
        Ok(&self.whole[one.at..end])
    }
}

/// Applies a plan to a document's bytes and appends the documents in `inputs`.
///
/// **Pure, and the last of the three to become so.** [`rewrite_update`] and
/// `crate::print::build_update` moved their parses into a worker on 2026-08-28
/// and 2026-09-01; this one is wider than either, because the incoming files are
/// documents tpdf never opened --- a reader picked them in a dialog. Nothing here
/// opens a file, names a path, or knows one exists.
///
/// `base` is the document on screen, `plan` is the reader's edits to it, and
/// `inputs` is every incoming file concatenated, with `spans` naming where each
/// begins and how long it is. See
/// [`crate::worker_proto::Request::Merge`] for why they arrive that way.
///
/// **The base takes the password and the incoming files do not.** `base` is the
/// document the reader unlocked, so a rewrite of it can keep its own encryption;
/// an incoming file is refused below, because one file cannot preserve two
/// documents' encryption, and tpdf holds no key for those anyway.
///
/// # Errors
///
/// Anything [`rewrite_update`] refuses; a span outside `inputs`; an incoming file
/// `lopdf` will not read or that is encrypted; or the merge itself.
pub fn merge_update(
    base: &[u8],
    plan: &Plan,
    inputs: Inputs<'_>,
    password: Option<&str>,
) -> Result<(Vec<u8>, u32), Refusal> {
    // **Not the reader's message, and deliberately not the same string.**
    // `write_merged` refuses an empty selection first, in the words a reader
    // needs --- *"choose at least one document to merge in"*. Reaching here with
    // nothing to merge means the coordinator sent a request it should not have,
    // and saying so in the reader's words would put a defect on this side of the
    // pipe into a sentence that reads as their mistake. Two copies of one string
    // would also be two things to drift.
    if inputs.each.is_empty() {
        return Err("a merge was asked for with no documents to merge in".into());
    }
    let base = rewrite_update(base, plan, Job::Save, password)?;
    let mut merged = Document::load_mem_with_options(
        &base,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            // **The same password, because `rewrite` has just put the encryption
            // back.** These are bytes this module wrote a line ago, and if the
            // source was encrypted so are they. Omitting it does not fail the
            // load: `lopdf` parses *no objects at all* for a document it cannot
            // authenticate and still answers `Ok`, so what arrives is an empty
            // document and the merge below fails at `into.catalog()` with a
            // message blaming this module's own writer. An absence and a lock
            // are the same reading, and the reassuring one is wrong.
            password: password.map(str::to_string),
            ..Default::default()
        },
    )
    // Not a refusal a reader can act on, and it should be unreachable: these are
    // bytes this module wrote a line ago.
    .map_err(|e| format!("tpdf could not read back the document it just built: {e}"))?;

    // **Off the document before the merge, and back on after it.** That is
    // `rewrite`'s constraint for `rewrite`'s reason: `Document::encrypt` walks
    // the object map and encrypts what it finds, so an object added after it
    // would be written in the clear beside objects that are not, and no reader
    // could open the result. `take` is also required rather than tidy --- a
    // document that was decrypted refuses to be re-encrypted while the state is
    // still on it.
    let encryption = merged.encryption_state.take();

    for one in inputs.each {
        let label = &one.label;
        let incoming = Document::load_mem_with_options(
            inputs.bytes_of(one)?,
            lopdf::LoadOptions {
                max_decompressed_size: Some(MAX_DECODE),
                ..Default::default()
            },
        )
        .map_err(|e| format!("could not read {label}: {e}"))?;
        // Both shapes, for `checked`'s reason: `lopdf` removes the trailer entry
        // the moment it authenticates -- and it tries the empty password
        // unprompted -- so asking whether the trailer says `/Encrypt` reports a
        // permission-restricted document as plain.
        if incoming.was_encrypted() || incoming.is_encrypted() {
            return Err(format!(
                "{label} is encrypted, and merging rewrites it --- which would silently \
                 remove that. Leave it out, or save an unencrypted copy of it first."
            )
            .into());
        }
        crate::merge::append(&mut merged, &incoming)
            .map_err(|why| format!("could not merge {label}: {why}"))?;
    }

    // Last, after every incoming file has been appended --- see the `take` above.
    // Without this the merge of an encrypted base would be written in the clear,
    // which is exactly the silent removal the incoming-file refusal a few lines
    // up exists to prevent, arriving through the base instead.
    if let Some(state) = &encryption {
        merged.encrypt(state).map_err(|e| {
            // Not a sentence about the reader's document: the state came out of
            // this same file a moment ago, so a failure here is tpdf's.
            format!("tpdf could not restore this document's encryption: {e}")
        })?;
    }

    let pages = merged.get_pages().len() as u32;
    Ok((serialise(&mut merged, "the merged document")?, pages))
}

/// Checks a plan against a document's bytes, writing nothing.
///
/// Every refusal a save can make about the *document* lives here; the one it
/// makes about the *file* is [`rewrite_ready`]. See [`Checked`] for why the
/// second split, between this and [`rewrite`], is a boundary rather than a
/// convenience.
///
/// # Errors
///
/// As [`rewrite_update`], minus the serialisation.
fn checked(
    original: &[u8],
    plan: &Plan,
    view: u8,
    password: Option<&str>,
) -> Result<Checked, Refusal> {
    if plan.pages.is_empty() {
        return Err("a document must keep at least one page".into());
    }

    // Not `mut`, and that is the split proving itself rather than a tidy-up:
    // every refusal below reads the document and none of them writes to it, so
    // the compiler now says what the comments used to. It went `mut` the moment
    // the two halves were one function.
    // `load_mem_with_options`, not `load_with_options`, and the difference is
    // the whole split: a path is authority and bytes are not, so a function
    // that takes the second can run where there is none. The append already
    // reads this way for the same reason.
    let mut doc = Document::load_mem_with_options(
        original,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            password: password.map(str::to_string),
            ..Default::default()
        },
    )
    .map_err(|e| format!("could not parse the document: {e}"))?;

    // Before anything is written, and before the page walk: a refusal that
    // arrives after a temporary file exists has to clean up, and a refusal that
    // arrives after a rename has nothing to clean up at all.
    //
    // **Two questions, because `lopdf` answers them with two different fields
    // and the one this used to ask is false for every document it most needed
    // to catch.** The guard was `doc.trailer.has(b"Encrypt")`, and `lopdf`
    // *removes* that entry the instant it authenticates --- trying the empty
    // password first, unprompted. So an AES-256 document with an empty user
    // password, which is what a permission-restricted file is and what opens
    // without a prompt in every reader, arrived here with the trailer entry
    // already gone and was reserialised in the clear. Measured 2026-08-23 on
    // `incr-encrypted-open.pdf`: `qpdf --is-encrypted` says yes for the source
    // and no for what this function wrote.
    //
    // `was_encrypted` is "there was encryption and we hold the key for it",
    // which is the case the rewrite must refuse: `lopdf`'s full serialiser
    // writes plaintext and drops the dictionary. `is_encrypted` is "there is
    // encryption and we do not", where the document is empty as well as locked.
    // Both refuse here; only the first is appendable, which `append_update`
    // says.
    // **Taken here, put back in `rewrite`, and it must be taken before anything
    // else reads the document.** `lopdf`'s full serialiser writes every object
    // in the clear and drops the `/Encrypt` dictionary with it, which is why
    // this used to be a refusal. The repair is not a different serialiser: it is
    // `Document::encrypt`, which re-encrypts every object with a state and
    // writes a fresh dictionary. Measured 2026-08-28 on
    // `testdata/incr-encrypted-pw.pdf` --- delete a page, re-encrypt, and
    // `qpdf --show-encryption` on the source and the output diff to nothing:
    // `R = 6`, `P = -4`, AESv3 for streams, strings and file, both passwords
    // unchanged.
    //
    // **The `take` is not a tidy-up, it is required.** `Document::encrypt`
    // begins `if self.is_encrypted() { return Err(AlreadyEncrypted) }`, and a
    // password load leaves `encryption_state` set --- so a document that was
    // decrypted refuses to be re-encrypted until the state is off it. That is
    // not guessable from the two method names, and it is the whole trick.
    let encryption = doc.encryption_state.take();

    // What is still refused: a document nobody unlocked. `lopdf` parsed no
    // objects at all, so the page walk below would see an empty document and
    // every check after it would agree about nothing --- which is exactly what
    // the first two runs of the spike that measured the repair above did, and
    // called a pass. `is_encrypted` reads the trailer's `/Encrypt`, which a
    // successful authentication removes, so it is true only for the locked case.
    if doc.is_encrypted() {
        return Err(
            "This document is encrypted and tpdf could not unlock it, so it cannot be \
             rewritten. Open it with its password first."
                .into(),
        );
    }

    let pages = ordered_pages(&doc);
    if pages.len() != plan.baseline as usize {
        // `Refusal::changed`, and it is not a formality: this refusal says the
        // file changed, and it is the one that fires when a changed file also
        // changed shape --- which is precisely the case `OnChange::Proceed` does
        // *not* wave through, so a copy lands here rather than being written
        // from pages the plan cannot name. A reader who reaches it needs the
        // same Reload the fingerprint refusal offers.
        return Err(Refusal::changed(format!(
            "the document on disk has {} page(s) and the edits were made against {} --- it has \
             changed since it was opened, so reopen it before saving",
            pages.len(),
            plan.baseline
        )));
    }

    // The baseline pages the plan keeps, in the reader's order. **A page tpdf
    // made is not one of them**, and that is what keeps everything below a
    // question about the file alone: `kept`, `dropped` and the shared-object
    // refusals are all statements about pages the file has.
    let baselines: Vec<u32> = plan
        .pages
        .iter()
        .filter_map(|page| match page.source {
            PageSource::Baseline(number) => Some(number),
            PageSource::Blank(_) => None,
        })
        .collect();

    // Whether the page tree has to be rebuilt. Read here, off the plan, because
    // after the deletion below the document's own page numbers are not the
    // plan's any more --- and because a plan that is already in document order
    // must not go near `reorder_pages`, which flattens the tree.
    //
    // **Two ways, and the first one is new.** An inserted page is not in the
    // tree at all, so it can only get there by the tree being rebuilt --- and
    // the file's own pages can still be in their own order around it, which is
    // exactly the case the window walk below answers `false` for. Reading the
    // length difference is what says a page was inserted: `baselines` drops one
    // entry per page no file supplies.
    let moved =
        baselines.len() != plan.pages.len() || baselines.windows(2).any(|two| two[0] >= two[1]);

    // One-based, because that is how `lopdf` numbers pages and how
    // `pagetree::drop_pages` reads them. The model's `PageSource::Baseline` is
    // the zero-based baseline page, and `ordered_pages` is that same order, so
    // the two line up by position rather than by anything either of them stores.
    let kept: Vec<u32> = baselines.iter().map(|number| number + 1).collect();
    if let Some(past) = kept.iter().find(|&&number| number as usize > pages.len()) {
        return Err(Refusal::changed(format!(
            "the edits name page {past}, which this document does not have"
        )));
    }

    // **The reader's own rotation is added here, to every page.** It is the one
    // thing a print job has that a save does not: rotating the *view* turns the
    // whole document on screen and is not an edit, so it never reaches the
    // model --- and a job that ignored it would print the document upright while
    // the reader is looking at it sideways.
    //
    // Added into the same list the per-page turns go in rather than applied
    // afterwards, because `agreed_turns` refuses one object asked for two
    // different angles: a constant added to every entry keeps that property,
    // where a second pass over the same objects would have to re-establish it.
    //
    // **A `Result` rather than the `filter_map` this was**, and the change is
    // not cosmetic: a source out of range used to drop the page silently, which
    // would write a shorter `/Kids` than the reader is looking at. The check
    // above makes that unreachable, so this refusal is the assertion that says
    // so rather than a second guard.
    let slots: Vec<(Slot, u8)> = plan
        .pages
        .iter()
        .map(|page| {
            let slot = match page.source {
                PageSource::Baseline(number) => {
                    Slot::Kept(*pages.get(number as usize).ok_or_else(|| {
                        Refusal::changed(format!(
                            "the edits name page {}, which this document does not have",
                            number + 1
                        ))
                    })?)
                }
                PageSource::Blank(size) => Slot::Made(size),
            };
            // **Both operands are reduced, and `page.turns` is the one that
            // matters.** `PageView::turns` documents 0 to 3, and the model
            // holds to it --- `docmodel` normalises with `rem_euclid(4)` on
            // every rotation. But a plan reaches this function from *outside*
            // it: across the worker boundary, out of a restored session, or
            // against a file replaced under the reader. That is the whole
            // reason this function refuses rather than trusts, and a
            // documented range is a contract the caller may break, not a
            // guarantee this side may lean on.
            //
            // Unreduced it was `page.turns + view % 4`, which overflows `u8`
            // from 253 up. **The release build was never wrong**, measured
            // over all 1,024 pairs: 256 is a multiple of 4, so wrapping and
            // then taking `% 4` gives the same answer as the true sum does.
            // What it was is a panic in every build with overflow checks on
            // --- `cargo test`, `cargo run` --- where a refusal or a correct
            // answer belongs. Found by `fuzz/fuzz_targets/save_rewrite_update.rs`,
            // whose `turns_of` had to reduce the value to reach anything
            // behind this.
            Ok((slot, (page.turns % 4 + view % 4) % 4))
        })
        .collect::<Result<Vec<_>, Refusal>>()?;

    // The last thing that can refuse, and it belongs on this side rather than
    // beside the deletion it guards: `unshared` rejects a plan that keeps and
    // drops one object at once, which is a fact about the plan and the file and
    // is knowable before anything is written. Below the split it would be a
    // refusal in the half that has already changed the graph.
    let dropped: Vec<u32> = if kept.len() == pages.len() {
        Vec::new()
    } else {
        let dropped: Vec<u32> = (1..=pages.len() as u32)
            .filter(|number| !kept.contains(number))
            .collect();
        unshared(&pages, &kept, &dropped)?;
        dropped
    };

    Ok(Checked {
        doc,
        pages,
        dropped,
        slots,
        moved,
        encryption,
    })
}

/// Applies a checked plan and serialises the result.
///
/// Everything here has changed the object graph by the time it returns, which is
/// what separates it from [`checked`]: a refusal above this point costs nothing,
/// and one below it leaves a half-rewritten document to abandon.
///
/// **The order of the steps is enforced by [`MarksWritten`] rather than by the
/// comments beside them.** See that type.
///
/// # Errors
///
/// A page tree that cannot be rebuilt, a mark that maps to nothing, two pages
/// that are one object and disagree, or a document `lopdf` will not serialise.
fn rewrite(plan: &Plan, checked: Checked) -> Result<Vec<u8>, Refusal> {
    let Checked {
        mut doc,
        pages,
        dropped,
        slots,
        moved,
        encryption,
    } = checked;

    // **First, and the position is load-bearing in one direction only.** A note
    // edit changes an annotation's own dictionary and depends on nothing else
    // here, so it could sit almost anywhere; what it must not sit after is
    // anything that can make the object unreachable. `materialise` unlinks a
    // dropped page's annotations and `sweep::collect` deletes them, so a note
    // edit written later would be refused with "this comment is not in the
    // document any more" -- true of the file this rewrite is building, and a
    // wrong thing to tell a reader about the document they are looking at.
    //
    // Writing it into an object that is then dropped costs a dictionary entry
    // nobody reads. That is the right trade: the reader deleted the page the
    // comment was on, so the edit is theirs to lose. `planned_notes` already
    // leaves out anything on a page the *plan* does not carry, so this only
    // reaches a page that is being kept -- unless the reader deleted it in the
    // same breath, which is the case this ordering forgives.
    rewrite_note_edits(&mut doc, &plan.notes)?;

    // **Beside the note edits, and above `materialise` for their reason.** A
    // reply names an annotation that is already in the file, and
    // `materialise` unlinks a dropped page's annotations while
    // `sweep::collect` deletes them --- so asking afterwards would report a
    // comment on a page the reader kept as missing from the document they are
    // looking at. Measured rather than reasoned: with the call below the page
    // surgery, a reply naming a *dropped* page's object was refused with "not
    // in this document any more" instead of "not an annotation", which is a
    // true sentence about the file being built and the wrong diagnosis of the
    // plan.
    let replies = check_replies(&doc, &plan.marks)?;

    // **After both of those, and the order is a rule rather than a preference.**
    // A comment can be rewritten and then deleted, so `rewrite_note_edits` has to
    // run first or `set_note` is refused with "not in this document any more" ---
    // a true sentence about the file being built and the wrong diagnosis of the
    // reader's plan, which is the mistake the two comments above record making
    // once. And `check_replies` has to run first because a reply names an
    // annotation by object: forgetting one before that check would refuse the
    // reply with "not an annotation", which is the same wrong diagnosis in the
    // other direction. The model refuses that combination outright --- see
    // `Refusal::ReplyAnswersIt` --- so this ordering is what keeps the diagnosis
    // right for a plan that reached here anyway.
    let discarded = discard_notes(&mut doc, &plan.discards)?;

    // **Below the last refusal and above `materialise`, and both halves of that
    // are load-bearing.** A page tpdf made is not in the tree, and the only step
    // that can put it there is the rebuild below --- which is handed a list of
    // object ids, so the objects have to exist first. It sits under the two
    // checks above rather than over them because it is the first thing here that
    // adds an object: a refusal that arrives after it has left a page in a
    // document nobody will serialise, which is harmless and is still a document
    // in a state no reader asked for. `Checked`'s doc comment records that
    // somebody would have to decide where this goes; this is the decision.
    let turns: Vec<(lopdf::ObjectId, u8)> = make_blank_pages(&mut doc, &slots)?;

    // What goes and in what order, in the one sequence both writers share ---
    // see `pagetree::materialise`, which carries why the outline is dropped for
    // a deletion and kept for a move, and why turning pages is *not* part of it.
    let order: Vec<lopdf::ObjectId> = turns.iter().map(|(id, _)| *id).collect();
    crate::pagetree::materialise(&mut doc, &dropped, moved.then_some(order.as_slice()))?;

    // Before `apply_turns`, and the order is load-bearing rather than tidy: a
    // mark was made against the rotation the file had when it was opened, and
    // the mapping below reads the rotation the file has *now*. Turn the page
    // first and every quad is a quarter turn out, on exactly the pages a reader
    // rotated.
    // Read and written in two steps, which on this path is one document twice
    // over --- the borrow checker will not have it both ways, and the append
    // genuinely needs two. See `mark_sites`.
    // `order` rather than the baseline pages: it is every position in the plan
    // resolved to the object that will hold it, made pages included, and it
    // exists a few lines up because `materialise` needs the same list.
    let sites = mark_sites(&doc, &order, &plan.marks)?;
    let written = write_marks(&mut doc, &plan.marks, &sites, replies)?;

    // After the deletion, and it has to be: `drop_pages` removes objects, and a
    // rotation written onto a page that is about to go is work thrown away. The
    // ids are unaffected --- the survivors are the same objects they were.
    turn_pages(&mut doc, &agreed_turns(&turns)?, &written)?;

    // After `write_marks` for the same reason `apply_turns` is: a mark's quads
    // are in the display space the file had when it was opened, and cropping
    // moves that space. Writing the crop first would place every highlight by
    // the *new* origin while the reader made it against the old one.
    //
    // A page named twice by the plan is refused rather than cropped twice ---
    // the two entries would be one object, so the second write would silently
    // win for both positions. `agreed_turns` refuses the same shape for the same
    // reason, and this reuses neither because the values it compares are four
    // numbers rather than one.
    let crops = agreed_crops(plan)?;
    let crops: Vec<(lopdf::ObjectId, [f64; 4])> = crops
        .into_iter()
        .filter_map(|(source, box_pt)| Some((*pages.get(source as usize)?, box_pt)))
        .collect();
    crop_pages(&mut doc, &crops, &written)?;

    // **Last, and everything above it is a reason this can be last rather than
    // an accident of ordering.** A redaction's ordinals were worked out against
    // the *file's* objects, by PDFium, in the worker --- so anything that
    // reordered the page's content stream before this ran would address the
    // wrong words while reporting success. Nothing above does: deleting and
    // reordering pages edits the page tree, a mark adds annotation objects, and
    // a turn and a crop write entries in the page dictionary. Not one of them
    // touches a content stream, which is the property that makes the ordinals
    // still true here.
    let redacted = apply_redactions(&mut doc, &pages, &plan.redactions)?;

    // **Only what this rewrite orphaned, and only when it orphaned something.**
    // `drop_pages` unlinks a page object and every reference to it, and
    // `reorder_pages` flattens the tree and leaves the intermediate `/Pages`
    // nodes behind --- so after either, objects the reader asked to be rid of
    // sit in `doc.objects` reachable from nothing, and `lopdf` writes every
    // object it holds. Measured on `links.pdf` before this call existed:
    // extracting page 1 of 8 produced a one-page file carrying **all eight**
    // content streams, 4,139 decodable bytes each, and a deletion left the
    // dropped page's text in the same way. That is `docs/THREAT-MODEL.md`
    // residual risk 16, which named the deletion and not the extract.
    //
    // Left alone when nothing was dropped or moved, and that is the position
    // rather than an omission: a plain copy is a serialisation and not a
    // sanitation (§T6.1), so a copy of somebody else's document carries their
    // orphans forward untouched. What this guarantees is narrower and is the
    // thing a reader can actually believe --- **tpdf does not leave behind what
    // it was told to remove**. Whole-graph sanitation is `docs/PLAN.md` §6 and
    // is a different promise about a different command.
    //
    // Costs what the print path's identical call costs: spike 0.4 measured the
    // sweep at 3.6 ms over 2,445 objects and 70.3 ms over 25,583, against 4.6 ms
    // and 66.6 ms for the plain save it is added to.
    //
    // A redaction that took an annotation joins them, and for the same reason
    // measured for pages: `forget` unlinks the annotation and every reference to
    // it, which leaves its appearance stream --- a drawing of the very words that
    // went --- reachable from nothing and written out regardless.
    //
    // So does one that took an outline entry, which is the same shape a third
    // time: an entry's destination may be an indirect array and its `/A` an
    // indirect action dictionary, and both name the page the redacted heading
    // was on. `forget` removes the entry and every reference to it; what it
    // cannot remove is what the entry was the only reference *to*.
    //
    // And so does one that took a **picture**, which is the same shape a fourth
    // time and is the one this condition was missing: `remove_images` deletes
    // the `Do` and drops the resource entry, which leaves the image stream
    // reachable from nothing --- and an unswept file still holds every byte of
    // it. `redact-apply-probe` found that by grepping the written bytes for the
    // picture's own pixels rather than asking what the page draws, which are
    // different claims and only the second is a redaction.
    // And so does a **deleted comment**, which is the same shape a fifth time and
    // is the one this condition was missing when the deletion landed.
    // `pagetree::forget` removes the annotation's dictionary and every reference
    // to it, which leaves its appearance stream --- a drawing of the words the
    // reader deleted --- reachable from nothing and written out regardless. That
    // is the picture's trap exactly, in the one subsystem where the leftover is
    // text somebody asked to be rid of.
    if !dropped.is_empty()
        || moved
        || redacted.annots > 0
        || redacted.outline > 0
        || redacted.fields > 0
        || redacted.images > 0
        || discarded > 0
    {
        crate::sweep::collect(&mut doc)?;
    }

    // **Last, and after the sweep.** `Document::encrypt` walks every object in
    // the map and encrypts its strings and streams, so anything added after it
    // would be written in the clear beside objects that are not --- a file no
    // reader can open. The sweep above removes objects, which is safe in either
    // order; everything that *adds* one is above this line, and that is the
    // constraint rather than a preference.
    //
    // A document that never had encryption has no state and nothing happens
    // here. One that had it and was unlocked gets exactly what it arrived with:
    // the state is the file's own, parsed from its `/Encrypt` and never
    // rebuilt, so the algorithm, the permission bits and both passwords come
    // back unchanged. `examples/encrypted_rewrite_probe.rs` is the evidence,
    // through `qpdf` rather than through the writer that produced them.
    if let Some(state) = &encryption {
        doc.encrypt(state).map_err(|e| {
            // Not a sentence about the reader's document: the state came out of
            // this same file a moment ago, so a failure here is tpdf's. Said
            // plainly rather than dressed up as something to act on.
            format!("tpdf could not restore this document's encryption: {e}")
        })?;
    }

    Ok(serialise(&mut doc, "the document")?)
}

/// Turns a document into the bytes that will be written, and checks them.
///
/// **The one place in tpdf where a `Document` becomes a file**, which is what
/// makes the check here rather than at each of the three callers: a check bound
/// to one caller covers only that caller, and `docs/TRAPS.md` records that
/// costing a defect already.
///
/// `what` names the artifact in the refusal, because the three callers produce
/// different things and a reader who merged four files should not be told about
/// "the document".
///
/// What [`crate::verify::structure`] does and deliberately does not do is
/// written there. In short: it is `docs/PLAN.md` §6 step 5's narrow half, it
/// catches only what cannot be legitimate in a file written a moment ago, and it
/// is not cross-reference validation --- which measurement says no parser in
/// this process performs.
///
/// **The refusal below has no reachable input today, and that is stated rather
/// than left to be discovered by whoever writes the mutation for it.** `lopdf`
/// 0.44 writes a header, one `%%EOF` and a `startxref` for *every* document it
/// will serialise, including an empty one --- measured: `Document::new()` comes
/// out as 125 structurally valid bytes. So no `Document` this crate can build
/// makes the check fire, and a mutation deleting the call would survive.
///
/// It is kept, and it is a guard rather than decoration, because it is the
/// standing assertion `docs/PLAN.md` §6 step 3 asks for in so many words ---
/// *exactly one logical revision and no trailing data* --- placed on the seam
/// where every future writer will arrive. Four things make it reachable: a
/// `lopdf` bump, a rewrite that starts writing update sections, a redaction path
/// that assembles bytes rather than serialising a graph, and any caller handing
/// [`crate::verify::structure`] bytes from somewhere else. Its *logic* is
/// covered head-on in `verify`'s own tests, where every complaint has a case.
///
/// # Errors
///
/// `lopdf` refusing to serialise, or the bytes it produced failing the check.
/// Removes the planned show operators from each page that has any.
///
/// The destructive half of a redaction, and the only place in this module that
/// takes content *out* of a page. Everything about which operators go was
/// decided by `redact::covered` against PDFium's own object list; this addresses
/// them and nothing else.
///
/// **A page named twice is refused rather than removed from twice.** The second
/// call would run against a content stream the first had already changed, so its
/// ordinals would name different operators --- and `remove_shows`'s own guard
/// would probably catch it, which is not the same as this being safe. Refusing
/// here says what happened; relying on the other guard would report a
/// correspondence failure for a caller's duplicate.
///
/// # Errors
///
/// A redaction naming a page the plan does not keep, a page named twice, or
/// `redact::remove_shows` refusing --- which it does when the operators `lopdf`
/// decodes disagree with the text objects PDFium counted.
fn apply_redactions(
    doc: &mut Document,
    pages: &[lopdf::ObjectId],
    redactions: &[crate::edits::PlannedRedaction],
) -> Result<Redacted, Refusal> {
    // **Every entry checked before any of them is acted on**, which is the half
    // that is about damage rather than about correctness. A refusal discovered
    // half way through leaves a document with some pages redacted and some not,
    // and this function's caller is about to serialise it --- so a plan that
    // cannot be carried out in full is refused before the first removal.
    let mut seen: Vec<u32> = Vec::new();
    let mut targets: Vec<(lopdf::ObjectId, &crate::edits::PlannedRedaction)> = Vec::new();
    for redaction in redactions {
        if seen.contains(&redaction.source) {
            return Err(format!(
                "page {} is named twice by the redaction plan",
                redaction.source + 1
            )
            .into());
        }
        seen.push(redaction.source);
        let Some(page) = pages.get(redaction.source as usize) else {
            return Err(format!(
                "a redaction names page {} of a document that has {}",
                redaction.source + 1,
                pages.len()
            )
            .into());
        };
        targets.push((*page, redaction));
    }

    // **XFA, and this is a refusal rather than a removal.** `docs/PLAN.md` §6
    // says so and had said so since before any of this was written, with nothing
    // reading it until 2026-08-27: an XFA form keeps a complete second copy of
    // its data as XML in `/AcroForm /XFA`, and a redaction that took the field
    // values while leaving that packet has removed nothing a reader could not
    // recover. Sanitising it is a second document editor, so the honest answer
    // is to say what tpdf cannot do.
    //
    // In the pre-flight, with the other two, because a refusal discovered half
    // way through leaves a document with some pages redacted and some not. And
    // guarded on there being a redaction at all, like every other clause here:
    // an ordinary Save a copy of an XFA form is a serialisation and must go on
    // working.
    if !redactions.is_empty() && crate::redact::has_xfa(doc) {
        return Err(Refusal::from(
            "this document carries an XFA form, which keeps its own copy of \
             every answer --- tpdf cannot redact one, and writing the file \
             would leave that copy behind"
                .to_string(),
        ));
    }

    let mut done = Redacted::default();
    // Every widget the annotation pass removed, across all pages. The field pass
    // below asks whether everything under a field has gone, which is not
    // answerable one page at a time: a field's widgets may sit on several.
    let mut widgets: std::collections::HashSet<lopdf::ObjectId> = std::collections::HashSet::new();
    for (page, redaction) in targets {
        let took = crate::redact::remove_shows(doc, page, &redaction.shows, redaction.text_objects)
            .map_err(Refusal::from)?;
        done.shows += took.removed;

        // **Then the text inside Form XObjects**, which PDFium enumerates as one
        // object apiece so `remove_shows` cannot address it --- `docs/PLAN.md` §6
        // measured that carrier at 9,310 of 154,095 realistic regions, three
        // times the image count. One call per form rather than one for the page:
        // each form has its own content stream, its own operator count and its
        // own refusal, and a form that is shared must fail without taking the
        // rest of the page's removal with it.
        let mut by_form: std::collections::BTreeMap<usize, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (at, ordinal) in &redaction.form_shows {
            by_form.entry(*at).or_default().push(*ordinal);
        }
        for (at, ordinals) in by_form {
            let took = crate::redact::remove_form_shows(
                doc,
                page,
                &redaction.form_text_objects,
                at,
                &ordinals,
            )
            .map_err(Refusal::from)?;
            done.shows += took.removed;
        }

        // **Then the pictures.** A region over an image removed nothing until
        // 2026-08-27, which on a scanned page is a redaction that does not
        // redact. Removing the `Do` stops the page drawing it; dropping the
        // resource entry is what leaves the object unreachable, so the sweep
        // this rewrite already runs takes the bytes with it.
        let took =
            crate::redact::remove_images(doc, page, &redaction.images, redaction.image_objects)
                .map_err(Refusal::from)?;
        done.images += took.removed;

        // **The annotations, and every reference to them.** An annotation over
        // the region is `docs/PLAN.md` §6's *Annotations* row: its `/Contents`
        // is a comment about the words, routinely quoting them, and every reader
        // goes on showing it after the drawing is gone. `redact::covered_annots`
        // decides which, including the popups and replies that hang off them.
        //
        // `pagetree::forget` rather than pruning `/Annots`, because pruning the
        // one list a caller has in mind is what leaves the object alive: a
        // structure element's `/OBJR` or an AcroForm's `/Fields` names it too,
        // and an annotation still reachable is an annotation still written.
        let taken = crate::redact::covered_annots(doc, page, &redaction.areas);
        if !taken.is_empty() {
            done.annots += taken.len();
            let taken: std::collections::HashSet<lopdf::ObjectId> = taken.into_iter().collect();
            widgets.extend(taken.iter().copied());
            crate::pagetree::forget(doc, &taken).map_err(Refusal::from)?;
        }
    }

    // **The document's own description of itself, and only when something was
    // redacted.** `docs/PLAN.md` §6's carrier table names XMP and DocInfo at
    // document level, and a title or a subject routinely restates what the
    // document is about --- which is what a reader is redacting.
    //
    // Taken whole rather than matched, and the measurement is the argument. Of
    // 41 real PDFs, 15 carry `dc:creator`, 14 `dc:title`, 5 `dc:description`
    // and 5 `pdf:Keywords`: free text describing the document, written by its
    // producer. A rule that removed entries *containing* the removed words
    // would clear an exact copy and leave a paraphrase, and a paraphrase of a
    // redacted line is not reachable by any string rule at all. There is
    // nothing to match against, so the only rule that reaches this carrier is
    // to remove it.
    //
    // The cost is stated rather than hidden: a reader redacting one line from
    // their own report gets a copy with no title and no author. That is normal
    // for a document being released and it is the visible half of the trade.
    //
    // **Guarded on there having been a redaction at all**, which is the half
    // that is about every other save: this function runs on every rewrite, so
    // without the guard an ordinary copy would quietly lose its metadata. The
    // control for it is `a_copy_that_is_not_a_redaction_keeps_its_metadata`.
    if done.shows > 0 || done.annots > 0 || !redactions.is_empty() {
        done.metadata = strip_metadata(doc)?;

        // **The outline, and it is the one carrier a reader can see in tpdf
        // itself.** A bookmark's title is the heading it points at, so redacting
        // the heading leaves a verbatim copy in the outline and the sidebar goes
        // on drawing it. Measured on 41 real PDFs on 2026-08-27: 8 carry outline
        // entries and 163 of their 165 titles are verbatim page text, against 4%
        // when each document's titles are matched against the next document's
        // pages --- the control that makes the 99% mean anything.
        //
        // Entry by entry rather than `pagetree::drop_outline`, which is right
        // for a page deletion --- where every destination names a page that is
        // gone --- and wrong here: one redacted heading must not cost a reader
        // 131 bookmarks. `redact::covered_outline` decides which, and
        // `redact::drop_outline_items` splices the chain before the objects go.
        // That splice is the whole of it, and `docs/TRAPS.md` says why `forget`
        // alone would truncate the outline silently.
        //
        // Under the same guard as the metadata for the same reason: this runs on
        // every rewrite, and an ordinary copy must keep its bookmarks.
        let taken: Vec<String> = redactions
            .iter()
            .flat_map(|redaction| redaction.taking.iter().cloned())
            .collect();
        let entries = crate::redact::covered_outline(doc, &taken);
        done.outline = crate::redact::drop_outline_items(doc, &entries).map_err(Refusal::from)?;

        // **The form fields, and this runs last because its first rule needs the
        // annotation pass to have finished.** A widget over a region is removed
        // as an annotation, because a widget *is* one --- what survives is the
        // field dictionary above it, which is a separate object whenever the
        // field has `/Kids`. Measured before this was written: the kid went, the
        // parent kept its `/V`, and nothing displayed the value while every
        // search still found it.
        //
        // The second rule is the value itself, which is what reaches §6's
        // *widgets outside the redacted rectangle*: the same answer in a second
        // copy of the field, or one whose widget is on another page.
        let fields = crate::redact::covered_fields(doc, &taken, &widgets);
        done.fields = crate::redact::drop_fields(doc, &fields).map_err(Refusal::from)?;
    }
    Ok(done)
}

/// Removes `/Info` and the catalog's `/Metadata`, and every reference to them.
///
/// `pagetree::forget` for the reason the annotations use it: the trailer names
/// `/Info` and the catalog names `/Metadata`, and either may be named somewhere
/// else as well --- an XMP packet is an ordinary stream and nothing stops a
/// producer pointing at it twice. Removing the object without the references
/// leaves a dangling name where there was a description.
///
/// Returns how many of the two were there.
///
/// # Errors
///
/// Only what `pagetree::forget` refuses: an object nested deeper than the
/// sweep's bound.
fn strip_metadata(doc: &mut Document) -> Result<usize, Refusal> {
    let mut doomed: std::collections::HashSet<lopdf::ObjectId> = std::collections::HashSet::new();
    if let Ok(info) = doc.trailer.get(b"Info").and_then(Object::as_reference) {
        doomed.insert(info);
    }
    if let Ok(metadata) = doc
        .catalog()
        .and_then(|catalog| catalog.get(b"Metadata"))
        .and_then(Object::as_reference)
    {
        doomed.insert(metadata);
    }
    let found = doomed.len();
    if found > 0 {
        crate::pagetree::forget(doc, &doomed).map_err(Refusal::from)?;
    }
    Ok(found)
}

/// What a redaction took out of the document.
///
/// Counts rather than one number, because they are separate carriers and the
/// caller acts on one of them: an annotation that went may have left an
/// appearance stream reachable from nothing, which is what the sweep is for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Redacted {
    /// Text-showing operations deleted from content streams.
    shows: usize,
    /// Annotations removed, dependents included.
    annots: usize,
    /// How many of `/Info` and `/Metadata` were there to remove.
    metadata: usize,
    /// Outline entries removed, the subtrees under them included.
    outline: usize,
    /// Form fields removed, the widgets under them included.
    fields: usize,
    /// Images removed, counted by the `Do` operations that drew them.
    images: usize,
}

pub fn serialise(doc: &mut Document, what: &str) -> Result<Vec<u8>, String> {
    // **`/Size` made right rather than checked**, and the difference is the
    // whole reason this is two lines instead of a guard.
    //
    // `lopdf` writes `/Size` as `max_id + 1` and nothing keeps `max_id` in step
    // with the objects a rewrite removed: spike 0.4's defect is exactly that,
    // and `qpdf --check` states the rule in its own words --- *reported number
    // of objects (142) is not one plus the highest object number (101)*. PDFium
    // renders such a file pixel-identically to a correct one, and measurement
    // on 2026-08-26 found no parser in this process that objects either.
    //
    // **The guard was written first and could not be shipped.** Asserting
    // `max_id == highest` fails on both encrypted fixtures --- `lopdf` removes
    // the `/Encrypt` object the moment it authenticates while `max_id` stays, so
    // the count it can see is one short and the *file* is fine. Refusing to save
    // a correct document is worse than the defect. And the obvious carve-out
    // would have been unreachable anyway: the encryption guard refuses those
    // rewrites several lines earlier, so no mutation could redden the
    // assertion and it would have read as covered. `docs/TRAPS.md` carries all
    // three rules that were tried and all three over-refusals.
    //
    // Lowering it is safe and is the tightest legal value: `/Size` has only to
    // exceed every object number written, and where `lopdf` allocates an object
    // of its own for an xref stream it takes `max_id + 1`, which this leaves
    // free. `sweep::collect` already does this for the graph it collects; here
    // it holds for every path, including a copy that dropped nothing.
    doc.max_id = doc.objects.keys().map(|id| id.0).max().unwrap_or(0);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes)
        .map_err(|e| format!("could not serialise {what}: {e}"))?;
    let wrong = crate::verify::structure(&bytes);
    if wrong.is_empty() {
        return Ok(bytes);
    }
    // The fact, and no instruction. Nothing was written, and there is nothing
    // for the reader to do about a defect in bytes this process built --- the
    // same position `verify_before_commit`'s message takes for the same reason.
    Err(format!(
        "tpdf built {what} and then found it malformed, so nothing was written: {}",
        wrong.join("; ")
    ))
}

/// A PDF date string for an instant, in UTC.
///
/// `D:YYYYMMDDHHmmSSZ`, which PDF 32000-1 §7.9.4 calls a date and `annots.rs`
/// reads back. UTC with a literal `Z` rather than a local offset: the offset
/// form is `+HH'mm'`, the apostrophes are load-bearing, and a machine's timezone
/// is not something a reader of the file needs in order to know when a mark was
/// made.
///
/// A clock before the epoch reads as the epoch, which is the same answer
/// `diag.rs` gives and for the same reason --- a machine whose clock is wrong by
/// decades should still get its mark written.
///
/// The civil-date arithmetic is `diag.rs`'s, shared rather than copied: it is
/// pinned there by a table of known instants including a leap day, and a second
/// copy here would have no table.
pub fn pdf_date(at: std::time::SystemTime) -> String {
    let seconds = at
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (year, month, day) = crate::diag::civil_from_days(seconds / 86_400);
    let rest = seconds % 86_400;
    format!(
        "D:{year:04}{month:02}{day:02}{:02}{:02}{:02}Z",
        rest / 3_600,
        (rest / 60) % 60,
        rest % 60
    )
}

mod marks;

/// The marks half of a save: see [`marks`] for what is in it and why.
///
/// Two constants are re-exported rather than reached through the module,
/// because they were `save::OUTLINE_WIDTH` and `save::is_wash` before the
/// split and are read from `docmodel.rs` and two probes. A module boundary
/// drawn inside this crate is not a reason to rename anything outside it.
pub use marks::{is_wash, OUTLINE_WIDTH, STAMP_CAP, STAMP_INSET};

use marks::{
    check_replies, crop_pages, discard_notes, mark_sites, rewrite_note_edits, turn_pages,
    write_marks, write_note_edits, AnnotsSite,
};

/// Refuses a deletion that cannot be expressed by removing page *objects*.
///
/// `/Kids` may name one page object twice, so two page numbers can be one page.
/// [`drop_pages`] works in objects and correctly keeps any object a surviving
/// number names --- which means "delete page 2" on such a document deletes
/// nothing, and the copy comes out with the page the reader removed still in it.
///
/// The alternative to refusing is removing one *entry* from the `/Kids` array
/// that holds it, which is a different operation on a different unit, and it is
/// worth saying plainly that this is a refusal of a real request rather than a
/// guard against a malformed one. It is the same shape as the conflicting-turns
/// refusal in [`agreed_turns`]: no output satisfies the plan, so the reader is
/// told instead of handed a file they would have to check.
///
/// `pages` is every page object in document order; `kept` and `dropped` are
/// one-based page numbers into it.
///
/// # Errors
///
/// A dropped page whose object a kept page also names.
fn unshared(pages: &[lopdf::ObjectId], kept: &[u32], dropped: &[u32]) -> Result<(), String> {
    let at = |number: &u32| pages.get(*number as usize - 1).copied();
    for gone in dropped {
        let Some(id) = at(gone) else { continue };
        let Some(shared) = kept.iter().find(|keep| at(keep) == Some(id)) else {
            continue;
        };
        return Err(format!(
            "pages {gone} and {shared} are the same page in this file, so page {gone} cannot be \
             removed on its own. Remove both, or keep both."
        ));
    }
    Ok(())
}

/// Gives every [`Slot::Made`] a page object, and answers the output order.
///
/// One `(object, turns)` per page the reader sees, in their order --- which is
/// what [`pagetree::materialise`](crate::pagetree::materialise) rebuilds the
/// tree from and what [`turn_pages`] writes.
///
/// **The dictionary is the smallest thing every reader accepts, and each absence
/// is a decision rather than an omission.** `/Type` and `/MediaBox` are what
/// make it a page; `/Resources` is an empty dictionary because the specification
/// says a page inherits one and several readers fault on a page that inherits
/// nothing; and there is **no `/Contents`**, which is the specification's own
/// spelling of an empty page --- writing a zero-length stream would be a second
/// object saying the same thing, and the sweep would then have to be taught it
/// is reachable.
///
/// `/Parent` is deliberately absent here and written by the rebuild, which sets
/// it on every page in the order. Writing it twice would be two places deciding
/// what the tree looks like.
///
/// # Errors
///
/// Nothing today: no branch here can fail, and the signature carries a `Result`
/// because the caller's chain does. That is worth naming rather than hiding ---
/// a `Result` with no `Err` is a guard `docs/TRAPS.md` records as unable to
/// fire, and it stays only until this has to read anything out of the document.
fn make_blank_pages(
    doc: &mut Document,
    slots: &[(Slot, u8)],
) -> Result<Vec<(lopdf::ObjectId, u8)>, Refusal> {
    let mut out = Vec::with_capacity(slots.len());
    for &(slot, turns) in slots {
        let id = match slot {
            Slot::Kept(id) => id,
            Slot::Made(size) => doc.add_object(lopdf::Dictionary::from_iter([
                ("Type", Object::Name(b"Page".to_vec())),
                (
                    "MediaBox",
                    Object::Array(vec![
                        Object::Real(0.0),
                        Object::Real(0.0),
                        // `as f32` because that is what `lopdf` stores, which
                        // is also what a PDF real is: the model carries `f64`
                        // for the crop's sake and nothing here needs the range.
                        Object::Real(size.width as f32),
                        Object::Real(size.height as f32),
                    ]),
                ),
                ("Resources", Object::Dictionary(lopdf::Dictionary::new())),
            ])),
        };
        out.push((id, turns));
    }
    Ok(out)
}

/// The crop each kept source page is to get, refusing two that disagree.
///
/// One entry per **source** page rather than per plan position, because a page
/// object can appear in the order twice --- nothing in tpdf duplicates a page
/// today, and `docmodel` states what would have to be proved first, but the
/// writer must not be the thing that discovers it. Two positions naming one
/// object with different boxes have no output that satisfies both, so this
/// refuses rather than letting the later write win for both.
///
/// The same shape as `pagetree::agreed_turns` and deliberately not sharing its
/// code: that one compares a `u8` and this compares four `f64`, and folding them
/// together would mean a generic whose only two instantiations are these.
fn agreed_crops(plan: &Plan) -> Result<Vec<(u32, [f64; 4])>, String> {
    let mut order: Vec<u32> = Vec::new();
    let mut chosen: std::collections::HashMap<u32, ([f64; 4], usize)> =
        std::collections::HashMap::new();
    for (at, page) in plan.pages.iter().enumerate() {
        let Some(want) = page.crop else { continue };
        // **Unreachable, and the arm has to exist**: `Command::Crop` refuses a
        // box on a page tpdf made, so no plan carries one --- see
        // `Refusal::CropOnMadePage`. The match must be exhaustive, so this is where
        // the model's refusal is written down on the writer's side, and it is
        // also the right answer if that refusal is ever lifted: this function
        // exists for one hazard, two positions that are one *object*, and a made
        // page gets an object of its own for every position it occupies.
        let PageSource::Baseline(source) = page.source else {
            continue;
        };
        match chosen.get(&source) {
            None => {
                chosen.insert(source, (want, at));
                order.push(source);
            }
            Some(&(first, first_at)) if first != want => {
                return Err(format!(
                    "pages {} and {} are the same page in this file, so they cannot be cropped \
                     differently. Crop them the same way, or leave both as they are.",
                    first_at + 1,
                    at + 1
                ));
            }
            Some(_) => {}
        }
    }
    Ok(order.into_iter().map(|s| (s, chosen[&s].0)).collect())
}

/// Writes `bytes` to a fresh sibling temporary file for `out`, and names it.
///
/// One definition of where the partial file goes, read by both save paths ---
/// the in-place one stages and commits with the document's close between them,
/// and a second copy of this is how the two halves would come to disagree about
/// which file the other meant.
///
/// **The name is fresh for each call in flight and the file is created
/// exclusively**, and both halves of that matter. Until 2026-08-22 this staged at
/// `out.with_extension(PARTIAL)` --- one fixed name, derived from the
/// destination and reused by every save --- and wrote it with `std::fs::write`,
/// which truncates whatever is at that path and follows a symlink there. Saving
/// `report.pdf` therefore destroyed any existing `report.tpdf-partial` beside it,
/// two saves to one destination shared a staging file and could rename or delete
/// each other's work, and the cleanup on failure removed the path whether or not
/// this call had created it. That is silent destruction outside the file the
/// reader asked to write, which is the one thing this module exists not to do.
///
/// `create_new` is what makes the name a claim rather than a guess: it is
/// `O_CREAT | O_EXCL`, so it fails rather than truncating, and it refuses a
/// symlink at that path outright. A collision is retried at the next attempt
/// index rather than reported, because a collision is not an error --- it means
/// another save of this destination got there first, which is exactly the case
/// the index is for.
///
/// The name appends rather than replacing the extension, so `report.pdf` stages
/// beside itself as `report.pdf.tpdf-partial-<pid>-<n>` and can never collide
/// with a document somebody named `report.tpdf-partial`. See [`staging_name`]
/// for why `n` counts attempts rather than saves.
/// The file name [`stage`] tries on its `attempt`-th try, for a destination
/// called `stem`.
///
/// **Deterministic per destination, deliberately, and it was a process-wide
/// counter for about an hour first.** With a counter, the name a given call
/// picks depends on how many other saves have happened in this process --- which
/// no test can know, because `cargo test` runs them in parallel and several of
/// them stage. A test planting a file at "the next name" would then sometimes
/// plant at a name nothing was going to use, pass, and stop testing the thing it
/// is named for. `docs/TRAPS.md` is largely about that failure, so shipping a
/// fresh instance of it to close a review finding would have been poor.
///
/// Starting every destination at zero costs nothing: a name that is taken is a
/// save of the same file already in flight, or a leftover, and `create_new` sends
/// this to the next index either way.
fn staging_name(stem: &std::ffi::OsStr, attempt: u32) -> std::ffi::OsString {
    let mut name = stem.to_os_string();
    name.push(format!(".{PARTIAL}-{}-{attempt}", std::process::id()));
    name
}

/// The file a pathname actually names, with every symlink resolved.
///
/// **A rename replaces a name, and a symlink is a name.** `std::fs::rename`
/// onto a symlink replaces the *link* --- the entry becomes an ordinary file and
/// the document it pointed at keeps its old contents. So an in-place rewrite
/// through a link left the reader with two files quietly diverging, while the
/// append, which opens the path and writes into it, followed the link and edited
/// the document. Two save modes, one file, opposite outcomes, and no message
/// either way.
///
/// Resolving here rather than refusing is the right half of that choice: a link
/// to a PDF is a thing people have on purpose, and "tpdf will not save this"
/// would be a worse answer than saving what they are looking at.
///
/// A path that cannot be canonicalized is handed back unchanged --- a
/// destination that does not exist yet is the ordinary case for a copy, and
/// there is nothing to resolve.
fn resolved(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Creates the temporary file and lets `write` fill it.
///
/// **A closure rather than a `&[u8]`, since 2026-08-28**, and the reason is the
/// whole of `docs/THREAT-MODEL.md` residual risk 18: a rewrite's bytes are
/// produced by a sandboxed worker writing through this file's own descriptor, so
/// there is no buffer here to hand in. Every guarantee below is unchanged and
/// stays in one place, which is what the split is protecting --- the exclusive
/// creation, the mode copy, the sync, and the removal of the partial file on any
/// failure.
///
/// `write` gets the file with nothing written to it and the offset at zero. Its
/// refusal is passed through rather than wrapped: it is a refusal about the
/// *document* --- a page the plan names that the file does not have --- and
/// prefixing it with "could not write" would report a parse failure as a disk
/// failure.
///
/// # Errors
///
/// No unused temporary name; the file cannot be created; `write` refuses; or the
/// contents cannot be got onto the platter.
fn stage(
    out: &Path,
    write: impl FnOnce(&mut std::fs::File) -> Result<(), Refusal>,
) -> Result<PathBuf, Refusal> {
    let dir = out.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = dir.unwrap_or(Path::new("."));
    let stem = out
        .file_name()
        .ok_or_else(|| Refusal::from(format!("{out:?} does not name a file to save to")))?;

    for attempt in 0..STAGING_ATTEMPTS {
        let partial = dir.join(staging_name(stem, attempt));
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Refusal::from(format!("could not create {partial:?}: {e}"))),
        };
        // **Before the bytes, not after them.** A staged file is created with
        // the process umask, usually `0644`, so a document a reader keeps at
        // `0600` in a shared directory came back world-readable after any page
        // edit --- a save is not the place to widen who can read a file. Doing
        // it before `write_all` also means the contents are never on disk under
        // the wider mode, which doing it after would not give.
        //
        // Best effort: a filesystem that does not carry modes has nothing to
        // copy, and failing a save over that would be a refusal about the wrong
        // thing. Unix only, because there is no counterpart to copy on Windows
        // --- a file there inherits its ACL from the directory it is created in,
        // which is the directory the original is in, so the replacement lands
        // with the neighbours' permissions rather than with a default. The
        // narrower question of an ACL set on the file itself is not answered by
        // either platform's branch here; `docs/THREAT-MODEL.md` §T6.7 says so.
        #[cfg(unix)]
        if let Ok(existing) = std::fs::metadata(out) {
            let _ = file.set_permissions(existing.permissions());
        }
        // Removed on any failure, and this call created it, so removing it
        // cannot take anything that was not ours.
        let written = write(&mut file).and_then(|()| {
            // So that the rename swaps in a file whose contents are on the
            // platter. Without it the atomicity is a claim about the directory
            // entry only: a crash after the rename can leave the new name
            // pointing at a file of zeros, which is worse than either outcome
            // the split is meant to guarantee.
            //
            // **This process's `sync_data`, whoever did the writing.** A worker
            // writing through a duplicate of this descriptor puts its bytes in
            // the kernel and stops there; the file is the same file, so the sync
            // here is what covers them too.
            file.sync_data()
                .map_err(|e| Refusal::from(format!("could not flush {partial:?}: {e}")))
        });
        if let Err(why) = written {
            drop(file);
            let _ = std::fs::remove_file(&partial);
            return Err(why);
        }
        return Ok(partial);
    }
    Err(Refusal::from(format!(
        "could not find an unused temporary name beside {out:?} after {STAGING_ATTEMPTS} tries"
    )))
}

/// Renames a staged file over `out`, removing it if that fails.
fn commit(staged: &Path, out: &Path) -> Result<(), String> {
    std::fs::rename(staged, out).map_err(|e| {
        let _ = std::fs::remove_file(staged);
        format!("could not put {out:?} in place: {e}")
    })
}

/// Whether two paths name the same file.
///
/// Canonicalized, so `./a.pdf` and an absolute path to the same file are one
/// file, and a symlink to the source is caught. A destination that does not
/// exist yet cannot be canonicalized --- which is the ordinary case --- so it
/// falls back to comparing the parent directory and the file name, and that
/// comparison is what makes the ordinary case answer correctly rather than
/// answering "different" for everything.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => canonical_parent(a) == canonical_parent(b) && a.file_name() == b.file_name(),
    }
}

fn canonical_parent(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or(Path::new("."));
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf())
}

#[cfg(test)]
mod tests;
