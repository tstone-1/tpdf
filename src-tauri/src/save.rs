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

use std::path::{Path, PathBuf};

use lopdf::{Document, IncrementalDocument};

use lopdf::{dictionary, Dictionary, Object, ObjectId};

use crate::docmodel::MarkKind;
use crate::edits::{Plan, PlannedMark};
use crate::encoding::MAX_DECODE;
use crate::fingerprint::{FileId, Fingerprint};
use crate::pagetree::{
    agreed_turns, apply_crops, apply_turns, displayed_page, ordered_pages, DisplayedPage,
};
use crate::textbox;

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
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// The bytes of a working document, and whether its source had changed.
///
/// **It carried the staging fingerprint too until 2026-08-28**, and that field
/// is gone rather than merely unread: [`stage_in_place`] was its only consumer,
/// and it no longer goes through here at all --- its bytes are written by a
/// worker rather than returned to this process. What is left is the three copy
/// paths, none of which ever read it.
struct Planned {
    bytes: Vec<u8>,
    /// The source had changed since it was opened, and `OnChange::Proceed` let
    /// it through. Always false under `OnChange::Refuse`, which returns an error
    /// instead of setting this.
    changed: bool,
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

/// What a save passes for the reader's own rotation: none.
///
/// Named rather than written as `0` at both call sites, because the parameter it
/// fills is a quarter-turn count and a bare zero there reads as "no rotation at
/// all" --- which is wrong: the plan's own per-page turns still apply. This says
/// *the view adds nothing*, which is the fact. Saving stores the document, and
/// how the reader happens to be holding it is not part of the document.
const NO_VIEW_TURN: u8 = 0;

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
/// **`OnChange::Proceed`, like a copy and unlike a save in place.** What is at
/// stake is a sheet of paper: a document that changed on disk since it was
/// opened is worth printing from what the reader is looking at, with the file
/// left untouched either way. Refusing would take away the one operation that
/// cannot lose anything.
///
/// # Errors
///
/// Everything [`planned_bytes`] refuses --- including an encrypted source, which
/// printing reaches only when the reader has edited it, since an untouched one
/// is handed over byte for byte and never parsed here. `password` is the
/// reader's, and it is what makes that refusal the encryption one rather than
/// the locked-document one; see the body.
pub fn print_bytes(
    source: &Path,
    plan: &Plan,
    view: u8,
    password: Option<&str>,
) -> Result<Vec<u8>, Refusal> {
    // **Its own refusal, between the two phases, and it is not a second copy of
    // `print::build`'s.** Until 2026-08-28 `checked` refused every document
    // `lopdf` had decrypted, and that refusal was what stopped this function
    // handing a print job something it should not have. Making the rewrite
    // preserve encryption removed it --- for the save paths deliberately, and for
    // this one by accident: `print::route`'s `Working` arm calls here
    // *directly*, so `print::build`'s guard never sees it. Removing a guard
    // removes it for every caller, including the ones the change was not about.
    //
    // Neither answer a rewrite can give is right for a print job. Re-encrypting
    // hands `NSPrintOperation` or `Windows.Data.Pdf` a document they cannot
    // read; not re-encrypting hands the platform a decrypted copy of a document
    // whose author encrypted it, which is a different decision from *let the
    // rewrite work* and has not been measured. So it is refused, and the
    // reader is told which operation is the one that works.
    //
    // Between the phases rather than before them, because `Checked` is where the
    // answer is: `checked` holds the state it took off the document, and asking
    // the file again here would be a second parse deciding the same fact.
    //
    // **And it takes the reader's password, for a refusal.** That reads like a
    // contradiction and is the whole point: without the key `checked` refuses
    // first, with *"tpdf could not unlock it ... Open it with its password
    // first"* --- said to a reader who has the document open with its password,
    // and naming an escape they have already taken. The refusal below is the one
    // that names an operation that works, and it is only reachable once the
    // parse in front of it succeeds. A guard whose neighbour refuses the same
    // input cannot be reached by it, and until this argument existed the only
    // document that reached this line was one with an *empty* user password,
    // which `lopdf` opens unprompted --- which is why the test for it uses that
    // fixture and why the wrong sentence went unmeasured.
    let original = std::fs::read(source).map_err(|e| format!("could not read {source:?}: {e}"))?;
    let checked = checked(&original, plan, view, password)?;
    if checked.encryption.is_some() {
        return Err(
            "This document is encrypted, and printing part of it would have to write a copy \
             the printer could not read. Print the whole document instead --- that is handed \
             over unchanged."
                .into(),
        );
    }
    rewrite(plan, checked)
}

/// Writes the pages `plan` keeps, each with its own turn, from `source` to `out`.
///
/// # Errors
///
/// Everything [`planned_bytes`] refuses; `out` is the source; or the write
/// fails. The temporary file is removed on every failing path that created one.
pub fn write_copy(
    source: &Path,
    plan: &Plan,
    out: &Path,
    password: Option<&str>,
) -> Result<Copied, Refusal> {
    if same_file(source, out) {
        return Err(
            "tpdf cannot save over the document it is reading --- choose another name".into(),
        );
    }
    // Named differently from `stage_in_place`'s, deliberately. The two calls are
    // otherwise character-for-character identical, which made one mutation's
    // anchor ambiguous the moment this line gained a binding --- and an ambiguous
    // anchor is refused, so the mutation stops being able to fail. Distinct names
    // are the fix; a longer anchor is only the workaround.
    let copy = planned_bytes(source, plan, OnChange::Proceed, NO_VIEW_TURN, password)?;
    write_atomically(out, &copy.bytes)?;
    Ok(Copied {
        changed: copy.changed,
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

    let mut changed = false;
    let mut written: Vec<String> = Vec::new();
    for (plan, target) in plans.iter().zip(&targets) {
        let part = planned_bytes(source, plan, OnChange::Proceed, NO_VIEW_TURN, password).map_err(
            |why| Refusal {
                message: format!(
                    "{} ({} of {} written)",
                    why.message,
                    written.len(),
                    plans.len()
                ),
                changed: why.changed,
            },
        )?;
        changed |= part.changed;
        write_atomically(target, &part.bytes).map_err(|why| {
            Refusal::from(format!(
                "{why} ({} of {} written)",
                written.len(),
                plans.len()
            ))
        })?;
        written.push(target.display().to_string());
    }
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
    let base = planned_bytes(source, plan, OnChange::Proceed, NO_VIEW_TURN, password)?;
    let mut merged = Document::load_mem_with_options(
        &base.bytes,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            // **The same password, because `rewrite` has just put the
            // encryption back.** These are bytes this module wrote a line ago,
            // and if the source was encrypted so are they. Omitting it does not
            // fail the load: `lopdf` parses *no objects at all* for a document
            // it cannot authenticate and still answers `Ok`, so what arrives is
            // an empty document and the merge below fails at `into.catalog()`
            // with a message blaming this module's own writer. An absence and a
            // lock are the same reading, and the reassuring one is wrong.
            password: password.map(str::to_string),
            ..Default::default()
        },
    )
    // Not a refusal a reader can act on, and it should be unreachable: these
    // are bytes this module wrote a line ago. Said plainly rather than dressed
    // up, because a message suggesting the reader do something about it would
    // be a wrong diagnosis.
    .map_err(|e| format!("tpdf could not read back the document it just built: {e}"))?;

    // **Off the document before the merge, and back on after it.** That is
    // `rewrite`'s constraint for `rewrite`'s reason: `Document::encrypt` walks
    // the object map and encrypts what it finds, so an object added after it
    // would be written in the clear beside objects that are not, and no reader
    // could open the result. `take` is also required rather than tidy --- a
    // document that was decrypted refuses to be re-encrypted while the state is
    // still on it.
    //
    // The load above authenticated, so this is the base's own state: the
    // algorithm, the permission bits and both passwords, parsed from its
    // `/Encrypt` and never rebuilt.
    let encryption = merged.encryption_state.take();

    for other in others {
        let incoming = Document::load_with_options(
            other,
            lopdf::LoadOptions {
                max_decompressed_size: Some(MAX_DECODE),
                ..Default::default()
            },
        )
        .map_err(|e| format!("could not read {}: {e}", name_of(other)))?;
        // Both shapes, for `planned_bytes`' reason: `lopdf` removes the trailer
        // entry the moment it authenticates -- and it tries the empty password
        // unprompted -- so asking whether the trailer says `/Encrypt` reports a
        // permission-restricted document as plain.
        if incoming.was_encrypted() || incoming.is_encrypted() {
            return Err(format!(
                "{} is encrypted, and merging rewrites it --- which would silently remove \
                 that. Leave it out, or save an unencrypted copy of it first.",
                name_of(other)
            )
            .into());
        }
        crate::merge::append(&mut merged, &incoming)
            .map_err(|why| format!("could not merge {}: {why}", name_of(other)))?;
    }

    // Last, after every incoming file has been appended --- see the `take`
    // above. Without this the merge of an encrypted base would be written in
    // the clear, which is exactly the silent removal the incoming-file refusal
    // a few lines up exists to prevent, arriving through the base instead.
    if let Some(state) = &encryption {
        merged.encrypt(state).map_err(|e| {
            // Not a sentence about the reader's document: the state came out of
            // this same file a moment ago, so a failure here is tpdf's.
            format!("tpdf could not restore this document's encryption: {e}")
        })?;
    }

    let bytes = serialise(&mut merged, "the merged document")?;
    write_atomically(out, &bytes)?;
    Ok(Merged {
        changed: base.changed,
        pages: merged.get_pages().len() as u32,
        files: others.len() as u32,
    })
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
    // **Opened here and read through, never re-opened by name.** Everything
    // `rewrite_ready` just established is about *this* file, and a rewriter that
    // opened `target` again would apply the plan to whatever has that name by
    // then --- the same race [`Reread`] is written to close, on the way in
    // rather than on the way out.
    let mut file = std::fs::File::open(&target)
        .map_err(|e| format!("could not open {target:?} to save it: {e}"))?;
    let len = file
        .metadata()
        .map_err(|e| format!("could not measure {target:?}: {e}"))?
        .len() as usize;

    // **The document refusals now arrive after the temporary file exists**, and
    // that is a change worth naming rather than leaving to be noticed. They used
    // to be made before anything was created, which the comment on this function
    // still gives as the reason for the split; what makes it safe is that
    // `stage` removes the partial file on every failing path, including this
    // one, and that nothing has been renamed either way. What is *not* given up
    // is the free half: `rewrite_ready` above still refuses a changed file
    // before a byte is written anywhere.
    let path = stage(&target, |out| {
        let wrote = rewriter.write(&mut file, len, out, plan, password)?;
        // **The one check on the way back**, and it is here rather than in the
        // rewriter because only this side can make it: the worker says how many
        // bytes it wrote and this file says how many it has, and the two are
        // independent statements about the same write. A short write, a second
        // rewrite appending to the first, or a reply built for another request
        // all disagree here. Neither number is derived from the other.
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
        Ok(())
    })?;
    Ok(Staged { path, verified })
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

    // Every page is kept --- `is_appendable` said so --- and the one-based
    // numbering is `write_marks`' shared-object refusal's, not a page selection.
    let kept: Vec<u32> = (1..=u32::try_from(pages.len()).unwrap_or(u32::MAX)).collect();
    let sites = mark_sites(&prev, &pages, &kept, &plan.marks)?;

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
        write_marks(&mut incremental.new_document, &plan.marks, &sites)?;

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
pub struct InWorker {
    /// Where `libpdfium` lives, which is all a worker needs to be started.
    library_dir: std::path::PathBuf,
}

impl InWorker {
    /// A verifier that will spawn its workers against this library directory.
    #[must_use]
    pub fn at(library_dir: std::path::PathBuf) -> Self {
        Self { library_dir }
    }
}

/// Waits for a worker's answer, and ends the worker if it does not come.
///
/// **The bound this path did not have.** `InWorker::pages` spawns its worker
/// outside the pool, so the pool's supervisor --- the thing that owns
/// [`crate::workers::DEFAULT_DEADLINE`] --- never sees it, and `Worker::call`
/// is a blocking read bounded only by how *long* a reply may be. A document
/// whose cross-reference sends `lopdf` round in circles would hold the
/// `spawn_blocking` thread for ever, with the reader's document already closed
/// and the appended bytes on disk unconfirmed.
///
/// `within` is a parameter for `overdue`'s reason: a check whose failure mode is
/// a wait cannot be exercised, so the decision has to be reachable without
/// hanging anything.
///
/// The pid is killed rather than the thread being asked to stop, because the
/// thread is blocked inside a pipe read and nothing can interrupt it. Ending the
/// process closes the pipe, the read fails, the thread drops its `Worker` and
/// exits --- so the timeout leaks neither a process nor a thread.
fn awaited<T>(
    rx: &std::sync::mpsc::Receiver<T>,
    within: std::time::Duration,
    pid: u32,
) -> Result<T, String> {
    match rx.recv_timeout(within) {
        Ok(answer) => Ok(answer),
        Err(_) => {
            crate::workers::kill_pid(pid);
            Err(format!(
                "the worker checking the saved file did not answer within {:.0} s, so the \
                 save could not be confirmed",
                within.as_secs_f64()
            ))
        }
    }
}

impl Reread for InWorker {
    fn pages(
        &self,
        file: &mut std::fs::File,
        len: usize,
        password: Option<&str>,
    ) -> Result<usize, String> {
        // The handle, never `source`. See [`Reread`]: mapping by name would
        // verify whichever file has that name now.
        let mapped = crate::worker_shm::Shm::map_open_file(file, len)?;
        let worker =
            crate::worker::Worker::spawn_shared(std::sync::Arc::new(mapped), &self.library_dir)?;

        // **Asked on a thread so the answer can be waited for with a bound.**
        // See `awaited`. The pid is read before the move, because afterwards
        // this thread no longer owns the worker.
        let pid = worker.pid();
        let key = password.map(str::to_string);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut worker = worker;
            let _ = tx.send(Self::ask(&mut worker, key.as_deref()));
        });
        awaited(&rx, crate::workers::DEFAULT_DEADLINE, pid)?
    }
}

impl InWorker {
    /// The two requests the read-back makes, on the thread that owns the worker.
    fn ask(worker: &mut crate::worker::Worker, password: Option<&str>) -> Result<usize, String> {
        use crate::worker_proto::{Reply, Request};

        // **Before the question, and only when there is one.** A locked document
        // that is not unlocked first parses to zero objects, so the count would
        // come back as 0 against the pages the save expects and roll back a file
        // that is correct --- the same failure `reread_pages` names, arriving one
        // process further out.
        if let Some(password) = password {
            let answered = worker.call(&Request::Unlock {
                password: password.to_string(),
            })?;
            if !answered.ok {
                return Err(format!(
                    "the worker could not take the document's password: {}",
                    answered.error
                ));
            }
        }

        let answered = worker.call(&Request::Reread)?;
        if !answered.ok {
            return Err(answered.error);
        }
        match answered.reply {
            Some(Reply::Reread(pages)) => Ok(pages),
            // A well-formed message answering a different question. Nothing in
            // the protocol checks that a reply matches its request --- `Reply`'s
            // own documentation says so --- so the caller does, and says which it
            // got rather than reporting a parse failure for a protocol one.
            other => Err(format!(
                "the worker answered the re-read with {}",
                match other {
                    Some(reply) => format!("{reply:?}"),
                    None => "no payload at all".to_string(),
                }
            )),
        }
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
        password: Option<&str>,
    ) -> Result<usize, Refusal>;
}

/// Where a parse of the reader's own document happens.
///
/// **One choice, two seams.** [`Reread`] verifies the file an append wrote and
/// [`Rewriter`] produces the file a rewrite writes; both are parses of
/// attacker-controlled bytes, and both belong in a sandboxed child wherever
/// there can be one. Naming the pair is what keeps the rule --- ask
/// `render::Backend`, take a worker where there is one, mark the run where there
/// is not --- stated once instead of at each seam, which is the second copy
/// `docs/TRAPS.md` records drifting.
///
/// It adds no method of its own, deliberately: the thing being named is the
/// *choice*, and a member here would be a third seam nobody asked for.
pub trait Outside: Reread + Rewriter {}

impl Outside for Here {}
impl Outside for InWorker {}

impl Rewriter for Here {
    fn write(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        plan: &Plan,
        password: Option<&str>,
    ) -> Result<usize, Refusal> {
        use std::io::Write as _;

        let original = read_whole(source, len).map_err(|e| e.to_string())?;
        let bytes = rewrite_update(&original, plan, NO_VIEW_TURN, password)?;
        out.write_all(&bytes)
            .and_then(|()| out.flush())
            .map_err(|e| format!("the rewritten document could not be written: {e}"))?;
        Ok(bytes.len())
    }
}

impl Rewriter for InWorker {
    fn write(
        &self,
        source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        plan: &Plan,
        password: Option<&str>,
    ) -> Result<usize, Refusal> {
        // The handles, never the pathnames. See [`Rewriter`].
        let mapped = crate::worker_shm::Shm::map_open_file(source, len)?;
        let worker = crate::worker::Worker::spawn_writing(
            std::sync::Arc::new(mapped),
            out,
            &self.library_dir,
        )?;

        // **Asked on a thread so the answer can be waited for with a bound**, as
        // in [`InWorker::pages`]: this worker is outside the pool, so nothing
        // else owns a deadline for it, and a document that sends `lopdf` round
        // in circles would otherwise hold the blocking thread for ever. The pid
        // is read before the move, because afterwards this thread no longer owns
        // the worker.
        let pid = worker.pid();
        let key = password.map(str::to_string);
        let plan = plan.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut worker = worker;
            let _ = tx.send(Self::ask_rewrite(&mut worker, &plan, key.as_deref()));
        });
        awaited(&rx, crate::workers::DEFAULT_DEADLINE, pid)?
    }
}

impl InWorker {
    /// The two requests a rewrite makes, on the thread that owns the worker.
    ///
    /// [`InWorker::ask`]'s counterpart, and the unlock in front of it is there
    /// for the same reason: `lopdf` parses no objects at all for a document it
    /// cannot authenticate, so a locked document would rewrite to an empty one
    /// rather than refusing.
    fn ask_rewrite(
        worker: &mut crate::worker::Worker,
        plan: &Plan,
        password: Option<&str>,
    ) -> Result<usize, Refusal> {
        use crate::worker_proto::{Reply, Request};

        if let Some(password) = password {
            let answered = worker.call(&Request::Unlock {
                password: password.to_string(),
            })?;
            if !answered.ok {
                return Err(format!(
                    "the worker could not take the document's password: {}",
                    answered.error
                )
                .into());
            }
        }

        let answered = worker.call(&Request::Rewrite {
            plan: plan.clone(),
            view: NO_VIEW_TURN,
        })?;
        if !answered.ok {
            // The one bit that has to survive the pipe: whether Reload is the
            // answer. See `Response::changed`.
            return Err(Refusal {
                message: answered.error,
                changed: answered.changed,
            });
        }
        match answered.reply {
            Some(Reply::Rewrote(bytes)) => Ok(bytes),
            // A well-formed message answering a different question --- see
            // [`InWorker::ask`], which says why the caller checks this rather
            // than the protocol.
            other => Err(format!(
                "the worker answered the rewrite with {}",
                match other {
                    Some(reply) => format!("{reply:?}"),
                    None => "no payload at all".to_string(),
                }
            )
            .into()),
        }
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
struct Checked {
    /// The parsed document, not yet touched.
    doc: Document,
    /// Every page object, in document order, as the file has them.
    pages: Vec<lopdf::ObjectId>,
    /// One-based numbers of the pages the plan keeps, in the reader's order.
    kept: Vec<u32>,
    /// One-based numbers of the pages the plan drops, empty for a plan that
    /// keeps everything --- which is what the rewrite branches on.
    dropped: Vec<u32>,
    /// Each kept page's object and the quarter turns it should end up with.
    turns: Vec<(lopdf::ObjectId, u8)>,
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

/// The bytes of the working document, ready to be written somewhere.
///
/// Everything both save paths share: the parse, the three refusals, the page
/// tree, the marks, the turns and the crops. Neither path names a destination
/// here --- a copy and a save in place differ in where the bytes go and in what
/// has to happen around the write, never in what is written.
///
/// Two phases, and the split is the one that matters rather than a tidy one ---
/// see [`Checked`].
///
/// # Errors
///
/// The plan is empty; `source` cannot be read or parsed; it is encrypted; it has
/// a different number of pages than the plan's baseline; the plan names a page
/// the file does not have; two of its pages are one object and disagree about
/// the turn or the crop, or one of them is dropped without the other; or a mark
/// maps to nothing.
fn planned_bytes(
    source: &Path,
    plan: &Plan,
    on_change: OnChange,
    view: u8,
    password: Option<&str>,
) -> Result<Planned, Refusal> {
    let ready = rewrite_ready(source, plan, on_change)?;
    let original = std::fs::read(source).map_err(|e| format!("could not read {source:?}: {e}"))?;
    Ok(ready.with(rewrite_update(&original, plan, view, password)?))
}

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

impl RewriteReady {
    /// Puts a builder's bytes together with what the caller checked itself.
    ///
    /// The fingerprint is deliberately not carried through: the only path that
    /// needs it is [`stage_in_place`], which reads it from here directly and
    /// never asks for the bytes. See [`Planned`].
    fn with(self, bytes: Vec<u8>) -> Planned {
        Planned {
            bytes,
            changed: self.changed,
        }
    }
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
/// `view` is the reader's own rotation in quarter turns, which a print job has
/// and a save does not --- see [`NO_VIEW_TURN`].
///
/// # Errors
///
/// As [`planned_bytes`], minus the file being readable.
pub fn rewrite_update(
    original: &[u8],
    plan: &Plan,
    view: u8,
    password: Option<&str>,
) -> Result<Vec<u8>, Refusal> {
    rewrite(plan, checked(original, plan, view, password)?)
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

    // Whether the reader moved anything. Read here, off the plan, because after
    // the deletion below the document's own page numbers are not the plan's any
    // more --- and because a plan that is already in document order must not go
    // near `reorder_pages`, which flattens the tree.
    let moved = plan
        .pages
        .windows(2)
        .any(|two| two[0].source >= two[1].source);

    // One-based, because that is how `lopdf` numbers pages and how
    // `pagetree::drop_pages` reads them. The model's `source` is the zero-based
    // baseline page, and `ordered_pages` is that same order, so the two line up
    // by position rather than by anything either of them stores.
    let kept: Vec<u32> = plan.pages.iter().map(|page| page.source + 1).collect();
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
    let turns: Vec<(lopdf::ObjectId, u8)> = plan
        .pages
        .iter()
        .filter_map(|page| {
            Some((
                *pages.get(page.source as usize)?,
                (page.turns + view % 4) % 4,
            ))
        })
        .collect();

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
        kept,
        dropped,
        turns,
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
        kept,
        dropped,
        turns,
        moved,
        encryption,
    } = checked;

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
    let sites = mark_sites(&doc, &pages, &kept, &plan.marks)?;
    let written = write_marks(&mut doc, &plan.marks, &sites)?;

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
    if !dropped.is_empty()
        || moved
        || redacted.annots > 0
        || redacted.outline > 0
        || redacted.fields > 0
        || redacted.images > 0
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

/// The opacity of a highlight's wash, as `/CA`.
///
/// Below 1 because a wash is meant to be read through. The blend mode does most
/// of that work --- see [`appearance_stream`] --- and this is what keeps the mark
/// legible in a reader that ignores the blend and paints the fill flat.
const WASH_ALPHA: f32 = 0.4;

/// Where one annotation goes, read out of the document it is measured against.
///
/// **Everything a mark needs from the *reader*, gathered before anything is
/// written.** The two save paths write into different documents: a rewrite adds
/// objects to the same `Document` it measured, and an append adds them to an
/// update section while the geometry, the page boxes and the existing `/Annots`
/// all live in the previous revision. Splitting the read from the write is what
/// lets one implementation serve both --- and this repository has just paid for
/// the alternative, where `print.rs` grew a second page walk and it silently
/// stopped writing marks at all.
///
/// It is also the reason [`attach`] takes a site rather than re-reading the
/// page: on the append path the page dictionary is frequently *not* in the
/// document being written to, because a page whose `/Annots` is its own object
/// never has to be rewritten.
struct MarkSite {
    /// The page object the annotation hangs off, in the previous revision.
    page: ObjectId,
    /// The page as it is displayed, for mapping the mark's quads into it.
    shown: DisplayedPage,
    /// Where this page keeps its annotation list.
    annots: AnnotsSite,
}

/// The three shapes a page's `/Annots` comes in, and they are not equivalent.
///
/// Which one a page has decides **how large the edit is**, which matters only on
/// the append path and matters a great deal there: extending an array that is
/// its own object leaves the page dictionary untouched, and an inline array
/// cannot be extended without rewriting the page. `docs/PLAN.md` §5 records that
/// as the one document-shape dependency to carry into Phase 2, and it is
/// measured rather than assumed --- the spike narrowed a signed document's
/// complaint to two objects by preferring the array.
enum AnnotsSite {
    /// `/Annots 12 0 R` --- its own object, and the cheap case.
    ArrayObject(ObjectId),
    /// Written out inside the page dictionary, which therefore has to be
    /// rewritten. The entries come along, because the writer does not have the
    /// page to read them back from.
    Inline(Vec<Object>),
    /// No `/Annots` at all. The page is rewritten, as for an inline array.
    Absent,
}

/// Reads what every mark needs, before anything is written.
///
/// `kept` is the one-based page numbers being written, used only for the
/// shared-object refusal.
///
/// # Errors
///
/// A mark naming a page the file does not have; a mark on a page object that
/// more than one kept page number names; or a mark whose quads map to nothing.
fn mark_sites(
    read: &Document,
    pages: &[ObjectId],
    kept: &[u32],
    marks: &[PlannedMark],
) -> Result<Vec<MarkSite>, String> {
    let mut sites = Vec::with_capacity(marks.len());
    for mark in marks {
        let page = *pages.get(mark.source as usize).ok_or_else(|| {
            format!(
                "a mark names page {}, which this document does not have",
                mark.source + 1
            )
        })?;

        // The same refusal as `unshared` and for the same reason, one level on:
        // an annotation is attached to a page *object*, so a mark made on page 3
        // would appear on page 7 as well when `/Kids` names one object twice.
        // `docs/TRAPS.md` has this shape twice already, once live in `print.rs`
        // for months.
        if kept
            .iter()
            .filter(|number| pages.get(**number as usize - 1) == Some(&page))
            .count()
            > 1
        {
            return Err(format!(
                "page {} is the same page object as another page in this file, so a mark on it \
                 would appear on both. tpdf will not write it.",
                mark.source + 1
            ));
        }

        let annots = match read
            .get_object(page)
            .and_then(Object::as_dict)
            .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?
            .get(b"Annots")
        {
            Ok(Object::Reference(array)) => AnnotsSite::ArrayObject(*array),
            Ok(Object::Array(entries)) => AnnotsSite::Inline(entries.clone()),
            // Anything else is a page whose `/Annots` is not a list --- a
            // malformed document, and the same answer as having none: a list of
            // our own replaces it. That is what the previous implementation did
            // through its `_` arm, stated rather than inherited.
            _ => AnnotsSite::Absent,
        };

        sites.push(MarkSite {
            page,
            shown: displayed_page(read, page),
            annots,
        });
    }
    Ok(sites)
}

/// Writes each mark as an annotation, into whichever document is being built.
///
/// `sites` is [`mark_sites`]'s answer for the same `marks`, in the same order.
/// The pairing is positional, which is why neither is public and both are
/// produced side by side at each of the two call sites.
///
/// **One object per mark, appended to its page's `/Annots`.** That array may be
/// absent, written inline, or an indirect reference to an array; `AGENTS.md`
/// records that the distinction decides how large an annotation edit is, and
/// [`attach`] handles all three.
///
/// **The coordinates are the whole of the difficulty.** A mark is held in
/// display space --- what the reader dragged across, y downwards from the
/// displayed page's top-left corner --- and `/QuadPoints` is the page's own
/// space, y upwards, before `/Rotate` and measured from the media box. The
/// mapping is [`crate::text::from_device`] followed by the crop box's origin,
/// which is exactly the two steps `annots.rs` performs in reverse when it reads
/// one back. Those are separate implementations, which is what makes
/// `annot-probe`'s round trip a differential rather than a tautology.
///
/// # Errors
///
/// A mark whose quads map to nothing.
fn write_marks(
    doc: &mut Document,
    marks: &[PlannedMark],
    sites: &[MarkSite],
) -> Result<MarksWritten, String> {
    for (mark, site) in marks.iter().zip(sites) {
        let MarkSite {
            page,
            shown,
            annots,
        } = site;
        let (page, shown) = (*page, *shown);
        let quads = user_quads(mark, shown);
        if quads.is_empty() {
            return Err(format!(
                "a mark on page {} covers no area in that page's own space",
                mark.source + 1
            ));
        }

        let rect = bounds(&quads);
        // **A comment gets no appearance stream from us, and that is not a gap.**
        // Every reader synthesises the icon for a `/Text` annotation --- the
        // specification describes `/Name` as choosing one and readers are
        // expected to draw it --- and they draw their own house style at their
        // own size whatever we write. Supplying one would mean shipping a
        // hand-drawn speech bubble that looks foreign in Acrobat and in Preview,
        // and `--mode noap` already measures that PDFium renders these without
        // one: the note icon it generates fills 637 of the 756 pixels in its
        // rectangle.
        //
        // The three markup kinds are the opposite case and keep theirs: a
        // reader that declines to synthesise a highlight shows nothing at all,
        // which is why `appearance_stream` exists. **A box is on that side of
        // the line, not the comment's**, and it is the reason this asks `paint`
        // rather than `is_note`: nothing synthesises a rectangle, so a
        // `/Square` with no `/AP` is an annotation Acrobat draws as nothing.
        let strokes = user_strokes(mark, shown);
        let appearance = if paint(mark.kind) == Paint::None {
            None
        } else {
            Some(appearance_stream(
                doc,
                mark,
                &quads,
                &strokes,
                rect,
                shown.turns,
            ))
        };
        let dictionary = mark_dictionary(mark, page, &quads, &strokes, rect, appearance);
        let annotation = doc.add_object(dictionary);
        attach(doc, page, annots, annotation)?;
    }
    Ok(MarksWritten)
}

/// [`pagetree::apply_turns`], once the marks are written.
///
/// A wrapper for exactly one reason, and it is not indirection for its own sake:
/// `apply_turns` is general --- `print.rs` calls it too, and there no mark has
/// been made --- so the ordering token belongs *here*, where the ordering is,
/// rather than on a page-tree function whose other callers have other orders.
///
/// See [`MarksWritten`] for what the order is and what going the other way costs.
fn turn_pages(
    doc: &mut Document,
    turns: &[(lopdf::ObjectId, u8)],
    _written: &MarksWritten,
) -> Result<(), String> {
    apply_turns(doc, turns)
}

/// [`pagetree::apply_crops`], once the marks are written.
///
/// Here for the reason [`turn_pages`] is, and for the same constraint: a crop
/// moves the origin a mark's quads were measured from.
fn crop_pages(
    doc: &mut Document,
    crops: &[(lopdf::ObjectId, [f64; 4])],
    _written: &MarksWritten,
) -> Result<(), String> {
    apply_crops(doc, crops)
}

/// A mark's quads in the page's own space, `[llx, lly, urx, ury]` each.
///
/// Degenerate quads are dropped rather than written. The model already refuses a
/// mark where *every* quad is empty ([`crate::docmodel::Refusal::EmptyMark`]);
/// this is the per-quad half, and it exists because a selection that runs to the
/// end of a line legitimately produces one empty rectangle after a real one.
fn user_quads(mark: &PlannedMark, shown: DisplayedPage) -> Vec<[f64; 4]> {
    let (ox, oy) = (f64::from(shown.origin.0), f64::from(shown.origin.1));
    mark.quads
        .iter()
        .filter(|quad| quad.covers_area())
        .map(|quad| {
            let page = crate::text::from_device(
                shown.turns,
                shown.width,
                shown.height,
                [quad.left, quad.top, quad.right, quad.bottom],
            );
            // The origin comes back on *after* the turn, because it came off
            // before one: `annots.rs` shifts into crop space and then maps.
            [page[0] + ox, page[1] + oy, page[2] + ox, page[3] + oy]
        })
        .collect()
}

/// A mark's strokes in the page's own space, one `(x, y)` list each.
///
/// **Built on [`crate::text::from_device`] rather than beside it**, by handing
/// it the point as a rectangle of no size and taking the corner back. That looks
/// roundabout and is the point: the flip and the turn that map display space
/// onto the page are one rule, and a second copy written for points would be a
/// second thing to get right at every `/Rotate`. The trap index has that under
/// *"two copies of a distinction drift, and a mutation of one survives"*, and
/// the mapping is exactly where it would bite --- a wrong turn puts ink on the
/// page, in a plausible place, sideways.
///
/// A degenerate rectangle is safe here for the one reason it is unsafe in
/// [`user_quads`]: nothing downstream asks whether it covers area. `from_device`
/// is pure arithmetic on the corners.
fn user_strokes(mark: &PlannedMark, shown: DisplayedPage) -> Vec<Vec<(f64, f64)>> {
    let (ox, oy) = (f64::from(shown.origin.0), f64::from(shown.origin.1));
    mark.strokes
        .iter()
        .map(|stroke| {
            stroke
                .points
                .iter()
                .map(|point| {
                    let mapped = crate::text::from_device(
                        shown.turns,
                        shown.width,
                        shown.height,
                        [point.x, point.y, point.x, point.y],
                    );
                    (mapped[0] + ox, mapped[1] + oy)
                })
                .collect()
        })
        .collect()
}

/// The rectangle enclosing every quad.
fn bounds(quads: &[[f64; 4]]) -> [f64; 4] {
    quads
        .iter()
        .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |acc, q| {
            [
                acc[0].min(q[0]),
                acc[1].min(q[1]),
                acc[2].max(q[2]),
                acc[3].max(q[3]),
            ]
        })
}

/// The annotation dictionary for one mark.
///
/// `/F 4` sets the Print flag, which is what makes a highlight appear on paper
/// and in a print-to-PDF --- an annotation without it is a screen-only mark, and
/// a reader who highlights a document in order to print it would get a blank
/// page back.
///
/// `/NM` is the mark's own id. It has to be unique within the page and ours are
/// unique within the document, which is the stronger property; it is written
/// because a reader that reopens the file and edits it needs a name for the
/// annotation that is not its position in an array.
fn mark_dictionary(
    mark: &PlannedMark,
    page: ObjectId,
    quads: &[[f64; 4]],
    strokes: &[Vec<(f64, f64)>],
    rect: [f64; 4],
    appearance: Option<ObjectId>,
) -> Dictionary {
    let note = is_note(mark.kind);
    let mut dictionary = Dictionary::new();
    dictionary.set("Type", Object::Name(b"Annot".to_vec()));
    dictionary.set("Subtype", Object::Name(subtype(mark.kind).to_vec()));
    dictionary.set("Rect", numbers(rect));
    // **`/QuadPoints` is a text-markup key**, and neither of the two kinds a
    // reader places themselves may carry one --- see [`is_text_markup`], which
    // is the question this used to ask as "is it a comment" because the comment
    // was then the only kind it was true of. Writing quads on a `/Square` is the
    // kind of thing most readers ignore and one day something does not, and it
    // would also be a lie: the quad there is the mark's own box, not a run of
    // words it covers.
    if is_text_markup(mark.kind) {
        dictionary.set("QuadPoints", quad_points(quads));
    }
    // **`/InkList` is required on an `/Ink` and meaningless on anything else**,
    // so it is asked of the paint rather than of the kind --- the same reasoning
    // `is_text_markup` above records, one kind later. It is written as well as
    // the appearance stream, not instead of it: the `/AP` is what every reader
    // actually draws, and the list is what a reader that regenerates
    // appearances, or an editor that wants to reshape the line, reads to find
    // out what was drawn. A file with only the `/AP` is a picture of ink rather
    // than ink.
    if paint(mark.kind) == Paint::Path {
        dictionary.set(
            "InkList",
            Object::Array(
                strokes
                    .iter()
                    .map(|stroke| {
                        Object::Array(
                            stroke
                                .iter()
                                .flat_map(|(x, y)| {
                                    [Object::Real(*x as f32), Object::Real(*y as f32)]
                                })
                                .collect(),
                        )
                    })
                    .collect(),
            ),
        );
    }
    dictionary.set(
        "C",
        Object::Array(mark.color.iter().map(|c| Object::Real(*c)).collect()),
    );
    dictionary.set(
        "CA",
        Object::Real(if is_wash(mark.kind) { WASH_ALPHA } else { 1.0 }),
    );
    dictionary.set("F", Object::Integer(4));
    dictionary.set("P", Object::Reference(page));
    // Which standard stamp this is, in the specification's spelling.
    //
    // **Written even though it is not what draws the stamp here.** `/AP` wins in
    // every reader that has one, and this file always writes one --- so this is
    // for a reader that would synthesise an appearance instead, and that reader
    // draws from PDF 32000-1's list and nothing else. Writing a name outside it
    // would be worse than writing none.
    if let Some(name) = mark.stamp {
        dictionary.set("Name", Object::Name(name.pdf_name().to_vec()));
    }
    if note {
        // The icon a reader sees. `/Comment` is the speech bubble in every
        // reader that draws these; `/Note` is the folded page, which is the
        // name a reader would guess from our own serde spelling and the wrong
        // picture for the thing this command makes.
        dictionary.set("Name", Object::Name(b"Comment".to_vec()));
        // Closed. A file whose comments all spring open on load buries the page
        // under popups, and every reader offers its own way to open one.
        dictionary.set("Open", Object::Boolean(false));
    }
    if mark.kind == MarkKind::TextBox {
        // **`/DA` is required on a `/FreeText` and on nothing else.** It is the
        // appearance a reader falls back to when it regenerates the annotation
        // itself -- which Acrobat does whenever the text is edited in *its* UI,
        // and which it cannot do at all without this. An `/AP` alone is enough
        // to *display* the mark and leaves it uneditable everywhere but here.
        //
        // The font name and size have to match the appearance stream's, or a
        // reader that regenerates redraws the same words at a different size.
        // Both come from the same two constants, so they cannot drift.
        //
        // The colour is the text's, written as `rg` because `/DA` describes a
        // fill. `/C` above is the annotation's *background* for this subtype
        // rather than its ink, which is why the two are not the same operator
        // and why a text box is the one kind whose `/C` a reader does not see as
        // the mark's colour.
        let [r, g, b] = mark.color;
        dictionary.set(
            "DA",
            Object::string_literal(format!(
                "/{TEXT_FONT} {size} Tf {r} {g} {b} rg",
                size = textbox::SIZE
            )),
        );
    }
    if let Some(appearance) = appearance {
        dictionary.set("AP", {
            let mut ap = Dictionary::new();
            ap.set("N", Object::Reference(appearance));
            Object::Dictionary(ap)
        });
    }
    // Written as PDFDocEncoded literals. Both are the reader's own text rather
    // than a document's, so the encoding question `annots.rs` answers on the way
    // in does not arise on the way out --- but a non-ASCII author would be
    // mangled by a literal, so anything outside ASCII goes out as UTF-16BE with
    // the byte-order mark the specification asks for.
    dictionary.set("T", text_string(&mark.author));
    dictionary.set("Contents", text_string(&mark.note));
    dictionary.set("M", text_string(&mark.made));
    dictionary
}

/// The PDF name for a mark's kind.
///
/// A `match` rather than a table, so that adding a [`MarkKind`] is a compile
/// error here rather than a mark that silently writes as a highlight.
fn subtype(kind: MarkKind) -> &'static [u8] {
    match kind {
        MarkKind::Highlight => b"Highlight",
        MarkKind::Underline => b"Underline",
        MarkKind::Squiggly => b"Squiggly",
        // `/StrikeOut`, with that capitalisation. The variant is `StrikeOut`
        // and the serde name is `strikeout`; this is the only place all three
        // spellings meet, which is why it is a `match` and not a `to_lowercase`.
        MarkKind::StrikeOut => b"StrikeOut",
        // `/Text`, which is a comment bubble and not text on the page. The
        // reader's word for it is "comment", the serde name is `note`, and this
        // is the third spelling --- the same arrangement as `StrikeOut` above,
        // and the reason both are a `match` rather than a `to_lowercase`.
        MarkKind::Note => b"Text",
        // `/Square`, which is a rectangle and is not necessarily square: the
        // specification uses that name for the family holding `/Circle` too.
        // The word a reader sees is "box"; the third spelling once again.
        MarkKind::Square => b"Square",
        // `/Ink`, and the one place in this `match` where the PDF name and the
        // variant agree while the reader's word does not: a reader sees "Draw".
        // Same three-spelling arrangement as the four above it.
        // `/Circle`, which is the specification's name for an ellipse and not
        // a claim that it is round --- exactly as `/Square` above is not a claim
        // that the box is square. Both are the names of one family.
        MarkKind::Ellipse => b"Circle",
        // `/FreeText`, where "free" means unattached to a text selection rather
        // than anything about the words. A reader sees "Text box".
        MarkKind::TextBox => b"FreeText",
        MarkKind::Ink => b"Ink",
        // `/Stamp`, and the one kind whose three spellings all agree.
        MarkKind::Stamp => b"Stamp",
    }
}

/// How a kind's ink is laid down.
///
/// **Called `Paint` rather than `Ink`, and the rename came with `MarkKind::Ink`.**
/// This answers *how* a mark is drawn; that names *which* mark it is, and
/// `ink(kind) -> Ink` beside `MarkKind::Ink` is legal Rust that reads as one
/// thing referring to itself. `Paint::Path` is the variant ink uses.
///
/// **One question with one exhaustive `match`.** This started as two booleans
/// and the box would have made it three, which is where copies of a distinction
/// begin to drift --- the trap index has that under its own title, and it is the
/// same argument `markband.ts` makes for being one function. What the writer
/// needs is a single value that decides the geometry, the blend mode and both
/// opacities together, because those four have never been independent.
///
/// `markband.ts` mirrors this across the language boundary for the overlay.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Paint {
    /// The whole quad, multiplied, so the words underneath stay readable.
    Wash,
    /// A band inside the quad, opaque and on top --- see [`line_rect`]. A
    /// translucent line reads as a smudge, and multiplied red over black text
    /// is black.
    Line,
    /// The quad's edge, opaque, leaving whatever is inside it visible. Which is
    /// the entire point of a box: it says "this", it does not cover it.
    Outline,
    /// The reader's own words, set in Helvetica inside the quad.
    ///
    /// **The only style whose content is not geometry.** Every other variant
    /// draws the mark's rectangle, a band inside it, its edge or a path; this
    /// one draws a string, which means the appearance stream needs a font in its
    /// resources and the writer needs to know how wide each glyph is. See
    /// `textbox.rs` for both.
    ///
    /// It is also the only one that reads [`PlannedMark::note`]. That field has
    /// always been carried to the writer --- it becomes `/Contents` for every
    /// kind --- and until now nothing drew from it.
    Text,
    /// A wave along the bottom of the quad, stroked.
    ///
    /// **Separate from [`Paint::Line`] for the reason [`Paint::Ellipse`] is
    /// separate from [`Paint::Outline`]**: the geometry differs, and geometry is
    /// what this enum decides. A rule is one filled rectangle; a wave is a
    /// stroked zigzag, and it is the only style here whose operator count
    /// depends on how wide the quad is.
    ///
    /// It is also the only one that sets its own line width. The header writes a
    /// single `w` for the whole stream, and a wave's thickness is
    /// [`LINE_FRACTION`] of *its own quad's* height --- which differs per quad on
    /// a run that crosses a heading. So the arm emits a `w` before each path.
    Wave,
    /// The quad's inscribed ellipse, stroked, leaving its inside visible.
    ///
    /// **Separate from [`Paint::Outline`] rather than a flag on it**, because
    /// the two differ in the one thing this enum exists to decide: the geometry.
    /// A box is one `re` operator; an ellipse is four Bézier arcs, since a PDF
    /// content stream has no ellipse primitive to call. Folding them together
    /// would mean an `Outline` arm that asks the kind again --- a second copy of
    /// the distinction `paint` already makes, which is the drift this enum's own
    /// doc comment above is about.
    ///
    /// Everything else the variant decides is the box's: opaque, on top, and
    /// inset by half the stroke width for [`outline_path`]'s reason.
    Ellipse,
    /// The strokes a reader drew, opaque, with round joins and caps.
    ///
    /// **The first that does not derive its geometry from the quad at all.** The
    /// other four are the quad, a band inside it, or its edge; this one is the
    /// path, and the quad is merely the rectangle that path happens to occupy.
    /// Round rather than mitred because the line is freehand: a mitre on a
    /// hand-drawn corner spikes, which reads as a rendering fault rather than as
    /// a style.
    Path,
    /// A word inside a border, both in the mark's colour.
    ///
    /// **The second style whose content is not geometry**, after [`Paint::Text`]
    /// --- and unlike that one the string is not the reader's. It is
    /// [`StampName::word`], one of a closed list, which is what lets the size be
    /// chosen to fit rather than wrapped: a stamp is one word and it should fill
    /// the rectangle the reader dragged.
    ///
    /// It is a border *and* a word rather than either alone. A word with no
    /// border reads as a text box, and a border with no word is a
    /// [`Paint::Outline`]; what makes a stamp recognisable is both together.
    Stamp,
    /// None of ours. The reader draws its own, which for `/Text` is the only
    /// way the icon can look like that reader's other comments.
    None,
}

/// Which of the seven a kind uses.
///
/// A `match` for [`subtype`]'s reason: adding a [`MarkKind`] has to be a compile
/// error here rather than a mark that silently draws as a highlight.
fn paint(kind: MarkKind) -> Paint {
    match kind {
        MarkKind::Highlight => Paint::Wash,
        MarkKind::Underline | MarkKind::StrikeOut => Paint::Line,
        MarkKind::Squiggly => Paint::Wave,
        MarkKind::Square => Paint::Outline,
        MarkKind::Ellipse => Paint::Ellipse,
        MarkKind::TextBox => Paint::Text,
        MarkKind::Ink => Paint::Path,
        MarkKind::Note => Paint::None,
        // **Not `Paint::None`, and the difference from the comment above was
        // measured.** A `/Stamp` with `/Name /Approved` and no `/AP` renders 0
        // non-white pixels through PDFium, against 336 for a `/Text` with no
        // `/AP` on the same page --- so a stamp with no appearance of ours is an
        // annotation that draws nothing at all, which is `MarkKind::Square`'s
        // situation rather than the comment's.
        MarkKind::Stamp => Paint::Stamp,
    }
}

/// Whether a kind is a comment bubble.
///
/// Narrower than it looks, and deliberately so. It used to answer three
/// questions at once --- whether to write `/Name` and `/Open`, whether to skip
/// `/QuadPoints`, and whether to write an appearance stream --- because the
/// comment was the only kind for which all three answers happened to coincide.
/// The box separated them: it also skips `/QuadPoints` and it very much needs an
/// appearance stream. So this now answers only the first, [`is_text_markup`]
/// answers the second and [`Paint::None`] the third.
fn is_note(kind: MarkKind) -> bool {
    matches!(kind, MarkKind::Note)
}

/// Whether a kind is a text-markup annotation, and therefore carries quads.
///
/// PDF 32000-1 lists `/QuadPoints` on `/Highlight`, `/Underline`, `/Squiggly`
/// and `/StrikeOut` and on no other subtype. The two kinds a reader places
/// themselves are positioned by `/Rect` alone, and writing quads on one would be
/// a lie as well as an unlisted key: the quad there is the mark's own box rather
/// than a run of words it covers.
fn is_text_markup(kind: MarkKind) -> bool {
    matches!(
        kind,
        MarkKind::Highlight | MarkKind::Underline | MarkKind::Squiggly | MarkKind::StrikeOut
    )
}

/// Whether a kind covers its quads rather than drawing inside or around them.
///
/// Derived from [`paint`] rather than matching again, so that a kind can never be
/// a wash here and something else there. It decides the blend mode and `/CA`.
///
/// **Public for `examples/turned_probe.rs`**, which has to know the same thing
/// for a reason that follows from the blend mode: a multiplied mark leaves a
/// pixel alone wherever the paper under it is already dark, so how much of its
/// box it inks is a reading about the page's content as well as about the mark.
/// A copy of this predicate there would be the second copy this doc comment
/// exists to refuse.
pub fn is_wash(kind: MarkKind) -> bool {
    paint(kind) == Paint::Wash
}

/// A line's thickness as a fraction of the marked text's height.
///
/// Proportional rather than PDFium's fixed 1 pt. Both are defensible for body
/// text and only one survives a heading: a 1 pt strikeout across 36 pt type is
/// a hairline, and a reader who cannot see the line they just drew tries again.
/// No floor is needed --- a quad with no area is dropped by [`user_quads`]
/// before this is reached.
const LINE_FRACTION: f64 = 0.07;

/// How tall a squiggle's band is, as a fraction of the marked text's height.
///
/// Peak to trough, measured from the bottom of the quad up. Proportional for
/// [`LINE_FRACTION`]'s reason --- the text decides how big the mark is --- and
/// **larger than it on purpose**: this is the number that makes a squiggle
/// distinguishable from an underline rather than a wobbly one.
///
/// At 0.18 against the rule's 0.07 there is a clear strip of quad, from 7% to
/// 18% of the height, where an underline has no ink and a squiggle does. Every
/// check that tells the two kinds apart reads somewhere in that strip, in the
/// file and on the overlay both.
///
/// **No check derives its band from this constant**, which would make the test
/// move with the thing it polices and stop being able to fail --- see the trap
/// about a check that measures along the axis it is policing. They use fixed
/// fractions chosen to sit inside the gap.
const SQUIGGLE_HEIGHT: f64 = 0.18;

/// One full cycle of a squiggle, as a multiple of [`SQUIGGLE_HEIGHT`]'s band.
///
/// Two, so a cycle is as wide as the band is tall twice over, and the zigzag
/// climbs at 45 degrees. Tied to the band rather than to the quad's width
/// because a wave whose period was a fraction of the *width* would have fewer,
/// longer cycles on a long run and more on a short one --- the same mark drawn
/// at two frequencies depending on how many words the reader picked.
///
/// `markband.ts` holds both of these, unavoidably: the overlay draws the same
/// wave in another language. They are compared by rendering rather than by
/// sharing a literal --- `annot-probe --mode wave` reads the file's and
/// `viewer_check.py`'s overlay phase reads the screen's, and **neither reads
/// these constants**, which is what lets either of them fail.
///
/// Private, like [`LINE_FRACTION`], because nothing outside this module has a
/// reason to know them: [`OUTLINE_WIDTH`] is `pub` only because the probe
/// measures a stroke it has to predict the width of, and no check here predicts
/// a wave's geometry --- they read a strip chosen to sit between the two
/// constants rather than on either.
const SQUIGGLE_PERIOD: f64 = 2.0;

/// One line of text as a hex string of `/WinAnsiEncoding` bytes.
///
/// **A hex string rather than a literal `(...)`, and the reason is an encoding
/// bug that would have been invisible in ASCII.** The content stream is built as
/// a Rust `String`, which is UTF-8, so pushing `ü` into it writes the two bytes
/// `C3 BC` where WinAnsi wants the one byte `FC`. Every English text box would
/// have looked perfect and every German one would have drawn `Ã¼`.
///
/// Hex also removes the other half of the problem: no escaping. A literal string
/// has to escape `(`, `)` and `\`, and a reader typing a smiley `:-)` into a
/// text box is not an unusual thing to do.
///
/// Latin-1 and WinAnsi agree byte for byte over `A0..=FF`, and
/// `textbox::encodable` admits nothing else above ASCII, so the code point *is*
/// the byte.
fn winansi_hex(line: &str) -> String {
    let mut out = String::with_capacity(line.len() * 2);
    for ch in line.chars() {
        let code = ch as u32;
        // Unencodable characters are refused long before a plan is built; this
        // is the floor under that, and it writes a space rather than a byte that
        // would decode to something else entirely.
        let byte = if code <= 0xff { code as u8 } else { b' ' };
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

/// The name the appearance stream's resources give Helvetica.
///
/// Written into `/DA` as well as into the stream, and they have to agree: a
/// `/DA` naming a font the resources do not have is what makes a reader
/// substitute one, which is the whole failure `textbox.rs` avoids by measuring a
/// font every reader is required to have.
const TEXT_FONT: &str = "Helv";

/// A line's own rectangle inside a quad: `(bottom, height)` in the page's space.
///
/// **It stays inside the quad**, which is not a nicety. The appearance stream's
/// `/BBox` is the bounds of every quad, so anything drawn outside is clipped ---
/// an underline centred on the quad's bottom edge would lose its lower half in
/// every reader, and look like a thinner line rather than like a bug.
///
/// So an underline sits *on* the bottom edge and a strikeout is centred on the
/// middle. Both are expressed here rather than as an offset the caller applies,
/// because the two need different arithmetic and an offset that had to be
/// `LINE_FRACTION / 2.0` for one of them is a coincidence waiting to be tidied
/// into a defect.
fn line_rect(kind: MarkKind, bottom: f64, top: f64) -> (f64, f64) {
    let full = top - bottom;
    let thickness = full * LINE_FRACTION;
    match kind {
        // Not reached: a wash fills its quad and `appearance_stream` branches
        // before asking. Answered rather than `unreachable!()`, so that a fourth
        // kind added without reading this is a wrong-looking mark rather than a
        // panic in front of a reader.
        MarkKind::Highlight => (bottom, full),
        MarkKind::Underline => (bottom, thickness),
        // **Reached, unlike five of the six arms around it.** A wave is drawn by
        // `Paint::Wave` rather than filled, and it asks here for the same reason
        // the two rules do: where a kind's ink sits inside its quad is one
        // question, and answering it in two places is how a mark comes out at
        // one height in the file and another on screen.
        //
        // The band is taller than a rule and starts at the same edge, which is
        // the whole of what tells the two apart once they are drawn.
        MarkKind::Squiggly => (bottom, full * SQUIGGLE_HEIGHT),
        // Not reached: a text box's ink is lines of type placed from its top
        // edge downwards, which is not a band inside a quad at all. The whole
        // quad, for the box's reason.
        MarkKind::TextBox => (bottom, full),
        MarkKind::StrikeOut => (bottom + full / 2.0 - thickness / 2.0, thickness),
        // Not reached either, and one step further out than the highlight
        // above: a comment has no appearance stream of ours at all, so nothing
        // asks where its line goes. Answered with the whole quad for the same
        // reason.
        MarkKind::Note => (bottom, full),
        // Not reached, and for a third reason: a box has an appearance stream,
        // it is drawn by `outline_path` rather than by a filled rectangle, and
        // it has no band inside its quad to describe. The whole quad again.
        MarkKind::Square => (bottom, full),
        // Not reached, for the box's reason exactly: ink is drawn from its
        // strokes and has no band either. The whole quad, a fourth time, and
        // the fourth unreached arm is the argument for `line_rect` eventually
        // taking only the kinds that have a band --- not today, because
        // narrowing it would need a second enum whose only job is to say which
        // three those are.
        MarkKind::Ink => (bottom, full),
        // Not reached, a sixth time: a stamp is a border and a word, both placed
        // from its own rectangle by `Paint::Stamp`. The whole quad.
        MarkKind::Stamp => (bottom, full),
        // Not reached, for the box's reason exactly: an ellipse is drawn from
        // its quad by `Paint::Ellipse` and has no band inside it either. The
        // whole quad a fifth time, which is the argument above getting stronger
        // rather than weaker -- five of six arms are now unreachable.
        MarkKind::Ellipse => (bottom, full),
    }
}

/// How thick a box's outline is, in points.
///
/// **Fixed, where [`LINE_FRACTION`] is proportional**, and the reason the two
/// differ is worth stating because the obvious move is to make them agree. A
/// line through text scales with the text because the *text* decides how big
/// that mark is; nothing decides how big a box is except the reader, so a
/// border that grew with the rectangle would draw a box round a figure four
/// times heavier than one round a word. `markband.ts` holds the same number.
///
/// Public because `annot-probe --mode outline` measures the stroke it draws and
/// has to know how thick to expect it. A second copy of the number in the probe
/// would agree with a wrong value here as readily as with a right one.
pub const OUTLINE_WIDTH: f64 = 1.5;

/// The Bézier circle constant: `4/3 * (sqrt(2) - 1)`.
///
/// How far a quarter-arc's control points sit from its endpoints, as a fraction
/// of the radius, for the cubic that best approximates it. **Not an arbitrary
/// tuning value** --- it is what makes the curve pass through the arc's midpoint
/// exactly, and the worst radial error anywhere else is about 0.027% of the
/// radius. On a 200 pt radius that is 0.05 pt, a thirtieth of the stroke's own
/// width.
///
/// Written out rather than computed, because `f64::sqrt` is not a `const fn`,
/// and named rather than inlined four times, because a reader meeting
/// `0.5522847498307936` in a content stream has no way to tell a constant from a
/// typo.
///
/// **`markband.ts` does *not* hold a copy of this**, which is the one place the
/// overlay and the writer deliberately do different arithmetic. A canvas has
/// `ctx.ellipse` and draws a true ellipse; a content stream has no ellipse
/// operator and has to approximate. So the constant stays in the one place that
/// cannot avoid it, and the two are compared by rendering rather than by sharing
/// a literal --- `annot-probe --mode outline --kind ellipse` is that comparison.
/// (`OUTLINE_WIDTH` above *is* duplicated there, and saying so here is the
/// point: the neighbouring constant's rule is not this one's.)
const KAPPA: f64 = 0.5522847498307936;

/// How far a stamp's word sits inside its border, in points.
///
/// Larger than [`textbox::INSET`] and deliberately: a text box's inset stops
/// type touching an edge it has no border on, and a stamp's has to leave the
/// border visible as a border rather than as an underline to the word.
pub const STAMP_INSET: f64 = 4.0;

/// A capital's height as a fraction of the font size, for Helvetica.
///
/// **Used to place a stamp's baseline and to bound its size, and it is a
/// property of the face rather than a constant to tune.** Helvetica's capital
/// height is 718 units of 1000, and every word a stamp draws is upper case ---
/// so the ink's height is this and not the font size, which includes descender
/// space no stamp uses. Centring on the size instead leaves a stamp visibly high
/// in its box.
pub const STAMP_CAP: f64 = 0.718;

/// A box's path inside its quad: `[x, y, width, height]` in the page's space.
///
/// **Inset by half the stroke width**, which is the same trap [`line_rect`] is
/// written around and it bites harder here. A stroke straddles its path, so a
/// rectangle stroked exactly on the quad's edge puts half of every side outside
/// the appearance stream's `/BBox`, and a `/BBox` clips. The result is a box
/// with hairline edges rather than a missing one --- it looks like a thin
/// border, not like a bug, which is why it is arithmetic here rather than a
/// comment somewhere.
///
/// A quad thinner than the stroke is not special-cased: the inset then crosses
/// over and the rectangle is drawn inside out, which PDF renders as nothing.
/// [`crate::docmodel`] refuses an empty mark and the frontend refuses a box
/// under four points, so reaching this needs a caller that has bypassed both.
fn outline_path(quad: [f64; 4]) -> [f64; 4] {
    let inset = OUTLINE_WIDTH / 2.0;
    [
        quad[0] + inset,
        quad[1] + inset,
        (quad[2] - quad[0]) - OUTLINE_WIDTH,
        (quad[3] - quad[1]) - OUTLINE_WIDTH,
    ]
}

/// `/QuadPoints`: four corners per quad, upper-left, upper-right, lower-left,
/// lower-right.
///
/// **That order is not the one PDF 32000-1 §12.5.6.10 appears to describe**, and
/// it is the one every producer writes and every consumer expects --- the
/// specification's wording is a known erratum. Writing the specification's
/// literal reading produces a highlight that draws as an hourglass or not at
/// all, which is why this is stated here rather than left to look arbitrary.
fn quad_points(quads: &[[f64; 4]]) -> Object {
    Object::Array(
        quads
            .iter()
            .flat_map(|&[llx, lly, urx, ury]| {
                [
                    Object::Real(llx as f32),
                    Object::Real(ury as f32),
                    Object::Real(urx as f32),
                    Object::Real(ury as f32),
                    Object::Real(llx as f32),
                    Object::Real(lly as f32),
                    Object::Real(urx as f32),
                    Object::Real(lly as f32),
                ]
            })
            .collect(),
    )
}

fn numbers(values: [f64; 4]) -> Object {
    Object::Array(values.iter().map(|v| Object::Real(*v as f32)).collect())
}

/// Writes each planned body over the annotation it names.
///
/// **Every refusal here is about the plan disagreeing with the file**, which is
/// the case a reader can act on: the document changed under them, or the comment
/// they edited is gone. None of them is about the text, which is theirs.
///
/// The object is named rather than searched for, and that is the point of
/// `annots::Comment::object` --- a scan-order id could not survive the round
/// trip through the frontend and back, because inserting a comment anywhere
/// earlier renumbers every later one.
///
/// # Errors
///
/// The object not being in the document, not being a dictionary, or not being an
/// annotation. The last is checked rather than assumed: a plan naming an
/// arbitrary object would otherwise let a caller write `/Contents` onto a page,
/// a font or the catalog, and `/Contents` means something else entirely on a
/// page. Nothing in the application builds such a plan --- this runs in the
/// worker, on a plan that crossed a process boundary, and a refusal is cheaper
/// than reasoning about who could have sent it.
fn write_note_edits(
    incremental: &mut IncrementalDocument,
    notes: &[crate::edits::PlannedNoteEdit],
) -> Result<(), Refusal> {
    for note in notes {
        let id = (note.object.0, note.object.1);
        incremental
            .opt_clone_object_to_new_document(id)
            .map_err(|e| {
                Refusal::changed(format!(
                    "the comment being edited is not in this document any more: {e}"
                ))
            })?;
        let object = incremental
            .new_document
            .get_object_mut(id)
            .map_err(|e| Refusal::changed(format!("that comment could not be read: {e}")))?;
        let dictionary = object
            .as_dict_mut()
            .map_err(|_| Refusal::from("that comment is not an annotation"))?;
        if dictionary
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_err()
        {
            // Not "has the subtype we expect": any annotation has one, and the
            // check that matters is that this is an annotation at all.
            return Err("that comment is not an annotation".into());
        }
        dictionary.set("Contents", text_string(&note.body));
        dictionary.set("M", text_string(&note.made));
    }
    Ok(())
}

/// A PDF text string: an ASCII literal, or UTF-16BE with a byte-order mark.
///
/// The two encodings `annots.rs` reads are PDFDocEncoding and UTF-16BE, and this
/// writes the subset of the first that needs no table --- ASCII --- falling back
/// to the second for anything else. Choosing by content rather than always
/// writing UTF-16 keeps an ordinary author's name readable in a hex dump, which
/// is worth something when the next person to debug this is reading bytes.
fn text_string(value: &str) -> Object {
    if value.is_ascii() {
        return Object::string_literal(value);
    }
    let mut bytes = vec![0xFE, 0xFF];
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Object::String(bytes, lopdf::StringFormat::Hexadecimal)
}

/// A mark's box as the reader saw it, and the map back into the page.
///
/// Every quad reaching [`appearance_stream`] has been through [`user_quads`],
/// which maps the reader's rectangle into the page's own space. That is right
/// for the rectangle, which is a set of points, and wrong for everything drawn
/// *inside* it that has a direction: a rule belongs under the words as they are
/// read, and a line of type runs the way they are read. On a page carrying
/// `/Rotate 90` those two directions are a quarter turn from the page's own.
///
/// **Measured before this was written**, one mark of each kind on a 300 x 40
/// box, `testdata/inherited.pdf` (`/Rotate 90`) against
/// `testdata/text-base14.pdf`, reading where the saved file's ink landed inside
/// the box *as displayed*:
///
/// | kind | upright | turned |
/// |------|---------|--------|
/// | underline | a band at y 0.93..0.99 | a rule down the left edge, x 0.00..0.07 |
/// | strikeout | y 0.46..0.53 | a vertical line, x 0.46..0.53 |
/// | squiggly | y 0.81..0.99 | x 0.00..0.15 |
/// | text box | x 0.01..0.34 | a column at x 0.82..0.98, wrapped to the box's *height* |
/// | stamp | 25,011 px | 11,024 px, sideways |
///
/// A highlight, a box and an ellipse came out right at both, and they are the
/// three whose shape is symmetric under a quarter turn --- which is why nothing
/// caught this: the window sweep's agreement check compares *coverage
/// fractions*, and a band turned through a right angle covers the same
/// fraction of the same rectangle. The text box was the one kind it did report,
/// and the diagnosis recorded at the time was the box being too short.
///
/// The text box's own arithmetic says the rest: `textbox::wrap` was being given
/// 40 points where the reader had dragged 300, so the model broke those words
/// into one line and the file into eighteen.
struct Upright {
    /// The box's width as the reader saw it, in points.
    width: f64,
    /// Its height as the reader saw it, in points.
    height: f64,
    /// The page-space point the box's displayed top-left corner sits at.
    origin: (f64, f64),
    /// One point to the reader's right, in page space.
    right: (f64, f64),
    /// One point down the reader's page, in page space.
    down: (f64, f64),
}

impl Upright {
    /// The reader's view of a page-space quad, on a page turned `turns` quarters.
    ///
    /// The inverse of what [`crate::text::from_device`] applies, for corners and
    /// directions rather than for rectangles. **Two copies of one turn is the
    /// drift the trap index warns about**, so this is not left to agree with that
    /// function by inspection: `an_upright_box_is_the_rectangle_the_reader_dragged`
    /// composes the two at every quarter and asserts the round trip.
    fn of(turns: u8, quad: [f64; 4]) -> Self {
        let (w, h) = (quad[2] - quad[0], quad[3] - quad[1]);
        match turns % 4 {
            0 => Self {
                width: w,
                height: h,
                origin: (quad[0], quad[3]),
                right: (1.0, 0.0),
                down: (0.0, -1.0),
            },
            1 => Self {
                width: h,
                height: w,
                origin: (quad[0], quad[1]),
                right: (0.0, 1.0),
                down: (1.0, 0.0),
            },
            2 => Self {
                width: w,
                height: h,
                origin: (quad[2], quad[1]),
                right: (-1.0, 0.0),
                down: (0.0, 1.0),
            },
            _ => Self {
                width: h,
                height: w,
                origin: (quad[2], quad[3]),
                right: (0.0, -1.0),
                down: (-1.0, 0.0),
            },
        }
    }

    /// The page-space point `u` to the right of the box's displayed top-left
    /// corner and `v` below it.
    fn at(&self, u: f64, v: f64) -> (f64, f64) {
        (
            self.origin.0 + u * self.right.0 + v * self.down.0,
            self.origin.1 + u * self.right.1 + v * self.down.1,
        )
    }

    /// A `Tm` operator setting type running the reader's way, its baseline at
    /// [`Upright::at`].
    ///
    /// **`Tm` rather than the `Td` this replaced**, and the reason is the turn:
    /// `Td` can only move an origin, so it cannot say which way the glyphs face,
    /// and every line of a turned text box would still come out along the page's
    /// own axis. Absolute rather than relative also removes the trap the old
    /// comment here warned about --- a `Td` chain stacks every line on the first
    /// if one offset is written as an absolute.
    ///
    /// The third and fourth coefficients are the *negated* downward direction,
    /// because text space measures up and a reader's box measures down.
    fn text_matrix(&self, u: f64, v: f64) -> String {
        let (x, y) = self.at(u, v);
        // Negating a zero gives `-0.0`, which formats as `-0`: a legal number
        // that every reader accepts and no human recognises as the identity.
        // `v == 0.0` is true of both zeros, so this returns the positive one.
        let flat = |value: f64| if value == 0.0 { 0.0 } else { value };
        format!(
            "{} {} {} {} {x} {y} Tm",
            flat(self.right.0),
            flat(self.right.1),
            flat(-self.down.0),
            flat(-self.down.1)
        )
    }

    /// `[x, y, width, height]` for a `re`, covering the reader's `u0..u1` by
    /// `v0..v1`.
    ///
    /// A quarter turn keeps a rectangle axis-aligned and swaps which corner is
    /// which, so the two mapped corners are sorted rather than assumed.
    fn rect(&self, u0: f64, v0: f64, u1: f64, v1: f64) -> [f64; 4] {
        let (ax, ay) = self.at(u0, v0);
        let (bx, by) = self.at(u1, v1);
        [ax.min(bx), ay.min(by), (bx - ax).abs(), (by - ay).abs()]
    }
}

/// The appearance stream a reader draws when it does not generate its own.
///
/// **Not optional, even though both PDFKit and PDFium generate one.** Measured
/// before this was written: a `/Highlight` with no `/AP` renders in Preview, so
/// the file would look right in the two readers this repository can drive --- and
/// what a reader generates is *its* wash, not ours, so the same file would
/// differ between them and could differ again after an update. An `/AP` is what
/// makes the appearance the document's own.
///
/// `/BBox` is the annotation's rectangle in page coordinates and the matrix is
/// the identity, so §12.5.5's mapping of the transformed box onto `/Rect` is a
/// no-op and the content below can be written in page coordinates rather than in
/// a translated space of its own.
///
/// `/Multiply` is what keeps the glyphs readable: a flat fill over the text
/// hides it, and the text is already in the pixels underneath. It is the same
/// choice, for the same reason, that the viewer's overlay makes with
/// `globalCompositeOperation`.
fn appearance_stream(
    doc: &mut Document,
    mark: &PlannedMark,
    quads: &[[f64; 4]],
    strokes: &[Vec<(f64, f64)>],
    rect: [f64; 4],
    turns: u8,
) -> ObjectId {
    let style = paint(mark.kind);
    let mut state = Dictionary::new();
    state.set("Type", Object::Name(b"ExtGState".to_vec()));
    // Multiply for a wash so the words show through it, Normal for anything
    // opaque so it is the colour it says it is. A multiplied red line over black
    // text is black, which is a strikeout nobody can see.
    state.set(
        "BM",
        Object::Name(if style == Paint::Wash {
            b"Multiply".to_vec()
        } else {
            b"Normal".to_vec()
        }),
    );
    state.set("CA", Object::Real(1.0));
    state.set("ca", Object::Real(1.0));
    let state = doc.add_object(state);

    let mut states = Dictionary::new();
    states.set("GS0", Object::Reference(state));
    let mut resources = Dictionary::new();
    resources.set("ExtGState", Object::Dictionary(states));
    // A font, for the one style that draws words. **Only for that style**: a
    // `/Font` entry on a highlight's resources is dead weight in every saved
    // file, and one of the standard fourteen costs nothing to name but is still
    // a dictionary and a reference per mark.
    //
    // Helvetica with `/WinAnsiEncoding` and no `/FontDescriptor`, `/Widths` or
    // `/FirstChar`: it is one of the fourteen every reader is required to have,
    // so there is no file to embed and nothing to subset -- which is what keeps
    // this clear of the two font traps this repository already records.
    if style == Paint::Text {
        let font = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let mut fonts = Dictionary::new();
        fonts.set(TEXT_FONT, Object::Reference(font));
        resources.set("Font", Object::Dictionary(fonts));
    }

    // `rg` sets the *fill* colour and `RG` the stroke's, and one operator does
    // not imply the other: a path stroked after only `rg` comes out black,
    // which on a red box looks like a colour that was ignored rather than one
    // that was never set. Both are written, in one colour, so the two can never
    // disagree.
    // **The stroke width and the joins belong to the style, not to the file.**
    // A box is four right angles, which mitre cleanly at the default `0 j`; a
    // freehand line turns at whatever angle the reader's hand made, and a mitre
    // on a sharp one spikes out to a point --- which reads as a rendering fault
    // rather than as a style. `1 J 1 j` is round caps and round joins, and it
    // also gives a stroke its ends, without which a line stops square.
    let (width, joins) = match style {
        Paint::Path => (crate::docmodel::INK_WIDTH, "1 J 1 j "),
        _ => (OUTLINE_WIDTH, ""),
    };
    let mut content = format!(
        "/GS0 gs {r} {g} {b} rg {r} {g} {b} RG {width} w {joins}\n",
        r = mark.color[0],
        g = mark.color[1],
        b = mark.color[2],
    );
    // **Each style draws in a function of its own, and the loop is inside it.**
    // Two properties, and the signatures hold both rather than care holding
    // them: an arm cannot be reached with the wrong collection, because
    // `draw_path` takes strokes where the seven per-quad styles take quads and
    // the types differ; and a tenth style is a compile error here, which is the
    // whole reason this is a `match` on an enum rather than a pair of booleans.
    //
    // **The extraction is what made the first property structural.** Until
    // 2026-08-26 the nine bodies were written out inline --- 268 lines between
    // this comment and the close --- and nothing stopped an arm looping over the
    // wrong collection except that none of them did, with both in scope
    // throughout. The comment defending that arrangement carried three counts and
    // every one had gone stale: it said *five* styles, the loop *written out
    // three times*, and a *sixth* style being the compile error, against nine
    // arms, seven per-quad loops and a tenth. `docs/TRAPS.md` has that entry, and
    // this is the extraction it measured.
    //
    // **No context type, which that measurement expected to be the cost.** Each
    // arm reads at most a collection, one field of the mark and the turn, so a
    // parameter list per style says exactly what that style draws from --- which
    // is the property, and is precisely what one shared struct would hand back.
    match style {
        Paint::Wash => draw_wash(&mut content, quads),
        Paint::Line => draw_line(&mut content, quads, mark.kind, turns),
        Paint::Outline => draw_outline(&mut content, quads),
        Paint::Text => draw_text(&mut content, quads, &mark.note, turns),
        Paint::Stamp => draw_stamp(&mut content, quads, mark.stamp, turns),
        Paint::Wave => draw_wave(&mut content, quads, mark.kind, turns),
        Paint::Ellipse => draw_ellipse(&mut content, quads),
        Paint::Path => draw_path(&mut content, strokes),
        // Nothing. Unreachable, because the caller does not build an
        // appearance stream for a kind that has none; written out rather
        // than caught by a wildcard so that a kind added later is a compile
        // error here as well as everywhere else.
        Paint::None => {}
    }

    let mut dictionary = Dictionary::new();
    dictionary.set("Type", Object::Name(b"XObject".to_vec()));
    dictionary.set("Subtype", Object::Name(b"Form".to_vec()));
    dictionary.set("FormType", Object::Integer(1));
    dictionary.set("BBox", numbers(rect));
    dictionary.set("Resources", Object::Dictionary(resources));
    doc.add_object(lopdf::Stream::new(dictionary, content.into_bytes()))
}

/// The whole quad, filled.
fn draw_wash(out: &mut String, quads: &[[f64; 4]]) {
    for quad in quads {
        let (x, y) = (quad[0], quad[1]);
        let (width, height) = (quad[2] - quad[0], quad[3] - quad[1]);
        out.push_str(&format!("{x} {y} {width} {height} re f\n"));
    }
}

/// A band inside it, filled. Same operator, different rectangle.
///
/// **Measured in the reader's frame, not the page's**, because "the
/// bottom of the quad" is where a rule goes and a turned page has two
/// bottoms. `line_rect` answers in a y-up frame, so it is handed the
/// reader's box and its answer read back as a distance from the reader's
/// own bottom edge.
fn draw_line(out: &mut String, quads: &[[f64; 4]], kind: MarkKind, turns: u8) {
    for quad in quads {
        let seen = Upright::of(turns, *quad);
        let (base, band) = line_rect(kind, 0.0, seen.height);
        let [x, y, width, height] = seen.rect(
            0.0,
            seen.height - base - band,
            seen.width,
            seen.height - base,
        );
        out.push_str(&format!("{x} {y} {width} {height} re f\n"));
    }
}

/// Its edge, stroked. `re S` rather than `re f`, and the path is
/// inset so the stroke lands inside the /BBox -- see `outline_path`.
fn draw_outline(out: &mut String, quads: &[[f64; 4]]) {
    for quad in quads {
        let [x, y, width, height] = outline_path(*quad);
        out.push_str(&format!("{x} {y} {width} {height} re S\n"));
    }
}

/// The reader's words, one `Tj` per line, from the top of the box down.
///
/// **The whole layout is in the reader's frame**, which is what the box's
/// width has to be: `wrap` decides where the lines break, and handing it
/// the page's width breaks them against the box's *height* on a turned
/// page --- eighteen lines two glyphs wide where the model, which works in
/// the reader's frame throughout, had made one. `Upright` records the
/// measurement.
///
/// Lines that would fall below the box are dropped rather than drawn: the
/// /BBox clips them anyway, and emitting ink nobody can see makes the
/// stream disagree with what the overlay shows. The rule is
/// `viewer.ts`'s exactly --- a line is drawn while its baseline is still
/// inside the box --- so the two renderers stop at the same line.
fn draw_text(out: &mut String, quads: &[[f64; 4]], note: &str, turns: u8) {
    for quad in quads {
        let seen = Upright::of(turns, *quad);
        let width = seen.width - textbox::INSET * 2.0;
        let lines = textbox::wrap(note, textbox::SIZE, width.max(1.0));
        if lines.is_empty() {
            continue;
        }
        let leading = textbox::SIZE * textbox::LEADING;
        out.push_str(&format!(
            "BT /{TEXT_FONT} {size} Tf\n",
            size = textbox::SIZE
        ));
        for (index, line) in lines.iter().enumerate() {
            // The baseline sits one ascent below the top inset, not at
            // it: a line placed *at* the top edge hangs its whole body
            // above the box.
            let down = textbox::INSET + textbox::SIZE + leading * (index as f64);
            if down > seen.height {
                break;
            }
            out.push_str(&format!("{}\n", seen.text_matrix(textbox::INSET, down)));
            out.push_str(&format!("<{}> Tj\n", winansi_hex(line)));
        }
        out.push_str("ET\n");
    }
}

/// A border and one word, both in the mark's colour.
///
/// **The size is computed rather than fixed**, which is the difference
/// from `Paint::Text` above and the reason a stamp needs no wrapping. A
/// stamp is one word and a reader who drags a large rectangle means a
/// large stamp, so the size is whatever makes the word span the box
/// between its insets --- bounded above by what the height can hold, so a
/// wide flat rectangle gives a word that fits rather than one clipped by
/// the /BBox.
///
/// `advance` is the same Helvetica table `textbox.rs` wraps with, and
/// `helvetica-probe` measures it against what PDFium actually inks. A
/// stamp is the second consumer of it, which is worth noting because a
/// wrong entry here is visible as a word that is off-centre rather than
/// as a word in the wrong place.
fn draw_stamp(
    out: &mut String,
    quads: &[[f64; 4]],
    stamp: Option<crate::docmodel::StampName>,
    turns: u8,
) {
    for quad in quads {
        let Some(name) = stamp else {
            continue;
        };
        let word = name.word();
        // The reader's box, for `Paint::Text`'s reason and one of its
        // own: the size is a ratio of width to height, so on a turned
        // page the page's own box does not merely rotate the word, it
        // sets it at the size a rectangle of the other shape would take.
        // The border is unaffected and stays in page space --- a
        // rectangle is the same set of points at every quarter.
        let seen = Upright::of(turns, *quad);
        let inner_w = seen.width - STAMP_INSET * 2.0;
        let inner_h = seen.height - STAMP_INSET * 2.0;
        let [x, y, width, height] = outline_path(*quad);
        out.push_str(&format!("{x} {y} {width} {height} re S\n"));
        if inner_w <= 0.0 || inner_h <= 0.0 {
            continue;
        }
        // The advance at one point, so the ratio is a division rather
        // than a search. `max` guards a name that measured zero, which
        // no entry in the list does and which a table edit could make
        // true.
        let unit = textbox::advance(word, 1.0).max(f64::EPSILON);
        let size = (inner_w / unit).min(inner_h / STAMP_CAP).max(1.0);
        // Centred both ways. The baseline sits half a cap height below
        // the middle, because a word centred *on* the middle hangs half
        // its body below it.
        let drawn = textbox::advance(word, size);
        let across = (seen.width - drawn) / 2.0;
        let down = (seen.height + size * STAMP_CAP) / 2.0;
        out.push_str(&format!("BT /{TEXT_FONT} {size} Tf\n"));
        out.push_str(&format!("{}\n", seen.text_matrix(across, down)));
        out.push_str(&format!("<{}> Tj\n", winansi_hex(word)));
        out.push_str("ET\n");
    }
}

/// A zigzag along the bottom of the quad, stroked.
///
/// **Straight segments rather than curves, and that is a decision.** A
/// squiggle could be drawn as arcs, and Acrobat's is; at this size the
/// difference is invisible and the cost is not. A zigzag is exact -- `l`
/// says what it means -- where a curve would put a second approximation
/// constant beside `KAPPA` for a shape whose whole peak-to-trough height
/// is under two points on body text.
///
/// Its own `w`, because the header wrote one width for the stream and
/// this thickness is a fraction of *this quad's* height. A run crossing a
/// heading has quads of two sizes and would otherwise get one thickness.
///
/// The trough sits half a stroke above the quad's bottom edge and the
/// peak half a stroke below the band's top, for `outline_path`'s reason:
/// the /BBox clips, and a stroke centred on the edge loses half its width
/// in every reader -- which reads as a thinner wave rather than as a bug.
fn draw_wave(out: &mut String, quads: &[[f64; 4]], kind: MarkKind, turns: u8) {
    for quad in quads {
        // The reader's frame, for the rule's reason: a wave runs along
        // the words and climbs away from them, and both of those are
        // directions rather than page axes.
        let seen = Upright::of(turns, *quad);
        let thickness = seen.height * LINE_FRACTION;
        let (base, band) = line_rect(kind, 0.0, seen.height);
        let low = base + thickness / 2.0;
        let high = base + band - thickness / 2.0;
        let half = band * SQUIGGLE_PERIOD / 2.0;
        // A quad too short to hold one climb would emit `m` and no
        // segment, which strokes nothing; a degenerate band is dropped
        // by `user_quads` long before this, and this guard is for the
        // arithmetic rather than for the data.
        if half <= 0.0 || high <= low {
            continue;
        }
        // `across` runs the way the words do and `up` measures from the
        // reader's bottom edge, which is what `line_rect` answered in.
        let point = |across: f64, up: f64| seen.at(across, seen.height - up);
        out.push_str(&format!("{thickness} w\n"));
        let (mx, my) = point(0.0, low);
        out.push_str(&format!("{mx} {my} m\n"));
        let mut along = 0.0;
        let mut climbing = true;
        while along < seen.width {
            let next = (along + half).min(seen.width);
            // The last segment is clipped to the quad's right edge, so
            // it ends part-way up its climb rather than overshooting.
            // Interpolated rather than snapped to the peak: a wave that
            // jumped to full height in a tenth of a period ends on a
            // near-vertical stroke, which looks like a stray tick.
            let reached = (next - along) / half;
            let (from, to) = if climbing { (low, high) } else { (high, low) };
            let (lx, ly) = point(next, from + (to - from) * reached);
            out.push_str(&format!("{lx} {ly} l\n"));
            along = next;
            climbing = !climbing;
        }
        out.push_str("S\n");
    }
}

/// Its inscribed ellipse, stroked. Four Bézier arcs, because a content
/// stream has no ellipse operator -- `re` is the only built-in shape
/// there is, and it is a rectangle.
///
/// KAPPA is what makes four cubics look like an ellipse rather than
/// nearly like one; `outline_path` insets first, for the reason it
/// gives, so the stroke lands inside the /BBox exactly as the box's does.
fn draw_ellipse(out: &mut String, quads: &[[f64; 4]]) {
    for quad in quads {
        let [x, y, width, height] = outline_path(*quad);
        let (rx, ry) = (width / 2.0, height / 2.0);
        let (cx, cy) = (x + rx, y + ry);
        let (ox, oy) = (rx * KAPPA, ry * KAPPA);
        // From the right of the ellipse, anticlockwise. `h` closes it
        // rather than the fourth arc's endpoint being trusted to land
        // back on the first: they agree to the last bit here, and a
        // path left open joins with a cap instead of a join, which
        // shows as a nick at three o'clock on a thick stroke.
        out.push_str(&format!("{} {cy} m\n", cx + rx));
        out.push_str(&format!(
            "{} {} {} {} {cx} {} c\n",
            cx + rx,
            cy + oy,
            cx + ox,
            cy + ry,
            cy + ry
        ));
        out.push_str(&format!(
            "{} {} {} {} {} {cy} c\n",
            cx - ox,
            cy + ry,
            cx - rx,
            cy + oy,
            cx - rx
        ));
        out.push_str(&format!(
            "{} {} {} {} {cx} {} c\n",
            cx - rx,
            cy - oy,
            cx - ox,
            cy - ry,
            cy - ry
        ));
        out.push_str(&format!(
            "{} {} {} {} {} {cy} c\n",
            cx + ox,
            cy - ry,
            cx + rx,
            cy - oy,
            cx + rx
        ));
        out.push_str("h S\n");
    }
}

/// The path itself: `m` to the first point, `l` to each of the rest, and
/// one `S` per stroke. A single `S` after all of them would join the end
/// of each stroke to the start of the next with a line the reader never
/// drew --- which is precisely the join `/InkList` exists to keep apart,
/// and it would look like a drawing rather than like a bug.
fn draw_path(out: &mut String, strokes: &[Vec<(f64, f64)>]) {
    for stroke in strokes {
        let Some(((x0, y0), rest)) = stroke.split_first() else {
            continue;
        };
        out.push_str(&format!("{x0} {y0} m\n"));
        for (x, y) in rest {
            out.push_str(&format!("{x} {y} l\n"));
        }
        out.push_str("S\n");
    }
}

/// Appends an annotation to a page's `/Annots`, whatever shape that array is in.
///
/// Three cases, and the middle one is why this is a function rather than three
/// lines at the call site: the entry may be missing, an inline array, or a
/// reference to an array object that other pages may also name. The reference is
/// followed and the array it points at is extended, so the file's own structure
/// is preserved rather than replaced with an inline copy.
///
/// # Errors
///
/// The page is not a dictionary, or `/Annots` is a reference to something that
/// is not an array. Both are malformed documents, and a mark written into one
/// anyway would be a mark nothing displays.
fn attach(
    doc: &mut Document,
    page: ObjectId,
    site: &AnnotsSite,
    annotation: ObjectId,
) -> Result<(), String> {
    // **The site is read rather than re-read, and that is what makes this work
    // on both paths.** It used to look the page up in `doc` --- correct for a
    // rewrite, where `doc` is the whole document, and wrong for an append, where
    // a page whose `/Annots` is its own object is deliberately *not* in the
    // update section. Asking there would fail on exactly the documents the
    // append is cheapest on.
    match site {
        AnnotsSite::ArrayObject(array_id) => {
            let array = doc
                .get_object_mut(*array_id)
                .and_then(Object::as_array_mut)
                .map_err(|e| format!("this page's /Annots is not an array: {e}"))?;
            array.push(Object::Reference(annotation));
        }
        // Both rewrite the page dictionary, and the entries come from the site
        // because the document being written to may not hold the page to read
        // them back from. A second mark on the same page finds the first through
        // `doc` --- which is why the array is read from there when it is already
        // present, and from the site when it is not.
        AnnotsSite::Inline(_) | AnnotsSite::Absent => {
            let existing = doc
                .get_object(page)
                .and_then(Object::as_dict)
                .ok()
                .and_then(|dict| dict.get(b"Annots").ok())
                .and_then(|found| found.as_array().ok())
                .cloned();
            let mut array = match (existing, site) {
                (Some(already), _) => already,
                (None, AnnotsSite::Inline(entries)) => entries.clone(),
                (None, _) => Vec::new(),
            };
            array.push(Object::Reference(annotation));
            doc.get_object_mut(page)
                .and_then(Object::as_dict_mut)
                .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?
                .set("Annots", Object::Array(array));
        }
    }
    Ok(())
}

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
        match chosen.get(&page.source) {
            None => {
                chosen.insert(page.source, (want, at));
                order.push(page.source);
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

/// Writes `bytes` to `out` via a sibling temporary file and a rename.
fn write_atomically(out: &Path, bytes: &[u8]) -> Result<(), String> {
    let staged = stage(out, |file| {
        use std::io::Write as _;
        file.write_all(bytes)
            .and_then(|()| file.flush())
            .map_err(|e| Refusal::from(format!("could not write {out:?}: {e}")))
    })
    .map_err(|why| why.message)?;
    commit(&staged, out)
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
mod tests {
    use super::*;
    use crate::edits::PageView;
    use crate::pagetree::effective_rotation;

    /// [`stage`] with a buffer, which is what it took before a worker did the
    /// writing.
    ///
    /// The tests below are about the staging file --- its name, its mode, that a
    /// collision moves to the next index --- and none of them is about where the
    /// bytes came from. Passing them through a closure at every call site would
    /// put four copies of the same two lines in front of the thing each test is
    /// actually asserting.
    fn stage_bytes(out: &Path, bytes: &[u8]) -> Result<PathBuf, Refusal> {
        stage(out, |file| {
            use std::io::Write as _;
            file.write_all(bytes)
                .map_err(|e| Refusal::from(format!("could not write {out:?}: {e}")))
        })
    }
    use lopdf::Object;
    use std::collections::HashSet;

    /// A plan that keeps every page of an `n`-page document, turning each by
    /// `turns[i]`.
    ///
    /// The ids are the model's own numbering --- one per baseline page, from 1 ---
    /// and nothing here reads them: a plan is addressed by `source`, and the id
    /// travels only so that this is the shape the model really produces.
    /// [`plan_of`], fingerprinted against a real file.
    ///
    /// Every in-place test needs one: `stage_in_place` refuses a plan with no
    /// fingerprint, which is the point of that refusal and is why three existing
    /// tests went red the moment it was added. They are the ones that exercise
    /// the path where the reader's own file is at stake.
    fn plan_opened_as(turns: &[u8], source: &Path) -> Plan {
        Plan {
            opened_as: Some(
                crate::fingerprint::Fingerprint::of(source).expect("fingerprint the fixture"),
            ),
            ..plan_of(turns)
        }
    }

    fn plan_of(turns: &[u8]) -> Plan {
        Plan {
            opened_as: None,
            baseline: turns.len() as u32,
            pages: turns
                .iter()
                .enumerate()
                .map(|(at, &turns)| PageView {
                    id: at as u64 + 1,
                    source: at as u32,
                    turns,
                    crop: None,
                })
                .collect(),
            redactions: Vec::new(),
            notes: Vec::new(),
            marks: Vec::new(),
        }
    }

    /// A plan over a `baseline`-page document that keeps only `kept`.
    ///
    /// `kept` is `(source, turns)`, zero-based, in the order the pages are to
    /// come out, which need not be the order the file has them.
    fn keeping(baseline: u32, kept: &[(u32, u8)]) -> Plan {
        Plan {
            opened_as: None,
            baseline,
            pages: kept
                .iter()
                .map(|&(source, turns)| PageView {
                    id: u64::from(source) + 1,
                    source,
                    turns,
                    crop: None,
                })
                .collect(),
            redactions: Vec::new(),
            notes: Vec::new(),
            marks: Vec::new(),
        }
    }

    #[cfg(target_os = "macos")]
    use crate::print_macos as os_pdf;
    #[cfg(not(target_os = "macos"))]
    use crate::print_win as os_pdf;

    /// Every staging file left beside `out`, whatever its counter.
    ///
    /// **Written the day the staging name stopped being predictable, because
    /// the four assertions it replaces became unable to fail.** They read
    /// `!out.with_extension(PARTIAL).exists()`, which was the exact name
    /// `stage` used to produce; it now produces `<name>.tpdf-partial-<pid>-<n>`,
    /// so that path is one no code writes and the assertion is satisfied by a
    /// directory full of leftovers. `docs/TRAPS.md`, *A property that holds by
    /// construction cannot test the thing it resembles*.
    fn partials_beside(out: &Path) -> Vec<PathBuf> {
        let dir = out.parent().unwrap_or(Path::new("."));
        let Some(name) = out.file_name().and_then(|n| n.to_str()) else {
            return Vec::new();
        };
        let prefix = format!("{name}.{PARTIAL}");
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
            })
            .collect()
    }

    /// A scratch directory that removes itself.
    struct Scratch(PathBuf);

    impl Scratch {
        /// A directory of this test's own, whatever anyone else called theirs.
        ///
        /// **The counter is not decoration.** The name used to be `{name}-{pid}`,
        /// so two tests that happened to pick the same string shared one
        /// directory --- and `new` begins by *deleting* it, while `Drop` deletes
        /// it again. Under `cargo test`'s thread pool that is one test removing
        /// another's working directory mid-run, which surfaces as an assertion
        /// failure in whichever test lost the race and says nothing about the
        /// name they share.
        ///
        /// It happened: `merge-encrypted` was taken by two tests, and the
        /// resulting flake was reproducible only in the full suite --- twelve
        /// isolated runs of the loser passed. The name is kept in the path
        /// because it is what makes a leftover directory legible; uniqueness is
        /// what makes it correct.
        fn new(name: &str) -> Scratch {
            use std::sync::atomic::{AtomicU32, Ordering};
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("tpdf-save-{name}-{}-{serial}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Scratch(dir)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(name: &str) -> Option<PathBuf> {
        let path = Path::new("../testdata").join(name);
        path.exists().then_some(path)
    }

    fn page_count(path: &Path) -> usize {
        Document::load(path).expect("load").get_pages().len()
    }

    /// How many pages `lopdf` finds **with the password**.
    ///
    /// Deliberately not [`page_count`], which loads without one: on an encrypted
    /// document that parses no objects at all and answers **0** on a perfectly
    /// good file. A test using it here would read a correct rewrite as an empty
    /// one, and --- worse in the other direction --- would read a rewrite that
    /// dropped the encryption as *more* correct, because the plaintext output
    /// would suddenly count.
    fn page_count_with(path: &Path, password: &str) -> usize {
        Document::load_with_options(
            path,
            lopdf::LoadOptions {
                password: Some(password.to_string()),
                ..Default::default()
            },
        )
        .expect("load")
        .get_pages()
        .len()
    }

    #[test]
    fn a_rewrite_of_an_encrypted_document_stays_encrypted() {
        // The whole increment, at the layer a mutation can aim at.
        // `examples/encrypted_rewrite_probe.rs` is the same claim checked by
        // `qpdf`, which is a reader sharing no code with the writer; this is
        // here so the mutation harness has something that goes red.
        let Some(source) = fixture("incr-encrypted-pw.pdf") else {
            println!("[SKIP] a_rewrite_of_an_encrypted_document_stays_encrypted: generate testdata/ (BUILD.md)");
            return;
        };
        let scratch = Scratch::new("enc-rewrite");
        let out = scratch.0.join("out.pdf");

        let before = page_count_with(&source, "swordfish");
        assert_eq!(before, 2, "the fixture is two pages");

        write_copy(&source, &keeping(2, &[(0, 0)]), &out, Some("swordfish")).expect("rewrite");

        // **Two assertions, and neither is redundant.** The page count says the
        // rewrite happened; the byte scan says the encryption came back. A
        // rewrite that silently dropped the encryption passes the first and
        // fails the second, which is exactly the defect that made this a
        // refusal for months.
        assert_eq!(
            page_count_with(&out, "swordfish"),
            1,
            "the rewrite should have dropped a page"
        );
        let raw = std::fs::read(&out).expect("read back");
        assert!(
            raw.windows(8).any(|w| w == b"/Encrypt"),
            "the rewritten document has no /Encrypt dictionary, so it was written in the clear"
        );
    }

    #[test]
    fn a_print_job_from_an_encrypted_document_is_refused_whatever_the_rewrite_can_do() {
        // **Written because the whole suite stayed green while this was broken.**
        // Making the rewrite preserve encryption removed `checked`'s blanket
        // refusal, and `print::route`'s `Working` arm calls `print_bytes`
        // *directly* -- so it never reaches `print::build`'s own guard, and the
        // refusal it had been relying on was one this increment took away.
        // `print::tests::an_encrypted_document_is_printed_whole_or_refused`
        // passed throughout, because every path it exercises goes through
        // `print::build`.
        //
        // The fixture is the empty-password one on purpose: it is the case
        // `lopdf` unlocks unprompted, so it is the one that stopped being
        // refused. The other is still refused by the locked guard and would pass
        // this test with the defect present.
        let Some(source) = fixture("incr-encrypted-open.pdf") else {
            println!("[SKIP] a_print_job_from_an_encrypted_document_is_refused_whatever_the_rewrite_can_do: generate testdata/ (BUILD.md)");
            return;
        };
        let why = print_bytes(&source, &keeping(2, &[(0, 0)]), NO_VIEW_TURN, None)
            .expect_err("a print job from an encrypted document must be refused");
        assert!(
            why.message.contains("encrypted"),
            "the refusal names the reason: {}",
            why.message
        );
        // And the same document IS rewritable, which is what makes the refusal
        // above a decision about printing rather than about the document.
        let scratch = Scratch::new("print-enc");
        write_copy(
            &source,
            &keeping(2, &[(0, 0)]),
            &scratch.0.join("out.pdf"),
            None,
        )
        .expect("the same document rewrites");
    }

    #[test]
    fn a_print_job_from_a_locked_document_names_the_escape_that_exists() {
        // **The sibling above, on the fixture it could not reach.** That test
        // uses the empty-password document because it was the only one that got
        // as far as the encryption refusal: without the reader's key `checked`
        // refuses first, and its sentence --- *open it with its password* --- is
        // advice to a reader who has done exactly that, naming an escape they
        // have already taken. This one asserts the message a reader can act on.
        //
        // A guard whose neighbour refuses the same input is untested by it, and
        // the neighbour here is one parse earlier.
        let Some(source) = fixture("incr-encrypted-pw.pdf") else {
            println!("[SKIP] a_print_job_from_a_locked_document_names_the_escape_that_exists: generate testdata/ (BUILD.md)");
            return;
        };
        let why = print_bytes(
            &source,
            &keeping(2, &[(0, 0)]),
            NO_VIEW_TURN,
            Some("swordfish"),
        )
        .expect_err("a print job from an encrypted document must be refused");
        assert!(
            why.message.contains("Print the whole document"),
            "the refusal must name the operation that works, not one the reader has already \
             done: {}",
            why.message
        );

        // **The control, and it is what makes the assertion above mean
        // something.** Without the password the refusal is the lock's, and it is
        // a different sentence --- so a `print_bytes` that quietly ignored its
        // new argument would pass the first assertion only by accident of which
        // message it happened to produce. These two must not be the same string.
        let locked = print_bytes(&source, &keeping(2, &[(0, 0)]), NO_VIEW_TURN, None)
            .expect_err("without the key the parse itself is refused");
        assert!(
            locked.message.contains("could not unlock"),
            "without the key the refusal is the lock's: {}",
            locked.message
        );
        assert_ne!(
            why.message, locked.message,
            "the key has to change which refusal the reader is given"
        );
    }

    #[test]
    fn a_merge_whose_base_is_password_protected_keeps_its_encryption() {
        // **Written because the reader would have been told tpdf broke.**
        // `write_merged` builds the base through `planned_bytes`, which since
        // the rewrite learned to preserve encryption hands back *encrypted*
        // bytes --- and the reload of those bytes did not take the password.
        // `lopdf` answers `Ok` with no objects for a document it cannot
        // authenticate, so the merge failed at the catalog and the message
        // blamed this module's own writer: "tpdf could not read back the
        // document it just built".
        //
        // Every other `write_merged` test passes `None`, so none of them could
        // have found this.
        let (Some(source), Some(other)) = (fixture("incr-encrypted-pw.pdf"), fixture("links.pdf"))
        else {
            println!("[SKIP] a_merge_whose_base_is_password_protected_keeps_its_encryption: generate testdata/ (BUILD.md)");
            return;
        };
        let scratch = Scratch::new("merge-encrypted-base");
        let out = scratch.join("merged.pdf");

        let merged = write_merged(
            &source,
            &keeping(2, &[(0, 0)]),
            std::slice::from_ref(&other),
            &out,
            Some("swordfish"),
        )
        .expect("a merge whose base is unlocked must go through");

        // Three assertions, and each answers a different way this can be wrong.
        // The count says the merge happened at all; reading it back *with* the
        // password says the output is a document rather than a shape; and the
        // `/Encrypt` scan says the base's own encryption survived, which is the
        // silent removal the incoming-file refusal exists to prevent arriving
        // through the base instead.
        assert_eq!(
            merged.pages as usize,
            1 + page_count(&other),
            "the plan decided what went in, not the file"
        );
        assert_eq!(
            page_count_with(&out, "swordfish"),
            1 + page_count(&other),
            "the merged file must reopen with the base's password"
        );
        let raw = std::fs::read(&out).expect("read back");
        assert!(
            raw.windows(8).any(|w| w == b"/Encrypt"),
            "the merge of an encrypted base was written in the clear"
        );
    }

    #[test]
    fn a_rewrite_without_the_password_is_refused_and_says_so() {
        // The control for the test above. Without it, deleting the whole
        // encryption branch leaves a probe that writes plaintext and a test
        // that never asked -- and a refusal is what a reader who has not
        // unlocked the document must still get.
        let Some(source) = fixture("incr-encrypted-pw.pdf") else {
            println!("[SKIP] a_rewrite_without_the_password_is_refused_and_says_so: generate testdata/ (BUILD.md)");
            return;
        };
        let scratch = Scratch::new("enc-locked");
        let out = scratch.0.join("out.pdf");
        let why = write_copy(&source, &keeping(2, &[(0, 0)]), &out, None)
            .expect_err("a locked document cannot be rewritten");
        assert!(
            why.message.contains("could not unlock"),
            "the refusal has to name the lock, not something the reader cannot act on: {}",
            why.message
        );
    }

    /// A rotation applied here has to be visible to a parser that shares no code
    /// with the one that wrote it.
    ///
    /// `rotated.pdf` is the fixture because its four pages carry 0/90/180/270 and
    /// are otherwise identical: on a document with one rotation throughout, a
    /// writer that turned the *wrong* page would produce the same set of
    /// rotations and nothing here could tell. The run says which of the two cases
    /// each fixture was, for the reason `print.rs` says it.
    #[test]
    fn a_third_parser_sees_the_turn_on_the_page_it_was_applied_to() {
        let scratch = Scratch::new("third-parser");
        let mut examined = 0;
        for name in ["rotated.pdf", "text-heavy.pdf", "mixed.pdf", "links.pdf"] {
            let Some(path) = fixture(name) else {
                println!("[SKIP] {name}: fixture not generated");
                continue;
            };
            let source = std::fs::read(&path).expect("read source");
            let Some(before) = os_pdf::read(&source) else {
                println!("[SKIP] {name}: the OS parser refused the source document");
                continue;
            };
            let count = before.pages.len();
            if count < 2 {
                println!("[SKIP] {name}: {count} page, nothing to leave alone");
                continue;
            }

            // One quarter turn on the second page and nothing anywhere else, so
            // the check sees both directions: the page that moved, and the pages
            // that must not have.
            let mut turns = vec![0u8; count];
            turns[1] = 1;

            let out = scratch.join(&format!("{name}.out.pdf"));
            write_copy(&path, &plan_of(&turns), &out, None)
                .unwrap_or_else(|e| panic!("{name}: {e}"));

            let written = std::fs::read(&out).expect("read written");
            let after = os_pdf::read(&written)
                .unwrap_or_else(|| panic!("{name}: the OS parser could not read the saved copy"));

            assert_eq!(after.pages.len(), count, "{name}: page count");
            let expected: Vec<i64> = before
                .pages
                .iter()
                .enumerate()
                .map(|(at, page)| (page.rotation + if at == 1 { 90 } else { 0 }).rem_euclid(360))
                .collect();
            let got: Vec<i64> = after
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect();
            assert_eq!(got, expected, "{name}: rotations");

            let distinct: HashSet<i64> = before
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect();
            let discriminating = if distinct.len() > 1 {
                "pins which page was turned"
            } else {
                "pins the composition only"
            };
            println!("[OK] {name:16} {count} pages, rotations {distinct:?} --- {discriminating}");
            examined += 1;
        }
        assert!(
            examined > 0,
            "no fixture was examined --- generate testdata/ (BUILD.md, Test fixtures)"
        );
    }

    /// Both documents' pages come out, in the order they were given.
    ///
    /// The page count is the assertion, and it is not a formality: a merge that
    /// dropped the incoming file, or wrote it twice, or lost the open one, is
    /// wrong in the count and in nothing else a smaller check would see.
    #[test]
    fn a_merge_holds_every_page_of_every_document() {
        let (Some(source), Some(other)) = (fixture("rotated.pdf"), fixture("links.pdf")) else {
            println!("[SKIP] a_merge_holds_every_page: generate testdata/ (BUILD.md)");
            return;
        };
        let scratch = Scratch::new("merge-both");
        let out = scratch.join("merged.pdf");
        let mine = page_count(&source);
        let theirs = page_count(&other);
        let merged =
            write_merged(&source, &plan_of(&vec![0u8; mine]), &[other], &out, None).expect("merge");
        // **The independent reader answers first, and the order is the point.**
        // Every other assertion here is `lopdf` reading back what `lopdf` wrote,
        // which agrees with itself about a page tree no shipping reader would
        // accept. The OS parser --- PDFKit on macOS, `Windows.Data.Pdf` on
        // Windows --- shares no code with the writer and none with PDFium either.
        //
        // Written before the `lopdf` count rather than after it because a check
        // that sits behind an assertion measuring the same quantity can never
        // go red: the one in front fires first, and the independent reader's
        // answer is never reached. This way a graft that added nothing reddens
        // the line that is evidence about *readers* rather than about us.
        //
        // Lenient, as every shipping parser is, so this says the file is
        // *readable* rather than well formed. A refusal is reported rather than
        // waved through: a merge the platform cannot open at all is the failure
        // this is here for.
        let written = std::fs::read(&out).expect("read the merge back");
        let read = os_pdf::read(&written).expect("the OS parser reads the merged document");
        assert_eq!(
            read.pages.len(),
            mine + theirs,
            "the OS parser counts every page of both documents"
        );
        assert_eq!(
            page_count(&out),
            mine + theirs,
            "and so does the parser that wrote it"
        );
        assert_eq!(merged.pages as usize, mine + theirs, "and it says so");
        assert_eq!(merged.files, 1);
        println!(
            "[OK] merged {mine} pages with {theirs} --- {} in all, {} to the OS parser",
            merged.pages,
            read.pages.len()
        );
    }

    /// The open document goes in **as the reader has it**, not as it is on disk.
    ///
    /// The one property that says the merge really goes through `planned_bytes`
    /// rather than reading the source file again. A plan that keeps two of four
    /// pages produces a merge two pages shorter than the file would --- and the
    /// turn is asserted beside it, because a count alone cannot tell a plan that
    /// was honoured in part from one that was honoured whole.
    #[test]
    fn the_open_documents_edits_reach_the_merge() {
        let (Some(source), Some(other)) = (fixture("rotated.pdf"), fixture("links.pdf")) else {
            println!("[SKIP] the_open_documents_edits: generate testdata/ (BUILD.md)");
            return;
        };
        let scratch = Scratch::new("merge-edited");
        let out = scratch.join("merged.pdf");
        let whole = page_count(&source);
        assert!(whole >= 2, "the fixture must have pages to drop");
        // Page 0 kept unturned, page 1 kept and turned a quarter. Both survive,
        // so a merge that dropped the plan and read the file would come out
        // `whole` pages rather than two.
        let plan = keeping(whole as u32, &[(0, 0), (1, 1)]);
        write_merged(&source, &plan, std::slice::from_ref(&other), &out, None).expect("merge");
        assert_eq!(
            page_count(&out),
            2 + page_count(&other),
            "the plan decided what went in, not the file"
        );
        let merged = Document::load(&out).expect("load");
        let pages: Vec<_> = merged.get_pages().into_values().collect();
        let source_doc = Document::load(&source).expect("load source");
        let before: Vec<_> = source_doc.get_pages().into_values().collect();
        let was = crate::pagetree::effective_rotation(&source_doc, before[1]);
        let now = crate::pagetree::effective_rotation(&merged, pages[1]);
        assert_eq!(
            now.rem_euclid(360),
            (was + 90).rem_euclid(360),
            "the reader's turn is in the merged file"
        );
        println!(
            "[OK] merged 2 edited pages of {whole} with {} others",
            page_count(&other)
        );
    }

    #[test]
    fn a_merge_of_no_documents_is_refused() {
        // Not a defensive check. The command's dialog can be dismissed, and a
        // merge of nothing that quietly wrote a copy would be a Save a copy the
        // reader did not ask for, under a name they chose for something else.
        let Some(source) = fixture("rotated.pdf") else {
            println!("[SKIP] a_merge_of_no_documents: generate testdata/ (BUILD.md)");
            return;
        };
        let scratch = Scratch::new("merge-empty");
        let out = scratch.join("merged.pdf");
        let why = write_merged(&source, &plan_of(&[0, 0, 0, 0]), &[], &out, None)
            .expect_err("nothing to merge");
        assert!(why.message.contains("at least one"), "{why}");
        assert!(!out.exists(), "and nothing was written");
    }

    #[test]
    fn a_merge_will_not_be_written_over_any_document_going_into_it() {
        // Two directions, one rule --- the open document and each incoming file.
        // The second is the one a single check would miss, and it is the easier
        // mistake for a reader to make: the file chooser they picked the inputs
        // in remembers the directory the save dialog then opens in.
        let (Some(source), Some(fixture_other)) = (fixture("rotated.pdf"), fixture("links.pdf"))
        else {
            println!("[SKIP] a_merge_will_not_be_written_over: generate testdata/ (BUILD.md)");
            return;
        };
        // **Both files are copied into the scratch directory first, and that is
        // not tidiness.** This test proves a guard by aiming a write at a file
        // that must not be written, so the mutation that deletes the guard makes
        // it *perform that write* --- and it did, twice, over `testdata/links.pdf`
        // itself, which grew from 8 pages to 12 and then to 16. Nothing said so:
        // the mutation was correctly reported as caught, the harness restores the
        // source file it edited and knows nothing about fixtures, and every later
        // run simply read a longer document. `docs/TRAPS.md` has the entry.
        //
        // `rotated.pdf` is copied for the same reason: the first assertion aims
        // at the *source*, so a broken guard rewrites that one instead.
        let scratch = Scratch::new("merge-elsewhere");
        let source = {
            let copy = scratch.join("open.pdf");
            std::fs::copy(&source, &copy).expect("copy the open document");
            copy
        };
        let other = {
            let copy = scratch.join("incoming.pdf");
            std::fs::copy(&fixture_other, &copy).expect("copy the incoming document");
            copy
        };
        let plan = plan_of(&vec![0u8; page_count(&source)]);
        let over_source = write_merged(&source, &plan, std::slice::from_ref(&other), &source, None)
            .expect_err("over the open document");
        assert!(over_source.message.contains("reading"), "{over_source}");
        let over_input = write_merged(&source, &plan, std::slice::from_ref(&other), &other, None)
            .expect_err("over a document going in");
        assert!(over_input.message.contains("going into it"), "{over_input}");
        // Neither file moved. Without this the two refusals above are the only
        // evidence, and a guard that reported a refusal *after* writing would
        // satisfy both --- which is the shape of the accident this test caused.
        assert_eq!(
            page_count(&source),
            page_count(&fixture_other.with_file_name("rotated.pdf")),
            "the open document was not written"
        );
        assert_eq!(
            page_count(&other),
            page_count(&fixture_other),
            "and neither was the document going in"
        );
        // The control: the same two files with a destination that is neither are
        // written. Without it both assertions above are satisfied by a function
        // that refuses everything.
        let out = scratch.join("merged.pdf");
        write_merged(&source, &plan, &[other], &out, None).expect("somewhere else is fine");
    }

    /// An encrypted document cannot be merged in, and is named.
    ///
    /// The same refusal `planned_bytes` states for the open document, for the
    /// same reason: `lopdf`'s serialiser writes plaintext and drops the
    /// dictionary, so a merged file would silently carry a
    /// permission-restricted document's pages with the restrictions gone.
    ///
    #[test]
    fn split_paths_number_from_one_and_never_use_the_chosen_name() {
        let names = split_paths(Path::new("/tmp/report.pdf"), 3);
        assert_eq!(
            names
                .iter()
                .map(|p| p.file_name().expect("named").to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["report-1.pdf", "report-2.pdf", "report-3.pdf"],
        );
        // The chosen name is not among them, which is the decision the doc
        // comment argues for rather than an accident of starting at one.
        assert!(
            !names
                .iter()
                .any(|p| p.file_name().is_some_and(|n| n == "report.pdf")),
            "the name the reader picked must not also be a part: {names:?}"
        );
    }

    #[test]
    fn split_paths_keep_a_dot_that_is_inside_the_stem() {
        // `file_stem` drops the last extension only. A reader who names a file
        // `report.v2.pdf` means the `.v2`, and `report-1.pdf` would eat it.
        let names = split_paths(Path::new("/tmp/report.v2.pdf"), 2);
        assert_eq!(
            names[0].file_name().expect("named").to_string_lossy(),
            "report.v2-1.pdf"
        );
    }

    /// Every page of the source comes out exactly once, in order, across the parts.
    ///
    /// `rotated.pdf` is the fixture for the reason its neighbour above gives:
    /// its four pages carry 0/90/180/270 and are otherwise identical, so the
    /// rotations *identify* the pages. A count per file is satisfied by a split
    /// that wrote the same two pages twice; reading which pages landed where is
    /// what makes an off-by-one in the group arithmetic visible here.
    #[test]
    fn a_split_writes_each_page_once_and_in_order() {
        let Some(source) = fixture("rotated.pdf") else {
            println!("[SKIP] a_split_writes_each_page_once: needs testdata/rotated.pdf");
            return;
        };
        let total = page_count(&source) as u32;
        assert_eq!(
            total, 4,
            "the fixture this test identifies pages by changed"
        );
        let scratch = Scratch::new("split-order");
        let out = scratch.join("part.pdf");

        let plans = [
            keeping(total, &[(0, 0), (1, 0)]),
            keeping(total, &[(2, 0), (3, 0)]),
        ];
        let done = write_split(&source, &plans, &out, None).expect("split");
        assert_eq!(done.paths.len(), 2);

        let mut seen: Vec<i64> = Vec::new();
        for path in &done.paths {
            let part = Document::load(path).expect("load a part");
            assert_eq!(
                part.get_pages().len(),
                2,
                "each part holds its own two pages"
            );
            for (_, id) in part.get_pages() {
                seen.push(
                    part.get_object(id)
                        .and_then(Object::as_dict)
                        .expect("page dictionary")
                        .get(b"Rotate")
                        .and_then(Object::as_i64)
                        .unwrap_or(0),
                );
            }
        }
        // The source's own four rotations, in the source's order. Any page
        // duplicated, dropped or reordered by the grouping changes this list.
        assert_eq!(seen, vec![0, 90, 180, 270], "written: {:?}", done.paths);
    }

    /// The refusal a split needs and a copy does not, with its control.
    ///
    /// The reader picked one name in a dialog and the platform asked about that
    /// one; every other part is a path this module invented, so replacing one is
    /// destroying a file nobody was warned about.
    #[test]
    fn a_split_refuses_an_existing_part_and_writes_nothing() {
        let Some(source) = fixture("rotated.pdf") else {
            println!("[SKIP] a_split_refuses_an_existing_part: needs testdata/rotated.pdf");
            return;
        };
        let total = page_count(&source) as u32;
        let scratch = Scratch::new("split-exists");
        let out = scratch.join("part.pdf");
        let plans = [keeping(total, &[(0, 0)]), keeping(total, &[(1, 0)])];

        // The *second* part, not the first: a guard that checks only as it goes
        // would have written part one before noticing, and the whole point is
        // that nothing is written.
        let taken = scratch.join("part-2.pdf");
        std::fs::write(&taken, b"not a pdf, and not to be destroyed").expect("plant");

        let why = write_split(&source, &plans, &out, None).expect_err("refused");
        assert!(why.message.contains("already exists"), "{why}");
        assert!(
            why.message.contains("part-2.pdf"),
            "the refusal has to name which file it was: {why}"
        );
        assert_eq!(
            std::fs::read(&taken).expect("still there"),
            b"not a pdf, and not to be destroyed",
            "the existing file is untouched"
        );
        assert!(
            !scratch.join("part-1.pdf").exists(),
            "and the part before it was never written"
        );

        // The control. Without it "refuses" is satisfied by a `write_split`
        // that refuses everything, and this whole test would pass against a
        // function whose body is one `Err`.
        std::fs::remove_file(&taken).expect("unplant");
        write_split(&source, &plans, &out, None).expect("the same call, with nothing in the way");
        assert!(scratch.join("part-1.pdf").exists() && scratch.join("part-2.pdf").exists());
    }

    #[test]
    fn a_split_into_one_file_is_refused() {
        let Some(source) = fixture("rotated.pdf") else {
            println!("[SKIP] a_split_into_one_file: needs testdata/rotated.pdf");
            return;
        };
        let scratch = Scratch::new("split-one");
        let plans = [keeping(page_count(&source) as u32, &[(0, 0)])];
        let why =
            write_split(&source, &plans, &scratch.join("part.pdf"), None).expect_err("refused");
        assert!(why.message.contains("at least two files"), "{why}");
    }

    /// **No `examined > 0` control**, unlike its neighbours. This fixture needs
    /// pyhanko, which the plain fixture run does not install --- so a checkout
    /// that generated `testdata/` the ordinary way has every other fixture and
    /// not this one, and a hard assertion here would be red on the machine with
    /// the fewest inputs rather than on the machine with a defect.
    #[test]
    fn an_encrypted_document_cannot_be_merged_in() {
        let (Some(source), Some(locked)) =
            (fixture("rotated.pdf"), fixture("incr-encrypted-open.pdf"))
        else {
            println!(
                "[SKIP] an_encrypted_document: needs testdata/incr-encrypted-open.pdf (pyhanko)"
            );
            return;
        };
        let scratch = Scratch::new("merge-encrypted");
        let out = scratch.join("merged.pdf");
        let plan = plan_of(&vec![0u8; page_count(&source)]);
        let why = write_merged(&source, &plan, std::slice::from_ref(&locked), &out, None)
            .expect_err("encrypted");
        assert!(why.message.contains("encrypted"), "{why}");
        assert!(
            why.message.contains("incr-encrypted-open.pdf"),
            "the refusal has to name which of the files it was: {why}"
        );
        assert!(!out.exists(), "and nothing was written");
        // The sentence a reader gets, as a sentence. Every assertion above is
        // satisfied by a message with a hole in it, and this one shipped with
        // eighteen spaces in the middle of it: a `\` line continuation inside
        // the Rust literal was eaten in transport, so the wrapped line arrived
        // as its own indentation. `cargo fmt` joining the line is what made it
        // visible, an hour after five mutations and the whole suite had passed
        // over it. A word check is the cheap general guard --- it does not
        // pin the wording, and it catches every member of that family.
        assert!(
            !why.message.contains("  "),
            "a refusal a reader reads must not carry the source's own wrapping: {why}"
        );
        println!("[OK] an encrypted document is refused by name");
    }

    /// A plan over `n` pages, cropping the page at `at` to `box_pt`.
    fn plan_cropping(n: usize, at: usize, box_pt: [f64; 4]) -> Plan {
        let mut plan = plan_of(&vec![0u8; n]);
        if let Some(page) = plan.pages.get_mut(at) {
            page.crop = Some(box_pt);
        }
        plan
    }

    /// A crop in the plan reaches the written file, on that page and no other.
    #[test]
    fn a_crop_reaches_the_file_it_was_planned_for() {
        let Some(path) = fixture("text.pdf") else {
            println!("[SKIP] text.pdf not generated");
            return;
        };
        let scratch = Scratch::new("crop");
        let count = page_count(&path);
        assert!(count > 1, "this needs a second page to be the control");
        let out = scratch.join("cropped.pdf");
        let want = [72.0, 100.0, 400.0, 600.0];
        write_copy(&path, &plan_cropping(count, 0, want), &out, None).expect("write");

        let after = Document::load(&out).expect("load written");
        let ids = ordered_pages(&after);
        let read = |id| crate::pagetree::box_on(&after, id, b"CropBox").map(|b| b.map(f64::from));
        assert_eq!(read(ids[0]), Some(want), "the cropped page");
        // The control: every other page is as it was. Without it a write onto
        // the shared `/Pages` node would satisfy the assertion above and crop
        // the whole document.
        for (at, id) in ids.iter().enumerate().skip(1) {
            assert_eq!(read(*id), None, "page {at} was cropped too");
        }
    }

    /// Two positions naming one page with different crops have no output.
    ///
    /// Nothing duplicates a page today, so this cannot arise from the model ---
    /// it is the writer refusing to be the thing that discovers it, the same
    /// shape `agreed_turns` refuses.
    #[test]
    fn one_page_cropped_two_ways_is_refused_and_cropped_one_way_is_not() {
        let mut plan = plan_of(&[0, 0]);
        plan.pages[1].source = 0;
        plan.pages[0].crop = Some([0.0, 0.0, 100.0, 100.0]);
        plan.pages[1].crop = Some([0.0, 0.0, 200.0, 200.0]);
        let why = agreed_crops(&plan).expect_err("two crops for one page");
        assert!(why.contains("cannot be cropped"), "{why}");

        // The control: the same document with the same crop twice is one entry
        // rather than a refusal, so this refuses a disagreement and not a
        // repetition.
        plan.pages[1].crop = plan.pages[0].crop;
        assert_eq!(
            agreed_crops(&plan).expect("one crop, twice"),
            vec![(0, [0.0, 0.0, 100.0, 100.0])]
        );
    }

    /// A plan with no crops asks the writer for nothing.
    #[test]
    fn a_plan_with_no_crop_writes_no_crop_box() {
        // The emptiness control. Without it, a version returning every page
        // unconditionally would pass the two tests above and write a `/CropBox`
        // onto every page of every document tpdf saves.
        assert_eq!(
            agreed_crops(&plan_of(&[0, 0, 0])).expect("no crops"),
            Vec::new()
        );
    }

    #[test]
    fn a_turn_composes_with_the_rotation_the_page_already_had() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("compose");
        let count = page_count(&path);
        // Two turns on every page, so each page's answer is its own start plus
        // 180 --- a writer that *set* rather than composed would produce 180
        // everywhere and this is the fixture where those differ.
        let turns = vec![2u8; count];
        let out = scratch.join("composed.pdf");
        write_copy(&path, &plan_of(&turns), &out, None).expect("write");

        let before = Document::load(&path).expect("load source");
        let after = Document::load(&out).expect("load written");
        let source_ids: Vec<_> = ordered_pages(&before);
        let written_ids: Vec<_> = ordered_pages(&after);
        for (at, (from, to)) in source_ids.iter().zip(&written_ids).enumerate() {
            let expected = (effective_rotation(&before, *from) + 180).rem_euclid(360);
            let got = effective_rotation(&after, *to).rem_euclid(360);
            assert_eq!(got, expected, "page {at}");
        }
    }

    /// A page nobody turned must come out byte-for-byte as it went in, and the
    /// interesting case is a page that *inherits* its rotation.
    ///
    /// Writing `/Rotate 0` onto such a page would be a change --- it would
    /// override the inherited value --- and it would look like a no-op in any
    /// check that only compares the pages that were turned.
    #[test]
    fn a_page_that_was_not_turned_keeps_an_inherited_rotation() {
        let scratch = Scratch::new("inherit");
        let source = scratch.join("inherited.pdf");
        std::fs::write(&source, inheriting_document()).expect("write fixture");

        let out = scratch.join("out.pdf");
        // Turn the second page only. The first inherits 90 from the tree and is
        // left alone.
        write_copy(&source, &plan_of(&[0, 1]), &out, None).expect("write");

        let after = Document::load(&out).expect("load written");
        let ids = ordered_pages(&after);
        assert_eq!(
            effective_rotation(&after, ids[0]).rem_euclid(360),
            90,
            "the untouched page still inherits its rotation"
        );
        assert_eq!(
            effective_rotation(&after, ids[1]).rem_euclid(360),
            180,
            "the turned page composed onto the inherited 90"
        );

        // The assertion that can actually fail, and the reason the two above
        // cannot. `effective_rotation` answers 90 whether the page states it or
        // inherits it, so writing the composed value onto an untouched page
        // leaves every number above unchanged --- the mutation that does exactly
        // that survived until this line existed.
        //
        // Absence of the key is the property the guard is for: a page the reader
        // did not turn comes out as it went in. It is not cosmetic. The walk in
        // `effective_rotation` is bounded at 64 and answers 0 when it gives up,
        // so writing its answer onto every page would silently *flatten* the
        // rotation of any page whose `/Parent` chain is longer than that, or
        // whose chain loops --- pages nobody asked to change.
        assert!(
            after
                .get_object(ids[0])
                .and_then(Object::as_dict)
                .expect("the untouched page is a dictionary")
                .get(b"Rotate")
                .is_err(),
            "the untouched page states no rotation of its own; it still inherits one"
        );
        assert!(
            after
                .get_object(ids[1])
                .and_then(Object::as_dict)
                .expect("the turned page is a dictionary")
                .get(b"Rotate")
                .is_ok(),
            "the control: the page that WAS turned does state one, so the \
             assertion above is about the guard rather than about lopdf dropping \
             every key it writes"
        );
    }

    /// Two pages under a `/Pages` node carrying `/Rotate 90`, built by hand so
    /// that nothing under test wrote the input.
    fn inheriting_document() -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let kids: Vec<Object> = (0..2)
            .map(|_| {
                doc.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => pages_id,
                    "Contents" => content,
                })
                .into()
            })
            .collect();
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => 2,
                "Rotate" => 90,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    /// Two page numbers, one page object: `/Kids` names the same page twice.
    ///
    /// Hand-built, because nothing in this repository writes a document like this
    /// and no fixture in the corpus is malformed this way --- which is exactly how
    /// the shape survived review twice.
    fn shared_page_document() -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page), Object::Reference(page)],
                "Count" => 2,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    /// The precondition every shared-page check rests on, asserted rather than
    /// assumed.
    ///
    /// `lopdf`'s `PageTreeIter` keeps no visited set today. If a future version
    /// deduplicates, the guards below become reachable by nothing while their
    /// outcome assertions keep passing --- so this is the check that says so, and
    /// it is the one to read first when it goes red.
    #[test]
    fn the_fixture_really_does_present_one_object_under_two_page_numbers() {
        let scratch = Scratch::new("shared-precondition");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");

        let doc = Document::load(&source).expect("load fixture");
        let ids = ordered_pages(&doc);
        assert_eq!(ids.len(), 2, "two page numbers");
        assert_eq!(
            ids[0], ids[1],
            "and both resolve to ONE object --- if lopdf has started deduplicating \
             its page walk, every shared-page guard is now dead code"
        );
    }

    #[test]
    fn a_page_reached_twice_is_turned_once() {
        let scratch = Scratch::new("shared-turn");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        // One quarter-turn asked for on each page number. They are one page, so
        // the answer is one quarter-turn, not two.
        write_copy(&source, &plan_of(&[1, 1]), &out, None).expect("agreeing turns are honoured");

        let after = Document::load(&out).expect("load written");
        let ids = ordered_pages(&after);
        assert_eq!(
            effective_rotation(&after, ids[0]).rem_euclid(360),
            90,
            "turned once. Composing per page number would read back the 90 it had \
             just written and leave 180"
        );
    }

    #[test]
    fn a_page_reached_twice_cannot_be_turned_two_ways() {
        let scratch = Scratch::new("shared-conflict");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        let why = write_copy(&source, &plan_of(&[1, 2]), &out, None).expect_err("must refuse");
        assert!(
            why.message.contains("same page"),
            "the message says why rather than naming an internal id: {why}"
        );
        assert!(
            why.message.contains('1') && why.message.contains('2'),
            "and names the two pages the reader can see: {why}"
        );
        assert!(
            !out.exists(),
            "and nothing was written --- the refusal comes before any bytes"
        );
    }

    /// The over-refusal control, and the reason this is not a blanket refusal.
    ///
    /// A document nobody edited has a plan of zeros. Refusing it because its page
    /// tree is malformed would deny a save that has nothing to reconcile, which is
    /// the common case by a wide margin.
    #[test]
    fn a_page_reached_twice_is_saved_normally_when_nothing_conflicts() {
        let scratch = Scratch::new("shared-benign");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        write_copy(&source, &plan_of(&[0, 0]), &out, None)
            .expect("an unedited document still saves");
        assert!(out.exists());
    }

    /// A deleted page is gone from the copy --- and the check says *which* pages
    /// are left, not how many.
    ///
    /// `rotated.pdf` again, and for the same reason it is the fixture for the
    /// turn: its four pages carry 0/90/180/270 and are otherwise identical, so a
    /// save that dropped the *wrong* page produces a document with the right page
    /// count and the wrong contents. The rotations are the only thing that tells
    /// those two apart, which is why a page-count assertion on its own would be
    /// satisfied by either.
    ///
    /// Read back through the platform's own parser, never through the `lopdf`
    /// that wrote it.
    #[test]
    fn a_third_parser_sees_the_pages_that_were_kept_and_not_the_one_that_was_not() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let source = std::fs::read(&path).expect("read source");
        let Some(before) = os_pdf::read(&source) else {
            println!("[SKIP] the OS parser refused rotated.pdf");
            return;
        };
        assert_eq!(
            before.pages.len(),
            4,
            "the fixture this check is written for"
        );
        let rotations: Vec<i64> = before
            .pages
            .iter()
            .map(|page| page.rotation.rem_euclid(360))
            .collect();
        assert_eq!(
            rotations.iter().collect::<HashSet<_>>().len(),
            4,
            "the fixture discriminates: four pages, four different rotations. \
             Without that, dropping the wrong page is invisible here"
        );

        let scratch = Scratch::new("delete");
        let out = scratch.join("kept.pdf");
        // Page 2 removed; the other three keep their own rotations.
        write_copy(&path, &keeping(4, &[(0, 0), (2, 0), (3, 0)]), &out, None).expect("write");

        let written = std::fs::read(&out).expect("read written");
        let after = os_pdf::read(&written).expect("the OS parser reads the saved copy");
        assert_eq!(
            after
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            vec![rotations[0], rotations[2], rotations[3]],
            "pages 1, 3 and 4 in that order --- a count of three is equally true of \
             the three WRONG pages"
        );
        println!(
            "[OK] rotated.pdf 4 pages {rotations:?} --- kept 1,3,4 and read back \
             through the platform parser"
        );
    }

    /// Deleting and turning in one plan, since the two arrive together.
    ///
    /// The turn is aimed at a page *after* the deleted one, which is the case
    /// that fails if anything resolves a plan entry against the document's page
    /// numbers after the pages have gone: `get_pages` renumbers from 1, so the
    /// old page 4 becomes page 3 and a turn aimed at "page 4" lands on nothing.
    #[test]
    fn a_turn_on_a_page_after_the_deleted_one_lands_where_it_was_aimed() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("delete-turn");
        let out = scratch.join("out.pdf");

        let before = Document::load(&path).expect("load source");
        let source_ids = ordered_pages(&before);
        // Drop page 2, and turn what was page 4 by a quarter.
        write_copy(&path, &keeping(4, &[(0, 0), (2, 0), (3, 1)]), &out, None).expect("write");

        let after = Document::load(&out).expect("load written");
        let ids = ordered_pages(&after);
        assert_eq!(ids.len(), 3);
        assert_eq!(
            effective_rotation(&after, ids[2]).rem_euclid(360),
            (effective_rotation(&before, source_ids[3]) + 90).rem_euclid(360),
            "the last page is the old page 4, a quarter past where it was"
        );
        assert_eq!(
            effective_rotation(&after, ids[1]).rem_euclid(360),
            effective_rotation(&before, source_ids[2]).rem_euclid(360),
            "and the page before it, which nobody turned, is untouched"
        );
    }

    /// The outline goes when pages do, with the control that says it stays.
    ///
    /// Its destinations name pages that are no longer in the file. Dropping it
    /// whole is a real loss and is the only option that cannot leave a
    /// *malformed* one --- see `pagetree::drop_outline`.
    #[test]
    fn deleting_a_page_drops_the_outline_and_keeping_them_all_does_not() {
        let Some(path) = fixture("outline-simple.pdf") else {
            println!("[SKIP] outline-simple.pdf not generated");
            return;
        };
        let scratch = Scratch::new("outline");
        let count = page_count(&path);
        assert!(count > 1, "the fixture needs a page to spare");
        assert!(
            has_outline(&Document::load(&path).expect("load source")),
            "the fixture carries one to begin with"
        );

        let kept: Vec<(u32, u8)> = (1..count as u32).map(|source| (source, 0)).collect();
        let trimmed = scratch.join("trimmed.pdf");
        write_copy(&path, &keeping(count as u32, &kept), &trimmed, None).expect("write");
        assert!(
            !has_outline(&Document::load(&trimmed).expect("load written")),
            "a page was dropped, so its destinations are gone"
        );

        // The control. Without it this check passes for a save that drops every
        // outline it ever sees, which is a different and much worse rule.
        let whole = scratch.join("whole.pdf");
        write_copy(&path, &plan_of(&vec![0u8; count]), &whole, None).expect("write");
        assert!(
            has_outline(&Document::load(&whole).expect("load written")),
            "nothing was dropped, so the bookmarks survive"
        );
    }

    fn has_outline(doc: &Document) -> bool {
        doc.catalog()
            .expect("a catalog")
            .get(b"Outlines")
            .map(|entry| {
                // A dangling reference is not an outline. `drop_outline` removes
                // the key, but a reader of this helper should not have to know
                // that to trust the answer.
                entry
                    .as_reference()
                    .is_ok_and(|id| doc.get_object(id).is_ok())
            })
            .unwrap_or(false)
    }

    /// Every page's content stream object, by page number.
    ///
    /// Read from the *source* document, so the ids are the ones a leak would
    /// survive under: a rewrite renumbers nothing, so an object that comes
    /// through keeps the number it had.
    fn content_streams(doc: &Document) -> Vec<(u32, ObjectId)> {
        doc.get_pages()
            .iter()
            .filter_map(|(number, page)| {
                let stream = doc
                    .get_object(*page)
                    .and_then(Object::as_dict)
                    .and_then(|dict| dict.get(b"Contents"))
                    .ok()?
                    .as_reference()
                    .ok()?;
                Some((*number, stream))
            })
            .collect()
    }

    /// Extracting one page does not carry the other seven along inside the file.
    ///
    /// **The measurement this was written from.** Before the sweep in
    /// [`rewrite`], extracting page 1 of `links.pdf` produced a file reporting
    /// one page and holding all eight content streams --- 4,139 decodable bytes
    /// each, `(Line 01 of page 2: ...)` among them. A reader who extracts a page
    /// to send it on has stated an intent to exclude the rest, and the file said
    /// otherwise.
    ///
    /// Asserted on the *objects* rather than on a byte scan, because the streams
    /// are Flate-compressed: a `strings` over the output finds nothing and would
    /// certify a file that leaks everything. That is the byte-scan rule
    /// `docs/PLAN.md` §6 arrived at from the other direction.
    #[test]
    fn extracting_a_page_leaves_the_other_pages_out_of_the_file() {
        let Some(path) = fixture("links.pdf") else {
            println!("[SKIP] links.pdf not generated");
            return;
        };
        let scratch = Scratch::new("extract-sweep");
        let before = Document::load(&path).expect("load source");
        let streams = content_streams(&before);
        assert!(
            streams.len() >= 4,
            "the fixture needs several pages to leak: {} found",
            streams.len()
        );
        let count = streams.len() as u32;
        let (kept_number, kept_stream) = streams[0];

        let out = scratch.join("one.pdf");
        write_copy(&path, &keeping(count, &[(0, 0)]), &out, None).expect("write");
        let after = Document::load(&out).expect("load written");
        assert_eq!(after.get_pages().len(), 1, "one page was asked for");

        let carried: Vec<u32> = streams
            .iter()
            .filter(|(number, stream)| *number != kept_number && after.objects.contains_key(stream))
            .map(|(number, _)| *number)
            .collect();
        assert!(
            carried.is_empty(),
            "pages {carried:?} were not extracted and their content is still in the file"
        );

        // The control, and the sweep needs it more than most: a collection that
        // deleted the whole graph satisfies the assertion above perfectly.
        assert!(
            after.objects.contains_key(&kept_stream),
            "the page that WAS extracted still has its content"
        );
    }

    /// The same for a deletion, which is the operation risk 16 named.
    #[test]
    fn deleting_a_page_leaves_its_content_out_of_the_file() {
        let Some(path) = fixture("links.pdf") else {
            println!("[SKIP] links.pdf not generated");
            return;
        };
        let scratch = Scratch::new("delete-sweep");
        let before = Document::load(&path).expect("load source");
        let streams = content_streams(&before);
        assert!(streams.len() >= 3, "the fixture needs a page to spare");
        let count = streams.len() as u32;
        let gone = streams[1];

        let kept: Vec<(u32, u8)> = (0..count)
            .filter(|source| *source != 1)
            .map(|s| (s, 0))
            .collect();
        let out = scratch.join("rest.pdf");
        write_copy(&path, &keeping(count, &kept), &out, None).expect("write");
        let after = Document::load(&out).expect("load written");

        assert!(
            !after.objects.contains_key(&gone.1),
            "page {}'s content survived the deletion",
            gone.0
        );
        // Over-collection control, in the direction that matters here: every
        // page that stayed still has the stream it had.
        for (number, stream) in &streams {
            if *number == gone.0 {
                continue;
            }
            assert!(
                after.objects.contains_key(stream),
                "page {number} was kept and lost its content"
            );
        }
    }

    /// A copy that drops nothing is still a serialisation and not a sanitation.
    ///
    /// The scope control for the two checks above, and it pins a **position**
    /// rather than an implementation detail: `docs/THREAT-MODEL.md` §T6.1 says a
    /// saved copy carries forward whatever the original carried, so a document
    /// somebody else left orphans in comes back with them. Sweeping every save
    /// would be a different and larger promise --- see `docs/PLAN.md` §6.
    ///
    /// `hostile-orphan.pdf` is the fixture because its orphan is deliberate and
    /// recorded in `hostile-manifest.json`; an ordinary document has none, so
    /// the check would hold by construction and could not fail.
    #[test]
    fn a_copy_that_drops_nothing_keeps_the_orphans_it_was_given() {
        let Some(path) = fixture("hostile-orphan.pdf") else {
            println!("[SKIP] hostile-orphan.pdf not generated");
            return;
        };
        let scratch = Scratch::new("orphan-copy");
        let before = Document::load(&path).expect("load source");
        let reachable = crate::sweep::reachable(&before).expect("walk the source");
        let orphans: Vec<ObjectId> = before
            .objects
            .keys()
            .copied()
            .filter(|id| !reachable.contains(id))
            .collect();
        assert!(
            !orphans.is_empty(),
            "the fixture discriminates: it has to carry an orphan for this to mean anything"
        );

        let count = before.get_pages().len();
        let out = scratch.join("copy.pdf");
        write_copy(&path, &plan_of(&vec![0u8; count]), &out, None).expect("write");
        let after = Document::load(&out).expect("load written");
        for orphan in &orphans {
            assert!(
                after.objects.contains_key(orphan),
                "{orphan:?} was unreachable in the source and a plain copy dropped it"
            );
        }
    }

    /// Every fixture, rewritten through the real save path, is structurally sound.
    ///
    /// **The control for `verify::structure`, and the reason it is here rather
    /// than beside the function.** That check's hand-built fixture agrees with
    /// whatever its author had in mind. This population does not: forty-odd real
    /// documents nobody wrote for it, put through the writer a reader actually
    /// uses, which is the only population the check is ever pointed at.
    ///
    /// It is what killed the first draft of a `/Size` rule --- *the trailer's
    /// `/Size` must equal the cross-reference table's entry count* --- which
    /// reported MISMATCH on a healthy swept rewrite of `links.pdf` (91 entries
    /// in three subsections against `/Size 102`, because sweeping makes object
    /// numbers sparse and an unlisted number is free). `qpdf --check` passes that
    /// file. A validator that fires on correct input is worse than none.
    ///
    /// Large fixtures are skipped by size and the number examined is asserted,
    /// so a checkout missing its fixtures fails rather than certifying nothing.
    #[test]
    fn every_rewritten_fixture_is_structurally_sound() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("testdata");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            println!("[SKIP] no testdata directory");
            return;
        };
        // The 321 MB scan rewrites in tens of seconds and adds nothing here ---
        // this is about the shape of what the writer emits, not about size.
        const LARGEST: u64 = 8 * 1024 * 1024;
        let scratch = Scratch::new("structural");
        let mut examined = 0;
        let mut refused = 0;
        let mut paths: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("pdf"))
            .collect();
        paths.sort();
        for path in paths {
            if std::fs::metadata(&path)
                .map(|m| m.len())
                .unwrap_or(u64::MAX)
                > LARGEST
            {
                continue;
            }
            let Ok(doc) = Document::load(&path) else {
                continue;
            };
            let count = doc.get_pages().len();
            if count == 0 {
                continue;
            }
            let name = format!("{examined}.out.pdf");
            let out = scratch.join(&name);
            // Every page kept, and one dropped where there is a page to spare,
            // so the sweep runs on half of them. Both are writers a reader uses.
            let plan = if count > 1 {
                keeping(
                    count as u32,
                    &(0..count as u32 - 1).map(|s| (s, 0)).collect::<Vec<_>>(),
                )
            } else {
                plan_of(&vec![0u8; count])
            };
            if write_copy(&path, &plan, &out, None).is_err() {
                // Encrypted, signed, or a shape the writer refuses. Its refusal
                // is another test's subject; what matters here is that a file it
                // *did* write is sound.
                refused += 1;
                continue;
            }
            let bytes = std::fs::read(&out).expect("read what was written");
            assert_eq!(
                crate::verify::structure(&bytes),
                Vec::<String>::new(),
                "the rewrite of {} is malformed",
                path.display()
            );
            examined += 1;
        }
        println!("[INFO] {examined} rewrites checked, {refused} plans refused");
        assert!(
            examined >= 20,
            "only {examined} fixtures were rewritten, which is too few to have tested \
             anything --- run scripts/make_fixtures.py"
        );
    }

    /// `/Size` comes out as one plus the highest object number, whatever the
    /// graph's `max_id` said.
    ///
    /// **qpdf's rule, and the one defect spike 0.4 found.** Its own message is
    /// *reported number of objects (142) is not one plus the highest object
    /// number (101)*, and nothing else reads it: `lopdf`'s loader and PDFKit
    /// both accept such a file, PDFium renders it pixel-identically to a correct
    /// one. So this is the direction that can be tested here --- the *detection*
    /// belongs to `examples/qpdf_probe.rs`, which owns the only reader that
    /// performs it.
    ///
    /// The input is reachable, which is what separates this from the structural
    /// check beside it: a `Document` with an inflated `max_id` is one line, and
    /// it is exactly what a sweep leaves behind when nothing lowers it.
    #[test]
    fn a_serialised_document_reports_the_size_its_objects_justify() {
        let Some(path) = fixture("links.pdf") else {
            println!("[SKIP] links.pdf not generated");
            return;
        };
        let mut doc = Document::load(&path).expect("load");
        let highest = doc.objects.keys().map(|id| id.0).max().expect("objects");
        // Spike 0.4's defect: claim forty objects that are not there.
        doc.max_id = highest + 40;

        let bytes = serialise(&mut doc, "the document").expect("serialise");
        let back = Document::load_mem(&bytes).expect("reload what was written");
        let written = back.objects.keys().map(|id| id.0).max().expect("objects");
        let size = back
            .trailer
            .get(b"Size")
            .ok()
            .and_then(|entry| entry.as_i64().ok())
            .expect("a trailer with a /Size");
        assert_eq!(
            size,
            i64::from(written) + 1,
            "/Size {size} against a highest object number of {written}"
        );
    }

    /// And it is not lowered past what the file needs.
    ///
    /// The over-correction control, and it is not hypothetical: the repair
    /// *lowers* a number, so the failure it can introduce is a `/Size` that no
    /// longer covers every object written --- the same defect in the opposite
    /// direction, and just as invisible to every reader in this process.
    /// Asserted against the objects the output actually holds, not against the
    /// ones it was built from.
    #[test]
    fn no_object_is_written_at_or_past_the_size_that_was_declared() {
        let Some(path) = fixture("comments.pdf") else {
            println!("[SKIP] comments.pdf not generated");
            return;
        };
        let count = page_count(&path);
        assert!(count > 1, "the fixture needs a page to spare");
        let scratch = Scratch::new("size-floor");
        let out = scratch.join("out.pdf");
        let kept: Vec<(u32, u8)> = (0..count as u32 - 1).map(|source| (source, 0)).collect();
        write_copy(&path, &keeping(count as u32, &kept), &out, None).expect("write");

        let back = Document::load(&out).expect("reload");
        let size = back
            .trailer
            .get(b"Size")
            .ok()
            .and_then(|entry| entry.as_i64().ok())
            .expect("a trailer with a /Size");
        assert!(
            !back.objects.is_empty(),
            "the control needs objects to compare against"
        );
        for id in back.objects.keys() {
            assert!(
                i64::from(id.0) < size,
                "object {} is at or past the declared /Size of {size}",
                id.0
            );
        }
    }

    /// Deleting one of two page numbers that are one page is refused.
    ///
    /// Not a guard against a malformed document --- it is a refusal of a request
    /// no output satisfies. `drop_pages` removes page *objects* and correctly
    /// keeps any object a surviving number names, so the deletion would silently
    /// do nothing: the reader would be handed a copy with the page they removed
    /// still in it, which is the plausible-wrong-answer shape this file is built
    /// against. Found by writing the test that expected the deletion to work.
    #[test]
    fn deleting_one_of_two_numbers_that_are_one_page_is_refused() {
        let scratch = Scratch::new("shared-delete");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        let why = write_copy(&source, &keeping(2, &[(0, 0)]), &out, None).expect_err("must refuse");
        assert!(
            why.message.contains("same page") && why.message.contains("on its own"),
            "the message says what cannot be done and what can: {why}"
        );
        assert!(!out.exists(), "and nothing was written");
    }

    /// The over-refusal control for the check above.
    ///
    /// Removing *both* numbers of a shared page is expressible --- the object goes
    /// --- and a save that refused every shared page outright would pass the test
    /// above while denying this one.
    #[test]
    fn deleting_both_numbers_of_a_shared_page_is_not_refused() {
        let scratch = Scratch::new("shared-delete-both");
        let source = scratch.join("shared.pdf");
        std::fs::write(&source, shared_page_and_a_spare()).expect("write fixture");
        let out = scratch.join("out.pdf");

        // Pages 1 and 2 are one object; page 3 is its own. Keep only page 3.
        write_copy(&source, &keeping(3, &[(2, 0)]), &out, None).expect("write");
        let after = Document::load(&out).expect("load written");
        assert_eq!(
            ordered_pages(&after).len(),
            1,
            "both numbers went, so the object they shared went with them"
        );
    }

    /// The shared-page fixture with one ordinary page after it.
    ///
    /// `shared_page_document` cannot express "delete both", because a document
    /// must keep at least one page and both of its numbers are the same object.
    fn shared_page_and_a_spare() -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let shared = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
        });
        let spare = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![
                    Object::Reference(shared),
                    Object::Reference(shared),
                    Object::Reference(spare),
                ],
                "Count" => 3,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    /// A moved page comes out where the reader put it, read by a third parser.
    ///
    /// `rotated.pdf` for the third time, and for the third reason: its four pages
    /// carry 0/90/180/270 and are otherwise identical, so the rotations are a
    /// *name* for each page. A save that wrote them in file order produces the
    /// same four pages and the same page count, and nothing but the sequence of
    /// rotations can tell the two apart.
    #[test]
    fn a_plan_whose_pages_have_moved_comes_out_in_the_order_the_reader_put_them() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let source = std::fs::read(&path).expect("read source");
        let Some(before) = os_pdf::read(&source) else {
            println!("[SKIP] the OS parser refused rotated.pdf");
            return;
        };
        let at: Vec<i64> = before
            .pages
            .iter()
            .map(|page| page.rotation.rem_euclid(360))
            .collect();
        assert_eq!(
            at.iter().collect::<HashSet<_>>().len(),
            4,
            "the fixture discriminates: four pages, four different rotations"
        );

        let scratch = Scratch::new("reordered");
        let out = scratch.join("out.pdf");
        write_copy(
            &path,
            &keeping(4, &[(2, 0), (0, 0), (3, 0), (1, 0)]),
            &out,
            None,
        )
        .expect("write");

        let written = std::fs::read(&out).expect("read written");
        let after = os_pdf::read(&written).expect("the OS parser reads the saved copy");
        assert_eq!(
            after
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            vec![at[2], at[0], at[3], at[1]],
            "the pages are in the reader's order, not the file's"
        );

        // The control, and it is not ceremony: a save that reordered *every*
        // plan would pass the assertion above and would flatten the page tree of
        // every document anyone ever saved.
        let untouched = scratch.join("untouched.pdf");
        write_copy(
            &path,
            &keeping(4, &[(0, 0), (1, 0), (2, 0), (3, 0)]),
            &untouched,
            None,
        )
        .expect("in order");
        let read_back = std::fs::read(&untouched).expect("read");
        assert_eq!(
            os_pdf::read(&read_back)
                .expect("read back")
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            at,
            "a plan in document order is the document"
        );
    }

    /// Moving, deleting and turning in one plan, since a reader does all three.
    ///
    /// The turn is on a page that both moved *and* sits after the deleted one,
    /// which is the entry that goes wrong if anything resolves the plan against
    /// the document's page numbers after the tree has been rewritten under it.
    #[test]
    fn a_page_that_moved_carries_its_turn_to_where_it_landed() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("move-delete-turn");
        let out = scratch.join("out.pdf");

        let before = Document::load(&path).expect("load source");
        let source_ids = ordered_pages(&before);
        // Page 2 dropped; the old page 4 moved to the front and turned a quarter.
        write_copy(&path, &keeping(4, &[(3, 1), (0, 0), (2, 0)]), &out, None).expect("write");

        let after = Document::load(&out).expect("load written");
        let ids = ordered_pages(&after);
        assert_eq!(ids, vec![source_ids[3], source_ids[0], source_ids[2]]);
        assert_eq!(
            effective_rotation(&after, ids[0]).rem_euclid(360),
            (effective_rotation(&before, source_ids[3]) + 90).rem_euclid(360),
            "the page that moved to the front is a quarter past where it was"
        );
        assert_eq!(
            effective_rotation(&after, ids[2]).rem_euclid(360),
            effective_rotation(&before, source_ids[2]).rem_euclid(360),
            "and a page that only moved is at the angle it always was"
        );
    }

    /// A moved page keeps a rotation it *inherited*, read by a third parser.
    ///
    /// The mechanism is `pagetree::reorder_pages`, which has its own checks on a
    /// document in memory. This is the end of the same wire: a real file, the
    /// save path, and PDFKit rather than the `lopdf` that wrote it. `/Rotate` is
    /// the inheritable attribute the OS parser reports, which is what makes the
    /// property observable here at all --- the size would need a field neither
    /// platform reading has.
    ///
    /// Without the push-down, the page hanging under the node that states
    /// `/Rotate 90` is reparented to a root that states nothing and comes back
    /// upright, in a file that opens and looks plausible.
    #[test]
    fn a_third_parser_sees_an_inherited_rotation_survive_a_move() {
        let scratch = Scratch::new("inherit-move");
        let source = scratch.join("nested.pdf");
        std::fs::write(&source, nested_document()).expect("write fixture");

        let original = std::fs::read(&source).expect("read");
        let Some(before) = os_pdf::read(&original) else {
            println!("[SKIP] the OS parser refused the hand-built nested document");
            return;
        };
        assert_eq!(
            before
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            vec![0, 0, 90, 90],
            "the precondition: two pages inherit 90 from a node the root knows \
             nothing about, and two inherit nothing"
        );

        let out = scratch.join("moved.pdf");
        // The last page to the front, so it leaves the node it inherited from.
        write_copy(
            &source,
            &keeping(4, &[(3, 0), (0, 0), (1, 0), (2, 0)]),
            &out,
            None,
        )
        .expect("write");

        let written = std::fs::read(&out).expect("read written");
        let after = os_pdf::read(&written).expect("the OS parser reads the saved copy");
        assert_eq!(
            after
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect::<Vec<_>>(),
            vec![90, 0, 0, 90],
            "the moved page took its inherited rotation with it"
        );
    }

    /// A plan the reader did not rearrange leaves the page tree where it is.
    ///
    /// The control for the two checks above, and the one that says why `moved`
    /// is computed at all. Rebuilding the tree produces the same *document* for
    /// a plan in document order --- same pages, same order, same rotations ---
    /// so nothing about the reader's view can tell the two apart. What differs
    /// is every page's ancestry, and a copy that reparented all 775 pages of a
    /// document nobody rearranged is a rewrite nobody asked for.
    #[test]
    fn a_plan_in_document_order_leaves_the_page_tree_as_it_found_it() {
        let scratch = Scratch::new("tree-untouched");
        let source = scratch.join("nested.pdf");
        std::fs::write(&source, nested_document()).expect("write fixture");
        assert_eq!(
            first_kid_type(&Document::load(&source).expect("load")),
            "Pages",
            "the precondition: the root's first child is a tree node, not a page"
        );

        let out = scratch.join("copied.pdf");
        write_copy(
            &source,
            &keeping(4, &[(0, 0), (1, 0), (2, 0), (3, 0)]),
            &out,
            None,
        )
        .expect("write");
        assert_eq!(
            first_kid_type(&Document::load(&out).expect("load written")),
            "Pages",
            "the tree is the one the file had"
        );

        // The control for the control: the same document rearranged does come
        // out flat, so the assertion above is about this plan rather than about
        // a reorder that never happens.
        let moved = scratch.join("moved.pdf");
        write_copy(
            &source,
            &keeping(4, &[(3, 0), (0, 0), (1, 0), (2, 0)]),
            &moved,
            None,
        )
        .expect("write");
        assert_eq!(
            first_kid_type(&Document::load(&moved).expect("load written")),
            "Page"
        );
    }

    /// The `/Type` of the first thing the catalog's page tree points at.
    fn first_kid_type(doc: &Document) -> String {
        let root = doc
            .catalog()
            .expect("a catalog")
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("a page tree");
        let first = doc
            .get_object(root)
            .and_then(Object::as_dict)
            .expect("the root")
            .get(b"Kids")
            .and_then(Object::as_array)
            .expect("kids")
            .first()
            .and_then(|entry| entry.as_reference().ok())
            .expect("a first kid");
        String::from_utf8_lossy(
            doc.get_object(first)
                .and_then(Object::as_dict)
                .expect("a kid")
                .get(b"Type")
                .and_then(Object::as_name)
                .expect("a type"),
        )
        .into_owned()
    }

    /// Four pages under two `/Pages` nodes, one of which states `/Rotate 90`.
    ///
    /// Hand-built because the corpus has nothing like it: `text-heavy.pdf` is the
    /// only nested fixture, three levels deep, and every inheritable attribute it
    /// has sits on the *root* --- so flattening it onto the root preserves
    /// everything and it cannot tell a reorder that carries inherited attributes
    /// from one that drops them.
    fn nested_document() -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let root_id = doc.new_object_id();
        let left_id = doc.new_object_id();
        let right_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));

        let page = |parent: lopdf::ObjectId, doc: &mut Document| {
            doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => parent,
                "Contents" => content,
            })
        };
        let a = page(left_id, &mut doc);
        let b = page(left_id, &mut doc);
        let c = page(right_id, &mut doc);
        let d = page(right_id, &mut doc);

        doc.objects.insert(
            left_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => root_id,
                "Kids" => vec![a.into(), b.into()],
                "Count" => 2,
            }),
        );
        doc.objects.insert(
            right_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => root_id,
                "Kids" => vec![c.into(), d.into()],
                "Count" => 2,
                "Rotate" => 90,
            }),
        );
        doc.objects.insert(
            root_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![left_id.into(), right_id.into()],
                "Count" => 4,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => root_id,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    /// A reorder keeps the bookmarks, where a deletion drops them.
    ///
    /// The two are one operation apart and land in opposite places, which is the
    /// reason this check exists beside the deletion one rather than instead of
    /// it: an outline destination names a page *object*, and a move leaves every
    /// object exactly where it was in the file. Dropping the outline for a move
    /// as well would be a loss nothing requires.
    #[test]
    fn a_reorder_keeps_the_outline_that_a_deletion_would_have_dropped() {
        let Some(path) = fixture("outline-simple.pdf") else {
            println!("[SKIP] outline-simple.pdf not generated");
            return;
        };
        let scratch = Scratch::new("outline-move");
        let count = page_count(&path);
        assert!(count > 2, "the fixture needs two pages to swap");

        // Every page kept, the first two swapped.
        let mut kept: Vec<(u32, u8)> = (0..count as u32).map(|source| (source, 0)).collect();
        kept.swap(0, 1);
        let out = scratch.join("swapped.pdf");
        write_copy(&path, &keeping(count as u32, &kept), &out, None).expect("write");

        assert!(
            has_outline(&Document::load(&out).expect("load written")),
            "nothing was deleted, so the bookmarks are still there --- they point \
             at page objects, and a move does not remove one"
        );
    }

    #[test]
    fn a_plan_naming_a_page_the_file_does_not_have_is_refused() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("past-end");
        let out = scratch.join("out.pdf");

        // A baseline that agrees with the file, and a page past its end. Only
        // reachable from a frontend sending something the model never produced,
        // which is exactly the argument for not trusting the number.
        let why =
            write_copy(&path, &keeping(4, &[(0, 0), (9, 0)]), &out, None).expect_err("must refuse");
        assert!(why.message.contains("does not have"), "{why}");
        assert!(!out.exists());
    }

    #[test]
    fn an_encrypted_document_is_refused_rather_than_quietly_decrypted() {
        let scratch = Scratch::new("encrypted");
        let source = scratch.join("locked.pdf");
        std::fs::write(&source, encrypted_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        let why = write_copy(&source, &plan_of(&[0]), &out, None).expect_err("must refuse");
        assert!(
            why.message.contains("encrypted"),
            "the message names the reason: {why}"
        );
        assert!(
            !out.exists(),
            "a refusal writes nothing, not even a temporary"
        );
        assert!(partials_beside(&out).is_empty(), "not even a temporary");
    }

    /// A *genuinely* encrypted document keeps its encryption, and it is the case
    /// the synthetic fixture cannot reach.
    ///
    /// **This asserted a refusal until 2026-08-28, and the refusal was a proxy.**
    /// What it was defending is in the paragraphs below: an encrypted document
    /// must never be written back in the clear. Refusing was how that was
    /// achieved while `lopdf`'s full serialiser was the only writer available;
    /// since `rewrite` re-encrypts with the state the load recorded, the
    /// property can be asserted directly instead. A test that pins the proxy
    /// rather than the property is what `docs/TRAPS.md` records as *a refusal
    /// that names a fallback has to keep the fallback open* --- and here it
    /// would have argued against the increment that closed it.
    ///
    /// The two fixtures now check different things, which is the whole reason
    /// there are two: one is unlocked by the empty password `lopdf` tries
    /// unprompted, so it is rewritten and must come back encrypted; the other
    /// is behind a real password nobody supplied, so it is still refused.
    ///
    /// **The fixture below claimed this test was redundant and it was wrong.**
    /// Its doc comment said "a genuinely encrypted fixture would test the same
    /// branch", and the branch is chosen by a predicate that is *false* for a
    /// real one: `lopdf` removes `/Encrypt` from the trailer the moment it
    /// authenticates, and it tries the empty password first. So every document
    /// with an empty user password --- which opens unprompted in every reader
    /// and is what a permission-restricted file is --- arrived here with the
    /// trailer entry already gone, sailed past the guard, and was reserialised
    /// with its encryption silently dropped. Exactly the failure the guard was
    /// written to prevent, in the fixture's own words.
    ///
    /// The synthetic fixture keeps `/Encrypt` only *because* the encryption is
    /// fake: authentication fails on it, so `lopdf` leaves the trailer alone.
    /// Two fixtures where the right rule and the wrong rule agree is one
    /// fixture; `docs/TRAPS.md` has that under its own title.
    #[test]
    fn a_really_encrypted_document_keeps_its_encryption_or_names_its_lock() {
        let scratch = Scratch::new("really-encrypted");
        let out = scratch.join("out.pdf");
        let mut examined = 0;

        // Opens on the empty password `lopdf` tries unprompted, so tpdf holds
        // the key without being given one: it is rewritten, and the check is
        // that the encryption came back. This is the exact document that was
        // being silently written in the clear before the guard was corrected.
        if let Some(path) = fixture("incr-encrypted-open.pdf") {
            examined += 1;
            write_copy(&path, &plan_of(&[0, 0]), &out, None)
                .expect("a document tpdf can unlock is rewritten");
            let raw = std::fs::read(&out).expect("read back");
            assert!(
                raw.windows(8).any(|w| w == b"/Encrypt"),
                "incr-encrypted-open.pdf came back with no /Encrypt dictionary, so its \
                 encryption was silently dropped"
            );
            assert!(
                partials_beside(&out).is_empty(),
                "incr-encrypted-open.pdf: no temporary is left behind"
            );
            std::fs::remove_file(&out).expect("clean up");
        } else {
            println!("[SKIP] incr-encrypted-open.pdf: fixture not generated");
        }

        // Behind a real password, and none was supplied. Still refused, and the
        // message has to name the lock rather than something the reader cannot
        // act on -- they can supply the password, and that is now the way
        // through rather than a dead end.
        if let Some(path) = fixture("incr-encrypted-pw.pdf") {
            examined += 1;
            let why = write_copy(&path, &plan_of(&[0, 0]), &out, None)
                .expect_err("a locked document must be refused");
            assert!(
                why.message.contains("encrypted"),
                "incr-encrypted-pw.pdf: the message names the reason: {why}"
            );
            assert!(
                !out.exists(),
                "incr-encrypted-pw.pdf: a refusal writes nothing"
            );
            assert!(
                partials_beside(&out).is_empty(),
                "incr-encrypted-pw.pdf: not even a temporary"
            );
        } else {
            println!("[SKIP] incr-encrypted-pw.pdf: fixture not generated");
        }
        assert!(
            examined > 0,
            "both encrypted fixtures are absent, so this checked nothing"
        );
    }

    /// A one-page document carrying an `/Encrypt` entry in its trailer.
    ///
    /// The encryption is not real --- nothing here encrypts any stream --- and it
    /// does not need to be: the guard is about the *presence* of the dictionary,
    /// which is what `lopdf` drops. A genuinely encrypted fixture would test the
    /// same branch and would additionally not load.
    fn encrypted_document() -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        let encrypt = doc.add_object(dictionary! {
            "Filter" => "Standard",
            "V" => 2,
            "R" => 3,
            "Length" => 128,
            "P" => -44i64,
        });
        doc.trailer.set("Root", catalog);
        doc.trailer.set("Encrypt", encrypt);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    #[test]
    fn a_plan_that_does_not_match_the_file_on_disk_is_refused() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("count");
        let out = scratch.join("out.pdf");
        let count = page_count(&path);

        let why = write_copy(&path, &plan_of(&vec![0u8; count + 1]), &out, None)
            .expect_err("must refuse");
        assert!(why.message.contains("changed since it was opened"), "{why}");
        assert!(!out.exists());

        // And the matching plan is accepted, so the refusal is about the
        // mismatch rather than about this document.
        write_copy(&path, &plan_of(&vec![0u8; count]), &out, None)
            .expect("the matching plan writes");
        assert!(out.exists());
    }

    #[test]
    fn an_empty_plan_is_refused() {
        let scratch = Scratch::new("empty");
        let out = scratch.join("out.pdf");
        let why = write_copy(
            Path::new("../testdata/rotated.pdf"),
            &plan_of(&[]),
            &out,
            None,
        )
        .expect_err("must refuse");
        assert!(why.message.contains("at least one page"), "{why}");
    }

    /// A **copy** is never the source, and that is what this refuses.
    ///
    /// Saving in place is a real operation now --- [`stage_in_place`] and
    /// [`commit_in_place`] below --- so what makes this refusal right is no
    /// longer "tpdf does not do that". It is that `write_copy` writes and
    /// renames in one step, with no room between them for the close that an
    /// in-place save needs, so letting the source through here would replace a
    /// mapped file and leave the reader's worker serving the document that was.
    #[test]
    fn saving_over_the_open_document_is_refused() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("inplace");
        let copy = scratch.join("copy.pdf");
        std::fs::copy(&path, &copy).expect("copy fixture");
        let before = std::fs::read(&copy).expect("read");

        let why = write_copy(&copy, &plan_of(&[1, 0, 0, 0]), &copy, None).expect_err("must refuse");
        assert!(why.message.contains("save over"), "{why}");
        assert_eq!(
            std::fs::read(&copy).expect("read"),
            before,
            "the document is untouched"
        );

        // The same file reached by a different spelling of the path is still the
        // same file --- a comparison of the strings would let this through.
        let indirect = scratch.join(".").join("copy.pdf");
        assert!(write_copy(&copy, &plan_of(&[1, 0, 0, 0]), &indirect, None).is_err());
    }

    /// Staging writes the whole document and changes nothing the reader has.
    ///
    /// This is the property the `reopen: false` half of `lib.rs`'s
    /// `SaveFailure` rests on: everything expensive and everything refusable
    /// happens while the source is still the source, so a save that fails here
    /// has cost the reader nothing.
    ///
    /// It is also the **control** for the test below it. "The file holds the
    /// edits after a commit" is satisfied by an implementation that wrote them
    /// during the staging, and the two tests together are what separate the
    /// steps: here the source must be untouched, there it must not be.
    #[test]
    fn staging_a_save_in_place_writes_beside_the_source_and_leaves_it_alone() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("stage");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");
        let before = std::fs::read(&open).expect("read");

        let staged = stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open), None, &Here)
            .expect("stage");

        assert!(staged.path.exists(), "the staged file is written");
        assert_ne!(staged.path, open, "and it is not the source");
        assert_eq!(
            std::fs::read(&open).expect("read"),
            before,
            "the document the reader has is untouched until the commit"
        );
    }

    /// A rewrite through a symlink must edit the document, not replace the link.
    ///
    /// **The two save modes disagreed about one file, and neither said so.**
    /// `std::fs::rename` onto a symlink replaces the *link*: the entry becomes an
    /// ordinary file holding the new bytes and the document it pointed at keeps
    /// the old ones. So a page turn left the reader with two files diverging,
    /// while a highlight --- which goes through the append, and the append opens
    /// the path rather than renaming over it --- followed the link and edited the
    /// document. Same file, same gesture, opposite results.
    ///
    /// Both assertions are needed and neither implies the other: a fix that
    /// resolved the link but staged in the wrong directory would keep the link a
    /// link and still not change the target.
    #[test]
    #[cfg(unix)]
    fn saving_in_place_through_a_symlink_edits_the_document_the_link_names() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        // **The link and the document live in different directories, and that is
        // the whole fixture.** With both in one directory, staging beside the
        // link and staging beside the document are the same place, so the two
        // rules agree and neither can be tested --- a mutation removing the
        // resolution from `stage` survived exactly that way. Apart, the staged
        // file's directory is the mechanism, readable directly.
        let scratch = Scratch::new("stage-symlink");
        let elsewhere = scratch.join("documents");
        std::fs::create_dir_all(&elsewhere).expect("target directory");
        let real = elsewhere.join("real.pdf");
        let link = scratch.join("link.pdf");
        std::fs::copy(&path, &real).expect("copy fixture");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let before = std::fs::read(&real).expect("read");

        let staged = stage_in_place(&link, &plan_opened_as(&[1, 0, 0, 0], &link), None, &Here)
            .expect("stage");
        // Beside the document, not beside the name it was reached by. On this
        // machine both are the same filesystem so the rename would work either
        // way; the property is that it *always* does, and a temporary file on a
        // different filesystem from its destination cannot be renamed onto it at
        // all. That is not reachable from a unit test, so the directory is what
        // is asserted --- the mechanism rather than its consequence.
        // Both sides canonicalized, because on macOS `/var` is itself a symlink
        // to `/private/var` and the scratch directory lives under it --- so the
        // staged path is resolved and the expectation, built from `temp_dir()`,
        // is not. Comparing them raw fails on a correct implementation, which is
        // the direction that wastes an afternoon.
        assert_eq!(
            staged
                .path
                .parent()
                .and_then(|dir| dir.canonicalize().ok())
                .as_deref(),
            elsewhere.canonicalize().ok().as_deref(),
            "the staged file must land beside the document it will replace"
        );
        commit_in_place(&staged.path, &link).expect("commit");

        assert!(
            std::fs::symlink_metadata(&link)
                .expect("stat the link")
                .file_type()
                .is_symlink(),
            "the save replaced the link instead of the document it names"
        );
        assert_ne!(
            std::fs::read(&real).expect("read the target"),
            before,
            "the document the link names is the one that must have changed"
        );
    }

    /// A rewrite must not widen who can read the document.
    ///
    /// A staged file is created with the process umask --- usually `0644` --- and
    /// then renamed over the original, so a document kept at `0600` in a shared
    /// directory came back readable by everyone after any page edit. Nothing
    /// reported it and no other check can see it: the bytes are correct and the
    /// page count is right.
    #[test]
    #[cfg(unix)]
    fn a_rewrite_keeps_the_documents_mode() {
        use std::os::unix::fs::PermissionsExt as _;

        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("stage-mode");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o640))
            .expect("set the document's mode");

        // **The control, and without it this test can hold by construction.**
        // The assertion below is only worth something if `0640` is not what a
        // freshly created file gets anyway --- under a `0026` umask it would be,
        // and then nothing could fail. Ask the filesystem rather than assume the
        // umask.
        let probe = scratch.join("umask-probe");
        std::fs::write(&probe, b"x").expect("probe");
        let ambient = std::fs::metadata(&probe)
            .expect("stat probe")
            .permissions()
            .mode()
            & 0o777;
        if ambient == 0o640 {
            println!("[SKIP] a_rewrite_keeps_the_documents_mode: this umask creates 0640 anyway");
            return;
        }

        let staged = stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open), None, &Here)
            .expect("stage");
        commit_in_place(&staged.path, &open).expect("commit");

        assert_eq!(
            std::fs::metadata(&open).expect("stat").permissions().mode() & 0o777,
            0o640,
            "the save replaced the document with one anyone can read (ambient mode is {ambient:o})"
        );
    }

    /// A read-back that never answers must end its worker, not wait for ever.
    ///
    /// `InWorker::pages` spawns its worker outside the pool, so the supervisor
    /// that owns the deadline never sees it and `Worker::call` blocks on a pipe
    /// with no bound. The document is already closed by then and the appended
    /// bytes are already on disk, so "for ever" means a save that can never be
    /// confirmed or rolled back.
    ///
    /// Exercised through `awaited` with a real process standing in for the
    /// worker, because the decision takes its duration as an argument --- a check
    /// whose only failure mode is a wait cannot fail.
    #[test]
    #[cfg(unix)]
    fn a_read_back_that_never_answers_ends_the_worker() {
        let mut victim = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn a stand-in worker");
        let pid = victim.id();
        // Nobody ever sends. `_tx` is held so the channel is not simply closed,
        // which would be a different outcome from silence.
        let (_tx, rx) = std::sync::mpsc::channel::<usize>();

        let began = std::time::Instant::now();
        let within = std::time::Duration::from_millis(150);
        let why = awaited(&rx, within, pid)
            .expect_err("a wait that gets no answer must not report success");
        let waited = began.elapsed();
        assert!(
            waited >= within,
            "it has to have waited for the deadline it was given, and waited {waited:?}"
        );
        // **The upper bound is the half that has teeth.** A lower bound alone is
        // satisfied by *any* longer wait, so a deadline a thousand times too long
        // passes it --- measured: the same assertion stayed green while the test
        // took 150 seconds instead of 0.17. A bound whose failure mode is a
        // longer wait is not a bound. Twenty times the deadline is loose enough
        // for a loaded runner and nowhere near a mistake worth catching.
        assert!(
            waited < within * 20,
            "the wait has to be about the deadline it was given, and took {waited:?}"
        );
        assert!(
            why.contains("did not answer"),
            "the refusal has to say what happened: {why}"
        );

        let mut gone = false;
        for _ in 0..200 {
            if victim.try_wait().expect("wait").is_some() {
                gone = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = victim.kill();
        let _ = victim.wait();
        assert!(
            gone,
            "the worker must be ended --- otherwise the timeout leaks the process and the \
             thread blocked reading its pipe"
        );
    }

    /// The control: an answer that arrives leaves the worker alone.
    ///
    /// Without it, an `awaited` that killed unconditionally would pass the test
    /// above, and every ordinary save would be ending a healthy worker.
    #[test]
    #[cfg(unix)]
    fn a_read_back_that_answers_in_time_leaves_the_worker_alone() {
        let mut victim = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn a stand-in worker");
        let pid = victim.id();
        let (tx, rx) = std::sync::mpsc::channel::<usize>();
        tx.send(7).expect("send the answer");

        assert_eq!(
            awaited(&rx, std::time::Duration::from_secs(5), pid).expect("the answer arrives"),
            7
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
        let still_there = victim.try_wait().expect("wait").is_none();
        let _ = victim.kill();
        let _ = victim.wait();
        assert!(
            still_there,
            "a call that was answered must not have its worker killed"
        );
    }

    /// A file that changed under the open document is not saved over.
    ///
    /// The general form of the page-count refusal, and this test is built so that
    /// the page-count guard **cannot** be what fires: the modification appends
    /// bytes after `%%EOF`, which every parser here ignores, so the document still
    /// has exactly the pages the plan names. Before the fingerprint, this staged
    /// happily and the reader's edits were written onto a graph parsed from
    /// somebody else's bytes.
    #[test]
    fn a_save_in_place_is_refused_when_the_file_changed_under_it() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("changed");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        // Taken while the file is what the reader opened, exactly as `open_document`
        // takes it.
        let plan = plan_opened_as(&[1, 0, 0, 0], &open);

        // Somebody else writes to it. Appended rather than rewritten so the page
        // count is untouched -- if this test passed because the count changed it
        // would be testing the guard that already existed.
        let mut bytes = std::fs::read(&open).expect("read");
        assert_eq!(
            page_count(&open),
            4,
            "the fixture has the pages the plan names"
        );
        bytes.extend_from_slice(
            b"
% written by something else
",
        );
        std::fs::write(&open, &bytes).expect("rewrite");
        assert_eq!(
            page_count(&open),
            4,
            "and still has them afterwards, so the page-count guard cannot fire"
        );

        let why = stage_in_place(&open, &plan, None, &Here).expect_err("must refuse");
        assert!(why.message.contains("changed on disk"), "{why}");
        // The message has to leave the reader somewhere to go: their edits are
        // still in the journal, and Save a copy is the way to keep them.
        assert!(why.message.contains("another name"), "{why}");
        assert!(
            partials_beside(&open).is_empty(),
            "and nothing is staged beside the document"
        );
    }

    /// The control for the test above, and it is not the same fixture untouched.
    ///
    /// A guard that refused every save would pass that test and protect nothing.
    /// What this asserts is that the *same* plan, against a file nobody wrote to,
    /// stages -- so the refusal is about the change rather than about the check
    /// existing.
    #[test]
    fn an_unchanged_file_still_stages() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("unchanged");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let plan = plan_opened_as(&[1, 0, 0, 0], &open);
        let staged = stage_in_place(&open, &plan, None, &Here).expect("must stage");
        assert!(staged.path.exists());
    }

    /// No fingerprint means no save in place, and the message names the way out.
    ///
    /// Fail closed. "Could not look" and "looked, and it was fine" are different
    /// facts, and a save path that treats them alike writes over a file it has no
    /// evidence about. The way out has to keep working, which the next test
    /// asserts -- a refusal pointing at a fallback that is also refused is a
    /// dead end wearing a helpful sentence.
    #[test]
    fn a_save_in_place_with_no_fingerprint_is_refused_and_points_at_save_a_copy() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("noprint");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let why =
            stage_in_place(&open, &plan_of(&[1, 0, 0, 0]), None, &Here).expect_err("must refuse");
        assert!(why.message.contains("could not record"), "{why}");
        assert!(why.message.contains("Save a copy"), "{why}");
        // The message is one a reader reads, so it has to be one sentence rather
        // than one that happens to contain the right words. This assertion is
        // here because it was not: a lost `\` continuation left a run of 21
        // spaces mid-sentence, and both assertions above passed over it --- they
        // check the ends and the defect was in the middle.
        assert!(
            !why.message.contains("  "),
            "run of spaces in a reader-facing message: {why}"
        );
    }

    /// The last look before the rename refuses a file that moved under the save.
    ///
    /// This guard lived inline in `save_document` until 2026-08-19, where no test
    /// could reach it --- which is the shape `docs/TRAPS.md` records twice and
    /// which `lib.rs`'s own comment cited about the guard three lines above it.
    #[test]
    fn the_last_look_before_the_rename_refuses_a_source_that_moved() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("last-look");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let staged = stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open), None, &Here)
            .expect("stage");
        assert!(staged.path.exists(), "there is something to lose");

        // Something else writes while the document is being closed. Longer, so
        // the length is what answers -- the timestamp would too, and a test that
        // cannot say which mechanism refused is one this module has already been
        // caught writing.
        let mut bytes = std::fs::read(&open).expect("read");
        bytes.extend_from_slice(
            b"
% written by something else
",
        );
        std::fs::write(&open, &bytes).expect("rewrite");

        let why = verify_before_commit(&staged, &open).expect_err("must refuse");
        assert!(why.message.contains("length"), "{why}");
        assert!(why.message.contains("nothing was written"), "{why}");
        // And it must not carry the advice that belongs to the check before it.
        // The document is closed by the time a reader reads this, so telling
        // them their edits are still here and to save them under another name is
        // an instruction they cannot follow --- and it used to arrive in the same
        // sentence as "the document has been closed".
        assert!(!why.message.contains("still here"), "{why}");
        assert!(!why.message.contains("another name"), "{why}");
        assert!(why.message.contains("has been closed"), "{why}");
        // And **no instruction at all**. `save_document`'s caller reopens the
        // file itself on every `after_close`, so "open the file again" is advice
        // addressed to somebody it has already been carried out for --- which is
        // the two-moments failure this module was caught on, one layer up.
        assert!(!why.message.contains("open the file"), "{why}");
        // The flag, not the wording, is what the window reads.
        assert!(why.changed, "{why}");
        // The staged file goes with the refusal. Nothing else is tracking it by
        // this point, so leaving it puts a file the reader never named beside
        // their document.
        assert!(!staged.path.exists(), "and the staged file is cleaned up");
        // And the reader's file is exactly what the other writer left.
        assert_eq!(std::fs::read(&open).expect("read"), bytes);
    }

    /// The control, and it is the half that says the guard is not simply strict.
    #[test]
    fn the_last_look_lets_an_untouched_source_through() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("last-look-ok");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let staged = stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open), None, &Here)
            .expect("stage");
        assert_eq!(verify_before_commit(&staged, &open), Ok(()));
        assert!(
            staged.path.exists(),
            "and leaves the staged file to be committed"
        );
    }

    /// A copy is written even with no fingerprint, because it risks nothing.
    ///
    /// The asymmetry is deliberate and is the whole reason `stage_in_place` has a
    /// refusal `planned_bytes` does not: a copy that turns out to be built from
    /// changed bytes is a bad new file beside an intact original, and a save in
    /// place is the original. This is also what keeps the refusal above honest.
    #[test]
    fn a_copy_is_written_even_when_no_fingerprint_was_taken() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("copy-noprint");
        let open = scratch.join("open.pdf");
        let out = scratch.join("out.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        write_copy(&open, &plan_of(&[1, 0, 0, 0]), &out, None)
            .expect("a copy needs no fingerprint");
        assert!(out.exists());
    }

    /// A copy IS written when the source changed, and says that it was.
    ///
    /// **This test asserted the opposite until 2026-08-19, and the assertion was
    /// the defect.** Refusing here closed the only door the in-place refusal
    /// points at: a reader whose file changed was told to save their edits under
    /// another name, and Save a copy was refused by the same guard one function
    /// down. `stage_in_place`'s own comment states the rule that was being broken
    /// --- "the fallback the message names has to keep working, or the refusal
    /// strands the reader" --- and it had been applied to a missing fingerprint
    /// and not to a changed file.
    ///
    /// The copy is not claimed to be correct, and `changed` is how it says so.
    /// What still refuses is a file whose *shape* changed, which the page-count
    /// guard catches whichever path asks.
    #[test]
    fn a_copy_is_written_when_the_source_changed_and_reports_it() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("copy-changed");
        let open = scratch.join("open.pdf");
        let out = scratch.join("out.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let plan = plan_opened_as(&[1, 0, 0, 0], &open);
        let mut bytes = std::fs::read(&open).expect("read");
        bytes.extend_from_slice(
            b"
% written by something else
",
        );
        std::fs::write(&open, &bytes).expect("rewrite");

        let copied = write_copy(&open, &plan, &out, None).expect("a copy risks nothing");
        assert!(copied.changed, "and it says the source had changed");
        assert!(out.exists(), "and the reader's edits are somewhere");
        // Still a real document rather than a placeholder, which is the half a
        // boolean cannot say.
        assert_eq!(page_count(&out), 4);
    }

    /// And the same source, unchanged, reports `changed: false`.
    ///
    /// The control. Without it the flag above is satisfied by a `changed` that is
    /// always true, which would put a warning on every copy a reader ever writes
    /// and teach them to ignore it.
    #[test]
    fn a_copy_from_an_untouched_source_does_not_claim_it_changed() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("copy-unchanged");
        let open = scratch.join("open.pdf");
        let out = scratch.join("out.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let copied = write_copy(&open, &plan_opened_as(&[1, 0, 0, 0], &open), &out, None)
            .expect("an untouched source copies");
        assert!(!copied.changed);
        assert!(out.exists());
    }

    /// A changed file whose page count also changed is still refused.
    ///
    /// The bound on the tolerance above, and the reason it is not simply "ignore
    /// the fingerprint for copies": the plan names pages by position, so a file
    /// that gained or lost one would have the edits land on pages nobody chose.
    /// That refusal carries `changed` too, so the window offers the same way out.
    #[test]
    fn a_copy_is_refused_when_the_changed_source_has_a_different_page_count() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("copy-reshaped");
        let open = scratch.join("open.pdf");
        let out = scratch.join("out.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");
        let plan = plan_opened_as(&[1, 0, 0, 0], &open);

        // A different document entirely, at the same path.
        let Some(other) = fixture("outline-simple.pdf") else {
            println!("[SKIP] outline-simple.pdf not generated");
            return;
        };
        std::fs::copy(&other, &open).expect("replace the source");
        assert_ne!(
            page_count(&open),
            4,
            "the fixture really is a different shape"
        );

        let why = write_copy(&open, &plan, &out, None).expect_err("must refuse");
        assert!(why.changed, "and it is offered as a change: {why}");
        assert!(!out.exists(), "and writes nothing");
    }

    /// Committing is what makes the file the reader opened the edited one.
    ///
    /// Read back through a second parse rather than by comparing bytes: the
    /// question is whether another reader of this file sees the turn, and a
    /// byte comparison would pass for a file that merely differs.
    #[test]
    fn committing_a_staged_save_puts_the_edits_in_the_file_the_reader_opened() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("commit");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");

        let before = Document::load(&open).expect("load source");
        let was: Vec<i64> = ordered_pages(&before)
            .iter()
            .map(|id| effective_rotation(&before, *id).rem_euclid(360))
            .collect();

        let staged = stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open), None, &Here)
            .expect("stage");
        commit_in_place(&staged.path, &open).expect("commit");

        assert!(!staged.path.exists(), "nothing of the staged file survives");
        let after = Document::load(&open).expect("load the file the reader opened");
        let now: Vec<i64> = ordered_pages(&after)
            .iter()
            .map(|id| effective_rotation(&after, *id).rem_euclid(360))
            .collect();
        assert_eq!(
            now[0],
            (was[0] + 90).rem_euclid(360),
            "the page the reader turned is turned in their own file"
        );
        assert_eq!(&now[1..], &was[1..], "and nothing else moved");
    }

    /// A refusal on the way to a save in place leaves no trace beside the file.
    ///
    /// The page-count mismatch stands in for every guard `planned_bytes` states:
    /// they all run before a byte is written. What is asserted is the *absence*
    /// of the partial file, because a staged file nobody commits is a `.pdf`'s
    /// worth of bytes sitting next to the reader's document with a name they
    /// never chose.
    #[test]
    fn a_refused_save_in_place_stages_nothing() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("refused");
        let open = scratch.join("open.pdf");
        std::fs::copy(&path, &open).expect("copy fixture");
        let before = std::fs::read(&open).expect("read");
        let count = page_count(&open);

        let why = stage_in_place(
            &open,
            &plan_opened_as(&vec![0u8; count + 1], &open),
            None,
            &Here,
        )
        .expect_err("must refuse");
        assert!(why.message.contains("changed since it was opened"), "{why}");
        assert!(
            partials_beside(&open).is_empty(),
            "no partial file is left beside the document"
        );
        assert_eq!(
            std::fs::read(&open).expect("read"),
            before,
            "and the document is untouched"
        );

        // The control: the same document with a plan that matches does stage,
        // so the refusal is about the mismatch rather than about this fixture.
        let staged = stage_in_place(
            &open,
            &plan_opened_as(&vec![0u8; count], &open),
            None,
            &Here,
        )
        .expect("stage");
        assert!(staged.path.exists());
    }

    #[test]
    fn a_destination_that_does_not_exist_yet_is_not_mistaken_for_the_source() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("fresh");
        let out = scratch.join("brand-new.pdf");
        assert!(!out.exists(), "the control: it really is absent");
        write_copy(&path, &plan_of(&[0, 0, 0, 0]), &out, None)
            .expect("a fresh destination is accepted");
        assert!(out.exists());
    }

    /// The name `stage` tries on its `attempt`-th try for `out`.
    ///
    /// Calls the production function rather than reproducing the format: a
    /// second copy of the naming rule would go on passing after the real one
    /// changed, which is the hazard this file's staging fix is about.
    fn staging_path(out: &Path, attempt: u32) -> PathBuf {
        out.parent()
            .unwrap_or(Path::new("."))
            .join(staging_name(out.file_name().expect("a file name"), attempt))
    }

    #[test]
    fn staging_never_writes_over_a_file_that_is_already_there() {
        // **The second blocker: every save staged to one predictable name and
        // wrote it with `std::fs::write`.** Saving `report.pdf` staged at
        // `report.tpdf-partial` --- so it truncated any file already there,
        // followed a symlink at that path, and deleted it on failure whether or
        // not this save had created it. That is destruction outside the file the
        // reader asked to write.
        //
        // `create_new` is what fixes it: a name that is taken is skipped, never
        // opened. The planted file is the control for that, and it is the next
        // name `stage` would have chosen rather than a guess at one.
        let scratch = Scratch::new("staging-collision");
        let out = scratch.join("report.pdf");
        let taken = staging_path(&out, 0);
        std::fs::write(&taken, b"somebody else's work").expect("plant it");

        let staged = stage_bytes(&out, b"%PDF-1.7 the new bytes").expect("stage");
        assert_eq!(
            staged,
            staging_path(&out, 1),
            "it must move on to the next attempt index, not reuse the taken one"
        );
        assert_eq!(
            std::fs::read(&taken).expect("read"),
            b"somebody else's work",
            "and leave the one that was taken exactly as it found it"
        );
        assert_eq!(
            std::fs::read(&staged).expect("read"),
            b"%PDF-1.7 the new bytes"
        );
    }

    #[test]
    fn two_saves_to_one_destination_do_not_share_a_staging_file() {
        // Two saves aimed at the same file used to stage to one path, so the
        // second truncated the first's bytes and either one could rename or
        // delete the other's work. Both files exist at once now, and hold what
        // their own call wrote.
        let scratch = Scratch::new("staging-concurrent");
        let out = scratch.join("report.pdf");
        let first = stage_bytes(&out, b"the first save").expect("stage");
        let second = stage_bytes(&out, b"the second save").expect("stage");

        assert_eq!(
            (first.clone(), second.clone()),
            (staging_path(&out, 0), staging_path(&out, 1))
        );
        assert_eq!(std::fs::read(&first).expect("read"), b"the first save");
        assert_eq!(std::fs::read(&second).expect("read"), b"the second save");
        assert_eq!(
            partials_beside(&out).len(),
            2,
            "and both are really on disk"
        );
    }

    #[cfg(unix)]
    #[test]
    fn staging_does_not_follow_a_symlink_left_at_its_name() {
        // The sharper half of the same blocker. `std::fs::write` follows a
        // symlink, so a link planted at the predictable staging name redirected
        // a save's bytes into whatever it pointed at --- outside the directory,
        // over a file the reader never named. `create_new` is `O_CREAT | O_EXCL`,
        // which refuses a symlink at the path outright rather than resolving it.
        let scratch = Scratch::new("staging-symlink");
        let out = scratch.join("report.pdf");
        let victim = scratch.join("someone-elses.txt");
        std::fs::write(&victim, b"do not overwrite me").expect("plant the victim");
        std::os::unix::fs::symlink(&victim, staging_path(&out, 0)).expect("plant the link");

        let staged = stage_bytes(&out, b"%PDF-1.7 the new bytes").expect("stage");
        assert_eq!(
            std::fs::read(&victim).expect("read"),
            b"do not overwrite me",
            "the bytes went to a file of ours, not through the link"
        );
        assert_eq!(
            std::fs::read(&staged).expect("read"),
            b"%PDF-1.7 the new bytes"
        );
    }

    #[test]
    fn nothing_of_the_partial_file_survives_a_successful_write() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("partial");
        let out = scratch.join("done.pdf");
        write_copy(&path, &plan_of(&[1, 1, 1, 1]), &out, None).expect("write");
        assert!(out.exists());
        assert!(
            partials_beside(&out).is_empty(),
            "the temporary was renamed, not copied"
        );
    }

    /// The rename is what makes the write atomic, and the way to show it is to
    /// put something at the destination first.
    ///
    /// A reader that finds the *old* bytes has seen an interrupted save leave a
    /// whole file; a reader that finds a truncated file has seen the thing this
    /// avoids. The check plants a distinguishable old file and asserts it was
    /// replaced whole --- see `docs/TRAPS.md` on why an atomic-write test has to
    /// plant the intermediate.
    #[test]
    fn the_destination_is_replaced_whole_rather_than_written_through() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("replace");
        let out = scratch.join("existing.pdf");
        let planted = b"this is not a PDF, and it is longer than nothing".to_vec();
        std::fs::write(&out, &planted).expect("plant");

        // A second name for the *same file*, kept as a witness. A rename replaces
        // the directory entry `out` and leaves this one holding the old bytes; a
        // write straight through `out` writes through the shared file and changes
        // this one too. That difference is the whole of "atomic" here, and it is
        // the only observable of it that does not need the write interrupted.
        //
        // Everything below this comment used to be the whole test, and a mutation
        // replacing the temporary path with the destination survived it: the old
        // bytes are gone, it starts with %PDF and it has four pages under a direct
        // write as well. The docstring above claimed atomicity and the assertions
        // could not see it.
        let witness = scratch.join("witness.pdf");
        std::fs::hard_link(&out, &witness).expect("link the witness to the destination");
        assert_eq!(
            std::fs::read(&witness).expect("read witness"),
            planted,
            "the control: the witness really is the same file as the destination, \
             so a change to one is visible in the other"
        );

        write_copy(&path, &plan_of(&[0, 0, 0, 0]), &out, None).expect("write");

        // Deliberately not `assert_eq!`: the failing side is a whole PDF, and
        // `assert_eq!` on two `Vec<u8>` prints every byte as a decimal number ---
        // ~1,700 of them here, which buries the one line that says what went
        // wrong. The lengths and the first bytes are what a reader needs.
        let witnessed = std::fs::read(&witness).expect("read witness");
        assert!(
            witnessed == planted,
            "the destination was renamed into place, not written through: the file \
             that was there is untouched and still holds its own bytes. It holds {} \
             bytes beginning {:?}, where the planted file was {} bytes",
            witnessed.len(),
            String::from_utf8_lossy(&witnessed[..witnessed.len().min(8)]),
            planted.len()
        );

        let after = std::fs::read(&out).expect("read");
        assert_ne!(after, planted, "the old bytes are gone");
        assert!(
            after.starts_with(b"%PDF"),
            "and what is there is a whole document, not a prefix of one"
        );
        assert_eq!(
            page_count(&out),
            4,
            "the replacement is the document, not a fragment of it"
        );
    }

    // --- Marks ---------------------------------------------------------------

    /// A one-page document whose `/Annots` is written in the shape `annots`
    /// describes: absent, inline, or an indirect reference to an array.
    ///
    /// The three exist because `AGENTS.md` records that this distinction decides
    /// how large an annotation edit is --- and because a writer tested only
    /// against the absent case would corrupt the other two silently, by
    /// replacing an array other objects point at.
    fn document_with_annots(annots: AnnotShape) -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Dictionary, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let resources = doc.add_object(Dictionary::new());
        let content = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        };
        // The existing annotation is created only where it is referenced. An
        // unreferenced one left in the file for the absent case would be counted
        // by every check below -- which is how the first version of this made
        // "absent" report two annotations and look like a writer defect.
        match annots {
            AnnotShape::Absent => {}
            AnnotShape::Inline => {
                let existing = doc.add_object(existing_note());
                page.set("Annots", vec![Object::Reference(existing)]);
            }
            AnnotShape::Indirect => {
                let existing = doc.add_object(existing_note());
                let array = doc.add_object(Object::Array(vec![Object::Reference(existing)]));
                page.set("Annots", Object::Reference(array));
            }
        }
        let page_id = doc.add_object(page);
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
                "Resources" => resources,
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise fixture");
        bytes
    }

    /// A comment the document already had, so that extending its `/Annots` can
    /// be told from replacing it.
    fn existing_note() -> lopdf::Dictionary {
        use lopdf::dictionary;
        dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            "Contents" => Object::string_literal("already here"),
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum AnnotShape {
        Absent,
        Inline,
        Indirect,
    }

    /// A plan for a one-page document carrying one highlight.
    fn plan_with_mark(quads: Vec<crate::docmodel::Quad>) -> Plan {
        plan_of_kind(MarkKind::Highlight, quads)
    }

    fn plan_of_kind(kind: MarkKind, quads: Vec<crate::docmodel::Quad>) -> Plan {
        Plan {
            opened_as: None,
            baseline: 1,
            pages: vec![PageView {
                id: 1,
                source: 0,
                turns: 0,
                crop: None,
            }],
            redactions: Vec::new(),
            notes: Vec::new(),
            marks: vec![PlannedMark {
                kind,
                // The biconditional the model enforces, restated here because
                // this builds a plan directly: a stamp with no name draws an
                // empty border, which is a box, so a test written for a stamp
                // would be measuring the wrong kind.
                stamp: (kind == MarkKind::Stamp).then_some(crate::docmodel::StampName::Draft),
                source: 0,
                quads,
                strokes: Vec::new(),
                color: [1.0, 0.9, 0.2],
                author: "a reader".to_string(),
                note: "a note".to_string(),
                made: "D:20260818120000Z".to_string(),
            }],
        }
    }

    // -----------------------------------------------------------------
    // Saving by appending an update section
    // -----------------------------------------------------------------

    /// A copy of a fixture in scratch, with a marks-only plan against it.
    ///
    /// A comment that came out of the file is overridden in place.
    ///
    /// The whole reason `annots::Comment::object` exists: an incremental update
    /// writes a *new version of an object*, so editing somebody else's note
    /// needs the object's own name and nothing else. The scan-order id could not
    /// do it --- inserting a comment on an earlier page renumbers every later
    /// one, and the plan crosses a process boundary.
    ///
    /// **Three assertions and none of them is "it did not error".** The body has
    /// to be the new one, `/M` has to be the plan's date rather than the file's,
    /// and the original bytes have to survive **byte for byte** as a prefix ---
    /// which is what an append *is*, and the property that would break first if
    /// this were quietly doing a rewrite.
    #[test]
    fn a_comment_out_of_the_file_is_overridden_by_its_object() {
        use lopdf::dictionary;

        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let annot = document.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            "Contents" => Object::string_literal("before"),
            "M" => Object::string_literal("D:20260101000000Z"),
        });
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Annots" => vec![annot.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog);
        let mut original = Vec::new();
        document
            .save_to(&mut original)
            .expect("the fixture must save");

        let plan = Plan {
            opened_as: None,
            baseline: 1,
            pages: vec![PageView {
                id: 1,
                source: 0,
                turns: 0,
                crop: None,
            }],
            marks: Vec::new(),
            redactions: Vec::new(),
            notes: vec![crate::edits::PlannedNoteEdit {
                object: (annot.0, annot.1),
                body: "after".into(),
                made: "D:20260829120000Z".into(),
            }],
        };
        assert!(
            plan.is_appendable(),
            "a plan carrying only a note edit must still be an append"
        );

        let built = append_update(original.clone(), &plan, None).expect("the append must build");

        let mut whole = original.clone();
        whole.extend_from_slice(&built.update);
        assert_eq!(
            &whole[..original.len()],
            &original[..],
            "an append must not rewrite a byte of the previous revision"
        );

        let after = Document::load_mem(&whole).expect("the appended file must parse");
        let dictionary = after
            .get_object(annot)
            .expect("the annotation must still be there")
            .as_dict()
            .expect("and must still be a dictionary");
        assert_eq!(
            dictionary
                .get(b"Contents")
                .and_then(Object::as_str)
                .expect("a body"),
            b"after",
            "the new body is what a reader typed"
        );
        assert_eq!(
            dictionary
                .get(b"M")
                .and_then(Object::as_str)
                .expect("a date"),
            b"D:20260829120000Z",
            "`/M` moves with the note, or every viewer shows somebody else's date"
        );
    }

    /// A copy rather than the fixture itself, and it is not tidiness: an append
    /// writes to the file it is given, so a test that pointed at `testdata/`
    /// would edit the corpus every other test reads.
    ///
    /// **Callers pass `comments.pdf` rather than `text-heavy.pdf`, and that is a
    /// coverage fix rather than a preference.** `text-heavy.pdf` is a real
    /// document supplied by hand --- no script writes it, `scripts/ci_fixtures.py`
    /// says so, and `BUILD.md` has recorded since 2026-07-30 that the Windows box
    /// has never had it. What nobody had drawn from that is what it does to a
    /// *unit test*: ten tests over this module's guards took their `else` arm and
    /// returned, here and on both CI runners, and a test that returns early
    /// passes exactly like one that ran. Every mutation aimed at those guards
    /// SURVIVED for that reason and for no other.
    ///
    /// Nothing in these tests needs a real document. They are about lengths,
    /// fingerprints and rollback, so what the fixture has to be is *appendable*
    /// and generated. `comments.pdf` is both, is built by one of the
    /// dependency-free scripts CI already runs, and carries `/Annots` of its own
    /// --- which a plain text document does not, so the array-bearing branch of
    /// `mark_sites` is now exercised as well.
    fn appendable(scratch: &Scratch, name: &str) -> Option<(PathBuf, Plan)> {
        appendable_with(scratch, name, None)
    }

    /// [`appendable`] for a document that needs a password to be counted.
    ///
    /// **The helper's own version of the defect this increment fixes**, which is
    /// worth saying rather than quietly parameterising: `page_count` loads with
    /// no password, and `lopdf` parses *no objects* for a document it cannot
    /// authenticate --- so a locked fixture came back as 0 pages and the plan
    /// built from it was refused for having the wrong baseline. The refusal was
    /// correct and named the wrong thing, which is what a count taken through a
    /// reader that could not read is always going to do.
    fn appendable_with(
        scratch: &Scratch,
        name: &str,
        password: Option<&str>,
    ) -> Option<(PathBuf, Plan)> {
        let source = fixture(name)?;
        let at = scratch.join(name);
        std::fs::copy(&source, &at).expect("copy the fixture");
        let count = match password {
            None => page_count(&at),
            Some(password) => Document::load_with_options(
                &at,
                lopdf::LoadOptions {
                    password: Some(password.to_string()),
                    ..Default::default()
                },
            )
            .expect("load with the password")
            .get_pages()
            .len(),
        };
        let mut plan = plan_opened_as(&vec![0u8; count], &at);
        plan.marks = vec![PlannedMark {
            kind: MarkKind::Highlight,
            stamp: None,
            source: 0,
            quads: one_quad(),
            strokes: Vec::new(),
            color: [1.0, 0.9, 0.2],
            author: "a reader".to_string(),
            note: "a note".to_string(),
            made: "D:20260822120000Z".to_string(),
        }];
        Some((at, plan))
    }

    #[test]
    fn a_plan_that_only_adds_marks_is_appended_and_anything_else_is_rewritten() {
        // The classification, and the negative half is the one with evidence
        // behind it: spike 0.6 put an appended *annotation* to four independent
        // parsers and never put an appended deletion, move, turn or crop to any
        // of them. Each of those four is asserted rather than the rule being
        // stated once, because what would ship is a predicate that let one
        // through.
        let small = 1_024;
        let mut marked = plan_of(&[0, 0, 0]);
        marked.marks = plan_of_kind(MarkKind::Highlight, one_quad()).marks;
        assert_eq!(mode_for(&marked, small), Mode::Append);

        assert_eq!(
            mode_for(&plan_of(&[0, 0, 0]), small),
            Mode::Rewrite,
            "a plan with no marks has nothing to append"
        );

        let mut turned = marked.clone();
        turned.pages[1].turns = 1;
        assert_eq!(mode_for(&turned, small), Mode::Rewrite, "a turn");

        let mut cropped = marked.clone();
        cropped.pages[1].crop = Some([10.0, 10.0, 100.0, 100.0]);
        assert_eq!(mode_for(&cropped, small), Mode::Rewrite, "a crop");

        let mut deleted = marked.clone();
        deleted.pages.remove(1);
        assert_eq!(mode_for(&deleted, small), Mode::Rewrite, "a deletion");

        let mut moved = marked.clone();
        moved.pages.swap(0, 1);
        assert_eq!(mode_for(&moved, small), Mode::Rewrite, "a move");
    }

    /// The size condition, at the boundary rather than near it.
    ///
    /// **Both sides of one byte**, because a threshold tested with a small file
    /// and a huge one passes for `<=`, for `<`, and for a comparison against a
    /// number that is not this one at all. The interesting inputs of a bound are
    /// the two either side of it, and `docs/TRAPS.md` records what a tolerance
    /// picked loosely enough to always hold does to a check.
    ///
    /// The plan is held fixed and marks-only throughout, so the only thing
    /// moving is the size --- otherwise a `Rewrite` here would be evidence about
    /// `is_appendable` rather than about the bound.
    #[test]
    fn a_marks_only_plan_is_rewritten_once_the_file_is_too_large_to_parse_twice() {
        let mut marked = plan_of(&[0, 0, 0]);
        marked.marks = plan_of_kind(MarkKind::Highlight, one_quad()).marks;

        assert_eq!(
            mode_for(&marked, APPEND_MAX_BYTES),
            Mode::Append,
            "the threshold itself is small enough"
        );
        assert_eq!(
            mode_for(&marked, APPEND_MAX_BYTES - 1),
            Mode::Append,
            "one byte under"
        );
        assert_eq!(
            mode_for(&marked, APPEND_MAX_BYTES + 1),
            Mode::Rewrite,
            "one byte over is a rewrite, however little the plan changes"
        );

        // The value, pinned. Not because 256 MiB is sacred -- it is a judgement
        // and `APPEND_MAX_BYTES` says so -- but because a bound that silently
        // moved would leave every number in `BUILD.md` describing a different
        // program, and the failure is a worker aborting on a document nobody
        // tested. Changing it should be a deliberate edit in two places.
        assert_eq!(APPEND_MAX_BYTES, 268_435_456, "256 MiB");

        // The relation to the measured ceiling is checked at build time instead,
        // beside the constant: it is a comparison between two constants, and one
        // of those inside a `#[test]` is an assertion that cannot fail.
    }

    /// A file whose size cannot be read is rewritten, not appended.
    ///
    /// The failure path, and it is the whole reason [`mode_for_source`] is a
    /// function rather than two lines inside the command. "Could not measure it"
    /// and "measured it, it is small" are the same answer to `mode_for` unless
    /// something decides otherwise, and what decides is a `u64::MAX` that no
    /// test could reach if it lived at the call site.
    ///
    /// The plan is marks-only, so `Append` is what the *other* condition asks
    /// for --- without that this would pass on a plan that could never be
    /// appended anyway, which is the check-that-cannot-fail shape.
    #[test]
    fn a_document_whose_size_cannot_be_read_is_rewritten() {
        let mut marked = plan_of(&[0, 0, 0]);
        marked.marks = plan_of_kind(MarkKind::Highlight, one_quad()).marks;

        let missing = std::env::temp_dir().join("tpdf-no-such-document-for-mode-for.pdf");
        assert!(
            !missing.exists(),
            "the control needs a path that is really absent"
        );
        assert_eq!(
            mode_for_source(&marked, &missing),
            Mode::Rewrite,
            "an unmeasurable file takes the arm with no memory bound over it"
        );

        // The control the assertion above needs: the same plan, through the same
        // function, on a file that *can* be measured, is an append. Without it a
        // `mode_for_source` that answered `Rewrite` for everything would pass.
        let present = std::env::temp_dir().join("tpdf-mode-for-source-control.pdf");
        std::fs::write(&present, b"%PDF-1.7\n").expect("a small file to measure");
        assert_eq!(
            mode_for_source(&marked, &present),
            Mode::Append,
            "a measurable small file is still an append"
        );
        let _ = std::fs::remove_file(&present);
    }

    #[test]
    fn an_append_leaves_every_byte_of_the_previous_revision_where_it_was() {
        // **The property the whole mode exists for.** A rewrite renumbers every
        // object in the document; an append adds to the end, so what was there
        // before is still there, at the same offsets --- which is what lets a
        // validator show exactly what a signature covered, and is why this is
        // not merely a faster rewrite.
        let scratch = Scratch::new("append-prefix");
        let Some((at, plan)) = appendable(&scratch, "comments.pdf") else {
            println!("[SKIP] comments.pdf: fixture not generated");
            return;
        };
        let before = std::fs::read(&at).expect("read before");

        let appended = append_bytes(&at, &plan, None).expect("build the update");
        append_in_place(&appended, &at, None, &Here).expect("append");

        let after = std::fs::read(&at).expect("read after");
        assert!(after.len() > before.len(), "something was written");
        assert_eq!(
            &after[..before.len()],
            &before[..],
            "the previous revision is byte for byte where it was"
        );
        assert_eq!(
            after.len() - before.len(),
            appended.len(),
            "and the file grew by exactly the update section"
        );
        // Small, and it is the claim `docs/PLAN.md` §5 makes about the mode
        // rather than a fact about this fixture: an update section is the
        // objects that changed, so it does not scale with the document. Ten
        // kilobytes is far above the measured 700-odd bytes and far below the
        // 1.4 MB this fixture would cost to rewrite.
        assert!(appended.len() < 10_000, "{} bytes", appended.len());
    }

    /// An encrypted document can take a mark, and comes back still encrypted.
    ///
    /// **The one save an encrypted document can have.** `lopdf`'s full
    /// serialiser writes every object in the clear and drops the `/Encrypt`
    /// dictionary with it, which is why [`planned_bytes`] refuses; an append
    /// never rewrites the previous revision, and `IncrementalDocument::save_to`
    /// encrypts each appended object with the state the load recorded and puts
    /// `/Encrypt` back in the appended trailer.
    ///
    /// **Both fixtures, because one cannot discriminate.** The empty-user-password
    /// document reaches every branch here without a password at all --- `lopdf`
    /// tries the empty one itself --- so a version of this that threaded the
    /// password nowhere would pass on it. The one behind `swordfish` is what
    /// makes each `Some(...)` below load-bearing: without it the parse reads no
    /// objects, and the read-back in [`append_through`] counts zero pages against
    /// the two it expects and rolls the save back.
    ///
    /// What is *not* asserted here is that the ciphertext is real, because this
    /// module's own writer and reader are the same library --- `docs/TRAPS.md`
    /// has that under *a writer and its own reader agree about a document that is
    /// wrong*. `examples/incremental_save.rs --mode encrypted` puts the result to
    /// `qpdf --is-encrypted` and greps the update section for a plaintext needle,
    /// and `examples/password_probe.rs` drives the production worker.
    #[test]
    fn an_encrypted_document_can_be_appended_to_and_stays_encrypted() {
        let scratch = Scratch::new("append-encrypted");
        let mut examined = 0;
        for (name, password) in [
            ("incr-encrypted-open.pdf", ""),
            ("incr-encrypted-pw.pdf", "swordfish"),
        ] {
            let Some((at, plan)) = appendable_with(&scratch, name, Some(password)) else {
                println!("[SKIP] {name}: fixture not generated");
                continue;
            };
            examined += 1;
            let before = std::fs::read(&at).expect("read before");

            let appended = append_bytes(&at, &plan, Some(password))
                .unwrap_or_else(|why| panic!("{name}: build the update: {why}"));
            append_in_place(&appended, &at, Some(password), &Here)
                .unwrap_or_else(|why| panic!("{name}: append: {why}"));

            let after = std::fs::read(&at).expect("read after");
            assert_eq!(
                &after[..before.len()],
                &before[..],
                "{name}: the previous revision is byte for byte where it was"
            );

            // The claim that matters, and it is about the *file* rather than
            // about what we believe we wrote: reopened from disk, it still
            // needs the same key, and it still has its pages.
            let reopened = Document::load_mem_with_options(
                &after,
                lopdf::LoadOptions {
                    password: Some(password.to_string()),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|why| panic!("{name}: the saved file must reopen: {why}"));
            assert!(
                reopened.was_encrypted(),
                "{name}: the saved file is still encrypted"
            );
            assert_eq!(
                reopened.get_pages().len(),
                plan.baseline as usize,
                "{name}: no page was added or lost"
            );
            assert!(
                listed_on_page_of(&at, 0, Some(password))
                    .iter()
                    .any(|kind| kind == "Highlight"),
                "{name}: the first page lists the mark"
            );
        }
        assert!(
            examined > 0,
            "both encrypted fixtures are absent, so this checked nothing"
        );
    }

    /// A locked document nobody unlocked is refused, and says what would help.
    ///
    /// The other side of the test above, and the one that keeps its `Some(...)`
    /// honest: without this, an append that ignored the password entirely would
    /// still be refused here for the right reason and pass, because `lopdf`
    /// leaves `/Encrypt` in the trailer for a document it could not authenticate.
    #[test]
    fn an_append_to_a_document_nobody_unlocked_is_refused() {
        let scratch = Scratch::new("append-locked");
        let Some((at, plan)) =
            appendable_with(&scratch, "incr-encrypted-pw.pdf", Some("swordfish"))
        else {
            println!("[SKIP] incr-encrypted-pw.pdf: fixture not generated");
            return;
        };
        let before = std::fs::read(&at).expect("read before");

        let why = append_bytes(&at, &plan, None).expect_err("must refuse");
        assert!(
            why.message.contains("password"),
            "the message names what would help: {why}"
        );
        assert_eq!(
            std::fs::read(&at).expect("read after"),
            before,
            "a refusal writes nothing"
        );

        // And the wrong password is refused by the parser before any of this,
        // which is a different message and a different mechanism.
        let why = append_bytes(&at, &plan, Some("hunter2")).expect_err("must refuse");
        assert!(
            why.message.contains("could not be parsed"),
            "a wrong password is the parser's refusal: {why}"
        );
    }

    #[test]
    fn an_appended_mark_is_listed_by_the_page_it_was_made_on() {
        // The append is not merely accepted, it carries the edit. Read back
        // through the same `subtypes_on` the rewrite path's tests use, so the
        // two modes are asserted to produce the same thing rather than each
        // being asserted to produce something.
        // `rotated.pdf` rather than the `comments.pdf` its neighbours use,
        // because the negative half below needs a page that lists *nothing* --
        // and a fixture that ships its own comments cannot provide one. Asking
        // instead whether page 1 gained a Highlight would not rescue it: this
        // fixture's own marks include highlights, so the assertion could not
        // tell "the mark went to the wrong page" from "the fixture was already
        // like that". Annotation-free is the control, not a preference.
        let scratch = Scratch::new("append-mark");
        let Some((at, plan)) = appendable(&scratch, "rotated.pdf") else {
            println!("[SKIP] rotated.pdf: fixture not generated");
            return;
        };
        let pages = page_count(&at);

        let appended = append_bytes(&at, &plan, None).expect("build the update");
        append_in_place(&appended, &at, None, &Here).expect("append");

        assert_eq!(page_count(&at), pages, "no page was added or lost");
        // Zero-based, which is `listed_on_page`'s own index into
        // `ordered_pages` --- the mark is on `source: 0`, the file's first page.
        let found = listed_on_page(&at, 0);
        assert!(
            found.iter().any(|name| name == "Highlight"),
            "the first page lists the mark: {found:?}"
        );
        assert!(
            listed_on_page(&at, 1).is_empty(),
            "and the second lists nothing, so the mark is on the page it names"
        );
    }

    #[test]
    fn an_append_to_a_file_that_changed_length_is_refused_and_writes_nothing() {
        // The update names byte offsets into the previous revision and chains
        // `/Prev` to its `startxref`, so appending it after any other length
        // produces a cross-reference pointing at the wrong bytes --- a file that
        // opens and is wrong, which is the worst of the three outcomes.
        let scratch = Scratch::new("append-moved");
        let Some((at, plan)) = appendable(&scratch, "comments.pdf") else {
            println!("[SKIP] comments.pdf: fixture not generated");
            return;
        };
        let appended = append_bytes(&at, &plan, None).expect("build the update");

        // Something else writes to the file in between.
        {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&at)
                .expect("open");
            file.write_all(b"% something else was here\n")
                .expect("write");
        }
        let meddled = std::fs::read(&at).expect("read");

        let refused = append_in_place(&appended, &at, None, &Here).expect_err("refused");
        // Derived from the fixture rather than transcribed, so a reworded
        // message keeps this honest and a message naming the wrong length
        // cannot pass: `docs/TRAPS.md`, *A test pinned a random value out of a
        // generated fixture*.
        assert!(
            refused.contains(&appended.was.to_string())
                && refused.contains(&meddled.len().to_string()),
            "must name the length it was built against and the length it found: {refused}"
        );
        assert!(
            refused.contains("nothing was written"),
            "and must say the file is untouched: {refused}"
        );
        assert_eq!(
            std::fs::read(&at).expect("read"),
            meddled,
            "and nothing was written on top of it"
        );
    }

    /// The same bytes with one comment byte changed: a different document of
    /// exactly the same length.
    ///
    /// PDF's second line is a binary comment by convention, and a comment runs
    /// to end of line and means nothing to a parser --- so flipping a byte in it
    /// leaves a file that loads, has the same pages and hashes differently. That
    /// combination is the whole point: length alone cannot tell the two apart.
    fn same_length_variant(bytes: &[u8]) -> Vec<u8> {
        let line_two = bytes
            .iter()
            .position(|b| *b == b'\n')
            .expect("a PDF has a header line")
            + 1;
        assert_eq!(
            bytes[line_two], b'%',
            "this fixture's second line is not a comment, so flipping a byte in \
             it would not leave a valid document"
        );
        let mut other = bytes.to_vec();
        other[line_two + 1] ^= 0xFF;
        assert_ne!(other, bytes, "the variant has to differ");
        assert_eq!(other.len(), bytes.len(), "and has to be the same length");
        other
    }

    #[test]
    fn an_update_built_against_a_different_length_is_refused() {
        // **The seam's own check, and it exists because the property it asserts
        // stopped holding by construction on 2026-08-22.** Until then one
        // function read the file and built the update from what it had read, so
        // "the length the update was built against" and "the length the caller
        // checked" were one number under two names --- the shape `docs/TRAPS.md`
        // records as a check whose operands are the same value.
        //
        // They are two numbers now: the parse happens in the worker holding the
        // document, and the file measurement happens here. A worker on a stale
        // mapping, or a file that moved between the two, produces an update whose
        // byte offsets point into a document nobody has --- and the result would
        // still open, which is what makes it worth refusing rather than
        // detecting later.
        let scratch = Scratch::new("append-mismatch");
        let Some((at, plan)) = appendable(&scratch, "comments.pdf") else {
            println!("[SKIP] comments.pdf: fixture not generated");
            return;
        };
        let original = std::fs::read(&at).expect("read");
        let ready = append_ready(&at, &plan).expect("check the file");
        let update = append_update(original, &plan, None).expect("build the update");

        // The control: the two halves as they really are agree, so the refusal
        // below is about the mismatch and not about the pair being unusable.
        assert_eq!(update.built_against as u64, ready.len());
        appended(
            append_ready(&at, &plan).expect("check again"),
            update.clone(),
        )
        .expect("the honest pair is accepted");

        let stale = Update {
            built_against: update.built_against + 1,
            ..update
        };
        let refused = appended(append_ready(&at, &plan).expect("check again"), stale)
            .expect_err("must refuse");
        assert!(
            refused.changed,
            "and must say the file is the reason: {refused}"
        );
        assert!(
            refused.message.contains(&ready.len().to_string()),
            "naming the length it checked: {refused}"
        );
    }

    #[test]
    fn a_plan_that_crosses_the_worker_boundary_leaves_its_fingerprint_behind() {
        // `Plan::opened_as` is `#[serde(skip)]`, and this is what says so
        // outside the derive. A fingerprint is a fact about a path; the worker
        // has neither a path nor any business asserting one, and `Request`'s
        // standing property is that it names nothing the worker could act on.
        //
        // **The compiler is the primary guard, not this test**, and that is
        // worth stating because it changes what this test is for. Deleting the
        // `#[serde(skip)]` does not produce a wrong value --- it produces
        // `error[E0277]`, because `Fingerprint` implements neither `Serialize`
        // nor `Deserialize`, so the attribute is what makes `Plan` derivable at
        // all. There is no mutation to write: the property is unexpressible
        // rather than merely untaken. `docs/TRAPS.md` records the attempt.
        //
        // What this test still catches is the change the compiler would wave
        // through: somebody adding serde to `Fingerprint` for an unrelated
        // reason and dropping the skip in the same edit, which type-checks and
        // silently puts a digest of the reader's file on the wire.
        //
        // The control is the rest of the plan: if serialisation dropped
        // everything the assertion would pass for the wrong reason.
        let Some(path) = fixture("text-heavy.pdf") else {
            println!("[SKIP] text-heavy.pdf: fixture not generated");
            return;
        };
        let plan = plan_opened_as(&[0], &path);
        assert!(
            plan.opened_as.is_some(),
            "the control: this plan really does carry one"
        );

        let wire = serde_json::to_string(&plan).expect("serialise");
        assert!(
            !wire.contains("opened_as"),
            "the fingerprint must not be on the wire at all: {wire}"
        );
        let back: Plan = serde_json::from_str(&wire).expect("deserialise");
        assert_eq!(back.opened_as, None, "and cannot come back carrying one");
        assert_eq!(
            (back.baseline, back.pages, back.marks),
            (plan.baseline, plan.pages.clone(), plan.marks.clone()),
            "while everything the builder needs survives the round trip"
        );
    }

    #[test]
    fn an_append_refuses_a_replacement_that_kept_the_length() {
        // **The blocker this file was carrying: `Appended::verified` held a full
        // fingerprint and nothing read it.** The guard was `now != appended.was`
        // --- a length, and only a length --- while the field's own doc comment
        // said it was the caller's last look and `lib.rs` called comparing a
        // length "a sharper answer" than comparing a length and a timestamp. A
        // document replaced by a distinct revision of the same size would have
        // had this update's byte offsets appended to an object graph they were
        // never computed against, and the read-back cannot see that: it asks the
        // page count, which a same-shape replacement keeps.
        //
        // The replacement here differs in one comment byte, so the *length* half
        // of the check cannot fire and only the modification time can. Its
        // sibling above covers the other half, where the length moves.
        let scratch = Scratch::new("append-swapped");
        let Some((at, plan)) = appendable(&scratch, "comments.pdf") else {
            println!("[SKIP] comments.pdf: fixture not generated");
            return;
        };
        let appended = append_bytes(&at, &plan, None).expect("build the update");
        let was = std::fs::read(&at).expect("read");

        // A different document of the same length and the same page count,
        // stamped with a modification time that is definitely not the original's
        // --- rather than trusting the clock to have moved between two writes,
        // which is how this test would flake on a filesystem with a coarse one.
        let other = same_length_variant(&was);
        assert_eq!(
            Document::load_mem(&other)
                .expect("the variant loads")
                .get_pages()
                .len(),
            Document::load_mem(&was)
                .expect("the original loads")
                .get_pages()
                .len(),
            "and has the same page count, so nothing downstream could tell them apart"
        );
        std::fs::write(&at, &other).expect("replace it");
        let stamped = std::fs::File::options()
            .write(true)
            .open(&at)
            .expect("open");
        stamped
            .set_times(
                std::fs::FileTimes::new().set_modified(
                    std::time::SystemTime::now() + std::time::Duration::from_secs(60),
                ),
            )
            .expect("stamp");
        drop(stamped);

        let refused = append_in_place(&appended, &at, None, &Here).expect_err("must refuse");
        assert!(
            refused.contains("nothing was written"),
            "and must say so: {refused}"
        );
        assert_eq!(
            std::fs::read(&at).expect("read"),
            other,
            "and the file that is there now is untouched"
        );
    }

    #[test]
    fn an_append_writes_through_its_handle_and_says_so_when_the_name_moves() {
        // **What holding the handle buys, and it is the only test that can
        // show it.** The window between opening the file and writing to it is
        // inside `append_in_place`, where nothing can be planted --- which is
        // why `append_through` takes the handle as an argument. Here the
        // pathname is made to name a *different* file after the handle is open:
        //
        //  - the checks pass, because they ask the handle and the file it holds
        //    has not changed. A check against the pathname would ask about the
        //    replacement, which is the wrong file to have an opinion about.
        //  - the update lands in the file that was opened, complete.
        //  - the file that has the name now is not touched at all --- and the
        //    old code would have appended to it, or truncated it in a roll-back.
        //  - and the save reports that it did not land where it was asked to,
        //    rather than reporting success.
        let scratch = Scratch::new("append-renamed");
        let Some((at, plan)) = appendable(&scratch, "comments.pdf") else {
            println!("[SKIP] comments.pdf: fixture not generated");
            return;
        };
        let appended = append_bytes(&at, &plan, None).expect("build the update");
        let was = std::fs::metadata(&at).expect("measure").len();

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&at)
            .expect("open the file this save is about");

        // Something else moves our file aside and puts its own there.
        let aside = scratch.join("moved-aside.pdf");
        std::fs::rename(&at, &aside).expect("move ours aside");
        // **Deliberately not a PDF**, and that is what makes this test able to
        // see where the read-back reads from. The read-back asks whether the
        // saved file still parses and still has its pages: through the handle it
        // asks about ours, which does; through the pathname it would ask about
        // this, which does not --- and would then roll *our* file back, which the
        // last assertion below would catch. A valid intruder makes both routes
        // answer the same way, and the check stops discriminating.
        let intruder = b"this is not a PDF at all\n".repeat(64);
        std::fs::write(&at, &intruder).expect("put a different file there");

        let refused =
            append_through(&mut file, &appended, &at, None, &Here).expect_err("must report");
        drop(file);
        assert!(
            refused.contains("renamed or removed it"),
            "and must name what happened rather than a length: {refused}"
        );
        assert_eq!(
            std::fs::read(&at).expect("read"),
            intruder,
            "the file that has the name now is byte-for-byte as it was"
        );

        let landed = std::fs::read(&aside).expect("read");
        assert_eq!(
            landed.len() as u64,
            was + appended.len() as u64,
            "and the update went to the file the handle held"
        );
        assert_eq!(
            Document::load_mem(&landed).expect("load").get_pages().len(),
            appended.pages,
            "complete, not half written"
        );
    }

    #[test]
    fn an_append_that_cannot_be_read_back_puts_the_file_back_as_it_was() {
        // **The rollback, and it needs the failure planted rather than hoped
        // for.** Every other outcome of `append_in_place` leaves the file valid,
        // so a test that only ever appended good bytes would exercise the
        // recovery path never --- which is the trap about a test for an atomic
        // write that does not plant the intermediate it is meant to prove.
        //
        // The update section is replaced with bytes that are not one. They are
        // appended, the re-read fails, and the file has to come back at exactly
        // its previous length and content.
        let scratch = Scratch::new("append-rollback");
        let Some((at, plan)) = appendable(&scratch, "comments.pdf") else {
            println!("[SKIP] comments.pdf: fixture not generated");
            return;
        };
        let before = std::fs::read(&at).expect("read before");
        let mut appended = append_bytes(&at, &plan, None).expect("build the update");
        // A trailer that names an offset into nothing. It parses as far as
        // `startxref` and then points at a cross-reference that is not there.
        appended.update = b"\nstartxref\n999999999\n%%EOF\n".to_vec();

        let refused =
            append_in_place(&appended, &at, None, &Here).expect_err("the re-read refuses");
        assert!(refused.contains("put back"), "{refused}");
        assert_eq!(
            std::fs::read(&at).expect("read after"),
            before,
            "the file is exactly what it was"
        );
    }

    /// A [`Reread`] that answers what it is told to and records what it was asked.
    ///
    /// The double that makes the seam observable. Without it the only way to ask
    /// "did the coordinator delegate the verification or do it itself" is to read
    /// the source, and a source-level assertion proves a shape rather than an
    /// ordering.
    struct Fake {
        answer: Result<usize, String>,
        asked: std::cell::RefCell<Vec<(usize, Option<String>)>>,
    }

    impl Fake {
        fn saying(answer: Result<usize, String>) -> Self {
            Self {
                answer,
                asked: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl Reread for Fake {
        fn pages(
            &self,
            _file: &mut std::fs::File,
            len: usize,
            password: Option<&str>,
        ) -> Result<usize, String> {
            self.asked
                .borrow_mut()
                .push((len, password.map(str::to_string)));
            self.answer.clone()
        }
    }

    #[test]
    fn the_coordinator_does_not_parse_the_file_it_wrote() {
        // **The keystone, and it is red on the code this replaced.** The file
        // written here does not parse --- the same trailer pointing into nothing
        // that `an_append_that_cannot_be_read_back_puts_the_file_back_as_it_was`
        // plants --- so a coordinator holding the bytes refuses it, whatever any
        // verifier says. The save succeeding on exactly those bytes is what says
        // the parse is somewhere else now: the answer is the verifier's, and
        // this process has nothing to second-guess it with.
        //
        // It is the accounting observable for a property that is otherwise
        // invisible. Every number a caller can see is identical whether the
        // parse happened here or in a worker, because the two agree wherever
        // both answer --- so the thing to assert is *who was asked*.
        let scratch = Scratch::new("append-delegates");
        let Some((at, plan)) = appendable(&scratch, "comments.pdf") else {
            println!("[SKIP] comments.pdf: fixture not generated");
            return;
        };
        let mut appended = append_bytes(&at, &plan, None).expect("build the update");
        appended.update = b"\nstartxref\n999999999\n%%EOF\n".to_vec();
        let fake = Fake::saying(Ok(appended.pages));

        append_in_place(&appended, &at, None, &fake).expect("the verifier's answer is the answer");

        assert_eq!(
            fake.asked.borrow().len(),
            1,
            "asked exactly once, so the write path has one verification and not two"
        );
    }

    /// A [`Rewriter`] that writes what it is told to and records what it was asked.
    ///
    /// [`Fake`]'s counterpart on the writing side, and it exists for the same
    /// reason: without it, the only way to ask whether the coordinator delegated
    /// the *parse* is to read the source, and a source-level assertion proves a
    /// shape rather than an ordering.
    struct FakeWriter {
        answer: Result<Vec<u8>, Refusal>,
        /// How many bytes to *claim* beyond what was written.
        ///
        /// Zero for an honest writer. It is here because the check it exercises
        /// --- the staged file's own size against the length reported back --- is
        /// the only thing standing between a short write in another process and
        /// a rename over the reader's document.
        overstate_by: usize,
        asked: std::cell::RefCell<Vec<(usize, Option<String>)>>,
    }

    impl FakeWriter {
        fn writing(answer: Result<Vec<u8>, Refusal>) -> Self {
            Self {
                answer,
                overstate_by: 0,
                asked: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl Rewriter for FakeWriter {
        fn write(
            &self,
            _source: &mut std::fs::File,
            len: usize,
            out: &mut std::fs::File,
            _plan: &Plan,
            password: Option<&str>,
        ) -> Result<usize, Refusal> {
            use std::io::Write as _;

            self.asked
                .borrow_mut()
                .push((len, password.map(str::to_string)));
            let bytes = self.answer.clone()?;
            out.write_all(&bytes).map_err(|e| e.to_string())?;
            Ok(bytes.len() + self.overstate_by)
        }
    }

    /// Whether anything is left beside `at` from a staging attempt.
    ///
    /// The observable for "the refusal cleaned up after itself". A staged file
    /// nothing renames is not merely untidy: it is a copy of the reader's
    /// document, possibly a partial one, sitting in their directory under a name
    /// they did not choose.
    fn leftovers_beside(at: &Path) -> Vec<String> {
        let dir = at.parent().expect("a parent");
        std::fs::read_dir(dir)
            .expect("read the directory")
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().to_string_lossy().into_owned();
                name.contains(PARTIAL).then_some(name)
            })
            .collect()
    }

    /// A document on disk and a plan that keeps every page of it, for the tests
    /// below that are about the seam rather than about any particular edit.
    fn staging_subject(scratch: &Scratch, name: &str) -> Option<(PathBuf, Plan)> {
        let at = scratch.join(name);
        std::fs::write(
            &at,
            b"%PDF-1.7\nnot a document this process will ever parse\n",
        )
        .expect("plant the source");
        let plan = plan_opened_as(&[1, 0, 0, 0], &at);
        Some((at, plan))
    }

    #[test]
    fn the_coordinator_does_not_parse_the_document_it_rewrites() {
        // **The keystone of the rewrite's move, and it is red on the code this
        // replaced.** The source planted here is not a PDF at all, so a
        // coordinator that parsed it would refuse before writing anything ---
        // which is exactly what `planned_bytes` did until 2026-08-28, on every
        // save that deletes a page, moves one, turns one or crops one.
        //
        // The save succeeding on those bytes is what says the parse is somewhere
        // else now: the bytes are the writer's, and this process has nothing to
        // second-guess them with. It is the accounting observable for a property
        // that is otherwise invisible --- every number a caller can see is the
        // same whichever process did the parsing, so the thing to assert is *who
        // was asked*.
        let scratch = Scratch::new("rewrite-delegates");
        let (at, plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
        let writer = FakeWriter::writing(Ok(b"%PDF-1.7 whatever the worker produced".to_vec()));

        let staged = stage_in_place(&at, &plan, None, &writer).expect("the writer's bytes are it");

        assert_eq!(
            writer.asked.borrow().len(),
            1,
            "asked exactly once, so the save has one rewrite in it and not two"
        );
        assert_eq!(
            std::fs::read(&staged.path).expect("read the staged file"),
            b"%PDF-1.7 whatever the worker produced",
            "the staged file holds what the writer wrote and nothing this process made"
        );
    }

    #[test]
    fn the_rewrite_is_asked_for_the_length_and_the_password() {
        // **Neither term has a failing case under `Here`**, which is why they are
        // pinned here rather than left to whichever implementation happens to
        // read them. `Here` passes `len` to `read_whole` as a capacity hint, so
        // a wrong one costs an allocation and changes no answer; it is the *map*
        // length for a worker, where being wrong means rewriting a prefix of the
        // document. And a password that never arrives makes `lopdf` parse no
        // objects at all, so an encrypted document would rewrite to an empty one
        // rather than refusing --- the same failure `reread_pages` names, on the
        // way in instead of the way out.
        let scratch = Scratch::new("rewrite-asks-for-length");
        let (at, plan) = staging_subject(&scratch, "measured.pdf").expect("a subject");
        let was = std::fs::metadata(&at).expect("measure").len() as usize;
        let writer = FakeWriter::writing(Ok(b"%PDF-1.7 rewritten".to_vec()));

        stage_in_place(&at, &plan, Some("hunter2"), &writer).expect("stage");

        assert_eq!(
            writer.asked.borrow().as_slice(),
            &[(was, Some("hunter2".to_string()))],
            "the file as it is on disk, and the key the reader opened it with"
        );
    }

    #[test]
    fn a_rewriter_that_overstates_what_it_wrote_is_refused() {
        // **The one check on the way back, and it is the only one there can be.**
        // The bytes never reach this process, so nothing here can look at them;
        // what it can do is compare two numbers that were arrived at
        // independently --- the length the writer reports and the length the file
        // has. A short write in another process, a reply built for a different
        // request, or a second rewrite appending to the first all disagree here.
        //
        // Without it, a rename would put a truncated document over the reader's
        // only copy and report success.
        let scratch = Scratch::new("rewrite-overstates");
        let (at, plan) = staging_subject(&scratch, "short.pdf").expect("a subject");
        let mut writer = FakeWriter::writing(Ok(b"%PDF-1.7 rewritten".to_vec()));
        writer.overstate_by = 1;

        let why = stage_in_place(&at, &plan, None, &writer).expect_err("must refuse");

        assert!(
            why.message.contains("was not completed"),
            "the refusal says the save did not finish: {}",
            why.message
        );
        assert_eq!(
            leftovers_beside(&at),
            Vec::<String>::new(),
            "and the partial file it refused over is gone"
        );
    }

    #[test]
    fn a_rewriter_that_refuses_says_so_without_a_disk_error_in_front_of_it() {
        // A refusal from the writer is about the *document* --- a page the plan
        // names that the file does not have --- and `stage` passes it through
        // rather than wrapping it. Wrapping would report a parse failure as a
        // disk failure, and send the reader looking at their filesystem.
        //
        // The `changed` half is the one that decides whether Reload is offered,
        // and it has to survive this path as well as the pipe: a refusal that
        // arrives correct from the worker and is flattened here reaches the
        // reader as a sentence with no action attached.
        let scratch = Scratch::new("rewrite-refuses");
        let (at, plan) = staging_subject(&scratch, "refused.pdf").expect("a subject");
        let writer = FakeWriter::writing(Err(Refusal::changed(
            "the edits name page 9, which this document does not have",
        )));

        let why = stage_in_place(&at, &plan, None, &writer).expect_err("must refuse");

        assert_eq!(
            why.message, "the edits name page 9, which this document does not have",
            "the writer's own words, not a wrapper's"
        );
        assert!(why.changed, "and the offer of Reload with them");
        assert_eq!(
            leftovers_beside(&at),
            Vec::<String>::new(),
            "and nothing is left beside the document"
        );
    }

    #[test]
    fn a_file_that_changed_is_refused_before_a_staging_file_exists() {
        // **The free half of the split, and the reason `rewrite_ready` is a
        // separate function.** Every refusal about the *document* now arrives
        // after the temporary file has been created, because the writer needs
        // somewhere to write before it can find anything wrong. The refusal
        // about the *file* does not, and must not: it is answerable by reloading,
        // and a reader who reloads should not find a partial copy of their
        // document beside it under a name they never chose.
        //
        // The writer here would succeed. It is never reached, which is the
        // assertion.
        let scratch = Scratch::new("rewrite-changed-first");
        let (at, plan) = staging_subject(&scratch, "moved.pdf").expect("a subject");
        std::fs::write(&at, b"%PDF-1.7 something else entirely, and longer\n")
            .expect("change it underneath");
        let writer = FakeWriter::writing(Ok(b"%PDF-1.7 rewritten".to_vec()));

        let why = stage_in_place(&at, &plan, None, &writer).expect_err("must refuse");

        assert!(why.changed, "answerable by reloading: {}", why.message);
        assert!(
            writer.asked.borrow().is_empty(),
            "the writer was never asked, so nothing parsed anything"
        );
        assert_eq!(
            leftovers_beside(&at),
            Vec::<String>::new(),
            "and no staging file was ever created"
        );
    }

    #[test]
    fn the_re_read_is_asked_for_the_length_the_save_produced() {
        // **The `len` argument has no failing case under `Here`**, which is the
        // reason this exists. `Here` passes it to `read_whole` as a capacity
        // hint, and a capacity that is wrong costs an allocation and changes no
        // answer --- so every test above would pass with that term computed any
        // way at all. It is the *map* length for a worker, where being wrong
        // means verifying a prefix of the file, or refusing to map it.
        //
        // So the term is pinned here against the two numbers it is made of,
        // rather than left to be discovered by the implementation that cannot
        // shrug it off.
        let scratch = Scratch::new("append-asks-for-length");
        let Some((at, plan)) = appendable(&scratch, "comments.pdf") else {
            println!("[SKIP] comments.pdf: fixture not generated");
            return;
        };
        let appended = append_bytes(&at, &plan, None).expect("build the update");
        let want =
            usize::try_from(appended.was).expect("a length that fits") + appended.update.len();
        let fake = Fake::saying(Ok(appended.pages));

        append_in_place(&appended, &at, None, &fake).expect("append");

        assert_eq!(
            fake.asked.borrow().as_slice(),
            &[(want, None)],
            "the file as it was, plus what was appended to it --- and no password for a plain document"
        );
        assert_eq!(
            std::fs::metadata(&at).expect("stat").len(),
            want as u64,
            "and that is the length the file actually has, so the two cannot drift apart quietly"
        );
    }

    #[test]
    fn an_append_that_parses_and_has_lost_pages_is_also_put_back() {
        // **Written because a mutation survived.** The verification has two
        // failing arms --- the file does not parse, and the file parses with the
        // wrong number of pages --- and the rollback test above reaches only the
        // first: it plants a trailer pointing at nothing, so `Document::load`
        // errors and the count is never compared. Replacing the count comparison
        // with `Ok(_) => Ok(())` passed every test in this module.
        //
        // So the update section here is a *real* one, built by `lopdf` from the
        // fixture and complete enough to parse, whose catalog names an empty page
        // tree. That is what a mis-chained cross-reference looks like when it
        // happens to land on something readable, and it is the outcome worth
        // refusing: a file that opens, and is empty.
        let scratch = Scratch::new("append-empty");
        let Some((at, plan)) = appendable(&scratch, "comments.pdf") else {
            println!("[SKIP] comments.pdf: fixture not generated");
            return;
        };
        let before = std::fs::read(&at).expect("read before");
        let mut appended = append_bytes(&at, &plan, None).expect("build the update");

        // A second revision over the same file, which replaces the catalog's
        // /Pages with a tree that has no kids.
        let original = std::fs::read(&at).expect("read");
        let prev = Document::load_mem(&original).expect("parse");
        let catalog = prev
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .expect("a catalog");
        let mut incremental = IncrementalDocument::create_from(original, prev);
        let empty = incremental.new_document.add_object(dictionary! {
            "Type" => "Pages",
            "Kids" => Object::Array(Vec::new()),
            "Count" => 0,
        });
        incremental
            .opt_clone_object_to_new_document(catalog)
            .expect("bring the catalog across");
        incremental
            .new_document
            .get_object_mut(catalog)
            .and_then(Object::as_dict_mut)
            .expect("the catalog is a dictionary")
            .set("Pages", Object::Reference(empty));
        let mut sink = Tail {
            skip: before.len(),
            seen: 0,
            tail: Vec::new(),
        };
        incremental
            .save_to(&mut sink)
            .expect("build the bad update");
        appended.update = sink.tail;

        let refused = append_in_place(&appended, &at, None, &Here).expect_err("the count refuses");
        assert!(refused.contains("page(s) and should have"), "{refused}");
        assert!(refused.contains("put back"), "{refused}");
        assert_eq!(
            std::fs::read(&at).expect("read after"),
            before,
            "the file is exactly what it was"
        );
    }

    #[test]
    fn an_append_is_refused_for_a_plan_that_needs_a_rewrite() {
        // `mode_for` is what chooses, so this is unreachable from the command ---
        // and it is the guard that stops a future caller getting it wrong
        // quietly, by writing an update section for an edit an update section
        // cannot express.
        let scratch = Scratch::new("append-wrong-mode");
        let Some((at, mut plan)) = appendable(&scratch, "comments.pdf") else {
            println!("[SKIP] comments.pdf: fixture not generated");
            return;
        };
        let before = std::fs::read(&at).expect("read before");
        plan.pages[0].turns = 1;

        let refused = append_bytes(&at, &plan, None).expect_err("refused");
        assert!(
            refused.message.contains("full rewrite"),
            "{}",
            refused.message
        );
        assert_eq!(
            std::fs::read(&at).expect("read"),
            before,
            "and wrote nothing"
        );
    }

    #[test]
    fn an_append_is_refused_when_the_file_changed_since_it_was_opened() {
        // The same guard `stage_in_place` has, and it has to be here too: the
        // two paths no longer share a function, so a refusal written once is a
        // refusal on one of them.
        let scratch = Scratch::new("append-changed");
        let Some((at, plan)) = appendable(&scratch, "comments.pdf") else {
            println!("[SKIP] comments.pdf: fixture not generated");
            return;
        };
        {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&at)
                .expect("open");
            file.write_all(b"% changed under the reader\n")
                .expect("write");
        }
        let meddled = std::fs::read(&at).expect("read");

        let refused = append_bytes(&at, &plan, None).expect_err("refused");
        assert!(
            refused.changed,
            "it is a changed-file refusal: {}",
            refused.message
        );
        assert_eq!(
            std::fs::read(&at).expect("read"),
            meddled,
            "and wrote nothing"
        );
    }

    #[test]
    fn a_third_parser_reads_an_appended_document() {
        // **The append's own third parser.** `lopdf` wrote the update and
        // `lopdf` verifies it inside `append_in_place`, which is a writer
        // agreeing with its own reader --- enough to catch a mis-chained
        // cross-reference and not enough to say the file is one other software
        // will open. Spike 0.6 put this to four parsers; this is the one of them
        // that is linked into the test binary.
        let scratch = Scratch::new("append-third");
        let mut examined = 0;
        for name in ["text-heavy.pdf", "rotated.pdf", "links.pdf", "mixed.pdf"] {
            let Some((at, plan)) = appendable(&scratch, name) else {
                println!("[SKIP] {name}: fixture not generated");
                continue;
            };
            let source = std::fs::read(&at).expect("read source");
            let Some(before) = os_pdf::read(&source) else {
                println!("[SKIP] {name}: the OS parser refused the source document");
                continue;
            };

            let appended = append_bytes(&at, &plan, None).expect("build the update");
            append_in_place(&appended, &at, None, &Here).expect("append");

            let after = os_pdf::read(&std::fs::read(&at).expect("read after"))
                .expect("the OS parser reads the appended document");
            assert_eq!(
                after.pages.len(),
                before.pages.len(),
                "{name}: every page survives"
            );
            assert_eq!(
                after.pages.iter().map(|p| p.rotation).collect::<Vec<_>>(),
                before.pages.iter().map(|p| p.rotation).collect::<Vec<_>>(),
                "{name}: and each at the rotation it had --- an append changes no page"
            );
            examined += 1;
        }
        assert!(examined > 0, "no fixture was examined");
    }

    /// What a rewrite costs in memory, which is what decides where it can run.
    ///
    /// **Measured because a design rested on it.** Moving the rewrite into the
    /// worker means the worker holds the serialised document, and a Windows
    /// worker is capped at `sandbox_win::WORKER_MEMORY_CAP` --- 1 GB of commit.
    /// Whether a rewrite of the largest fixture fits under that is the whole
    /// question, and reasoning about it from the file size would have been a
    /// guess: `lopdf` holds the parsed object graph *and* the output buffer, and
    /// neither is the file's length.
    ///
    /// Reports the process footprint before and after, which on macOS excludes
    /// clean file-backed pages --- so what it shows is what this rewrite made
    /// dirty rather than what the fixture weighs on disk.
    ///
    /// ```text
    /// cargo test --release --manifest-path src-tauri/Cargo.toml \
    ///     -- --ignored --nocapture bench_rewrite_footprint
    /// ```
    #[test]
    #[ignore]
    fn bench_rewrite_footprint() {
        let me = std::process::id();
        for name in ["text-heavy.pdf", "incr-scan-5p.pdf", "incr-scan-40p.pdf"] {
            let Some(path) = fixture(name) else {
                println!("[SKIP] {name}: fixture not generated");
                continue;
            };
            let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let count = page_count(&path);
            let before = crate::worker::phys_footprint(me).unwrap_or(0);

            // **The parse on its own first**, because the two terms have to be
            // separated to choose a design. A worker that *streams* its output
            // holds the object graph and never a full output buffer; a worker
            // that hands one back holds both. Measuring only the pair cannot
            // tell those apart, and the first version of this bench did exactly
            // that and was read as ruling out streaming.
            let source = std::fs::read(&path).expect("read");
            let parsed = Document::load_mem_with_options(
                &source,
                lopdf::LoadOptions {
                    max_decompressed_size: Some(MAX_DECODE),
                    ..Default::default()
                },
            )
            .expect("parse");
            let graph = crate::worker::phys_footprint(me).unwrap_or(0);
            drop(parsed);
            drop(source);

            let plan = plan_opened_as(&vec![0u8; count], &path);
            let started = std::time::Instant::now();
            let built = planned_bytes(&path, &plan, OnChange::Refuse, NO_VIEW_TURN, None)
                .expect("rewrite the document");
            let took = started.elapsed();
            let peak = crate::worker::phys_footprint(me).unwrap_or(0);

            // **Absolute footprints, not deltas, and that is a correction.**
            // The first version printed `saturating_sub(before)` and reported
            // **+0.0 MB** for reading and parsing a 337 MB file --- which is not
            // a measurement, it is a *negative* delta clamped to zero. The
            // baseline moves between iterations: `phys_footprint` is what the
            // process holds now, the allocator does not return everything at
            // `drop`, and a later iteration can start above where an earlier one
            // ended. A clamp turned "the baseline moved" into "this cost
            // nothing", which is the more reassuring of the two readings and the
            // wrong one. Printed absolute, a decrease is visible as a decrease.
            println!(
                "[bench] {name:<20} file {bytes:>10} B | out {:>10} B | footprint \
                 idle {:>7.1} -> parsed {:>7.1} -> rewritten {:>7.1} MB | {:>7.1} ms",
                built.bytes.len(),
                before as f64 / 1e6,
                graph as f64 / 1e6,
                peak as f64 / 1e6,
                took.as_secs_f64() * 1e3,
            );
            drop(built);
        }
        // Named rather than read: `sandbox_win` is `#[cfg(windows)]`, so a Mac
        // cannot ask it. Written as the constant's value with its name beside
        // it, so a reader can check the one against the other -- which is the
        // whole of what a number in a comment can offer.
        println!(
            "[bench] a Windows worker is capped at 1 GB of commit (sandbox_win::WORKER_MEMORY_CAP)"
        );
    }

    /// What an append costs against a rewrite, and where a save's time goes.
    ///
    /// `#[ignore]`, so it runs only when asked --- it copies a 337 MB fixture
    /// three times. Kept beside the code rather than as an example because the
    /// numbers it produces are what decided the mode's design, and a measurement
    /// nobody can re-run is a claim.
    ///
    /// ```text
    /// cargo test --release --lib save::tests::bench_append -- --ignored --nocapture
    /// ```
    ///
    /// **Measured 2026-08-22, release, M5 MacBook Pro, warm page cache.** The
    /// A/B is interleaved per round rather than run as two blocks, which is this
    /// repository's standing rule, and the best of three is reported because
    /// what is being compared is the work rather than the scheduling noise.
    ///
    /// | fixture | size | append | bytes | rewrite | bytes | ratio |
    /// |---|---|---|---|---|---|---|
    /// | text-heavy   | 1.4 MB | 13.4 ms | 867 | 5.8 ms  | 1,345,132 | 0.4x |
    /// | scan, 5 pages  | 42 MB  | 89.8 ms | 824 | 84.4 ms | 42,078,652 | 0.9x |
    /// | scan, 20 pages | 168 MB | 336.9 ms | 830 | 344.9 ms | 168,312,340 | 1.0x |
    /// | scan, 40 pages | 337 MB | 667.2 ms | 839 | 739.2 ms | 336,624,052 | 1.1x |
    ///
    /// **The wall-clock claim in `docs/PLAN.md` §5 does not survive this, and
    /// the bytes-written claim survives it completely.** §5 records 8.2x at
    /// 337 MB. What it measured is the *writer* in isolation; what a reader
    /// waits for is a save, and a save is dominated by something neither mode
    /// chooses: the open-time fingerprint's streamed SHA-256 of the whole file,
    /// which this run times separately at **603 ms of the append's 667**. Both
    /// modes pay it, so the mode moves about 64 ms of a 670 ms save.
    ///
    /// The rest of the append's own cost is 21 ms reading the file, 6 ms parsing
    /// it and 43 ms parsing it again to verify the result --- a check the rewrite
    /// does not perform at all, since it verifies the *source* before a rename
    /// rather than the file it produced.
    ///
    /// So the reason to append is what it writes: **839 bytes rather than 337
    /// megabytes**, which matters for a document in a synced folder, for the
    /// life of the disk, and because the previous revision survives byte for
    /// byte inside the new file. It is not the speed, and this file should not
    /// be read as claiming it is.
    #[test]
    #[ignore]
    fn bench_append_against_rewrite() {
        for name in [
            "text-heavy.pdf",
            "incr-scan-5p.pdf",
            "incr-scan-20p.pdf",
            "incr-scan-40p.pdf",
        ] {
            let scratch = Scratch::new("bench");
            let Some((at, plan)) = appendable(&scratch, name) else {
                println!("[SKIP] {name}: fixture not generated");
                continue;
            };
            let size = std::fs::metadata(&at).expect("stat").len();
            let out = scratch.join("rewritten.pdf");
            let mut appends: Vec<(f64, usize)> = Vec::new();
            let mut rewrites: Vec<(f64, usize)> = Vec::new();
            for round in 0..3 {
                // A fresh copy per round: an append changes the file, so a
                // second round over the same one would be measuring a document
                // with a revision already on it.
                let fresh = scratch.join(&format!("round-{round}.pdf"));
                std::fs::copy(&at, &fresh).expect("copy");
                let mut this = plan.clone();
                this.opened_as =
                    Some(crate::fingerprint::Fingerprint::of(&fresh).expect("fingerprint"));

                let clock = std::time::Instant::now();
                let update = append_bytes(&fresh, &this, None).expect("build the update");
                let added = update.len();
                append_in_place(&update, &fresh, None, &Here).expect("append");
                appends.push((clock.elapsed().as_secs_f64() * 1000.0, added));

                let clock = std::time::Instant::now();
                let whole = planned_bytes(&at, &plan, OnChange::Proceed, NO_VIEW_TURN, None)
                    .expect("rewrite");
                let wrote = whole.bytes.len();
                write_atomically(&out, &whole.bytes).expect("write");
                rewrites.push((clock.elapsed().as_secs_f64() * 1000.0, wrote));

                let _ = std::fs::remove_file(&fresh);
            }

            // The fingerprint on its own, because it is most of both numbers and
            // is the finding rather than an aside.
            let clock = std::time::Instant::now();
            let _ = crate::fingerprint::Fingerprint::of(&at).expect("fingerprint");
            let hashing = clock.elapsed().as_secs_f64() * 1000.0;

            let best =
                |runs: &[(f64, usize)]| runs.iter().map(|run| run.0).fold(f64::MAX, f64::min);
            println!(
                "[bench] {name:18} {size:>10} B | append {:>7.1} ms {:>7} B | \
                 rewrite {:>7.1} ms {:>12} B | {:.1}x | fingerprint {hashing:.1} ms",
                best(&appends),
                appends[0].1,
                best(&rewrites),
                rewrites[0].1,
                best(&rewrites) / best(&appends),
            );
        }
    }

    fn one_quad() -> Vec<crate::docmodel::Quad> {
        vec![crate::docmodel::Quad {
            left: 72.0,
            top: 100.0,
            right: 300.0,
            bottom: 118.0,
        }]
    }

    /// The subtypes a saved page **lists** in its own `/Annots`, in order.
    ///
    /// **Not a scan of the file's objects**, which is what this was first
    /// written as --- and a mutation that cleared the page's array before
    /// appending survived it, because the annotation the array used to name is
    /// still an object in the file. An orphaned annotation is on no page and is
    /// reported by every reader as absent, so counting objects answers a
    /// question nobody is asking.
    fn listed_on_page(path: &Path, page: usize) -> Vec<String> {
        listed_on_page_of(path, page, None)
    }

    /// [`listed_on_page`] for a document that needs a password to be read.
    ///
    /// Same reason as [`appendable_with`]: without the key `lopdf` parses no
    /// objects, so `ordered_pages` is empty and the index below panics --- which
    /// reads as a save that lost every page rather than as a reader that could
    /// not look.
    fn listed_on_page_of(path: &Path, page: usize, password: Option<&str>) -> Vec<String> {
        let doc = Document::load_with_options(
            path,
            lopdf::LoadOptions {
                password: password.map(str::to_string),
                ..Default::default()
            },
        )
        .expect("reopen");
        let id = ordered_pages(&doc)[page];
        let entry = doc
            .get_object(id)
            .and_then(Object::as_dict)
            .expect("a page dictionary")
            .get(b"Annots")
            .cloned();
        let array = match entry {
            Ok(Object::Array(array)) => array,
            Ok(Object::Reference(at)) => doc
                .get_object(at)
                .and_then(Object::as_array)
                .expect("an /Annots reference points at an array")
                .clone(),
            Ok(other) => panic!("/Annots is neither an array nor a reference: {other:?}"),
            Err(_) => Vec::new(),
        };
        array
            .iter()
            .map(|item| {
                let dictionary = match item {
                    Object::Reference(at) => doc
                        .get_object(*at)
                        .and_then(Object::as_dict)
                        .expect("an /Annots entry points at a dictionary"),
                    Object::Dictionary(inline) => inline,
                    other => panic!("an /Annots entry is neither: {other:?}"),
                };
                let subtype = dictionary
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .expect("an annotation has a /Subtype");
                String::from_utf8_lossy(subtype).to_string()
            })
            .collect()
    }

    #[test]
    fn a_mark_is_written_whatever_shape_the_page_s_annots_is_in() {
        for shape in [AnnotShape::Absent, AnnotShape::Inline, AnnotShape::Indirect] {
            let scratch = Scratch::new(&format!("annots-{shape:?}"));
            let source = scratch.join("in.pdf");
            let out = scratch.join("out.pdf");
            std::fs::write(&source, document_with_annots(shape)).expect("write fixture");

            write_copy(&source, &plan_with_mark(one_quad()), &out, None)
                .unwrap_or_else(|e| panic!("{shape:?}: {e}"));

            // What the *page* lists, in order. The comment that was already
            // there must still be first and the mark must be appended after it:
            // an `/Annots` replaced rather than extended loses the first, and
            // one written in the wrong order would put a new highlight above
            // comments the document came with.
            let listed = listed_on_page(&out, 0);
            let expected: Vec<&str> = match shape {
                AnnotShape::Absent => vec!["Highlight"],
                _ => vec!["Text", "Highlight"],
            };
            assert_eq!(listed, expected, "{shape:?}: the page lists {listed:?}");
        }
    }

    #[test]
    fn a_marked_page_lists_the_mark_in_its_own_annots() {
        // Written into the *page*, not merely into the file. An annotation
        // object nothing points at is in the document and on no page, which
        // every reader reports as a document with no comments -- and which every
        // assertion counting objects would pass.
        let scratch = Scratch::new("annots-reachable");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");
        write_copy(&source, &plan_with_mark(one_quad()), &out, None).expect("save");

        let doc = Document::load(&out).expect("reopen");
        let page = ordered_pages(&doc)[0];
        let listed = doc
            .get_object(page)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Annots"))
            .cloned()
            .expect("the page has an /Annots");
        let array = match listed {
            Object::Array(array) => array,
            Object::Reference(id) => doc
                .get_object(id)
                .and_then(Object::as_array)
                .expect("an /Annots reference points at an array")
                .clone(),
            other => panic!("/Annots is neither an array nor a reference: {other:?}"),
        };
        assert_eq!(array.len(), 1);
    }

    /// A mark on a page the reader also rotated lands where they put it.
    ///
    /// **The behaviour [`MarksWritten`] protects, and nothing covered it.** The
    /// ordering in [`rewrite`] carried a comment saying *"the order is
    /// load-bearing rather than tidy: a mark was made against the rotation the
    /// file had when it was opened, and the mapping below reads the rotation the
    /// file has now. Turn the page first and every quad is a quarter turn out, on
    /// exactly the pages a reader rotated."* Twelve mark tests, and not one of
    /// them turned a page.
    ///
    /// The type now makes the inversion unwriteable, so there is no mutation to
    /// pair with this --- `docs/TRAPS.md` records that a guard the type system
    /// makes unexpressible has no mutation to write, and that weakening the code
    /// to have one is the wrong trade. What a test can still do is pin the
    /// *behaviour*, so a future restructuring that keeps the token and moves the
    /// work is caught.
    ///
    /// **The assertion is an equality between two saves, not a transcribed
    /// number.** A mark's position in page space does not depend on how the
    /// reader later turned the view: the page's content did not move, only the
    /// angle it is displayed at. So the same mark saved with a turn and without
    /// one must produce the same `/QuadPoints`, and no coordinate has to be
    /// written down here --- which matters, because a transcribed coordinate is
    /// how this repository has already had a test pin a value it could not
    /// justify.
    #[test]
    fn a_mark_on_a_page_the_reader_turned_is_placed_by_the_rotation_they_made_it_against() {
        let quads = |source: &std::path::Path, out: &std::path::Path, turns: u8| -> Vec<f32> {
            let mut plan = plan_with_mark(one_quad());
            plan.pages[0].turns = turns;
            write_copy(source, &plan, out, None).expect("save");
            let doc = Document::load(out).expect("reopen");
            let page = ordered_pages(&doc)[0];
            let annots = doc
                .get_object(page)
                .and_then(Object::as_dict)
                .and_then(|d| d.get(b"Annots"))
                .cloned()
                .expect("the page has an /Annots");
            let array = match annots {
                Object::Array(array) => array,
                Object::Reference(id) => doc
                    .get_object(id)
                    .and_then(Object::as_array)
                    .expect("an /Annots reference points at an array")
                    .clone(),
                other => panic!("/Annots is neither an array nor a reference: {other:?}"),
            };
            let mark = array[0].as_reference().expect("an annotation reference");
            doc.get_object(mark)
                .and_then(Object::as_dict)
                .and_then(|d| d.get(b"QuadPoints"))
                .and_then(Object::as_array)
                .expect("a highlight states its quads")
                .iter()
                .filter_map(|value| value.as_float().ok())
                .collect::<Vec<f32>>()
        };

        let scratch = Scratch::new("mark-on-turned-page");
        let source = scratch.join("in.pdf");
        std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");

        let straight = quads(&source, &scratch.join("straight.pdf"), 0);
        let turned = quads(&source, &scratch.join("turned.pdf"), 1);

        // The control on the reading itself: an empty list would compare equal to
        // an empty list, and this assertion would hold on a save that wrote no
        // quads at all.
        assert_eq!(straight.len(), 8, "a highlight has one quad of four points");
        assert_eq!(
            turned, straight,
            "a quarter turn of the view must not move the mark in the page's own space"
        );
    }

    #[test]
    fn a_mark_on_a_page_two_numbers_share_is_refused() {
        // The same refusal `unshared` makes for a deletion, one level on: an
        // annotation hangs off a page *object*, so a mark made on page 1 would
        // appear on page 2 as well. `docs/TRAPS.md` records this shape twice
        // already, once live in `print.rs` for months.
        let scratch = Scratch::new("annots-shared");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, shared_page_document()).expect("write fixture");

        let plan = Plan {
            opened_as: None,
            baseline: 2,
            pages: vec![
                PageView {
                    id: 1,
                    source: 0,
                    turns: 0,
                    crop: None,
                },
                PageView {
                    id: 2,
                    source: 1,
                    turns: 0,
                    crop: None,
                },
            ],
            redactions: Vec::new(),
            notes: Vec::new(),
            marks: vec![PlannedMark {
                kind: MarkKind::Highlight,
                stamp: None,
                source: 0,
                quads: one_quad(),
                strokes: Vec::new(),
                color: [1.0, 0.9, 0.2],
                author: String::new(),
                note: String::new(),
                made: "D:20260818120000Z".to_string(),
            }],
        };
        let why =
            write_copy(&source, &plan, &out, None).expect_err("a shared page must be refused");
        assert!(
            why.message.contains("same page object"),
            "the refusal does not say why: {why}"
        );
        assert!(!out.exists(), "a refused save left a file behind");
    }

    #[test]
    fn a_mark_on_an_unshared_page_of_a_document_that_has_a_shared_one_is_written() {
        // The control for the refusal above, and the first version of it could
        // not run: it kept one of the two shared numbers, which `unshared`
        // refuses first for the deletion that implies -- so it exercised the
        // deletion guard and never reached the mark guard at all.
        //
        // This one keeps every page of a document where pages 1 and 2 are one
        // object and page 3 is its own, and marks page 3. A guard written as
        // "this file contains a shared page" rather than "this mark's page is
        // shared" would refuse it, and a reader would be told they cannot
        // highlight a page that has nothing to do with the malformed one.
        let scratch = Scratch::new("annots-shared-spare");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, shared_page_and_a_spare()).expect("write fixture");

        let plan = Plan {
            opened_as: None,
            baseline: 3,
            pages: (0..3)
                .map(|source| PageView {
                    id: u64::from(source) + 1,
                    source,
                    turns: 0,
                    crop: None,
                })
                .collect(),
            redactions: Vec::new(),
            notes: Vec::new(),
            marks: vec![PlannedMark {
                kind: MarkKind::Highlight,
                stamp: None,
                source: 2,
                quads: one_quad(),
                strokes: Vec::new(),
                color: [1.0, 0.9, 0.2],
                author: String::new(),
                note: String::new(),
                made: "D:20260818120000Z".to_string(),
            }],
        };
        write_copy(&source, &plan, &out, None).expect("a mark on the unshared page is fine");
        assert_eq!(listed_on_page(&out, 2), vec!["Highlight".to_string()]);
        // And nowhere else: a writer that put the mark on the first page it
        // found would satisfy the line above on a one-page document and is
        // exactly what this three-page fixture is for.
        assert!(listed_on_page(&out, 0).is_empty());
    }

    #[test]
    fn a_mark_whose_quads_all_collapse_is_refused_rather_than_written_empty() {
        let scratch = Scratch::new("annots-empty");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");

        let flat = vec![crate::docmodel::Quad {
            left: 72.0,
            top: 100.0,
            right: 72.0,
            bottom: 118.0,
        }];
        let why = write_copy(&source, &plan_with_mark(flat), &out, None)
            .expect_err("a mark covering nothing must be refused");
        assert!(why.message.contains("no area"), "{why}");
    }

    #[test]
    fn a_plan_carrying_a_mark_is_not_the_file_on_disk() {
        // `is_identity` is what lets the print path hand the original bytes over
        // untouched. A plan with a mark in it must never qualify, or a reader
        // prints a highlighted document and gets an unhighlighted one -- with
        // nothing failing, because the file it printed is a perfectly good file.
        let plain = Plan {
            opened_as: None,
            baseline: 1,
            pages: vec![PageView {
                id: 1,
                source: 0,
                turns: 0,
                crop: None,
            }],
            redactions: Vec::new(),
            notes: Vec::new(),
            marks: Vec::new(),
        };
        assert!(plain.is_identity());
        assert!(!plan_with_mark(one_quad()).is_identity());
    }

    /// A plan that only redacts is not the file, and is never an append.
    ///
    /// **The two predicates that could ship an unredacted file**, and neither
    /// mentions a redaction unless somebody adds the clause. `is_identity` is
    /// what lets the print path hand the original bytes over; a plan with a
    /// redaction answering `true` there would produce a "redacted" print of the
    /// document with every word in it. `is_appendable` is what routes a save
    /// to the append, which writes an update section and never touches a content
    /// stream --- so the same plan answering `true` there writes a file that has
    /// been added to and had nothing taken out.
    ///
    /// Both are reached with **no other edit at all**, which is the case that
    /// matters: a reader who opens a document, drags one region and redacts has
    /// changed nothing else, so every other clause of both predicates is
    /// satisfied and only the new one can refuse.
    #[test]
    fn a_plan_that_only_redacts_is_neither_the_file_nor_an_append() {
        let mut plan = Plan {
            opened_as: None,
            baseline: 1,
            pages: vec![PageView {
                id: 1,
                source: 0,
                turns: 0,
                crop: None,
            }],
            redactions: Vec::new(),
            notes: Vec::new(),
            marks: Vec::new(),
        };
        assert!(plan.is_identity(), "the control: nothing is edited");
        plan.redactions = vec![crate::edits::PlannedRedaction {
            source: 0,
            shows: vec![0],
            text_objects: 4,
            areas: Vec::new(),
            taking: Vec::new(),
            form_shows: Vec::new(),
            form_text_objects: Vec::new(),
            images: Vec::new(),
            image_objects: 0,
        }];
        assert!(
            !plan.is_identity(),
            "a redaction is a change the file does not have"
        );
        assert!(
            !plan.is_appendable(),
            "and it is not something an append could do"
        );
        assert_eq!(
            mode_for(&plan, 1_000),
            Mode::Rewrite,
            "so a save carrying one takes the rewrite whatever the file's size"
        );

        // **A mark AND a redaction**, which is the only input where the
        // redaction clause of `is_appendable` decides anything. Without it the
        // predicate is short-circuited by the empty marks and a mutation
        // deleting the clause survived --- the trap about a guard whose
        // neighbour refuses the same input, arriving in the predicate that
        // routes a save to the append. A reader who highlights something and
        // also redacts is the case: an update section adds objects and never
        // touches a content stream, so that save would be written, be bigger,
        // and have nothing taken out of it.
        let mut both = plan_with_mark(one_quad());
        assert!(both.is_appendable(), "the control: a mark alone appends");
        both.redactions = vec![crate::edits::PlannedRedaction {
            source: 0,
            shows: vec![0],
            text_objects: 4,
            areas: Vec::new(),
            taking: Vec::new(),
            form_shows: Vec::new(),
            form_text_objects: Vec::new(),
            images: Vec::new(),
            image_objects: 0,
        }];
        assert!(
            !both.is_appendable(),
            "a mark beside a redaction is not an append"
        );
        assert!(!both.is_identity(), "and it is not the file either");
    }

    /// A page named twice by the redaction plan is refused, not removed twice.
    ///
    /// The second call would run against a stream the first had already changed,
    /// so its ordinals would name different operators. `remove_shows` has a
    /// guard of its own that would probably catch it --- which is not the same as
    /// this being safe, and it would report a correspondence failure for what is
    /// actually a caller's duplicate.
    #[test]
    fn a_page_named_twice_by_the_redaction_plan_is_refused() {
        let twice = vec![
            crate::edits::PlannedRedaction {
                source: 0,
                shows: vec![0],
                text_objects: 1,
                areas: Vec::new(),
                taking: Vec::new(),
                form_shows: Vec::new(),
                form_text_objects: Vec::new(),
                images: Vec::new(),
                image_objects: 0,
            },
            crate::edits::PlannedRedaction {
                source: 0,
                shows: vec![0],
                text_objects: 1,
                areas: Vec::new(),
                taking: Vec::new(),
                form_shows: Vec::new(),
                form_text_objects: Vec::new(),
                images: Vec::new(),
                image_objects: 0,
            },
        ];
        let mut doc = Document::with_version("1.7");
        let why = apply_redactions(&mut doc, &[(1, 0)], &twice)
            .expect_err("one page named twice must be refused");
        assert!(why.message.contains("named twice"), "{why}");
    }

    /// The annotation carrier, through the writer rather than through the walk.
    ///
    /// `redact::covered_annots` decides *which* annotations go and is tested
    /// there against rectangles a test wrote down. This is the other half, and
    /// it is the half a walk cannot answer: that the writer actually removes
    /// them, and removes the object rather than the one reference it had in
    /// mind. The control is the annotation away from the region --- a writer
    /// that emptied `/Annots` would satisfy the first assertion perfectly.
    #[test]
    fn an_annotation_over_a_redacted_region_is_removed_and_its_neighbour_is_not() {
        use lopdf::{dictionary, Stream};

        let mut doc = Document::with_version("1.7");
        let content = doc.add_object(Stream::new(dictionary! {}, b"BT (secret) Tj ET".to_vec()));
        let over = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![100.into(), 100.into(), 200.into(), 120.into()],
            "Contents" => Object::string_literal("about the secret"),
        });
        let away = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![400.into(), 400.into(), 500.into(), 420.into()],
            "Contents" => Object::string_literal("about something else"),
        });
        let pages_id = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Annots" => vec![Object::Reference(over), Object::Reference(away)],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
            }),
        );

        let done = apply_redactions(
            &mut doc,
            &[page],
            &[crate::edits::PlannedRedaction {
                source: 0,
                shows: vec![0],
                text_objects: 1,
                areas: vec![[90.0, 90.0, 210.0, 130.0]],
                taking: Vec::new(),
                form_shows: Vec::new(),
                form_text_objects: Vec::new(),
                images: Vec::new(),
                image_objects: 0,
            }],
        )
        .expect("the plan is applicable");

        assert_eq!(done.shows, 1);
        assert_eq!(done.annots, 1, "one annotation, not both and not none");
        assert!(
            doc.get_object(over).is_err(),
            "the annotation over the region is gone from the document"
        );
        assert!(
            doc.get_object(away).is_ok(),
            "and the reader's other comment is not"
        );
        // The reference as well as the object: an entry left on `/Annots`
        // pointing at nothing is a different defect wearing the same result.
        let Ok(Object::Array(entries)) = doc.get_dictionary(page).and_then(|d| d.get(b"Annots"))
        else {
            panic!("the page still has an /Annots array")
        };
        assert_eq!(entries.len(), 1, "{entries:?}");
    }

    /// The reference the caller was not thinking of, which is the whole reason
    /// this goes through `pagetree::forget`.
    ///
    /// A page's `/Annots` is one of several places an annotation is named. A
    /// structure element's `/OBJR`, an AcroForm's `/Fields` and another
    /// annotation's `/IRT` all name it too, and an object still reachable is an
    /// object still written --- so pruning the one array a caller has in mind
    /// removes the annotation from the *page* and leaves the comment in the
    /// *file*. This plants a second reference in the catalog and asserts both
    /// ends: the object is gone, and nothing still points at it.
    #[test]
    fn a_redacted_annotation_loses_the_references_that_are_not_on_the_page() {
        use lopdf::{dictionary, Stream};

        let mut doc = Document::with_version("1.7");
        let content = doc.add_object(Stream::new(dictionary! {}, b"BT (secret) Tj ET".to_vec()));
        let over = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "Rect" => vec![100.into(), 100.into(), 200.into(), 120.into()],
            "Contents" => Object::string_literal("about the secret"),
        });
        let pages_id = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Annots" => vec![Object::Reference(over)],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
            }),
        );
        // The second name for it: an AcroForm field list, which is exactly where
        // a widget annotation is named twice in every form in existence.
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "AcroForm" => dictionary! { "Fields" => vec![Object::Reference(over)] },
        });
        doc.trailer.set("Root", catalog);

        apply_redactions(
            &mut doc,
            &[page],
            &[crate::edits::PlannedRedaction {
                source: 0,
                shows: vec![0],
                text_objects: 1,
                areas: vec![[90.0, 90.0, 210.0, 130.0]],
                taking: Vec::new(),
                form_shows: Vec::new(),
                form_text_objects: Vec::new(),
                images: Vec::new(),
                image_objects: 0,
            }],
        )
        .expect("the plan is applicable");

        assert!(doc.get_object(over).is_err(), "the object is gone");
        let Ok(form) = doc
            .get_dictionary(catalog)
            .and_then(|root| root.get(b"AcroForm"))
            .and_then(Object::as_dict)
        else {
            panic!("the catalog still has an /AcroForm")
        };
        let Ok(Object::Array(fields)) = form.get(b"Fields") else {
            panic!("/Fields is still an array")
        };
        assert!(
            fields.is_empty(),
            "the other reference to it went too: {fields:?}"
        );
    }

    /// A redaction takes the document's own description of itself.
    ///
    /// §6's carrier table names XMP and DocInfo at document level, and a title
    /// or a subject routinely restates what the document is about. Both go, and
    /// the objects go with the references.
    #[test]
    fn a_redaction_removes_the_documents_own_description_of_itself() {
        let (mut doc, page, info, metadata) = described_document();
        let done = apply_redactions(&mut doc, &[page], &redaction_of(page)).expect("applicable");
        assert_eq!(done.metadata, 2, "both were there");
        assert!(doc.get_object(info).is_err(), "/Info is gone");
        assert!(doc.get_object(metadata).is_err(), "/Metadata is gone");
        assert!(
            !doc.trailer.has(b"Info"),
            "and so is the trailer's name for it"
        );
        assert!(
            !doc.catalog().expect("catalog").has(b"Metadata"),
            "and the catalog's"
        );
    }

    /// **The control, and it is about every other save rather than this one.**
    ///
    /// `apply_redactions` runs on every rewrite, so a strip that did not ask
    /// whether anything was redacted would quietly take the title off every copy,
    /// extract, split and merge tpdf writes. §T6.1's position is that a copy is a
    /// serialisation and not a sanitation, and this is where that stays true.
    #[test]
    fn a_copy_that_is_not_a_redaction_keeps_its_metadata() {
        let (mut doc, page, info, metadata) = described_document();
        let done = apply_redactions(&mut doc, &[page], &[]).expect("applicable");
        assert_eq!(done.metadata, 0);
        assert!(
            doc.get_object(info).is_ok(),
            "/Info survives an ordinary save"
        );
        assert!(
            doc.get_object(metadata).is_ok(),
            "and so does the XMP packet"
        );
    }

    /// A document describing itself nowhere is not an error, and reports none.
    #[test]
    fn a_document_with_no_metadata_at_all_reports_none() {
        let (mut doc, page, info, metadata) = described_document();
        crate::pagetree::forget(&mut doc, &[info, metadata].into_iter().collect()).expect("strip");
        let done = apply_redactions(&mut doc, &[page], &redaction_of(page)).expect("applicable");
        assert_eq!(done.metadata, 0);
    }

    /// A redaction takes the outline entry that names what went.
    ///
    /// §6's *Document level* row, and the one carrier a reader can see in tpdf
    /// itself: the sidebar draws the outline, so a heading redacted off the page
    /// comes back on screen in the file that was supposed to have lost it.
    #[test]
    fn a_redaction_takes_the_outline_entry_naming_what_it_removed() {
        let (mut doc, page, chain) = outlined_document();
        let done = apply_redactions(&mut doc, &[page], &naming_the_secret(page)).expect("ok");
        assert_eq!(done.outline, 2, "the entry and the child under it");
        assert!(doc.get_object(chain.carrier).is_err(), "the entry is gone");
        assert!(doc.get_object(chain.child).is_err(), "and its subtree");
    }

    /// **The control that catches the linked-list defect.**
    ///
    /// `pagetree::forget` removes a dictionary key whose value names a doomed
    /// object, which is right for `/Info` and wrong for a sibling chain: it
    /// would take `/Next` off the entry *before* the removed one, so a reader
    /// walking `/First` then `/Next` stops there and never reaches what follows.
    /// The file stays valid and no parser complains.
    ///
    /// So the carrier sits in the middle on purpose and this asserts the entry
    /// **after** it is still reachable by walking, rather than merely still in
    /// `doc.objects` --- which it would be either way.
    ///
    /// **Named for the outline rather than for what it asserts**, and that is
    /// not cosmetic. It was first called
    /// `the_entries_around_a_removed_one_are_still_reachable_by_walking`, and a
    /// `cargo test outline` run --- the obvious way to exercise this group ---
    /// silently did not include it. The mutation that deletes the splice was
    /// then read as reddening one test when it reddens two, and the check
    /// written for exactly that defect looked incapable of failing. A filtered
    /// run is only as good as the names, which is why the mutation harness runs
    /// the whole suite.
    #[test]
    fn an_outline_removal_leaves_the_entries_around_it_reachable() {
        let (mut doc, page, chain) = outlined_document();
        apply_redactions(&mut doc, &[page], &naming_the_secret(page)).expect("ok");

        let mut walked = Vec::new();
        let mut at = doc
            .get_dictionary(chain.root)
            .and_then(|root| root.get(b"First"))
            .and_then(Object::as_reference)
            .ok();
        while let Some(id) = at {
            walked.push(id);
            assert!(walked.len() < 10, "the chain loops: {walked:?}");
            at = doc
                .get_dictionary(id)
                .and_then(|item| item.get(b"Next"))
                .and_then(Object::as_reference)
                .ok();
        }
        assert_eq!(
            walked,
            vec![chain.before, chain.after],
            "both survivors, in order, reached from /First"
        );
        assert_eq!(
            doc.get_dictionary(chain.after)
                .and_then(|item| item.get(b"Prev"))
                .and_then(Object::as_reference)
                .ok(),
            Some(chain.before),
            "/Prev names the entry that is now before it"
        );
        assert_eq!(
            doc.get_dictionary(chain.root)
                .and_then(|root| root.get(b"Last"))
                .and_then(Object::as_reference)
                .ok(),
            Some(chain.after),
            "the root still names its last child"
        );
    }

    /// `/Count` is recomputed rather than left saying what the outline was.
    ///
    /// The `/Size` shape from spike 0.4, one subsystem along: a stale count
    /// renders identically and is structurally wrong.
    #[test]
    fn a_removal_leaves_the_outline_counting_what_is_left() {
        let (mut doc, page, chain) = outlined_document();
        apply_redactions(&mut doc, &[page], &naming_the_secret(page)).expect("ok");
        assert_eq!(
            doc.get_dictionary(chain.root)
                .and_then(|root| root.get(b"Count"))
                .and_then(Object::as_i64)
                .ok(),
            Some(2),
            "four entries were visible and two are left"
        );
    }

    /// **The over-removal control.** An entry naming nothing that went stays.
    ///
    /// A rule that dropped the whole outline --- which is what a page deletion
    /// correctly does --- would pass every check above. One redacted heading
    /// must not cost a reader their table of contents.
    #[test]
    fn an_outline_entry_naming_something_else_survives_a_redaction() {
        let (mut doc, page, chain) = outlined_document();
        apply_redactions(&mut doc, &[page], &naming_the_secret(page)).expect("ok");
        assert!(doc.get_object(chain.before).is_ok(), "before it");
        assert!(doc.get_object(chain.after).is_ok(), "and after it");
        assert!(
            doc.catalog().expect("catalog").has(b"Outlines"),
            "and the document still has an outline at all"
        );
    }

    /// A copy that is not a redaction keeps every bookmark.
    ///
    /// The metadata control's twin, and it exists for the same reason: this runs
    /// on every rewrite, so without the guard an ordinary Save a copy would
    /// quietly lose the entry naming whatever the reader had happened to select.
    ///
    /// **Its mutation is the metadata one**, `save: strip metadata on every save
    /// rather than on a redaction`, because there is one condition guarding both
    /// and a mutation of it reddens both. That entry names its twin; a second
    /// entry with the same anchor and an equivalent replacement would be
    /// padding. The mutation written for this specifically was deleted: feeding
    /// `covered_outline` an empty needle makes it match *nothing*, which reddens
    /// the three removal checks and leaves this one exactly as green as a clean
    /// tree does --- an over-removal control cannot be proved by a mutation that
    /// under-removes.
    #[test]
    fn a_copy_that_is_not_a_redaction_keeps_its_outline() {
        let (mut doc, page, chain) = outlined_document();
        let done = apply_redactions(&mut doc, &[page], &[]).expect("applicable");
        assert_eq!(done.outline, 0);
        for id in [
            chain.root,
            chain.before,
            chain.carrier,
            chain.child,
            chain.after,
        ] {
            assert!(
                doc.get_object(id).is_ok(),
                "{id:?} survives an ordinary save"
            );
        }
    }

    /// A title too short to be distinctive is left alone.
    ///
    /// A bookmark called `1` is a substring of almost any line, so matching on
    /// it would take the outline off a document for the sake of a chapter
    /// number.
    #[test]
    fn a_very_short_outline_title_is_not_matched() {
        let (mut doc, page, chain) = outlined_document();
        if let Ok(Object::Dictionary(item)) = doc.get_object_mut(chain.carrier) {
            item.set("Title", Object::string_literal("re"));
        }
        let done = apply_redactions(&mut doc, &[page], &naming_the_secret(page)).expect("ok");
        assert_eq!(done.outline, 0, "nothing matched");
        assert!(doc.get_object(chain.carrier).is_ok());
    }

    /// The object ids `outlined_document` hands back.
    struct Chain {
        root: lopdf::ObjectId,
        before: lopdf::ObjectId,
        carrier: lopdf::ObjectId,
        child: lopdf::ObjectId,
        after: lopdf::ObjectId,
    }

    /// A document whose outline is four entries with the carrier in the middle.
    ///
    /// ```text
    /// /Outlines
    ///   OUTLINE-BEFORE
    ///   "the secret account"      <- a substring of what the redaction takes
    ///     OUTLINE-CHILD
    ///   OUTLINE-AFTER
    /// ```
    ///
    /// The carrier is the **second** of three siblings, which is the whole shape
    /// of the fixture: a removal that drops the object without splicing takes
    /// `/Next` off `OUTLINE-BEFORE`, and `OUTLINE-AFTER` becomes unreachable
    /// while every object is still present.
    fn outlined_document() -> (Document, lopdf::ObjectId, Chain) {
        use lopdf::{dictionary, Stream};

        let mut doc = Document::with_version("1.7");
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"BT (Holding the secret account here) Tj ET".to_vec(),
        ));
        let pages_id = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
            }),
        );

        let root = doc.new_object_id();
        let before = doc.new_object_id();
        let carrier = doc.new_object_id();
        let child = doc.new_object_id();
        let after = doc.new_object_id();
        doc.objects.insert(
            before,
            Object::Dictionary(dictionary! {
                "Title" => Object::string_literal("OUTLINE-BEFORE"),
                "Parent" => root,
                "Next" => carrier,
            }),
        );
        doc.objects.insert(
            carrier,
            Object::Dictionary(dictionary! {
                "Title" => Object::string_literal("the secret account"),
                "Parent" => root,
                "Prev" => before,
                "Next" => after,
                "First" => child,
                "Last" => child,
                "Count" => 1,
            }),
        );
        doc.objects.insert(
            child,
            Object::Dictionary(dictionary! {
                "Title" => Object::string_literal("OUTLINE-CHILD"),
                "Parent" => carrier,
            }),
        );
        doc.objects.insert(
            after,
            Object::Dictionary(dictionary! {
                "Title" => Object::string_literal("OUTLINE-AFTER"),
                "Parent" => root,
                "Prev" => carrier,
            }),
        );
        doc.objects.insert(
            root,
            Object::Dictionary(dictionary! {
                "Type" => "Outlines",
                "First" => before,
                "Last" => after,
                "Count" => 4,
            }),
        );

        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "Outlines" => root,
        });
        doc.trailer.set("Root", catalog);
        (
            doc,
            page,
            Chain {
                root,
                before,
                carrier,
                child,
                after,
            },
        )
    }

    /// A plan reporting that it took the line `outlined_document` draws.
    fn naming_the_secret(_page: lopdf::ObjectId) -> Vec<crate::edits::PlannedRedaction> {
        vec![crate::edits::PlannedRedaction {
            source: 0,
            shows: vec![0],
            text_objects: 1,
            areas: Vec::new(),
            taking: vec!["Holding the secret account here".to_string()],
            form_shows: Vec::new(),
            form_text_objects: Vec::new(),
            images: Vec::new(),
            image_objects: 0,
        }]
    }

    /// A redaction is refused outright on an XFA form.
    ///
    /// §6's rule since before any of this was written, and unread until
    /// 2026-08-27: an XFA packet is a complete second copy of every answer, so a
    /// redaction that took the field values and left it has removed nothing.
    #[test]
    fn a_redaction_of_an_xfa_form_is_refused_rather_than_half_done() {
        let (mut doc, page, ids) = formed_document();
        let form = ids
            .iter()
            .find(|(name, _)| *name == "/AcroForm")
            .map(|(_, id)| *id)
            .expect("the fixture has an /AcroForm");
        if let Ok(Object::Dictionary(dict)) = doc.get_object_mut(form) {
            dict.set("XFA", Object::string_literal("<xdp:xdp/>"));
        }
        let why = apply_redactions(&mut doc, &[page], &over_the_widget(page))
            .expect_err("an XFA form must be refused");
        assert!(why.message.contains("XFA"), "{}", why.message);
        // Nothing half-done: the refusal is in the pre-flight, so every widget
        // the plan would have taken is still there.
        for (name, id) in ids {
            assert!(doc.get_object(id).is_ok(), "{name} survives a refusal");
        }
    }

    /// **The control.** A copy of an XFA form is not a redaction and is written.
    ///
    /// §T6.1's position, and the reason the refusal is guarded: a serialisation
    /// makes no claim about what it removed, so there is nothing for XFA to
    /// falsify. Refusing here would make tpdf unable to open-and-save a whole
    /// class of document for the sake of a promise it is not making.
    #[test]
    fn a_copy_of_an_xfa_form_is_not_refused() {
        let (mut doc, page, ids) = formed_document();
        let form = ids
            .iter()
            .find(|(name, _)| *name == "/AcroForm")
            .map(|(_, id)| *id)
            .expect("the fixture has an /AcroForm");
        if let Ok(Object::Dictionary(dict)) = doc.get_object_mut(form) {
            dict.set("XFA", Object::string_literal("<xdp:xdp/>"));
        }
        apply_redactions(&mut doc, &[page], &[]).expect("a copy is not a redaction");
    }

    /// A field whose widgets have all gone goes with them.
    ///
    /// The gap measured before this was built: `covered_annots` removes a widget
    /// over a region because a widget is an annotation, and the field dictionary
    /// above it survives holding the value. Nothing draws it and every search
    /// finds it.
    ///
    /// **`orphan` is asserted first because it is the only subject this rule
    /// decides alone** --- its value names nothing that went, so a mutation
    /// disabling the rule can only show up here. `parent` is the realistic
    /// shape and both rules fire on it, which is why it cannot be the control.
    #[test]
    fn a_field_whose_widgets_all_went_does_not_keep_its_value() {
        let (mut doc, page, ids) = formed_document();
        let by = |want: &str| {
            ids.iter()
                .find(|(name, _)| *name == want)
                .map(|(_, id)| *id)
                .expect(want)
        };
        apply_redactions(&mut doc, &[page], &over_the_widget(page)).expect("ok");
        assert!(
            doc.get_object(by("its orphan widget")).is_err(),
            "the widget over the region went"
        );
        assert!(
            doc.get_object(by("orphan field")).is_err(),
            "and the field above it, though its value named nothing that went"
        );
        assert!(doc.get_object(by("its widget")).is_err(), "the widget went");
        assert!(
            doc.get_object(by("parent field")).is_err(),
            "and so did the field holding its value"
        );
    }

    /// A field matched by its value takes the widgets under it.
    ///
    /// `held`'s widget is nowhere near the region, so the annotation pass leaves
    /// it: the value rule is what takes the field, and a removal that stopped at
    /// the field dictionary would leave a widget on the page drawing the answer
    /// from a `/Parent` that is no longer there.
    #[test]
    fn a_matched_field_takes_the_widgets_under_it() {
        let (mut doc, page, ids) = formed_document();
        let by = |want: &str| {
            ids.iter()
                .find(|(name, _)| *name == want)
                .map(|(_, id)| *id)
                .expect(want)
        };
        apply_redactions(&mut doc, &[page], &over_the_widget(page)).expect("ok");
        assert!(
            doc.get_object(by("held field")).is_err(),
            "its value was in what went"
        );
        assert!(
            doc.get_object(by("its held widget")).is_err(),
            "and the widget under it came too"
        );
    }

    /// A default value is a copy of the answer, and goes with it.
    ///
    /// `/DV` is what the field was pre-filled from --- the same string in the
    /// same dictionary --- so a redaction that took `/V` and left `/DV` removed
    /// nothing. `defaulted` carries no `/V` at all, so it is the only subject
    /// reading `/DV` can decide.
    #[test]
    fn a_field_whose_default_holds_what_went_is_taken_too() {
        let (mut doc, page, ids) = formed_document();
        let id = ids
            .iter()
            .find(|(name, _)| *name == "defaulted field")
            .map(|(_, id)| *id)
            .expect("defaulted field");
        apply_redactions(&mut doc, &[page], &over_the_widget(page)).expect("ok");
        assert!(doc.get_object(id).is_err(), "a default is a copy of it");
    }

    /// **The second over-removal control.** Two letters are not a match.
    ///
    /// `short` holds `ME`, which occurs inside `MERGED-SECRET` and inside a
    /// great many other words. A form is full of answers this short --- `Yes`,
    /// a title, an initial --- and matching them would empty the form on the
    /// first redaction of any line.
    #[test]
    fn a_field_value_too_short_to_be_distinctive_is_not_matched() {
        let (mut doc, page, ids) = formed_document();
        let id = ids
            .iter()
            .find(|(name, _)| *name == "short field")
            .map(|(_, id)| *id)
            .expect("short field");
        apply_redactions(&mut doc, &[page], &over_the_widget(page)).expect("ok");
        assert!(
            doc.get_object(id).is_ok(),
            "two letters occur everywhere, and are nobody's answer"
        );
    }

    /// A field whose value is text that went goes, wherever its widget sits.
    ///
    /// §6 names *widgets outside the redacted rectangle* explicitly. The away
    /// widget is nowhere near the region and holds the same answer, which is
    /// what a second copy of a field on another page looks like.
    #[test]
    fn a_field_holding_what_went_goes_even_with_its_widget_elsewhere() {
        let (mut doc, page, ids) = formed_document();
        let away = ids
            .iter()
            .find(|(name, _)| *name == "away widget")
            .map(|(_, id)| *id)
            .expect("away widget");
        apply_redactions(&mut doc, &[page], &over_the_widget(page)).expect("ok");
        assert!(doc.get_object(away).is_err(), "its value was in what went");
    }

    /// **The over-removal control.** A field naming nothing that went stays.
    ///
    /// A rule that emptied `/AcroForm` would pass every check above, and a form
    /// is a document's usefulness: a reader who redacted one line must not get a
    /// copy with every other answer wiped.
    #[test]
    fn a_field_naming_nothing_that_went_survives_a_redaction() {
        let (mut doc, page, ids) = formed_document();
        let keep = ids
            .iter()
            .find(|(name, _)| *name == "unrelated field")
            .map(|(_, id)| *id)
            .expect("unrelated field");
        apply_redactions(&mut doc, &[page], &over_the_widget(page)).expect("ok");
        assert!(
            doc.get_object(keep).is_ok(),
            "a different answer is not ours"
        );
        assert!(
            doc.catalog().expect("catalog").has(b"AcroForm"),
            "and the form itself is still there"
        );
    }

    /// A checkbox's `/V` is a name, and a name is not compared against text.
    #[test]
    fn a_checkbox_is_never_taken_by_its_value() {
        let (mut doc, page, ids) = formed_document();
        let box_id = ids
            .iter()
            .find(|(name, _)| *name == "checkbox")
            .map(|(_, id)| *id)
            .expect("checkbox");
        apply_redactions(&mut doc, &[page], &over_the_widget(page)).expect("ok");
        assert!(doc.get_object(box_id).is_ok(), "a name is not a value");
    }

    /// A copy that is not a redaction keeps every field.
    ///
    /// The metadata and outline controls' third sibling, guarded by the same
    /// condition --- so `save: strip metadata on every save rather than on a
    /// redaction` is the mutation that proves all three, and names one.
    #[test]
    fn a_copy_that_is_not_a_redaction_keeps_its_fields() {
        let (mut doc, page, ids) = formed_document();
        let done = apply_redactions(&mut doc, &[page], &[]).expect("applicable");
        assert_eq!(done.fields, 0);
        for (name, id) in ids {
            assert!(
                doc.get_object(id).is_ok(),
                "{name} survives an ordinary save"
            );
        }
    }

    /// A form whose every field went loses the `/AcroForm` too.
    ///
    /// Kept empty it reads as a document that never had a form, while `/DA`,
    /// `/DR` and `/NeedAppearances` go on describing fields that are gone. The
    /// same reasoning as an emptied outline, and `drop_fields` is called
    /// directly because no redaction of this fixture takes every field --- the
    /// over-removal controls exist precisely to stop that happening.
    #[test]
    fn a_form_with_nothing_left_in_it_goes_as_well() {
        let (mut doc, _page, ids) = formed_document();
        let every: Vec<lopdf::ObjectId> = ids
            .iter()
            .filter(|(name, _)| *name != "its appearance" && *name != "/AcroForm")
            .map(|(_, id)| *id)
            .collect();
        assert!(
            doc.catalog().expect("catalog").has(b"AcroForm"),
            "the control: it is there to begin with"
        );
        let gone = crate::redact::drop_fields(&mut doc, &every).expect("dropped");
        assert_eq!(gone, every.len());
        assert!(
            !doc.catalog().expect("catalog").has(b"AcroForm"),
            "and an empty form is not a form"
        );
    }

    /// A rewrite that removed only a picture still sweeps.
    ///
    /// **The condition in `rewrite`, not the sweep itself**, and that is the
    /// distinction the check below it does not make: every other test here calls
    /// `sweep::collect` by hand, so all of them passed while a rewrite that
    /// removed a picture never swept at all. The `Do` went, the resource entry
    /// went, the stream stayed reachable from nothing, and every byte of the
    /// picture was written out.
    ///
    /// `redact-apply-probe` found it by grepping the written bytes for the
    /// picture's own pixels; this is that finding at a level `cargo test`
    /// reaches, and it is why the check greps rather than asking what the page
    /// draws --- those are different claims and only the second is a redaction.
    #[test]
    fn a_rewrite_that_removed_a_picture_sweeps_it_out_of_the_file() {
        let scratch = Scratch::new("sweep-image");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        // Four bytes that occur nowhere else in the file, so "gone" is
        // unambiguous. Uncompressed for the same reason the fixture is.
        const PIXELS: &[u8] = &[0xde, 0xad, 0xbe, 0xef, 0xde, 0xad, 0xbe, 0xef];
        std::fs::write(&source, document_drawing_an_image(PIXELS)).expect("write fixture");

        let mut plan = plan_of(&[0]);
        plan.redactions = vec![crate::edits::PlannedRedaction {
            source: 0,
            shows: Vec::new(),
            text_objects: 0,
            areas: Vec::new(),
            taking: Vec::new(),
            form_shows: Vec::new(),
            form_text_objects: Vec::new(),
            images: vec![0],
            image_objects: 1,
        }];
        write_copy(&source, &plan, &out, None).expect("save");
        let bytes = std::fs::read(&out).expect("read back");
        assert!(
            !bytes.windows(PIXELS.len()).any(|w| w == PIXELS),
            "the picture's own pixels are still in the written file"
        );
    }

    /// A one-page document drawing one uncompressed image.
    fn document_drawing_an_image(pixels: &[u8]) -> Vec<u8> {
        use lopdf::Stream;
        let mut doc = Document::with_version("1.7");
        let image = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2,
                "Height" => 1,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
            },
            pixels.to_vec(),
        ));
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"q 10 0 0 10 0 0 cm /Im0 Do Q\n".to_vec(),
        ));
        let pages_id = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("serialise the fixture");
        out
    }

    /// The appearance stream of a removed widget draws the value it held.
    ///
    /// **Not a new mechanism --- a property of the existing sweep, pinned here
    /// because nothing said it held.** A widget's `/AP` is a separate object
    /// reachable only from the widget, so removing the widget orphans a drawing
    /// of the very answer that went, and `lopdf` writes every object it holds.
    /// `sweep::collect` reaches it, and `rewrite` runs the sweep on exactly the
    /// condition a field removal satisfies. Measured rather than assumed: before
    /// the sweep it survives, after it, it does not.
    #[test]
    fn the_appearance_a_removed_widget_drew_its_value_with_is_collected() {
        let (mut doc, page, ids) = formed_document();
        let ap = ids
            .iter()
            .find(|(name, _)| *name == "its appearance")
            .map(|(_, id)| *id)
            .expect("appearance");
        apply_redactions(&mut doc, &[page], &over_the_widget(page)).expect("ok");
        assert!(
            doc.get_object(ap).is_ok(),
            "the control: unreachable, and still in the file until the sweep"
        );
        crate::sweep::collect(&mut doc).expect("sweep");
        assert!(doc.get_object(ap).is_err(), "the sweep takes it");
    }

    /// A one-page form with every shape the two rules have to tell apart.
    ///
    /// Two rules decide a field: every widget under it went, or its value is
    /// text that went. **Four of these shapes exist so that exactly one rule
    /// decides them** --- a fixture where both fire on every field cannot tell
    /// the two apart, and four mutations survived against exactly that.
    ///
    /// ```text
    ///   merged      field and widget in one object, over the region
    ///   parent      holds the value; its one widget is over the region
    ///   orphan      widget over the region, value naming nothing that went
    ///   held        holds a value that went; its widget is nowhere near
    ///   defaulted   carries what went in /DV, with no /V at all
    ///   short       /V is two letters, and they occur inside what went
    ///   away        holds the same answer, widget nowhere near the region
    ///   unrelated   holds a different answer, widget nowhere near it
    ///   checkbox    /V is a NAME, over the region's page but not its rectangle
    /// ```
    ///
    /// `orphan` is the only one the first rule decides alone, `held` and
    /// `defaulted` the only ones the second decides alone, and `short` is the
    /// only one the length guard saves.
    fn formed_document() -> (
        Document,
        lopdf::ObjectId,
        Vec<(&'static str, lopdf::ObjectId)>,
    ) {
        use lopdf::{dictionary, Stream};

        let mut doc = Document::with_version("1.7");
        let content = doc.add_object(Stream::new(dictionary! {}, b"BT (page) Tj ET".to_vec()));
        let pages_id = doc.new_object_id();

        // The copy that survives removing `/V`, and the reason the sweep matters.
        let ap = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 100.into(), 20.into()],
            },
            b"BT (MERGED-SECRET) Tj ET".to_vec(),
        ));
        let merged = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
            "T" => Object::string_literal("merged"),
            "V" => Object::string_literal("MERGED-SECRET"),
            "Rect" => vec![100.into(), 100.into(), 200.into(), 120.into()],
            "AP" => dictionary! { "N" => ap },
        });

        let kid = doc.new_object_id();
        let parent = doc.add_object(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal("split"),
            "V" => Object::string_literal("PARENT-SECRET"),
            "Kids" => vec![kid.into()],
        });
        doc.objects.insert(
            kid,
            Object::Dictionary(dictionary! {
                "Type" => "Annot", "Subtype" => "Widget",
                "Parent" => parent,
                "Rect" => vec![100.into(), 130.into(), 200.into(), 150.into()],
            }),
        );

        // Every widget under it goes, and its value names nothing that went ---
        // so the first rule is the only thing that can take it.
        let orphan_kid = doc.new_object_id();
        let orphan = doc.add_object(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal("orphan"),
            "V" => Object::string_literal("UNSAID-ANSWER"),
            "Kids" => vec![orphan_kid.into()],
        });
        doc.objects.insert(
            orphan_kid,
            Object::Dictionary(dictionary! {
                "Type" => "Annot", "Subtype" => "Widget",
                "Parent" => orphan,
                "Rect" => vec![100.into(), 95.into(), 200.into(), 115.into()],
            }),
        );

        // Its widget survives the annotation pass, so the value rule is what
        // takes it --- and the widget has to come with it.
        let held_kid = doc.new_object_id();
        let held = doc.add_object(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal("held"),
            "V" => Object::string_literal("HELD-SECRET"),
            "Kids" => vec![held_kid.into()],
        });
        doc.objects.insert(
            held_kid,
            Object::Dictionary(dictionary! {
                "Type" => "Annot", "Subtype" => "Widget",
                "Parent" => held,
                "Rect" => vec![400.into(), 400.into(), 500.into(), 420.into()],
            }),
        );

        // Never filled in, and pre-populated with the answer anyway.
        let defaulted = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
            "T" => Object::string_literal("defaulted"),
            "DV" => Object::string_literal("DEFAULT-SECRET"),
            "Rect" => vec![400.into(), 300.into(), 500.into(), 320.into()],
        });
        // `me` occurs inside `merged-secret`, and a form is full of answers this
        // short.
        let short = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
            "T" => Object::string_literal("short"),
            "V" => Object::string_literal("ME"),
            "Rect" => vec![400.into(), 200.into(), 500.into(), 220.into()],
        });

        let away = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
            "T" => Object::string_literal("away"),
            "V" => Object::string_literal("AWAY-SECRET"),
            "Rect" => vec![400.into(), 700.into(), 500.into(), 720.into()],
        });
        let unrelated = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
            "T" => Object::string_literal("unrelated"),
            "V" => Object::string_literal("SOMEBODY-ELSES-ANSWER"),
            "Rect" => vec![400.into(), 600.into(), 500.into(), 620.into()],
        });
        let checkbox = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn",
            "T" => Object::string_literal("agreed"),
            "V" => Object::Name(b"MERGED-SECRET".to_vec()),
            "Rect" => vec![400.into(), 500.into(), 420.into(), 520.into()],
        });

        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Annots" => vec![
                merged.into(), kid.into(), orphan_kid.into(), held_kid.into(),
                defaulted.into(), short.into(), away.into(), unrelated.into(),
                checkbox.into(),
            ],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let form = doc.add_object(dictionary! {
            "Fields" => vec![
                merged.into(), parent.into(), orphan.into(), held.into(),
                defaulted.into(), short.into(), away.into(), unrelated.into(),
                checkbox.into(),
            ],
            "DA" => Object::string_literal("/Helv 0 Tf 0 g"),
        });
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages_id, "AcroForm" => form,
        });
        doc.trailer.set("Root", catalog);
        (
            doc,
            page,
            vec![
                ("merged widget", merged),
                ("its appearance", ap),
                ("parent field", parent),
                ("its widget", kid),
                ("orphan field", orphan),
                ("its orphan widget", orphan_kid),
                ("held field", held),
                ("its held widget", held_kid),
                ("defaulted field", defaulted),
                ("short field", short),
                ("away widget", away),
                ("unrelated field", unrelated),
                ("checkbox", checkbox),
                ("/AcroForm", form),
            ],
        )
    }

    /// A region over the two widgets at the bottom left, and nothing else.
    ///
    /// `taking` names all three secrets because route B removes a whole line and
    /// this fixture's answers are what that line held --- which is what makes
    /// `away` reachable by the value rule and `unrelated` not.
    fn over_the_widget(_page: lopdf::ObjectId) -> Vec<crate::edits::PlannedRedaction> {
        vec![crate::edits::PlannedRedaction {
            source: 0,
            shows: Vec::new(),
            text_objects: 1,
            areas: vec![[90.0, 90.0, 210.0, 160.0]],
            taking: vec![
                "MERGED-SECRET PARENT-SECRET AWAY-SECRET DEFAULT-SECRET HELD-SECRET".to_string(),
            ],
            form_shows: Vec::new(),
            form_text_objects: Vec::new(),
            images: Vec::new(),
            image_objects: 0,
        }]
    }

    /// A one-page document that describes itself in both places.
    ///
    /// Returns the page, the `/Info` object and the XMP packet.
    fn described_document() -> (Document, lopdf::ObjectId, lopdf::ObjectId, lopdf::ObjectId) {
        use lopdf::{dictionary, Stream};

        let mut doc = Document::with_version("1.7");
        let content = doc.add_object(Stream::new(dictionary! {}, b"BT (secret) Tj ET".to_vec()));
        let pages_id = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
            }),
        );
        let metadata = doc.add_object(Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            b"<x:xmpmeta><dc:title>secret</dc:title></x:xmpmeta>".to_vec(),
        ));
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "Metadata" => metadata,
        });
        doc.trailer.set("Root", catalog);
        let info = doc.add_object(dictionary! {
            "Title" => Object::string_literal("secret"),
            "Author" => Object::string_literal("A. Beispiel"),
        });
        doc.trailer.set("Info", info);
        (doc, page, info, metadata)
    }

    /// The plan that redacts the one line `described_document` draws.
    fn redaction_of(_page: lopdf::ObjectId) -> Vec<crate::edits::PlannedRedaction> {
        vec![crate::edits::PlannedRedaction {
            source: 0,
            shows: vec![0],
            text_objects: 1,
            areas: Vec::new(),
            taking: Vec::new(),
            form_shows: Vec::new(),
            form_text_objects: Vec::new(),
            images: Vec::new(),
            image_objects: 0,
        }]
    }

    /// A redaction naming a page the plan does not keep is refused.
    ///
    /// Unreachable from the model as it stands --- `Edits::redaction_targets`
    /// walks the live pages --- and the failure it guards against is the one
    /// worth refusing loudly: an index past the end would otherwise be an
    /// arithmetic accident away from naming a *different* page, and removing
    /// text from a page nobody marked is the confident wrong answer this
    /// subsystem exists to prevent.
    #[test]
    fn a_redaction_naming_a_page_that_is_not_kept_is_refused() {
        let past = vec![crate::edits::PlannedRedaction {
            source: 4,
            shows: vec![0],
            text_objects: 1,
            areas: Vec::new(),
            taking: Vec::new(),
            form_shows: Vec::new(),
            form_text_objects: Vec::new(),
            images: Vec::new(),
            image_objects: 0,
        }];
        let mut doc = Document::with_version("1.7");
        let why = apply_redactions(&mut doc, &[(1, 0)], &past)
            .expect_err("a page the plan does not keep must be refused");
        assert!(why.message.contains("page 5"), "{why}");
        assert!(why.message.contains("that has 1"), "{why}");
    }

    /// Nothing to redact removes nothing, and says so by not refusing.
    ///
    /// The emptiness control for the two refusals above: a guard that fired on
    /// an empty list would make every ordinary save refuse, and one that could
    /// not fire at all would look exactly like this.
    #[test]
    fn a_plan_with_no_redactions_removes_nothing() {
        let mut doc = Document::with_version("1.7");
        assert_eq!(
            apply_redactions(&mut doc, &[(1, 0)], &[]).expect("no redactions is not a refusal"),
            Redacted::default()
        );
    }

    #[test]
    fn a_date_is_written_in_the_form_the_scan_reads_back() {
        // Fixed instants rather than `now`, and the epoch among them: the
        // arithmetic is shared with `diag.rs`, so what this pins is the *format*
        // -- the `D:` prefix, the zero padding and the trailing `Z`.
        assert_eq!(
            pdf_date(std::time::UNIX_EPOCH),
            "D:19700101000000Z",
            "the epoch"
        );
        assert_eq!(
            pdf_date(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_781_438_400)),
            "D:20260614120000Z"
        );
        // A clock before the epoch reads as the epoch rather than refusing, for
        // the reason `diag.rs` gives: the mark is worth more than the timestamp.
        assert_eq!(
            pdf_date(std::time::UNIX_EPOCH - std::time::Duration::from_secs(60)),
            "D:19700101000000Z"
        );
    }

    #[test]
    fn a_marked_document_still_refuses_what_it_refused_before() {
        // Marks are written before the turns and after the deletions, so the
        // three refusals `write_copy` documents have to survive one being
        // present. Encryption is the one that would be worst to lose: a mark
        // would then be the thing that silently stripped it.
        let scratch = Scratch::new("annots-encrypted");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, encrypted_document()).expect("write fixture");
        let why = write_copy(&source, &plan_with_mark(one_quad()), &out, None)
            .expect_err("an encrypted source must still be refused");
        assert!(why.message.contains("encrypted"), "{why}");
    }

    /// The written annotation for a mark of `kind`, reopened from the file.
    fn written_mark(kind: MarkKind, scratch: &Scratch) -> Dictionary {
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");
        write_copy(&source, &plan_of_kind(kind, one_quad()), &out, None).expect("save");
        let doc = Document::load(&out).expect("reopen");
        // The one annotation on the page, followed rather than searched for:
        // the fixture is written with none, so anything found here is ours.
        let page = ordered_pages(&doc)[0];
        let entry = doc
            .get_object(page)
            .and_then(Object::as_dict)
            .expect("a page dictionary")
            .get(b"Annots")
            .cloned()
            .expect("the page has an /Annots");
        let array = match entry {
            Object::Array(array) => array,
            Object::Reference(at) => doc
                .get_object(at)
                .and_then(Object::as_array)
                .expect("an /Annots reference points at an array")
                .clone(),
            other => panic!("/Annots is neither an array nor a reference: {other:?}"),
        };
        assert_eq!(array.len(), 1, "the fixture starts with no annotations");
        let Object::Reference(id) = array[0] else {
            panic!("the mark is not an indirect object")
        };
        doc.get_object(id)
            .and_then(Object::as_dict)
            .expect("the mark is a dictionary")
            .clone()
    }

    /// The blend mode the appearance stream's graphics state sets.
    ///
    /// In the `/ExtGState` the stream's `/Resources` names, not in the content
    /// -- which is where the first version of the test beside this looked, and
    /// why it wrote an assertion with an `||` in it that passed for the wrong
    /// reason. The content only ever says `/GS0 gs`.
    fn blend_of(kind: MarkKind, scratch: &Scratch) -> String {
        let (doc, stream) = written_appearance(kind, scratch);
        let states = stream
            .dict
            .get(b"Resources")
            .and_then(Object::as_dict)
            .and_then(|r| r.get(b"ExtGState"))
            .and_then(Object::as_dict)
            .expect("the appearance names an /ExtGState");
        let state = match states.get(b"GS0").expect("GS0") {
            Object::Reference(id) => doc
                .get_object(*id)
                .and_then(Object::as_dict)
                .expect("GS0 points at a dictionary")
                .clone(),
            Object::Dictionary(inline) => inline.clone(),
            other => panic!("GS0 is {other:?}"),
        };
        String::from_utf8(
            state
                .get(b"BM")
                .and_then(Object::as_name)
                .expect("the state sets a blend mode")
                .to_vec(),
        )
        .expect("a blend mode is a name")
    }

    /// The appearance stream's content for a written mark.
    fn appearance_of(kind: MarkKind, scratch: &Scratch) -> String {
        let (_, stream) = written_appearance(kind, scratch);
        String::from_utf8(
            stream
                .decompressed_content()
                .unwrap_or(stream.content.clone()),
        )
        .expect("the appearance stream is text")
    }

    /// The appearance stream's content for a plan the caller built.
    ///
    /// [`appearance_of`]'s shape, for the one test that needs a mark whose
    /// *note* is something other than the default: a text box draws its note, so
    /// the note is the thing under test rather than a field the fixture fills
    /// in.
    fn appearance_of_plan(plan: &Plan, scratch: &Scratch) -> String {
        let (_, stream) = written_appearance_of(plan, scratch);
        String::from_utf8(
            stream
                .decompressed_content()
                .unwrap_or(stream.content.clone()),
        )
        .expect("the appearance stream is text")
    }

    /// The reopened document and the one form XObject a written mark adds.
    fn written_appearance(kind: MarkKind, scratch: &Scratch) -> (Document, lopdf::Stream) {
        written_appearance_of(&plan_of_kind(kind, one_quad()), scratch)
    }

    /// The same, for a plan the caller built.
    fn written_appearance_of(plan: &Plan, scratch: &Scratch) -> (Document, lopdf::Stream) {
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");
        write_copy(&source, plan, &out, None).expect("save");
        let doc = Document::load(&out).expect("reopen");
        // Every form XObject in the file, of which the fixture has none.
        let stream = doc
            .objects
            .values()
            .find_map(|object| match object {
                Object::Stream(stream)
                    if stream.dict.get(b"Subtype").and_then(Object::as_name).ok()
                        == Some(b"Form".as_slice()) =>
                {
                    Some(stream.clone())
                }
                _ => None,
            })
            .expect("the mark has an appearance stream");
        (doc, stream)
    }

    /// A plan carrying one two-stroke drawing on page 0.
    fn plan_with_ink() -> Plan {
        let strokes = vec![
            crate::docmodel::Stroke {
                points: vec![
                    crate::docmodel::Point { x: 72.0, y: 90.0 },
                    crate::docmodel::Point { x: 300.0, y: 90.0 },
                ],
            },
            crate::docmodel::Stroke {
                points: vec![
                    crate::docmodel::Point { x: 72.0, y: 140.0 },
                    crate::docmodel::Point { x: 300.0, y: 140.0 },
                ],
            },
        ];
        let mut plan = plan_of_kind(
            MarkKind::Ink,
            crate::docmodel::Stroke::bounds(&strokes, (crate::docmodel::INK_WIDTH / 2.0) as f32)
                .into_iter()
                .collect(),
        );
        plan.marks[0].strokes = strokes;
        plan
    }

    #[test]
    fn each_stroke_is_its_own_path_in_the_appearance_stream() {
        // **One `S` per stroke, and that is the whole assertion.** A writer that
        // emitted a single path across both would join the end of the first to
        // the start of the second with a line the reader never drew --- which is
        // precisely what `/InkList` being a list of lists exists to prevent, and
        // it would look like a drawing rather than like a defect.
        // `annot-probe --mode strokes` measures the same thing in pixels, by
        // asserting the band between two strokes is empty.
        let scratch = Scratch::new("annots-ink-paths");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");
        write_copy(&source, &plan_with_ink(), &out, None).expect("save");
        let doc = Document::load(&out).expect("reopen");
        let stream = doc
            .objects
            .values()
            .filter_map(|object| object.as_stream().ok())
            .find(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|name| name == b"Form")
            })
            .expect("the mark added a form XObject")
            .clone();
        let content = String::from_utf8(
            stream
                .decompressed_content()
                .unwrap_or(stream.content.clone()),
        )
        .expect("the appearance stream is text");

        assert_eq!(
            content.matches(" m\n").count(),
            2,
            "one move-to per stroke: {content}"
        );
        assert_eq!(
            content.matches("S\n").count(),
            2,
            "one stroke operator per stroke: {content}"
        );
        assert!(
            content.contains("1 J 1 j"),
            "round caps and joins, or a hand-drawn corner spikes: {content}"
        );
        // Not filled. A drawing is a line, and `f` here would flood whatever it
        // was drawn around --- the box's mistake, one kind later.
        assert!(
            !content.contains(" re f"),
            "ink is stroked, never filled: {content}"
        );
    }

    #[test]
    fn the_ink_list_is_written_for_ink_and_for_nothing_else() {
        // The `/AP` above is what every reader draws; this is what an editor
        // reads, and a file with only the first is a picture of ink. Both
        // directions, because a writer that emitted the key unconditionally
        // satisfies the first half exactly as a correct one does.
        let scratch = Scratch::new("annots-ink-list");
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");
        write_copy(&source, &plan_with_ink(), &out, None).expect("save");
        let doc = Document::load(&out).expect("reopen");
        let lists: Vec<&Vec<Object>> = doc
            .objects
            .values()
            .filter_map(|object| object.as_dict().ok())
            .filter_map(|dictionary| dictionary.get(b"InkList").ok())
            .filter_map(|entry| entry.as_array().ok())
            .collect();
        assert_eq!(lists.len(), 1, "one /InkList, on the one drawing");
        assert_eq!(lists[0].len(), 2, "one array per stroke");
        for stroke in lists[0] {
            let points = stroke.as_array().expect("a stroke is an array");
            assert_eq!(points.len(), 4, "two points, four numbers");
        }

        // The other direction: a highlight written the same way carries none.
        let out2 = scratch.join("out2.pdf");
        write_copy(&source, &plan_with_mark(one_quad()), &out2, None).expect("save");
        let doc2 = Document::load(&out2).expect("reopen");
        assert!(
            doc2.objects
                .values()
                .filter_map(|object| object.as_dict().ok())
                .all(|dictionary| dictionary.get(b"InkList").is_err()),
            "an /InkList on a highlight is as wrong as its absence on ink"
        );
    }

    #[test]
    fn a_stamp_is_a_border_and_a_word_rather_than_either_alone() {
        // **Both halves, because each is a way of drawing a stamp that looks
        // exactly like another kind.** A stamp with only its border is a
        // `/Square`; a stamp with only its word is a `/FreeText`. Both would
        // pass a check that asked for ink and nothing more, and `annot-probe
        // --mode stamp` measures the same two things in pixels for the same
        // reason.
        let scratch = Scratch::new("annots-stamp");
        let content = appearance_of(MarkKind::Stamp, &scratch);

        assert!(content.contains(" re S"), "a stamp is bordered: {content}");
        assert!(content.contains("Tj"), "a stamp says something: {content}");
        // The word itself, hex-encoded as `winansi_hex` writes it --- `DRAFT`,
        // which is what `plan_of_kind` gives a stamp. Asserted rather than left
        // to "some text is drawn", because a stamp drawing the *note* instead
        // of its name would satisfy every reading above and put the wrong word
        // on the page.
        assert!(
            content.contains("<4452414654>"),
            "a stamp draws its own name: {content}"
        );
    }

    #[test]
    fn a_stamp_fills_the_rectangle_it_was_dragged_out_at() {
        // The size is computed from the rectangle, so a stamp dragged twice as
        // wide is set twice as large. Two plans differing in nothing but the
        // quad, compared by the `Tf` size each writes.
        let scratch = Scratch::new("annots-stamp-size");
        let small = appearance_of_plan(
            &plan_of_kind(
                MarkKind::Stamp,
                vec![crate::docmodel::Quad {
                    left: 72.0,
                    top: 100.0,
                    right: 172.0,
                    bottom: 130.0,
                }],
            ),
            &scratch,
        );
        let large = appearance_of_plan(
            &plan_of_kind(
                MarkKind::Stamp,
                vec![crate::docmodel::Quad {
                    left: 72.0,
                    top: 100.0,
                    right: 372.0,
                    bottom: 190.0,
                }],
            ),
            &scratch,
        );
        let size_of = |content: &str| -> f64 {
            let at = content.find(" Tf").expect("a stamp sets a font size");
            content[..at]
                .rsplit(' ')
                .next()
                .expect("a size before Tf")
                .parse()
                .expect("the size is a number")
        };
        let (a, b) = (size_of(&small), size_of(&large));
        assert!(
            b > a * 1.5,
            "a stamp three times as wide is set larger: {a} then {b}"
        );
    }

    #[test]
    fn a_box_is_stroked_on_a_path_inset_by_half_its_own_width() {
        // **Two assertions about one line of the content stream, and neither is
        // sufficient alone.** `re S` says the box is a frame; the inset says the
        // frame is all there. A stroke straddles its path, so a rectangle
        // stroked on the quad's own edge puts half of every side outside the
        // appearance stream's `/BBox`, which clips. The result is a box with
        // hairline edges --- it looks like a thin border rather than like a bug,
        // and `annot-probe --mode outline` measures the same thing in pixels.
        let scratch = Scratch::new("annots-box-stroke");
        let content = appearance_of(MarkKind::Square, &scratch);

        assert!(
            content.contains(" re S"),
            "a box is stroked, not filled: {content}"
        );
        assert!(
            !content.contains(" re f"),
            "a filled box hides what it was drawn around: {content}"
        );
        // The stroke colour as well as the fill colour, because `rg` does not
        // imply `RG` and a path stroked after only `rg` comes out black.
        assert!(
            content.contains(" RG"),
            "a stroke needs its own colour operator: {content}"
        );

        // The quad is 72..300 by 100..118 in display space, so 228 by 18 in
        // page space whichever way up it is; the path is that less one stroke
        // width, anchored half a width in. Written as numbers rather than
        // derived from `outline_path`, so the test cannot agree with a wrong
        // implementation of the arithmetic it is checking.
        // Named `half` rather than `inset`: `let inset = OUTLINE_WIDTH / 2.0;`
        // here is a superstring of the same line in `outline_path`, and the
        // mutation anchored on that one then matched twice.
        let half = OUTLINE_WIDTH / 2.0;
        let path = content
            .lines()
            .find(|line| line.ends_with(" re S"))
            .expect("a stroked rectangle");
        let numbers: Vec<f64> = path
            .split_whitespace()
            .take(4)
            .map(|n| n.parse().expect("a number"))
            .collect();
        assert!((numbers[0] - (72.0 + half)).abs() < 1e-3, "x: {path}");
        assert!(
            (numbers[2] - (228.0 - OUTLINE_WIDTH)).abs() < 1e-3,
            "width: {path}"
        );
        assert!(
            (numbers[3] - (18.0 - OUTLINE_WIDTH)).abs() < 1e-3,
            "height: {path}"
        );
        // The y is the *lower* edge in page space, which is not 100: the quad
        // arrives in display space and `user_quads` maps it. Asserted through
        // the height above and the round trip in `annot-probe` rather than
        // restated here, because a number copied out of a failing run is a
        // second implementation of the mapping and agrees with any of them.
        assert!(numbers[1] > 0.0, "the path starts on the page: {path}");

        // And the width the reader will see, stated once so the stroke cannot
        // silently become a hairline.
        assert!(
            content.contains(&format!("{OUTLINE_WIDTH} w")),
            "the stroke names its width: {content}"
        );
    }

    #[test]
    fn the_wash_and_the_rules_fill_rather_than_stroke() {
        // The control for the test above. "Contains `re S`" is satisfied by a
        // writer that stroked *everything*, which would turn every highlight
        // into an outline of itself -- and that is a change no assertion about
        // the box alone can see.
        //
        // **This test has now been renamed twice, by two successive kinds, and
        // the second time is the instructive one.** It was
        // `only_a_box_is_stroked` until the ellipse arrived, which was accurate
        // when written and false the moment a second kind was stroked. It was
        // then renamed to `the_text_markup_kinds_fill_and_are_not_stroked` --
        // and the squiggly is a text-markup kind that is *stroked*, so that name
        // was false within the day.
        //
        // Both names described the population the loop happened to cover.
        // Neither described the property it asserts, which never changed: these
        // three kinds fill a rectangle. Name a test for what it checks, not for
        // the set that currently satisfies it -- a population is what the next
        // kind changes, and the body stays correct while the name quietly stops
        // being true.
        for kind in [
            MarkKind::Highlight,
            MarkKind::Underline,
            MarkKind::StrikeOut,
        ] {
            let scratch = Scratch::new("annots-not-stroked");
            let content = appearance_of(kind, &scratch);
            assert!(
                content.contains(" re f"),
                "{kind:?} fills its rectangle: {content}"
            );
            assert!(
                !content.contains(" re S"),
                "{kind:?} is not an outline: {content}"
            );
        }
    }

    #[test]
    fn each_kind_writes_its_own_subtype() {
        // The one thing every other reader keys on. A wrong subtype produces a
        // mark that draws correctly from our own `/AP` and is reported as the
        // wrong kind by Acrobat, Preview and the sidebar -- which is the failure
        // that looks like nothing is wrong.
        for (kind, expected) in [
            (MarkKind::Highlight, "Highlight"),
            (MarkKind::Underline, "Underline"),
            (MarkKind::StrikeOut, "StrikeOut"),
            (MarkKind::Note, "Text"),
            (MarkKind::Square, "Square"),
            // The pair whose two names differ, and the one arm here that would
            // catch a copy-and-paste from the box above. Our own `/AP` draws the
            // right ellipse whatever the subtype says, so a wrong `/Circle` is
            // invisible on screen and wrong in every other program.
            (MarkKind::Ellipse, "Circle"),
            (MarkKind::Squiggly, "Squiggly"),
            (MarkKind::TextBox, "FreeText"),
        ] {
            let scratch = Scratch::new("annots-subtype");
            let written = written_mark(kind, &scratch);
            assert_eq!(
                written.get(b"Subtype").and_then(Object::as_name).ok(),
                Some(expected.as_bytes()),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_text_box_carries_the_da_the_specification_requires_and_nothing_else_does() {
        // **`/DA` is required on a `/FreeText` and forbidden nowhere else, so
        // both halves of this are assertions.** A text box without it displays
        // from its `/AP` and cannot be *edited* in any other reader: Acrobat
        // regenerates the appearance when a reader types, and `/DA` is what it
        // regenerates from. A highlight carrying one would be an unlisted key
        // whose meaning for that subtype is undefined.
        let scratch = Scratch::new("annots-freetext-da");
        let written = written_mark(MarkKind::TextBox, &scratch);
        let da = written
            .get(b"DA")
            .and_then(Object::as_str)
            .expect("a /FreeText carries /DA");
        let da = String::from_utf8_lossy(da);

        // The font name and the size have to be the ones the appearance stream
        // used, or a reader that regenerates redraws the same words at another
        // size. Compared against the constants rather than against a literal, so
        // changing the size moves both together or fails here.
        assert!(
            da.contains(&format!("/{TEXT_FONT} ")),
            "/DA names the appearance stream's font: {da}"
        );
        assert!(
            da.contains(&format!("{} Tf", textbox::SIZE)),
            "/DA names the size the stream set: {da}"
        );
        assert!(da.contains("rg"), "/DA sets a fill colour: {da}");

        // The control, and it is what makes the assertion above mean "required
        // *here*" rather than "written everywhere".
        let scratch = Scratch::new("annots-freetext-da-control");
        let other = written_mark(MarkKind::Highlight, &scratch);
        assert!(other.get(b"DA").is_err(), "only a /FreeText carries /DA");
    }

    #[test]
    fn a_text_box_draws_its_words_as_winansi_hex_rather_than_a_literal() {
        // **The encoding bug this would otherwise have shipped with.** The
        // content stream is a Rust `String`, so an umlaut pushed into it as a
        // literal is two UTF-8 bytes where WinAnsi wants one — every English
        // text box correct, every German one drawing `Ã¼`. Hex removes the
        // question, and removes the escaping question with it.
        let scratch = Scratch::new("annots-freetext-hex");
        let mut plan = plan_of_kind(MarkKind::TextBox, one_quad());
        "Grüße".clone_into(&mut plan.marks[0].note);
        let content = appearance_of_plan(&plan, &scratch);

        assert!(
            !content.contains("Tj") || content.contains("> Tj"),
            "the text is a hex string: {content}"
        );
        // `ü` is one byte, `FC`, and it is the byte that would be `C3 BC` if the
        // stream had been built as UTF-8.
        assert!(
            content.contains("FC"),
            "the umlaut is one WinAnsi byte: {content}"
        );
        assert!(
            !content.contains("C3BC"),
            "and not two UTF-8 ones: {content}"
        );
        // A font to draw it with, in the appearance stream's own resources. A
        // `Tf` naming a font the resources do not have draws nothing at all.
        assert!(
            content.contains(&format!("/{TEXT_FONT} ")),
            "the stream names its font: {content}"
        );
        let scratch = Scratch::new("annots-freetext-font");
        let (_, stream) = written_appearance(MarkKind::TextBox, &scratch);
        assert!(
            font_names(&stream).contains(&TEXT_FONT.to_string()),
            "the resources carry the font the stream names"
        );

        // **The control, and it is what the comment at the call site claims.**
        // The writer adds `/Font` only for `Paint::Text`, on the grounds that a
        // font on a highlight's resources is dead weight in every saved file --
        // a claim with no test until a surviving mutation said so. An assertion
        // that a text box *has* a font passes equally well if every kind does.
        let scratch = Scratch::new("annots-freetext-font-control");
        let (_, plain) = written_appearance(MarkKind::Highlight, &scratch);
        assert!(
            font_names(&plain).is_empty(),
            "only a text box's appearance carries a font"
        );
    }

    /// The names in an appearance stream's `/Resources /Font`, if it has any.
    fn font_names(stream: &lopdf::Stream) -> Vec<String> {
        stream
            .dict
            .get(b"Resources")
            .and_then(Object::as_dict)
            .ok()
            .and_then(|r| r.get(b"Font").and_then(Object::as_dict).ok())
            .map(|fonts| {
                fonts
                    .iter()
                    .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------
    // A mark on a page the document says is turned
    // -----------------------------------------------------------------

    /// The appearance stream one mark writes over a caller's fixture.
    ///
    /// [`written_appearance_of`]'s shape, with the source document as an
    /// argument: every test below is a comparison between the same mark on an
    /// upright page and on a turned one, which needs two fixtures rather than
    /// the one that helper hard-codes.
    fn appearance_over(source_bytes: Vec<u8>, plan: &Plan, scratch: &Scratch) -> String {
        let source = scratch.join("in.pdf");
        let out = scratch.join("out.pdf");
        std::fs::write(&source, source_bytes).expect("write fixture");
        write_copy(&source, plan, &out, None).expect("save");
        let doc = Document::load(&out).expect("reopen");
        let stream = doc
            .objects
            .values()
            .find_map(|object| match object {
                Object::Stream(stream)
                    if stream.dict.get(b"Subtype").and_then(Object::as_name).ok()
                        == Some(b"Form".as_slice()) =>
                {
                    Some(stream.clone())
                }
                _ => None,
            })
            .expect("the mark has an appearance stream");
        String::from_utf8(
            stream
                .decompressed_content()
                .unwrap_or(stream.content.clone()),
        )
        .expect("the appearance stream is text")
    }

    /// A plan over `pages` untouched pages, carrying one mark of `kind` on the
    /// first.
    ///
    /// The count is an argument because the two fixtures differ: a plan naming
    /// fewer pages than the file has is a page deletion, which would put the
    /// rewrite path under a test about geometry, and `write_copy` refuses one
    /// naming more.
    fn one_mark_over(pages: usize, kind: MarkKind, quad: crate::docmodel::Quad) -> Plan {
        let mut plan = plan_of(&vec![0u8; pages]);
        plan.marks.push(PlannedMark {
            kind,
            stamp: (kind == MarkKind::Stamp).then_some(crate::docmodel::StampName::Draft),
            source: 0,
            quads: vec![quad],
            strokes: Vec::new(),
            color: [1.0, 0.9, 0.2],
            author: "a reader".to_string(),
            note: "the reader typed this".to_string(),
            made: "D:20260824120000Z".to_string(),
        });
        plan
    }

    /// The box every comparison below uses, in the space the reader drags in.
    ///
    /// 300 by 40 points, which both fixtures hold with room to spare: one is
    /// 612 x 792 displayed and the other 792 x 612, and a box that fitted only
    /// one of them would make the comparison a statement about clipping.
    fn readers_box() -> crate::docmodel::Quad {
        crate::docmodel::Quad {
            left: 72.0,
            top: 100.0,
            right: 372.0,
            bottom: 140.0,
        }
    }

    #[test]
    fn an_upright_box_is_the_rectangle_the_reader_dragged() {
        // `Upright` and `text::from_device` are two statements of one turn, and
        // the trap index has what happens to two copies of a distinction. This
        // is the one test that pins them together, so it runs the pair rather
        // than restating either: map a reader's rectangle into the page with
        // `from_device`, ask `Upright` what the reader saw, and require the
        // answer back.
        let (w, h) = (792.0f32, 612.0f32);
        let device = [72.0f32, 100.0, 372.0, 140.0];
        let (dragged_w, dragged_h) = (
            (device[2] - device[0]) as f64,
            (device[3] - device[1]) as f64,
        );
        for turns in 0..4u8 {
            // The displayed size swaps with the page's own at odd quarters,
            // exactly as both functions expect it to.
            let (dw, dh) = if turns % 2 == 0 { (w, h) } else { (h, w) };
            let quad = crate::text::from_device(turns, dw, dh, device);
            let seen = Upright::of(turns, quad);
            assert!(
                (seen.width - dragged_w).abs() < 0.01 && (seen.height - dragged_h).abs() < 0.01,
                "at {turns} quarters the reader's 300 x 40 came back {} x {}",
                seen.width,
                seen.height
            );
            // The corners, which the size alone cannot pin: a box the right
            // shape anchored at the wrong corner puts every mark somewhere else
            // on the page, and three of the four turns move which corner of the
            // page-space quad the reader's top-left is.
            let top_left = seen.at(0.0, 0.0);
            let bottom_right = seen.at(seen.width, seen.height);
            let xs = [top_left.0, bottom_right.0];
            let ys = [top_left.1, bottom_right.1];
            assert!(
                (xs[0].min(xs[1]) - quad[0]).abs() < 0.01
                    && (ys[0].min(ys[1]) - quad[1]).abs() < 0.01
                    && (xs[0].max(xs[1]) - quad[2]).abs() < 0.01
                    && (ys[0].max(ys[1]) - quad[3]).abs() < 0.01,
                "at {turns} quarters the two corners span {top_left:?}..{bottom_right:?}, \
                 not the quad {quad:?}"
            );
        }
    }

    #[test]
    fn a_text_box_wraps_to_the_width_the_reader_dragged_however_the_page_is_turned() {
        // **The defect this is written for, and it shipped.** `user_quads` maps
        // the reader's rectangle into the page's own space, so on a page
        // carrying `/Rotate 90` a box dragged 300 wide and 40 tall arrives 40
        // wide. `wrap` was given that 40, and broke these four words into
        // eighteen lines two glyphs across, drawn down the page --- against the
        // one line the model made from the same box, which is what the overlay
        // draws. Measured on `testdata/inherited.pdf` before the repair.
        //
        // Stated as a comparison between the two pages rather than against a
        // number, because the number is `wrap`'s own answer: asserting it here
        // would be this file agreeing with the function it calls, and would pass
        // just as well if both were given the wrong width.
        let upright = appearance_over(
            document_with_annots(AnnotShape::Absent),
            &one_mark_over(1, MarkKind::TextBox, readers_box()),
            &Scratch::new("turned-text-upright"),
        );
        let turned = appearance_over(
            inheriting_document(),
            &one_mark_over(2, MarkKind::TextBox, readers_box()),
            &Scratch::new("turned-text-turned"),
        );
        let words = |content: &str| {
            content
                .lines()
                .filter(|line| line.ends_with("Tj"))
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        assert!(
            !words(&upright).is_empty(),
            "the control drew nothing, so the comparison is between two absences: {upright}"
        );
        assert_eq!(
            words(&turned),
            words(&upright),
            "one box, one string, two pages: the same words in the same lines"
        );
    }

    #[test]
    fn type_runs_the_readers_way_on_a_turned_page() {
        // The other half, and the half the line count cannot reach: wrapping to
        // the right width still draws each line along the *page's* axis unless
        // the text matrix says otherwise, so a text box on a turned page came
        // out sideways with exactly the right number of lines in it.
        //
        // `0 1 -1 0` is a quarter turn anticlockwise in page space, which is
        // what a page displayed a quarter clockwise needs to read upright.
        let turned = appearance_over(
            inheriting_document(),
            &one_mark_over(2, MarkKind::TextBox, readers_box()),
            &Scratch::new("turned-text-matrix"),
        );
        assert!(
            turned.contains("0 1 -1 0 "),
            "type on a page turned one quarter is set on a turned matrix: {turned}"
        );
        // The control, and it is what makes the assertion above about the turn
        // rather than about `Tm` being emitted at all: an upright page gets the
        // identity, so a matrix hard-coded to the quarter would fail here.
        let upright = appearance_over(
            document_with_annots(AnnotShape::Absent),
            &one_mark_over(1, MarkKind::TextBox, readers_box()),
            &Scratch::new("upright-text-matrix"),
        );
        assert!(
            upright.contains("1 0 0 1 ") && !upright.contains("0 1 -1 0 "),
            "an upright page is set on the identity: {upright}"
        );
    }

    #[test]
    fn a_rule_sits_under_the_words_however_the_page_is_turned() {
        // An underline's band is `LINE_FRACTION` of the quad's height, at the
        // quad's bottom, and on a turned page both of those were the page's
        // rather than the reader's: the rule came out down the left edge of the
        // words. Measured at x 0.00..0.07 against the upright y 0.93..0.99.
        //
        // **Read back in the reader's frame through `text::to_device`**, which
        // is the independent half: `Upright` is the code under test, and asking
        // it where its own rectangle went would be the writer agreeing with
        // itself. The band is then a fraction of the box the reader dragged,
        // and the two pages must give the same four numbers.
        //
        // Written first as "long the way the words run, thin across them",
        // which is the axis the defect is on --- and a mutation taking the
        // thickness from the page's box survived it, because a rule 7.5 times
        // too thick is still thinner than the box. A proportion measured along
        // the axis it is policing cannot see a magnitude; the differential can.
        let band_in_the_box = |content: &str, turns: u8| {
            let [x, y, w, h] = only_rectangle(content);
            // The fixtures are one page each, 612 x 792 before the turn.
            let (dw, dh) = if turns % 2 == 0 {
                (612.0, 792.0)
            } else {
                (792.0, 612.0)
            };
            let shown = crate::text::to_device(turns, dw, dh, [x, y, x + w, y + h]);
            let box_pt = readers_box();
            [
                (shown[0] - box_pt.left) / (box_pt.right - box_pt.left),
                (shown[1] - box_pt.top) / (box_pt.bottom - box_pt.top),
                (shown[2] - box_pt.left) / (box_pt.right - box_pt.left),
                (shown[3] - box_pt.top) / (box_pt.bottom - box_pt.top),
            ]
        };
        for kind in [MarkKind::Underline, MarkKind::StrikeOut] {
            let upright = band_in_the_box(
                &appearance_over(
                    document_with_annots(AnnotShape::Absent),
                    &one_mark_over(1, kind, readers_box()),
                    &Scratch::new("upright-rule"),
                ),
                0,
            );
            let turned = band_in_the_box(
                &appearance_over(
                    inheriting_document(),
                    &one_mark_over(2, kind, readers_box()),
                    &Scratch::new("turned-rule"),
                ),
                1,
            );
            // The control, and it is not decoration: a band that came back
            // spanning the whole box in both readings would satisfy the
            // comparison below while telling nothing apart. A rule is thin.
            assert!(
                upright[3] - upright[1] < 0.25,
                "{kind:?} upright covers {:.2} of the box's height, which is not a rule",
                upright[3] - upright[1]
            );
            for (at, (a, b)) in upright.iter().zip(turned.iter()).enumerate() {
                assert!(
                    (a - b).abs() < 0.01,
                    "{kind:?} edge {at}: {a:.3} of the box upright against {b:.3} turned"
                );
            }
        }
    }

    #[test]
    fn a_stamps_word_is_sized_by_the_box_the_reader_dragged() {
        // A stamp's size is a ratio of the box's width to its height, so a
        // turned page did not merely rotate the word: it set it at the size a
        // 40 x 300 rectangle would take. Measured as 11,024 inked pixels
        // against the upright 25,011 for one box.
        let size_of = |content: &str| {
            content
                .lines()
                .find_map(|line| line.strip_prefix(&format!("BT /{TEXT_FONT} ")))
                .and_then(|rest| rest.strip_suffix(" Tf"))
                .and_then(|size| size.parse::<f64>().ok())
                .unwrap_or_else(|| panic!("no font size in {content}"))
        };
        let upright = appearance_over(
            document_with_annots(AnnotShape::Absent),
            &one_mark_over(1, MarkKind::Stamp, readers_box()),
            &Scratch::new("upright-stamp"),
        );
        let turned = appearance_over(
            inheriting_document(),
            &one_mark_over(2, MarkKind::Stamp, readers_box()),
            &Scratch::new("turned-stamp"),
        );
        assert!(
            (size_of(&turned) - size_of(&upright)).abs() < 0.01,
            "one box dragged to one shape: {} turned against {} upright",
            size_of(&turned),
            size_of(&upright)
        );
        assert!(
            turned.contains("0 1 -1 0 "),
            "and the word is set the reader's way: {turned}"
        );
    }

    /// The `[x, y, width, height]` of the one `re` in a content stream.
    ///
    /// Panics on none and on more than one, which is what makes it usable as a
    /// reader: a stream with two rectangles in it is a different mark from the
    /// one the caller thinks it is measuring.
    fn only_rectangle(content: &str) -> [f64; 4] {
        let found: Vec<[f64; 4]> = content
            .lines()
            .filter_map(|line| {
                let rest = line
                    .strip_suffix(" re f")
                    .or_else(|| line.strip_suffix(" re S"))?;
                let numbers: Vec<f64> = rest
                    .split_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect();
                <[f64; 4]>::try_from(numbers).ok()
            })
            .collect();
        assert_eq!(found.len(), 1, "one rectangle, not {found:?}, in {content}");
        found[0]
    }

    #[test]
    fn a_squiggle_is_a_stroked_zigzag_in_a_band_taller_than_a_rule() {
        // **The check the subtype cannot make.** `/Squiggly` with a flat rule in
        // its appearance stream is a mark every reader files as a squiggle and
        // draws as an underline, and the subtype test above passes it. The two
        // halves fail in opposite directions, which is why they are two tests.
        //
        // `annot-probe --mode wave` measures the same claim in pixels through
        // PDFium, with an underline as its control; this one names the operators,
        // so a failure says *what* was drawn rather than that a strip was empty.
        let scratch = Scratch::new("annots-squiggle-zigzag");
        let content = appearance_of(MarkKind::Squiggly, &scratch);

        // Stroked, and made of line segments. A wave drawn with `re` would be a
        // rule; one drawn with `c` would be the curve this deliberately is not.
        assert!(content.contains("S\n"), "a squiggle is stroked: {content}");
        assert!(
            !content.contains(" re "),
            "a squiggle is not a rectangle: {content}"
        );
        assert!(
            !content.contains(" c\n"),
            "a squiggle is straight segments, not curves: {content}"
        );
        // Many of them. `one_quad` is 228 pt wide and 18 pt tall, so the band is
        // 3.24 pt and a half-period is 3.24 pt -- about seventy segments. The
        // bound is loose because the count is arithmetic this test should not
        // restate; what it rules out is a "wave" of one or two segments, which
        // is a diagonal line.
        let segments = content.matches(" l\n").count();
        assert!(
            segments > 20,
            "a squiggle is many segments and this has {segments}: {content}"
        );

        // **The band is taller than a rule's, which is the property every check
        // that tells the two kinds apart depends on.** Compared against the
        // underline rather than against a number, so the two constants cannot
        // drift into agreement without this failing.
        let (_, rule) = line_rect(MarkKind::Underline, 0.0, 100.0);
        let (_, wave) = line_rect(MarkKind::Squiggly, 0.0, 100.0);
        assert!(
            wave > rule * 2.0,
            "a squiggle's band ({wave}) must clear a rule's ({rule}) by enough to \
             read between them"
        );
        // And both start at the same edge, which is what makes the gap a strip
        // above the rule rather than two bands somewhere else.
        let (rule_base, _) = line_rect(MarkKind::Underline, 0.0, 100.0);
        let (wave_base, _) = line_rect(MarkKind::Squiggly, 0.0, 100.0);
        assert_eq!(rule_base, wave_base, "both sit on the quad's bottom edge");
    }

    #[test]
    fn an_ellipse_is_drawn_as_four_curves_and_not_as_a_rectangle() {
        // **The check the subtype cannot make and the subtype check cannot
        // make.** They fail in opposite directions and neither sees the other's
        // defect: a `/Circle` whose appearance stream says `re` is a rectangle
        // that every reader files under "ellipse", and a correct set of arcs
        // written under `/Square` is an ellipse every reader calls a rectangle.
        //
        // `annot-probe --mode outline --kind ellipse` measures the same claim in
        // pixels through PDFKit, which is the reader that has no idea what we
        // intended. This one is here because it runs in `cargo test` and names
        // the operator, so a failure says *what* was drawn rather than that a
        // corner had ink in it.
        let scratch = Scratch::new("annots-ellipse-curves");
        let content = appearance_of(MarkKind::Ellipse, &scratch);

        // Four, because that is what a whole ellipse takes: one cubic per
        // quadrant. Three would be a defect that still looks curved, which is
        // why this is an equality rather than `> 0`.
        assert_eq!(
            content.matches(" c\n").count(),
            4,
            "an ellipse is four Bézier arcs: {content}"
        );
        // The operator a rectangle would use. This is the assertion that fails
        // if `Paint::Ellipse` is ever folded back into `Paint::Outline`.
        assert!(
            !content.contains(" re "),
            "an ellipse is not a rectangle: {content}"
        );
        // Stroked and closed. `h` before `S` so the curve joins itself at three
        // o'clock rather than being capped there --- see the writer.
        assert!(
            content.contains("h S"),
            "the path is closed and stroked: {content}"
        );
        assert!(
            !content.contains(" f\n"),
            "a filled ellipse hides what it was drawn around: {content}"
        );
        // The stroke colour, for the box's reason: `rg` does not imply `RG`,
        // and a path stroked after only `rg` comes out black.
        assert!(
            content.contains(" RG"),
            "a stroke needs its own colour operator: {content}"
        );

        // **Where it starts, which is the reading that says the arcs describe
        // the reader's rectangle rather than some other one.** `one_quad` is
        // 72..300 by 100..118 in display space; the writer works in the page's,
        // so only the horizontal extreme is compared here -- it is unaffected by
        // the y-flip, where every vertical figure is not, and comparing one
        // number correctly is worth more than four through a mapping this test
        // would then be asserting twice.
        //
        // `outline_path` insets by half the stroke, so the rightmost point is
        // the quad's right edge less that. A `KAPPA` typo does not move this
        // point; the pixel probe is what catches one.
        // Spelled out rather than via a local `inset`, which `outline_path`
        // already uses: an identical line in two places makes an existing
        // mutation's anchor ambiguous, and the `anchors` gate refuses that. It
        // refused this, on the first run.
        let rightmost = 300.0 - OUTLINE_WIDTH / 2.0;
        let start = content
            .lines()
            .find(|line| line.ends_with(" m"))
            .expect("the path starts with a moveto");
        let x: f64 = start
            .split_whitespace()
            .next()
            .expect("a moveto has two numbers")
            .parse()
            .expect("the x is a number");
        assert!(
            (x - rightmost).abs() < 0.01,
            "the arc starts at the right of the inset quad, not at {x}: {start}"
        );
    }

    #[test]
    fn a_comment_carries_no_text_markup_keys_and_the_others_do() {
        // Two absence assertions and their control, in one test because apart
        // they are worth much less: "the comment has no /QuadPoints" is
        // satisfied by a writer that stopped emitting them for everything, and
        // this repository has the entry about an absence assertion that could
        // not fail. The three markup kinds in the same loop are what make the
        // two `is_none` lines mean something.
        //
        // `/QuadPoints` is listed by PDF 32000-1 on the text-markup subtypes
        // and on no other; a comment is positioned by `/Rect`. `/AP` is ours to
        // write for a markup kind and the reader's to synthesise for a comment
        // icon --- see the note at the call site.
        for kind in [
            MarkKind::Highlight,
            MarkKind::Underline,
            MarkKind::StrikeOut,
            // The fourth and last subtype the specification lists `/QuadPoints`
            // on. With it here, this loop is the whole of that list rather than
            // a sample of it, which is what lets the comment's `is_none` beside
            // it mean "not a markup kind" instead of "not one of three".
            MarkKind::Squiggly,
        ] {
            let scratch = Scratch::new("annots-markup-keys");
            let written = written_mark(kind, &scratch);
            assert!(
                written.get(b"QuadPoints").is_ok(),
                "{kind:?} should carry /QuadPoints"
            );
            assert!(written.get(b"AP").is_ok(), "{kind:?} should carry an /AP");
            assert!(
                written.get(b"Name").is_err(),
                "{kind:?} should carry no icon name"
            );
        }

        // **A box is the other kind with no quads, and it separates two things
        // this test used to assert together.** Until the box existed, "not a
        // markup kind" and "no appearance stream of ours" were true of exactly
        // the same one variant, so a single predicate satisfied both and no
        // test could tell which of them it was checking. A box carries no
        // `/QuadPoints` *and* needs an `/AP`, so the two assertions below now
        // disagree about it --- which is what makes them two assertions.
        let scratch = Scratch::new("annots-box-keys");
        let written = written_mark(MarkKind::Square, &scratch);
        assert!(
            written.get(b"QuadPoints").is_err(),
            "a box is not a text-markup annotation and must not carry /QuadPoints"
        );
        assert!(
            written.get(b"AP").is_ok(),
            "nothing synthesises a rectangle, so a box needs its own appearance"
        );
        assert!(
            written.get(b"Name").is_err(),
            "an icon name belongs to a comment, not to a box"
        );
        assert!(written.get(b"Open").is_err(), "a box has no popup to open");

        let scratch = Scratch::new("annots-comment-keys");
        let written = written_mark(MarkKind::Note, &scratch);
        assert!(
            written.get(b"QuadPoints").is_err(),
            "a comment must not carry /QuadPoints"
        );
        assert!(
            written.get(b"AP").is_err(),
            "a comment leaves its icon to the reader"
        );
        assert_eq!(
            written.get(b"Name").and_then(Object::as_name).ok(),
            Some(b"Comment".as_slice()),
            "a comment names the speech-bubble icon"
        );
        assert_eq!(
            written.get(b"Open").and_then(Object::as_bool).ok(),
            Some(false),
            "a comment opens closed"
        );
        // And the keys it shares with every other mark, so that "carries fewer
        // keys" cannot be satisfied by a dictionary that lost the rest of them.
        assert!(written.get(b"Rect").is_ok(), "a comment needs a rectangle");
        assert!(
            written.get(b"Contents").is_ok(),
            "a comment needs what it says"
        );
    }

    #[test]
    fn a_line_is_opaque_and_a_wash_is_not() {
        // Two dictionary entries and one stream entry, all deciding the same
        // thing: a wash multiplies with the words under it at 40%, a line is
        // drawn over them at full strength. A multiplied red line over black
        // text is black, which is a strikeout nobody can see.
        for (kind, alpha, blend) in [
            (MarkKind::Highlight, WASH_ALPHA, "Multiply"),
            (MarkKind::Underline, 1.0, "Normal"),
            (MarkKind::StrikeOut, 1.0, "Normal"),
            // A box is opaque for the same reason a line is, and it matters
            // more: a translucent frame over a figure reads as a printing
            // artifact rather than as something a reader drew.
            (MarkKind::Square, 1.0, "Normal"),
        ] {
            let scratch = Scratch::new("annots-alpha");
            let written = written_mark(kind, &scratch);
            let got = written
                .get(b"CA")
                .and_then(Object::as_float)
                .unwrap_or_else(|_| panic!("{kind:?} has no /CA"));
            assert!((got - alpha).abs() < 1e-6, "{kind:?}: /CA is {got}");
            assert_eq!(blend_of(kind, &scratch), blend, "{kind:?}");
        }
    }

    #[test]
    fn a_line_stays_inside_the_quad_it_marks() {
        // The `/BBox` is the bounds of the quads, so anything drawn outside is
        // clipped -- an underline centred on the bottom edge would lose its
        // lower half in every reader and look like a thinner line rather than
        // like a defect. The quad is 100..118 from the page top on a 792 pt
        // page, so in the page's own space it runs 674..692.
        for kind in [MarkKind::Underline, MarkKind::StrikeOut] {
            let scratch = Scratch::new("annots-inside");
            let content = appearance_of(kind, &scratch);
            let rect: Vec<f64> = content
                .lines()
                .find(|line| line.ends_with("re f"))
                .expect("the appearance draws a rectangle")
                .split_whitespace()
                .take(4)
                .map(|n| n.parse().expect("a number"))
                .collect();
            let (bottom, height) = (rect[1], rect[3]);
            assert!(
                bottom >= 674.0 - 1e-6 && bottom + height <= 692.0 + 1e-6,
                "{kind:?}: the line runs {bottom}..{} outside 674..692",
                bottom + height
            );
            // And it is a line rather than the wash: a quad 18 pt tall gives a
            // rule about 1.3 pt thick, so anything over a quarter of the quad
            // is the fill this is meant to be distinguishable from.
            assert!(height < 18.0 / 4.0, "{kind:?}: {height} pt is not a line");
        }
    }

    #[test]
    fn a_strikeout_crosses_the_text_and_an_underline_sits_under_it() {
        // The discrimination the test above cannot make: both kinds draw a thin
        // rule inside the quad, and only where it sits tells them apart. A
        // strikeout drawn at the bottom is an underline with the wrong subtype,
        // which every check keyed on the subtype would pass.
        let scratch = Scratch::new("annots-where");
        let bottom_of = |kind| {
            appearance_of(kind, &scratch)
                .lines()
                .find(|line| line.ends_with("re f"))
                .and_then(|line| line.split_whitespace().nth(1).map(str::to_string))
                .expect("a rectangle")
                .parse::<f64>()
                .expect("a number")
        };
        let under = bottom_of(MarkKind::Underline);
        let through = bottom_of(MarkKind::StrikeOut);
        // The quad is 674..692, so its middle is 683.
        assert!((under - 674.0).abs() < 1e-6, "underline sits at {under}");
        assert!(
            (through - 683.0).abs() < 1.0,
            "strikeout sits at {through}, not near the middle"
        );
    }
}
