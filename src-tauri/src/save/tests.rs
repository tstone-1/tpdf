//! The unit tests for [`crate::save`], moved out of it on 2026-09-01.
//!
//! **A file split rather than a design change: nothing here was rewritten.** The
//! bodies are the same code in the same order, dedented by one level and nothing
//! else --- and the move was verified by re-indenting the result and comparing it
//! byte for byte against the block it came from, because a mechanical edit over
//! nine thousand lines is exactly the case where reading the diff proves nothing.
//! Six lines were deliberately left alone: they are the interior of multi-line
//! string literals that carry no indentation, and the other twenty-five such
//! lines are safe to dedent only because each is a backslash continuation, where
//! Rust already strips the leading whitespace.
//!
//! **Why it was worth doing.** `save.rs` was 14,844 lines, of which 9,112 were
//! this module: a reader opening the file to find out what a save *does* met its
//! tests first and outnumbered two to one. An outside review named the file's
//! size as the one maintainability finding against the crate. Splitting the
//! production code by concern is the other half of that and is a design question;
//! moving the tests out is not, and it is where two thirds of the length was.
//!
//! **It changes no test path.** These are still `save::tests::*`, because a
//! submodule of `save` is what this is --- which matters beyond tidiness:
//! `scripts/mutate_rust.py` selects which tests may run through `FILTERS`, a list
//! of module prefixes matched as substrings, and `docs/TRAPS.md` records a move
//! that silently took two mutations out of every sweep by landing them in a
//! module no filter reached. Nothing here moves out from under `save::`.
//!
//! Every mutation anchor naming `src/save.rs` was checked against this move:
//! all 156 are in production code, so not one had to be re-aimed.

use lopdf::{dictionary, Dictionary};

use crate::docmodel::MarkKind;
use crate::edits::PlannedMark;
use crate::textbox;

// The marks module's own items, which `use super::*` cannot reach: `save::tests`
// is `save::marks`'s sibling, not its child, and what `save.rs` imports from it
// is only what `save.rs` itself uses. See `save/marks.rs`.
use super::marks::*;
use super::*;

/// [`write_copy`] with the in-process writer, which is what a copy took
/// before a worker did the parsing.
///
/// The tests below are about what a copy *contains* --- which pages, which
/// turns, which crops, what a redaction took out --- and not one of them is
/// about which process parsed it. The seam has its own tests, aimed at it and
/// named for it. Writing `&Here` at fifty call sites would put the answer to
/// a question none of them asks in front of every one of them, and the
/// alias says which writer is running more plainly than the argument does.
fn copy_here(
    source: &Path,
    plan: &Plan,
    out: &Path,
    password: Option<&str>,
) -> Result<Copied, Refusal> {
    write_copy(source, plan, out, password, &Here)
}

/// The file's bytes, rewritten under a plan, in this process.
///
/// **What `planned_bytes` was.** That function went with the merge on
/// 2026-09-01 --- it was the last caller, and every writing path now hands
/// the parse to a [`Rewriter`] instead. Two benchmarks below measure exactly
/// this composition, and they are measurements of the *rewrite* rather than
/// of any path a reader takes, so a helper here is the honest home for it:
/// production has no function of this shape any more, and reintroducing one
/// would be a second way to write a document.
fn rewritten_here(source: &Path, plan: &Plan, on_change: OnChange) -> Vec<u8> {
    rewrite_ready(source, plan, on_change).expect("the file is the one the plan was made on");
    let original = std::fs::read(source).expect("read the source");
    rewrite_update(&original, plan, Job::Save, None).expect("rewrite the document")
}

/// Bytes put in place through a staged file and a rename.
///
/// **What `write_atomically` was**, and it went the same day and for the
/// same reason: every writing path stages a file and hands its *handle* to a
/// writer, so nothing in production has bytes of a document to write. The
/// atomicity itself is unchanged --- this is `stage` and `commit`, which is
/// what those paths do.
fn written_atomically(out: &Path, bytes: &[u8]) {
    let staged = stage(out, |file| {
        use std::io::Write as _;
        file.write_all(bytes)
            .and_then(|()| file.flush())
            .map_err(|e| Refusal::from(format!("could not write {out:?}: {e}")))
    })
    .expect("stage");
    commit(&staged, out).expect("commit");
}

/// [`write_split`] with the in-process writer. [`copy_here`]'s counterpart,
/// for the same reason.
fn split_here(
    source: &Path,
    plans: &[Plan],
    out: &Path,
    password: Option<&str>,
) -> Result<Split, Refusal> {
    write_split(source, plans, out, password, &Here)
}

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
                source: PageSource::Baseline(at as u32),
                turns,
                crop: None,
            })
            .collect(),
        redactions: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
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
                source: PageSource::Baseline(source),
                turns,
                crop: None,
            })
            .collect(),
        redactions: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
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
        let dir =
            std::env::temp_dir().join(format!("tpdf-save-{name}-{}-{serial}", std::process::id()));
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

    copy_here(&source, &keeping(2, &[(0, 0)]), &out, Some("swordfish")).expect("rewrite");

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
    let _serial = crate::save::print_lock();
    let why = print_bytes(&source, &keeping(2, &[(0, 0)]), 0, None, &Here)
        .expect_err("a print job from an encrypted document must be refused");
    assert!(
        why.message.contains("encrypted"),
        "the refusal names the reason: {}",
        why.message
    );
    // And the same document IS rewritable, which is what makes the refusal
    // above a decision about printing rather than about the document.
    let scratch = Scratch::new("print-enc");
    copy_here(
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
    let _serial = crate::save::print_lock();
    let why = print_bytes(&source, &keeping(2, &[(0, 0)]), 0, Some("swordfish"), &Here)
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
    let locked = print_bytes(&source, &keeping(2, &[(0, 0)]), 0, None, &Here)
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
fn a_print_job_over_a_changed_file_is_refused_or_says_so() {
    // **The state that gets a save refused and a print served.** The file
    // planted over the document here is a *valid* PDF of the same page
    // count, which is what makes this a test of the fingerprint rather than
    // of anything already in the path: `checked`'s page-count refusal cannot
    // fire on it, and the parse succeeds. The control runs first and is the
    // same call on the same file before anything lands over it.
    let Some(source) = fixture("rotated.pdf") else {
        println!(
            "[SKIP] a_print_job_over_a_changed_file_is_refused_or_says_so: generate testdata/ (BUILD.md)"
        );
        return;
    };
    let _serial = crate::save::print_lock();
    let scratch = Scratch::new("print-changed");
    let at = scratch.0.join("open.pdf");
    std::fs::copy(&source, &at).expect("plant the document the reader opened");
    let plan = plan_opened_as(&[0, 0, 0, 0], &at);

    // The control, first, so that what follows is a difference and not a
    // reading. A file nobody touched prints.
    print_bytes(&at, &plan, 0, None, &Here)
        .expect("an unchanged file is the whole point of the guard letting it through");

    // A newer copy of the same report landing over the open document: same
    // four pages, different bytes.
    let newer = scratch.0.join("newer.pdf");
    copy_here(
        &source,
        &keeping(4, &[(0, 1), (1, 0), (2, 0), (3, 0)]),
        &newer,
        None,
    )
    .expect("a sibling of the same shape");
    std::fs::copy(&newer, &at).expect("land it over the open document");

    let why = print_bytes(&at, &plan, 0, None, &Here)
        .expect_err("a print job over a file that is not the one opened must be refused");
    // The flag, not the wording, is what a caller branches on, and since
    // 2026-09-01 `print_document` rejects with the whole `Refusal` --- so
    // this is the value the window branches on rather than a fact set for a
    // reader that did not exist. The message names the escape as well,
    // which is what serves a reader whose window drew no button.
    assert!(
        why.changed,
        "the refusal is about the file, which is the fact a caller branches on: {}",
        why.message
    );
    assert!(
        why.message.contains("changed on disk since you opened it"),
        "the refusal names the fact: {}",
        why.message
    );
    // Not the page-count refusal wearing the same `changed` flag. That one
    // says how many pages each side has and is reached through the parse;
    // this one has to be reached before it.
    assert!(
        !why.message.contains("page(s)"),
        "the fingerprint refused it, not the page count: {}",
        why.message
    );
    assert!(
        why.message
            .contains("A print job is built from the file on disk"),
        "and names the operation it is about: {}",
        why.message
    );

    // **What crosses the IPC boundary, asserted against literals.** The
    // window parses these two names out of a rejection and nothing on this
    // side is compiled against its parser, so a rename here is silent in
    // both directions: `changed` under any other spelling is a flag the
    // frontend reads as absent, which is the reassuring branch --- no
    // Reload offered, no error, and a reader told a sentence with nothing
    // beside it. Asserted on the refusal this test already produced rather
    // than on a synthetic one, so what is checked is the value that
    // actually reaches the wire.
    let wire = serde_json::to_value(&why).expect("a refusal is what the command rejects with");
    assert_eq!(
        wire,
        serde_json::json!({ "message": why.message, "changed": true }),
        "the wire shape is two fields and their names are the contract: {wire}"
    );
}

/// The rejection shape `print_document` and the window agree on.
///
/// Separate from the test above because that one needs a fixture and this
/// one is about the derive: a `[SKIP]` on a missing `testdata/` must not
/// take the contract with it.
#[test]
fn the_wire_shape_of_a_refusal_is_a_message_and_a_changed_flag() {
    let wire =
        serde_json::to_value(Refusal::changed("the file changed")).expect("a refusal serialises");
    assert_eq!(
        wire,
        serde_json::json!({ "message": "the file changed", "changed": true })
    );
    // The other direction, and it is the one that matters for a caller
    // deciding whether to offer Reload: everything that is not the file
    // having moved has to say so rather than say nothing.
    assert_eq!(
        serde_json::to_value(Refusal::from("could not be read")).expect("a refusal serialises"),
        serde_json::json!({ "message": "could not be read", "changed": false })
    );
    // A third field would be legal JSON, parse without complaint at the far
    // end, and be exactly how this contract grows past what the window was
    // written for. `json!` above compares whole objects, so it already
    // refuses one; this says so where a reader is looking.
    let object = serde_json::to_value(Refusal::from("x")).expect("a refusal serialises");
    assert_eq!(
        object
            .as_object()
            .expect("an object, not a bare string")
            .len(),
        2,
        "two fields and no more: {object}"
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
        &Here,
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
    let why = copy_here(&source, &keeping(2, &[(0, 0)]), &out, None)
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
        copy_here(&path, &plan_of(&turns), &out, None).unwrap_or_else(|e| panic!("{name}: {e}"));

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
    let merged = write_merged(
        &source,
        &plan_of(&vec![0u8; mine]),
        &[other],
        &out,
        None,
        &Here,
    )
    .expect("merge");
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
    write_merged(
        &source,
        &plan,
        std::slice::from_ref(&other),
        &out,
        None,
        &Here,
    )
    .expect("merge");
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
    let why = write_merged(&source, &plan_of(&[0, 0, 0, 0]), &[], &out, None, &Here)
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
    let (Some(source), Some(fixture_other)) = (fixture("rotated.pdf"), fixture("links.pdf")) else {
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
    let over_source = write_merged(
        &source,
        &plan,
        std::slice::from_ref(&other),
        &source,
        None,
        &Here,
    )
    .expect_err("over the open document");
    assert!(over_source.message.contains("reading"), "{over_source}");
    let over_input = write_merged(
        &source,
        &plan,
        std::slice::from_ref(&other),
        &other,
        None,
        &Here,
    )
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
    write_merged(&source, &plan, &[other], &out, None, &Here).expect("somewhere else is fine");
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
    let done = split_here(&source, &plans, &out, None).expect("split");
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

    let why = split_here(&source, &plans, &out, None).expect_err("refused");
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
    split_here(&source, &plans, &out, None).expect("the same call, with nothing in the way");
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
    let why = split_here(&source, &plans, &scratch.join("part.pdf"), None).expect_err("refused");
    assert!(why.message.contains("at least two files"), "{why}");
}

/// **No `examined > 0` control**, unlike its neighbours. This fixture needs
/// pyhanko, which the plain fixture run does not install --- so a checkout
/// that generated `testdata/` the ordinary way has every other fixture and
/// not this one, and a hard assertion here would be red on the machine with
/// the fewest inputs rather than on the machine with a defect.
#[test]
fn an_encrypted_document_cannot_be_merged_in() {
    let (Some(source), Some(locked)) = (fixture("rotated.pdf"), fixture("incr-encrypted-open.pdf"))
    else {
        println!("[SKIP] an_encrypted_document: needs testdata/incr-encrypted-open.pdf (pyhanko)");
        return;
    };
    let scratch = Scratch::new("merge-encrypted");
    let out = scratch.join("merged.pdf");
    let plan = plan_of(&vec![0u8; page_count(&source)]);
    let why = write_merged(
        &source,
        &plan,
        std::slice::from_ref(&locked),
        &out,
        None,
        &Here,
    )
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
    copy_here(&path, &plan_cropping(count, 0, want), &out, None).expect("write");

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
    plan.pages[1].source = PageSource::Baseline(0);
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
    copy_here(&path, &plan_of(&turns), &out, None).expect("write");

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
    copy_here(&source, &plan_of(&[0, 1]), &out, None).expect("write");

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
    copy_here(&source, &plan_of(&[1, 1]), &out, None).expect("agreeing turns are honoured");

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

    let why = copy_here(&source, &plan_of(&[1, 2]), &out, None).expect_err("must refuse");
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

    copy_here(&source, &plan_of(&[0, 0]), &out, None).expect("an unedited document still saves");
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
    copy_here(&path, &keeping(4, &[(0, 0), (2, 0), (3, 0)]), &out, None).expect("write");

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
    copy_here(&path, &keeping(4, &[(0, 0), (2, 0), (3, 1)]), &out, None).expect("write");

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
    copy_here(&path, &keeping(count as u32, &kept), &trimmed, None).expect("write");
    assert!(
        !has_outline(&Document::load(&trimmed).expect("load written")),
        "a page was dropped, so its destinations are gone"
    );

    // The control. Without it this check passes for a save that drops every
    // outline it ever sees, which is a different and much worse rule.
    let whole = scratch.join("whole.pdf");
    copy_here(&path, &plan_of(&vec![0u8; count]), &whole, None).expect("write");
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
    copy_here(&path, &keeping(count, &[(0, 0)]), &out, None).expect("write");
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
    copy_here(&path, &keeping(count, &kept), &out, None).expect("write");
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
    copy_here(&path, &plan_of(&vec![0u8; count]), &out, None).expect("write");
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
        if copy_here(&path, &plan, &out, None).is_err() {
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
    copy_here(&path, &keeping(count as u32, &kept), &out, None).expect("write");

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

    let why = copy_here(&source, &keeping(2, &[(0, 0)]), &out, None).expect_err("must refuse");
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
    copy_here(&source, &keeping(3, &[(2, 0)]), &out, None).expect("write");
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
    copy_here(
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
    copy_here(
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
    copy_here(&path, &keeping(4, &[(3, 1), (0, 0), (2, 0)]), &out, None).expect("write");

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
    copy_here(
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
    copy_here(
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
    copy_here(
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
    copy_here(&path, &keeping(count as u32, &kept), &out, None).expect("write");

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
        copy_here(&path, &keeping(4, &[(0, 0), (9, 0)]), &out, None).expect_err("must refuse");
    assert!(why.message.contains("does not have"), "{why}");
    assert!(!out.exists());
}

#[test]
fn an_encrypted_document_is_refused_rather_than_quietly_decrypted() {
    let scratch = Scratch::new("encrypted");
    let source = scratch.join("locked.pdf");
    std::fs::write(&source, encrypted_document()).expect("write fixture");
    let out = scratch.join("out.pdf");

    let why = copy_here(&source, &plan_of(&[0]), &out, None).expect_err("must refuse");
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
        copy_here(&path, &plan_of(&[0, 0]), &out, None)
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
        let why = copy_here(&path, &plan_of(&[0, 0]), &out, None)
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

    let why =
        copy_here(&path, &plan_of(&vec![0u8; count + 1]), &out, None).expect_err("must refuse");
    assert!(why.message.contains("changed since it was opened"), "{why}");
    assert!(!out.exists());

    // And the matching plan is accepted, so the refusal is about the
    // mismatch rather than about this document.
    copy_here(&path, &plan_of(&vec![0u8; count]), &out, None).expect("the matching plan writes");
    assert!(out.exists());
}

#[test]
fn an_empty_plan_is_refused() {
    let scratch = Scratch::new("empty");
    let out = scratch.join("out.pdf");
    let why = copy_here(
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

    let why = copy_here(&copy, &plan_of(&[1, 0, 0, 0]), &copy, None).expect_err("must refuse");
    assert!(why.message.contains("save over"), "{why}");
    assert_eq!(
        std::fs::read(&copy).expect("read"),
        before,
        "the document is untouched"
    );

    // The same file reached by a different spelling of the path is still the
    // same file --- a comparison of the strings would let this through.
    let indirect = scratch.join(".").join("copy.pdf");
    assert!(copy_here(&copy, &plan_of(&[1, 0, 0, 0]), &indirect, None).is_err());
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

    let staged =
        stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open), None, &Here).expect("stage");

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

    let staged =
        stage_in_place(&link, &plan_opened_as(&[1, 0, 0, 0], &link), None, &Here).expect("stage");
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

    let staged =
        stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open), None, &Here).expect("stage");
    commit_in_place(&staged.path, &open).expect("commit");

    assert_eq!(
        std::fs::metadata(&open).expect("stat").permissions().mode() & 0o777,
        0o640,
        "the save replaced the document with one anyone can read (ambient mode is {ambient:o})"
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

    let why = stage_in_place(&open, &plan_of(&[1, 0, 0, 0]), None, &Here).expect_err("must refuse");
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

    let staged =
        stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open), None, &Here).expect("stage");
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

    let staged =
        stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open), None, &Here).expect("stage");
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

    copy_here(&open, &plan_of(&[1, 0, 0, 0]), &out, None).expect("a copy needs no fingerprint");
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

    let copied = copy_here(&open, &plan, &out, None).expect("a copy risks nothing");
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

    let copied = copy_here(&open, &plan_opened_as(&[1, 0, 0, 0], &open), &out, None)
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

    let why = copy_here(&open, &plan, &out, None).expect_err("must refuse");
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

    let staged =
        stage_in_place(&open, &plan_opened_as(&[1, 0, 0, 0], &open), None, &Here).expect("stage");
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
    copy_here(&path, &plan_of(&[0, 0, 0, 0]), &out, None).expect("a fresh destination is accepted");
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
    copy_here(&path, &plan_of(&[1, 1, 1, 1]), &out, None).expect("write");
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

    copy_here(&path, &plan_of(&[0, 0, 0, 0]), &out, None).expect("write");

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
            source: PageSource::Baseline(0),
            turns: 0,
            crop: None,
        }],
        redactions: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
        marks: vec![PlannedMark {
            kind,
            // The biconditional the model enforces, restated here because
            // this builds a plan directly: a stamp with no name draws an
            // empty border, which is a box, so a test written for a stamp
            // would be measuring the wrong kind.
            stamp: (kind == MarkKind::Stamp).then_some(crate::docmodel::StampName::Draft),
            reply_to: None,
            at: 0,
            quads,
            strokes: Vec::new(),
            color: [1.0, 0.9, 0.2],
            width: crate::docmodel::INK_WIDTH,
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
            source: PageSource::Baseline(0),
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
        discards: Vec::new(),
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

/// A two-page document with one annotation, on the second page.
///
/// Two pages so that a plan keeping one of them is **not** an append, which
/// is the only way the rewrite path is reached at all --- and the annotation
/// is on the page that survives, so a refusal here would be about the
/// comment rather than about the page it went with.
fn document_with_a_comment_on_the_second_page() -> (Vec<u8>, lopdf::ObjectId) {
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
    let first = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    let second = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Annots" => vec![annot.into()],
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![first.into(), second.into()],
            "Count" => 2,
        }),
    );
    let catalog = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("the fixture must save");
    (bytes, annot)
}

/// A two-page fixture whose comment on the second page carries a **drawn
/// appearance**, and the marker its stream holds.
///
/// [`document_with_a_comment_on_the_second_page`]'s shape with one addition,
/// and the addition is the whole point: `/AP` is a stream reachable only from
/// the annotation, so removing the annotation orphans it. A fixture without
/// one cannot tell a writer that sweeps from one that does not --- `forget`
/// deletes the annotation's own dictionary either way, and the leftover a
/// reader would care about is the picture of the words.
///
/// **Uncompressed on purpose.** The check greps the written bytes for the
/// marker, which is the only way to ask whether the stream is *gone* rather
/// than merely unreachable, and a deflated stream answers that question with
/// noise. `docs/TRAPS.md` records the image removal this is copied from.
fn document_with_a_drawn_comment() -> (Vec<u8>, lopdf::ObjectId, &'static str) {
    use lopdf::dictionary;
    use lopdf::Stream;

    const MARKER: &str = "sackedFromTheBoard";
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let appearance = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()],
        },
        format!("BT /F1 12 Tf ({MARKER}) Tj ET").into_bytes(),
    ));
    let annot = document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "Contents" => Object::string_literal("before"),
        "M" => Object::string_literal("D:20260101000000Z"),
        "AP" => dictionary! { "N" => appearance },
    });
    let first = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    let second = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Annots" => vec![annot.into()],
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![first.into(), second.into()],
            "Count" => 2,
        }),
    );
    let catalog = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("the fixture must save");
    assert!(
        String::from_utf8_lossy(&bytes).contains(MARKER),
        "the fixture must hold the marker in the clear, or the check below \
         cannot fail"
    );
    (bytes, annot, MARKER)
}

#[test]
fn a_deleted_comment_leaves_the_page_and_leaves_no_bytes_behind() {
    // **Two assertions and only the second is the interesting one.** That the
    // page stops listing the annotation is what `pagetree::forget` does and
    // is easy; that the annotation's *appearance stream* is gone from the
    // file is the sweep, which runs on a condition this deletion had to be
    // added to. Without that clause the page draws nothing, the annotation
    // is unreachable, and every byte of the words the reader deleted is
    // still in the file --- which is the picture's trap, in the one place
    // the leftover is text somebody asked to be rid of.
    let (original, annot, marker) = document_with_a_drawn_comment();

    let mut plan = plan_of(&[0, 0]);
    plan.discards = vec![crate::edits::PlannedDiscard {
        object: (annot.0, annot.1),
    }];
    assert!(
        !plan.is_appendable(),
        "the premise: a deletion cannot be an append"
    );

    let bytes = rewrite_update(&original, &plan, Job::Save, None).expect("the rewrite");
    let after = Document::load_mem(&bytes).expect("the rewritten file must parse");
    assert_eq!(after.get_pages().len(), 2, "both pages were kept");

    for page in after.get_pages().values() {
        let annots = after
            .get_dictionary(*page)
            .expect("the page")
            .get(b"Annots")
            .and_then(Object::as_array)
            .map_or(0, Vec::len);
        assert_eq!(annots, 0, "a page still lists the deleted comment");
    }

    assert!(
        !String::from_utf8_lossy(&bytes).contains(marker),
        "the deleted comment's appearance stream is still in the file"
    );

    // **The control, and it is what makes the grep mean anything.** The same
    // fixture and the same writer with nothing deleted: the marker is there.
    // Without this the assertion above passes for a writer that never wrote
    // the stream at all, and for a fixture that never held it.
    let kept = rewrite_update(&original, &plan_of(&[0, 0]), Job::Save, None)
        .expect("the rewrite with nothing deleted");
    assert!(
        String::from_utf8_lossy(&kept).contains(marker),
        "the control lost the stream too, so the check above says nothing"
    );
}

#[test]
fn a_deletion_naming_something_that_is_not_an_annotation_is_refused() {
    // `set_note`'s refusal and its reason: a plan naming an arbitrary object
    // would otherwise let a caller delete a font, a page or the catalog out
    // of the file. Both halves, because a writer that refused everything
    // would satisfy either one alone --- and the control is the same call
    // with the real annotation, which is the test above.
    let (original, _, _) = document_with_a_drawn_comment();

    let mut plan = plan_of(&[0, 0]);
    plan.discards = vec![crate::edits::PlannedDiscard { object: (9_999, 0) }];
    assert!(
        rewrite_update(&original, &plan, Job::Save, None).is_err(),
        "an object the document does not have was accepted"
    );

    // A page, which is in the document and is not an annotation.
    let page = *Document::load_mem(&original)
        .expect("reload")
        .get_pages()
        .values()
        .next()
        .expect("a page");
    plan.discards = vec![crate::edits::PlannedDiscard {
        object: (page.0, page.1),
    }];
    assert!(
        rewrite_update(&original, &plan, Job::Save, None).is_err(),
        "a page was accepted as a comment to delete"
    );
}

/// A page tpdf made is written as a page, in the reader's order, and is
/// blank.
///
/// **The order is the assertion that matters**, and a count of pages cannot
/// make it: three pages come out whether the new one landed in the middle or
/// at either end. What distinguishes them is which page object each slot
/// holds, so this reads `/Kids` and asks which of the three is the one with
/// no `/Contents`.
#[test]
fn an_inserted_page_is_written_between_the_two_it_was_put_between() {
    let (original, _) = document_with_a_comment_on_the_second_page();

    let mut plan = plan_of(&[0, 0]);
    plan.pages.insert(
        1,
        PageView {
            id: 99,
            source: PageSource::Blank(crate::docmodel::Size {
                width: 200.0,
                height: 400.0,
            }),
            turns: 0,
            crop: None,
        },
    );
    assert!(
        !plan.is_appendable(),
        "the premise: a plan carrying a page no file supplies cannot be an append"
    );

    let bytes = rewrite_update(&original, &plan, Job::Save, None).expect("the rewrite");
    let after = Document::load_mem(&bytes).expect("the rewritten file must parse");
    let pages = ordered_pages(&after);
    assert_eq!(pages.len(), 3, "two from the file and one made");

    let dictionaries: Vec<&lopdf::Dictionary> = pages
        .iter()
        .map(|&id| after.get_dictionary(id).expect("a page dictionary"))
        .collect();
    // **Which slot holds which page, by a property only one of them has.**
    // The made page is the only one 200 by 400, and the file's second page
    // is the only one carrying an annotation --- so this says the new page
    // landed *between* them rather than merely being somewhere in a file of
    // three. An assertion about `/Contents` would say nothing: the fixture's
    // own pages have none either, so it would hold by construction.
    let media = |at: usize| -> Vec<f64> {
        dictionaries[at]
            .get(b"MediaBox")
            .and_then(Object::as_array)
            .expect("a media box")
            .iter()
            .map(|v| v.as_float().expect("a number") as f64)
            .collect()
    };
    assert_eq!(media(0), vec![0.0, 0.0, 595.0, 842.0], "the file's first");
    assert_eq!(media(1), vec![0.0, 0.0, 200.0, 400.0], "the made page");
    assert_eq!(media(2), vec![0.0, 0.0, 595.0, 842.0], "the file's second");
    assert!(
        !dictionaries[0].has(b"Annots") && dictionaries[2].has(b"Annots"),
        "the comment the fixture put on its second page is still on it, and \
         it is last"
    );
    assert!(
        !dictionaries[1].has(b"Contents") && !dictionaries[1].has(b"Annots"),
        "the made page draws nothing and carries nothing"
    );

    assert_eq!(
        dictionaries[1]
            .get(b"Type")
            .and_then(Object::as_name)
            .expect("a type"),
        b"Page"
    );
    assert!(
        dictionaries[1].has(b"Resources"),
        "a page that inherits no resources faults in several readers"
    );
    // Written by the tree rebuild rather than by the page's own creation,
    // which is the one thing about `make_blank_pages` that is easy to write
    // twice --- see its doc comment.
    assert!(dictionaries[1].has(b"Parent"), "it hangs under the tree");
}

/// A made page takes the reader's turn, on the object it was given.
///
/// Separate from the test above because it is the half that could quietly do
/// nothing: `turn_pages` is handed object ids, and a made page's is one this
/// rewrite created a few statements earlier rather than one `ordered_pages`
/// found. The control is the file's own first page, which took no turn.
///
/// There is no crop here because a made page cannot have one --- see
/// `Refusal::CropOnMadePage`.
#[test]
fn a_made_page_is_turned_like_any_other() {
    let (original, _) = document_with_a_comment_on_the_second_page();

    let mut plan = plan_of(&[0, 0]);
    plan.pages.push(PageView {
        id: 99,
        source: PageSource::Blank(crate::docmodel::Size {
            width: 200.0,
            height: 400.0,
        }),
        turns: 1,
        crop: None,
    });

    let bytes = rewrite_update(&original, &plan, Job::Save, None).expect("the rewrite");
    let after = Document::load_mem(&bytes).expect("the rewritten file must parse");
    let pages = ordered_pages(&after);
    let made = after.get_dictionary(pages[2]).expect("the made page");

    assert_eq!(
        made.get(b"Rotate")
            .and_then(Object::as_i64)
            .expect("a rotation"),
        90
    );
    // The control: the file's own pages took no turn, so this is evidence
    // about the made page rather than about the writer turning everything.
    let first = after.get_dictionary(pages[0]).expect("the first page");
    assert!(!first.has(b"Rotate"));
}

/// A mark on a page tpdf made lands on that page and on no other.
///
/// **The reason `PlannedMark` names a page by its position rather than by
/// its baseline number.** A made page has no baseline number, so under the
/// old addressing there was nothing to put in the plan --- which is why the
/// model refused the mark at the moment the reader drew it, rather than
/// letting the save be the first thing to say no.
///
/// Two assertions and both are needed. That the made page carries the
/// annotation is what the increment is for; that the file's own pages do
/// not is what says the mark went where the reader put it, because a writer
/// that resolved the position against the wrong list would still have
/// written *a* mark somewhere and the first assertion alone cannot see that.
/// The plan puts the made page last, so its position (2) is a number no
/// baseline page in this two-page document has --- under the old addressing
/// this is a refusal rather than a wrong page, and the test is written to
/// pass only on the right one.
#[test]
fn a_mark_on_a_page_tpdf_made_is_written_onto_that_page() {
    let (original, _) = document_with_a_comment_on_the_second_page();

    let mut plan = plan_of(&[0, 0]);
    plan.pages.push(PageView {
        id: 99,
        source: PageSource::Blank(crate::docmodel::Size {
            width: 200.0,
            height: 400.0,
        }),
        turns: 0,
        crop: None,
    });
    plan.marks = vec![PlannedMark {
        kind: MarkKind::Highlight,
        stamp: None,
        reply_to: None,
        at: 2,
        quads: vec![crate::docmodel::Quad {
            left: 10.0,
            top: 10.0,
            right: 100.0,
            bottom: 40.0,
        }],
        strokes: Vec::new(),
        color: [1.0, 0.9, 0.2],
        width: crate::docmodel::INK_WIDTH,
        author: "Reader".into(),
        note: String::new(),
        made: "D:20260830120000Z".into(),
    }];
    assert!(
        !plan.is_appendable(),
        "the premise: a plan carrying a page tpdf made is not the file, so \
         this goes out through the rewrite"
    );

    let bytes = rewrite_update(&original, &plan, Job::Save, None).expect("the rewrite");
    let after = Document::load_mem(&bytes).expect("the rewritten file must parse");
    let pages = ordered_pages(&after);
    assert_eq!(
        pages.len(),
        3,
        "two of the file's pages and the one tpdf made"
    );

    let subtypes = |page: lopdf::ObjectId| -> Vec<String> {
        let Ok(dictionary) = after.get_dictionary(page) else {
            return Vec::new();
        };
        let Ok(annots) = dictionary.get(b"Annots").and_then(Object::as_array) else {
            return Vec::new();
        };
        annots
            .iter()
            .filter_map(|entry| {
                let object = after.get_object(entry.as_reference().ok()?).ok()?;
                let subtype = object.as_dict().ok()?.get(b"Subtype").ok()?;
                Some(String::from_utf8_lossy(subtype.as_name().ok()?).into_owned())
            })
            .collect()
    };

    assert_eq!(subtypes(pages[2]), vec!["Highlight".to_string()]);
    assert!(
        subtypes(pages[0]).is_empty(),
        "the first page of the file has nothing on it"
    );
    assert_eq!(
        subtypes(pages[1]),
        vec!["Text".to_string()],
        "and the second keeps the comment it came with, and gains nothing"
    );
}

/// A rewrite writes the note edits an append would, on a plan no append can
/// take.
///
/// **The path a reader reaches by editing a comment and then deleting a
/// page**, which is the case `write_note_edits` alone does not cover:
/// [`Plan::is_appendable`] says no the moment the pages are not the file, so
/// the save goes through the full serialiser instead --- and until this
/// wiring existed it went through it *silently*, dropping the edit and
/// reporting a successful save.
#[test]
fn a_rewrite_writes_the_new_body_over_the_old() {
    let (original, annot) = document_with_a_comment_on_the_second_page();

    let mut plan = plan_of(&[0, 0]);
    // Keep only the second page, which is what makes this a rewrite.
    plan.pages = vec![PageView {
        id: 2,
        source: PageSource::Baseline(1),
        turns: 0,
        crop: None,
    }];
    plan.notes = vec![crate::edits::PlannedNoteEdit {
        object: (annot.0, annot.1),
        body: "after".into(),
        made: "D:20260829120000Z".into(),
    }];
    assert!(
        !plan.is_appendable(),
        "the premise: a plan that drops a page cannot be an append, so this \
         test exercises the writer the append test cannot reach"
    );

    let bytes = rewrite_update(&original, &plan, Job::Save, None).expect("the rewrite");
    let after = Document::load_mem(&bytes).expect("the rewritten file must parse");
    assert_eq!(after.get_pages().len(), 1, "one page was kept");

    // Found by walking the surviving page rather than by the object id: a
    // rewrite renumbers nothing today, and a test that assumed so would be
    // asserting the writer's bookkeeping instead of what a reader sees.
    let page = *after.get_pages().values().next().expect("the kept page");
    let annots = after
        .get_dictionary(page)
        .expect("the page")
        .get(b"Annots")
        .and_then(Object::as_array)
        .expect("the page keeps its annotations");
    assert_eq!(annots.len(), 1);
    let dictionary = after
        .get_object(annots[0].as_reference().expect("an indirect annotation"))
        .expect("the annotation")
        .as_dict()
        .expect("a dictionary");
    assert_eq!(
        dictionary
            .get(b"Contents")
            .and_then(Object::as_str)
            .expect("a body"),
        b"after",
        "the rewrite must carry the note edit the append carries"
    );
    assert_eq!(
        dictionary
            .get(b"M")
            .and_then(Object::as_str)
            .expect("a date"),
        b"D:20260829120000Z",
        "`/M` moves on this path too, or the two writers disagree about one \
         comment"
    );
}

/// A note edit naming something that is not an annotation is refused, on the
/// rewrite path as on the append.
///
/// The `/Subtype` guard, reached through the second caller. It is the reason
/// [`set_note`] is shared rather than copied: without it a plan could write
/// `/Contents` onto a page object, where the key means the page's content
/// stream, and the document would be destroyed by a save that reported
/// success.
#[test]
fn a_reply_is_written_as_one_and_reads_back_as_one() {
    // **Read back through `annots::scan` rather than through the object
    // graph here, and never through PDFium.** `pdfium-render` does not
    // expose `/IRT` at all --- which is the reason comments are read through
    // `lopdf` in the first place --- so a reply that failed to set it would
    // not fail loudly through that reader, it would arrive as an unrelated
    // second note by another author. `annots.rs` is a separate
    // implementation from this file, which is what makes this a differential
    // rather than the writer agreeing with itself.
    let (original, annot) = document_with_a_comment_on_the_second_page();

    let mut plan = plan_of(&[0, 0]);
    plan.marks = vec![PlannedMark {
        kind: MarkKind::Note,
        at: 1,
        quads: vec![crate::docmodel::Quad {
            left: 10.0,
            top: 10.0,
            right: 30.0,
            bottom: 30.0,
        }],
        strokes: Vec::new(),
        stamp: None,
        reply_to: Some((annot.0, annot.1)),
        color: [1.0, 0.9, 0.2],
        width: crate::docmodel::INK_WIDTH,
        author: "Reader".into(),
        note: "I answer it.".into(),
        made: "D:20260829120000Z".into(),
    }];
    assert!(
        plan.is_appendable(),
        "the premise: a reply only adds, so it goes out through the append"
    );

    let update = append_update(original.clone(), &plan, None).expect("the append");
    let mut after = original.clone();
    after.extend_from_slice(&update.update);

    let scanned = crate::annots::scan(&after, 2, None).expect("the appended file must scan");
    assert_eq!(scanned.items.len(), 2, "the file's comment and the reply");

    // Found by body rather than by position, because the scan's order is the
    // file's and this test is not about that.
    let parent = scanned
        .items
        .iter()
        .position(|item| item.body == "before")
        .expect("the comment that was already there");
    let reply = scanned
        .items
        .iter()
        .position(|item| item.body == "I answer it.")
        .expect("the reply");
    assert_eq!(
        scanned.items[reply].reply_to,
        Some(parent as u32),
        "the reply must answer the comment it named"
    );
    assert_eq!(
        scanned.items[parent].reply_to, None,
        "and the comment it answers must answer nothing"
    );

    // **`/RT` is asserted here and by nothing else**, because `annots.rs`
    // reads `/IRT` and never asks what kind of relationship it is --- so the
    // scan above cannot see this key at all. The dictionary comment calls it
    // belt and braces, which is exactly the sort of claim that stays true
    // only while somebody checks it: a reader that saw `/Group` instead
    // would show the two comments as one annotation rather than as a thread.
    let after = Document::load_mem(&after).expect("the appended file must parse");
    let written = after
        .objects
        .values()
        .filter_map(|object| object.as_dict().ok())
        .find(|dictionary| dictionary.get(b"IRT").is_ok())
        .expect("the reply's own dictionary");
    assert_eq!(
        written.get(b"RT").and_then(Object::as_name).expect("/RT"),
        b"R",
        "the reply must say it is a reply rather than a group"
    );
}

/// The control for the test above: the same reply, on a mark that answers
/// nothing, is threaded under nobody.
///
/// Without it, a scan that reported *every* comment as a reply to the first
/// would pass the assertion above and read as the writer working.
#[test]
fn a_comment_that_answers_nothing_is_threaded_under_nobody() {
    let (original, _) = document_with_a_comment_on_the_second_page();

    let mut plan = plan_of(&[0, 0]);
    plan.marks = vec![PlannedMark {
        kind: MarkKind::Note,
        at: 1,
        quads: vec![crate::docmodel::Quad {
            left: 10.0,
            top: 10.0,
            right: 30.0,
            bottom: 30.0,
        }],
        strokes: Vec::new(),
        stamp: None,
        reply_to: None,
        color: [1.0, 0.9, 0.2],
        width: crate::docmodel::INK_WIDTH,
        author: "Reader".into(),
        note: "I answer it.".into(),
        made: "D:20260829120000Z".into(),
    }];

    let update = append_update(original.clone(), &plan, None).expect("the append");
    let mut after = original.clone();
    after.extend_from_slice(&update.update);

    let scanned = crate::annots::scan(&after, 2, None).expect("the appended file must scan");
    assert!(
        scanned.items.iter().all(|item| item.reply_to.is_none()),
        "no comment here answers another"
    );
}

/// A reply naming an object that is not an annotation is refused, on both
/// save paths.
///
/// **Both, in one test, because the guard has two call sites.** It cannot
/// live inside `write_marks` --- the two paths hand that function different
/// documents --- so it is called once in each, and a test exercising one
/// would pass while the other wrote `/IRT` at a page. The proof token is
/// what makes forgetting the call a compile error; this is what makes the
/// call correct.
#[test]
fn a_reply_naming_something_that_is_not_an_annotation_is_refused_on_both_paths() {
    let (original, annot) = document_with_a_comment_on_the_second_page();
    let page_object = Document::load_mem(&original)
        .expect("the fixture parses")
        .get_pages()
        .into_iter()
        .next()
        .expect("a page")
        .1;

    let reply_naming = |object: lopdf::ObjectId| PlannedMark {
        kind: MarkKind::Note,
        at: 1,
        quads: vec![crate::docmodel::Quad {
            left: 10.0,
            top: 10.0,
            right: 30.0,
            bottom: 30.0,
        }],
        strokes: Vec::new(),
        stamp: None,
        reply_to: Some((object.0, object.1)),
        color: [1.0, 0.9, 0.2],
        width: crate::docmodel::INK_WIDTH,
        author: "Reader".into(),
        note: "I answer a page.".into(),
        made: "D:20260829120000Z".into(),
    };

    let mut appending = plan_of(&[0, 0]);
    appending.marks = vec![reply_naming(page_object)];
    let refused =
        append_update(original.clone(), &appending, None).expect_err("a page is not a comment");
    assert!(
        refused.message.contains("not an annotation"),
        "the append's refusal must say what is wrong: {refused:?}"
    );

    let mut rewriting = plan_of(&[0, 0]);
    rewriting.pages = vec![PageView {
        id: 2,
        source: PageSource::Baseline(1),
        turns: 0,
        crop: None,
    }];
    rewriting.marks = vec![reply_naming(page_object)];
    assert!(
        !rewriting.is_appendable(),
        "the premise: dropping a page is what forces the other writer"
    );
    let refused = rewrite_update(&original, &rewriting, Job::Save, None)
        .expect_err("a page is not a comment on this path either");
    assert!(
        refused.message.contains("not an annotation"),
        "the rewrite's refusal must say what is wrong: {refused:?}"
    );

    // The control, and it is the one that keeps both assertions above from
    // being satisfied by a writer that refuses every reply.
    let mut honest = plan_of(&[0, 0]);
    honest.marks = vec![reply_naming(annot)];
    assert!(
        append_update(original, &honest, None).is_ok(),
        "a reply naming the real comment must go through"
    );
}

#[test]
fn a_rewrite_refuses_a_note_edit_that_names_a_page() {
    let (original, _) = document_with_a_comment_on_the_second_page();
    let page_object = Document::load_mem(&original)
        .expect("the fixture parses")
        .get_pages()
        .into_iter()
        .next()
        .expect("a page")
        .1;

    let mut plan = plan_of(&[0, 0]);
    plan.pages = vec![PageView {
        id: 2,
        source: PageSource::Baseline(1),
        turns: 0,
        crop: None,
    }];
    plan.notes = vec![crate::edits::PlannedNoteEdit {
        object: (page_object.0, page_object.1),
        body: "after".into(),
        made: "D:20260829120000Z".into(),
    }];

    let refused =
        rewrite_update(&original, &plan, Job::Save, None).expect_err("a page is not a comment");
    assert!(
        refused.message.contains("not an annotation"),
        "the refusal must say what is wrong: {refused:?}"
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
        reply_to: None,
        at: 0,
        quads: one_quad(),
        strokes: Vec::new(),
        color: [1.0, 0.9, 0.2],
        width: crate::docmodel::INK_WIDTH,
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

    // The fifth, and it is the one whose evidence is structural rather than
    // spike 0.6's: an append cannot create a page object, so a plan carrying
    // a page no file supplies has nothing an append could write.
    let mut inserted = marked.clone();
    inserted.pages.push(PageView {
        id: 99,
        source: PageSource::Blank(crate::docmodel::Size {
            width: 595.0,
            height: 842.0,
        }),
        turns: 0,
        crop: None,
    });
    assert_eq!(mode_for(&inserted, small), Mode::Rewrite, "an insert");

    // The sixth, and the only one that is not about the pages. An append
    // adds objects and cannot remove one, so a deletion has nothing it could
    // write --- and unlike the five above, the fixture has to keep the mark:
    // a plan holding only the deletion has no marks and no note edits, which
    // makes the answer `Rewrite` for the *first* clause whether or not this
    // one exists. `docs/TRAPS.md` has that as the entry about a fixture where
    // the right rule and the wrong rule agree.
    let mut deleted_comment = marked.clone();
    deleted_comment.discards = vec![crate::edits::PlannedDiscard { object: (7, 0) }];
    assert_eq!(
        mode_for(&deleted_comment, small),
        Mode::Rewrite,
        "a deleted comment, beside a mark that would otherwise append"
    );
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
    let Some((at, plan)) = appendable_with(&scratch, "incr-encrypted-pw.pdf", Some("swordfish"))
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
        refused.contains(&appended.was.to_string()) && refused.contains(&meddled.len().to_string()),
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
    let refused =
        appended(append_ready(&at, &plan).expect("check again"), stale).expect_err("must refuse");
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
            std::fs::FileTimes::new()
                .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60)),
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

    let refused = append_through(&mut file, &appended, &at, None, &Here).expect_err("must report");
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

    let refused = append_in_place(&appended, &at, None, &Here).expect_err("the re-read refuses");
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
    /// What each call said the rewrite was for.
    ///
    /// Kept beside `asked` rather than as a third element of its tuples,
    /// because the tests that read `asked` are about the length and the
    /// password and would all have to be rewritten to say nothing new.
    jobs: std::cell::RefCell<Vec<Job>>,
    /// Every page-range job this writer was asked to build.
    ///
    /// A list of its own rather than a third `jobs` variant, because it
    /// answers a different question: `jobs` says what a *plan* was for, and
    /// this says that a plan was never involved. A test asserting the
    /// coordinator delegated the range parse has to see the range.
    ranges: std::cell::RefCell<Vec<crate::print::Job>>,
    /// Every merge this writer was asked for: the bytes handed over, and the
    /// spans naming the documents inside them.
    ///
    /// The bytes are kept whole rather than counted, because the question a
    /// test asks here is whether the *files the reader chose* crossed --- and
    /// a length is equally satisfied by the same file twice.
    merges: std::cell::RefCell<Vec<(Vec<u8>, Vec<Incoming>)>>,
}

impl FakeWriter {
    fn writing(answer: Result<Vec<u8>, Refusal>) -> Self {
        Self {
            answer,
            overstate_by: 0,
            asked: std::cell::RefCell::new(Vec::new()),
            jobs: std::cell::RefCell::new(Vec::new()),
            ranges: std::cell::RefCell::new(Vec::new()),
            merges: std::cell::RefCell::new(Vec::new()),
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
        job: Job,
        password: Option<&str>,
    ) -> Result<usize, Refusal> {
        use std::io::Write as _;

        self.asked
            .borrow_mut()
            .push((len, password.map(str::to_string)));
        self.jobs.borrow_mut().push(job);
        let bytes = self.answer.clone()?;
        out.write_all(&bytes).map_err(|e| e.to_string())?;
        Ok(bytes.len() + self.overstate_by)
    }

    fn merge(
        &self,
        _source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        _plan: &Plan,
        inputs: Inputs<'_>,
        password: Option<&str>,
    ) -> Result<(usize, u32), Refusal> {
        use std::io::Write as _;

        self.asked
            .borrow_mut()
            .push((len, password.map(str::to_string)));
        self.merges
            .borrow_mut()
            .push((inputs.whole.to_vec(), inputs.each.to_vec()));
        let bytes = self.answer.clone()?;
        out.write_all(&bytes).map_err(|e| e.to_string())?;
        // A page count this writer invents. The coordinator has no way to
        // check it and must not: counting the pages here would mean parsing
        // the merged document, which is the parse that moved.
        Ok((bytes.len() + self.overstate_by, 7))
    }

    fn write_range(
        &self,
        _source: &mut std::fs::File,
        len: usize,
        out: &mut std::fs::File,
        job: &crate::print::Job,
    ) -> Result<usize, Refusal> {
        use std::io::Write as _;

        self.asked.borrow_mut().push((len, None));
        self.ranges.borrow_mut().push(job.clone());
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
fn the_coordinator_does_not_parse_the_document_it_copies() {
    // **The keystone of the copy path's move, and it is red on the code
    // this replaced.** The source planted here is not a PDF at all, so a
    // coordinator that parsed it would refuse before writing anything ---
    // which is what `planned_bytes` did until 2026-09-01, on Save a copy,
    // on Extract and on Redact to a copy. Save a copy is also the operation
    // an in-place refusal points a reader at, so it is the parse a reader
    // is most often told to reach for.
    //
    // The copy landing on those bytes is what says the parse is somewhere
    // else now. It is the accounting observable for a property that is
    // otherwise invisible: every number a caller can see is the same
    // whichever process parsed, so the thing to assert is *who was asked*.
    let scratch = Scratch::new("copy-delegates");
    let (at, plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
    let out = scratch.join("copy.pdf");
    let writer = FakeWriter::writing(Ok(b"%PDF-1.7 whatever the worker produced".to_vec()));

    write_copy(&at, &plan, &out, None, &writer).expect("the writer's bytes are it");

    assert_eq!(
        writer.asked.borrow().len(),
        1,
        "asked exactly once, so the copy has one rewrite in it and not two"
    );
    assert_eq!(
        std::fs::read(&out).expect("read the copy"),
        b"%PDF-1.7 whatever the worker produced",
        "the copy holds what the writer wrote and nothing this process made"
    );
    assert_eq!(
        leftovers_beside(&out),
        Vec::<String>::new(),
        "and the staging file it was written through is gone"
    );
}

#[test]
fn a_copy_whose_writer_refuses_leaves_nothing_at_the_destination() {
    // The other direction, and it is what makes the test above a statement
    // about delegation rather than about a file appearing. A refusal from
    // the writer has to reach the reader as its own sentence, with no
    // partial document under the name they chose and no staging file beside
    // it --- `stage` owns the second half and this is what says the copy
    // path really goes through it.
    let scratch = Scratch::new("copy-refused");
    let (at, plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
    let out = scratch.join("copy.pdf");
    let writer = FakeWriter::writing(Err(
        "this document keeps a page the plan does not name".into()
    ));

    let why = write_copy(&at, &plan, &out, None, &writer).expect_err("must refuse");

    assert!(
        why.message.contains("keeps a page"),
        "the writer's own refusal reaches the reader: {}",
        why.message
    );
    assert!(!out.exists(), "and nothing was left at the name they chose");
    assert_eq!(
        leftovers_beside(&out),
        Vec::<String>::new(),
        "nor beside it under a name they did not choose"
    );
}

#[test]
fn the_coordinator_does_not_parse_the_document_it_splits() {
    // [`the_coordinator_does_not_parse_the_document_it_copies`] for the
    // path that writes several files, and the count is the second half of
    // it: a split asks the writer once per part, so a fixture of two plans
    // that produced one answer would be a split writing one document twice.
    let scratch = Scratch::new("split-delegates");
    let (at, plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
    let out = scratch.join("part.pdf");
    let writer = FakeWriter::writing(Ok(b"%PDF-1.7 one part of a split".to_vec()));

    let done = write_split(&at, &[plan.clone(), plan], &out, None, &writer).expect("split");

    assert_eq!(
        writer.asked.borrow().len(),
        2,
        "asked once per part, so every file is its own rewrite"
    );
    assert_eq!(done.paths.len(), 2, "two plans, two files");
    for path in &done.paths {
        assert_eq!(
            std::fs::read(path).expect("read a part"),
            b"%PDF-1.7 one part of a split",
            "every part holds what the writer wrote and nothing this process made"
        );
    }
    assert_eq!(
        leftovers_beside(&out),
        Vec::<String>::new(),
        "and no staging file survives the run"
    );
}

/// How many print-job scratch files are sitting in the temporary directory.
///
/// The observable for "the job's own file was removed". It holds the
/// reader's document decrypted and reordered, so one left behind is a copy
/// of their document in a shared directory under a name they never chose.
/// Counted rather than named, because `job_scratch` picks its own name and a
/// test that guessed it would pass by agreeing with itself. Read under
/// [`print_lock`], which is what makes the count answerable at all.
fn print_scratch_files() -> usize {
    std::fs::read_dir(std::env::temp_dir())
        .map(|entries| {
            entries
                .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
                .filter(|name| name.starts_with("tpdf-print-job."))
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn the_coordinator_does_not_parse_the_document_it_prints() {
    // **The keystone of the print job's move.** The source planted here is
    // not a PDF, so a coordinator that parsed it would refuse before
    // building anything --- which is what `print_bytes` did until
    // 2026-09-01, on every print of a document the reader had edited.
    //
    // The job coming back on those bytes is what says the parse is
    // somewhere else now. What still happens here is the *read back* of the
    // job, which is bytes tpdf wrote a moment ago rather than the reader's
    // document, and the platform's own parse of them afterwards --- which is
    // the disclosed readback and the whole point of it being independent.
    let _serial = crate::save::print_lock();
    let scratch = Scratch::new("print-delegates");
    let (at, plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
    let writer = FakeWriter::writing(Ok(b"%PDF-1.7 the job the worker built".to_vec()));
    let before = print_scratch_files();

    let bytes = print_bytes(&at, &plan, 2, None, &writer).expect("the writer's bytes are it");

    assert_eq!(
        writer.asked.borrow().len(),
        1,
        "asked exactly once, so the job has one rewrite in it and not two"
    );
    assert_eq!(
        bytes, b"%PDF-1.7 the job the worker built",
        "the job is what the writer wrote and nothing this process made"
    );
    // **The reader's own rotation reaches the writer, and it is what a print
    // job has that a save does not.** A `Job::Save` here would put the pages
    // on paper the way the file holds them rather than the way the reader is
    // looking at them, and every byte-level assertion above would still pass.
    assert_eq!(
        writer.jobs.borrow().as_slice(),
        &[Job::Print { view: 2 }],
        "the writer is told this is paper, and which way up"
    );
    assert_eq!(
        print_scratch_files(),
        before,
        "and the scratch file the job was built in is gone"
    );
}

#[test]
fn a_print_job_whose_writer_refuses_leaves_no_scratch_file_behind() {
    // The other direction, and it is the one that matters most: the scratch
    // file holds the reader's document with its encryption off and its pages
    // reordered. A refusal is exactly when a cleanup is easiest to skip.
    let _serial = crate::save::print_lock();
    let scratch = Scratch::new("print-refused");
    let (at, plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
    let writer = FakeWriter::writing(Err("this document is encrypted".into()));
    let before = print_scratch_files();

    let why = print_bytes(&at, &plan, 0, None, &writer).expect_err("must refuse");

    assert!(
        why.message.contains("encrypted"),
        "the writer's own refusal reaches the reader: {}",
        why.message
    );
    assert_eq!(
        print_scratch_files(),
        before,
        "and nothing was left in the temporary directory"
    );
}

#[test]
fn the_coordinator_does_not_parse_the_document_it_prints_a_range_of() {
    // **The keystone of the last print route's move**, and the same shape as
    // the test above it: the source planted here is not a PDF, so a
    // coordinator that parsed it would refuse before building anything ---
    // which is what `print::build` did until 2026-09-01, on every print of a
    // page range a reader typed.
    //
    // `docs/THREAT-MODEL.md` residual risk 18 never named this path, because
    // that risk lists the operations that *write* a document and a range
    // print writes nothing. It parses the reader's document all the same,
    // and the way to reach it is to open a file and type two numbers.
    let _serial = crate::save::print_lock();
    let scratch = Scratch::new("range-delegates");
    let (at, _plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
    let job = crate::print::Job {
        pages: crate::print::Pages::Only(vec![crate::print::PagePlan {
            number: 2,
            turns: 1,
        }]),
        turns: 3,
    };
    let writer = FakeWriter::writing(Ok(b"%PDF-1.7 the range the worker built".to_vec()));
    let before = print_scratch_files();

    let bytes = print_range_bytes(&at, &job, &writer).expect("the writer's bytes are it");

    assert_eq!(
        bytes, b"%PDF-1.7 the range the worker built",
        "the job is what the writer wrote and nothing this process made"
    );
    // **The range reaches the writer intact**, which is the half a
    // byte-level assertion cannot see: a writer handed `Pages::All` would
    // produce a perfectly good job of the wrong pages, and every assertion
    // above would still pass.
    assert_eq!(
        writer.ranges.borrow().as_slice(),
        &[job],
        "the pages and the reader's own rotation are what crossed"
    );
    assert!(
        writer.jobs.borrow().is_empty(),
        "and no plan was involved --- a range says nothing about edits"
    );
    assert_eq!(
        print_scratch_files(),
        before,
        "and the scratch file the job was built in is gone"
    );
}

#[test]
fn a_range_print_whose_writer_refuses_leaves_no_scratch_file_behind() {
    // The cleanup, on the route that has no plan. **The removal itself is
    // shared** --- both routes go through `into_scratch`, so it happens by
    // construction and no mutation can tell the two apart, which is the
    // outcome-two-mechanisms-can-produce shape. What this pins is the other
    // half: that a range builder's *refusal* reaches the reader as its own
    // sentence and takes the same path out.
    let _serial = crate::save::print_lock();
    let scratch = Scratch::new("range-refused");
    let (at, _plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
    let job = crate::print::Job {
        pages: crate::print::Pages::Only(vec![crate::print::PagePlan {
            number: 1,
            turns: 0,
        }]),
        turns: 0,
    };
    let writer = FakeWriter::writing(Err("page 9 is not in this document".into()));
    let before = print_scratch_files();

    let why = print_range_bytes(&at, &job, &writer).expect_err("must refuse");

    assert!(
        why.message.contains("page 9"),
        "the writer's own refusal reaches the reader: {}",
        why.message
    );
    assert_eq!(
        print_scratch_files(),
        before,
        "and nothing was left in the temporary directory"
    );
}

#[test]
fn a_range_print_that_overstates_what_it_wrote_is_refused() {
    // `landed_is` is one function serving two builders, and a guard proved
    // on one caller is proved on one caller. What it stands between here is
    // a short write in another process and a document reaching paper with
    // pages missing off the end of it --- which no page count can see,
    // because a truncated job parses as however many pages survived.
    let _serial = crate::save::print_lock();
    let scratch = Scratch::new("range-overstated");
    let (at, _plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
    let job = crate::print::Job {
        pages: crate::print::Pages::Only(vec![crate::print::PagePlan {
            number: 1,
            turns: 0,
        }]),
        turns: 0,
    };
    let mut writer = FakeWriter::writing(Ok(b"%PDF-1.7 short".to_vec()));
    writer.overstate_by = 4_096;
    let before = print_scratch_files();

    let why = print_range_bytes(&at, &job, &writer).expect_err("must refuse");

    assert!(
        why.message.contains("was not completed"),
        "the length the writer claimed is checked against the file: {}",
        why.message
    );
    assert_eq!(
        print_scratch_files(),
        before,
        "and the partial job is not left behind"
    );
}

#[test]
fn the_coordinator_does_not_parse_the_documents_it_merges() {
    // **The keystone of the widest move.** Neither the source nor either
    // incoming file is a PDF, so a coordinator that parsed any of them would
    // refuse before building anything --- which is what `write_merged` did
    // until 2026-09-01, on every merge, for every file the reader picked in
    // a dialog.
    //
    // What this process still does is *read* those files, which is the whole
    // of its remaining part: it copies their bytes into one buffer and never
    // asks what they mean.
    let scratch = Scratch::new("merge-delegates");
    let (at, plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
    let first = scratch.join("first.pdf");
    let second = scratch.join("second.pdf");
    std::fs::write(&first, b"not a PDF either").expect("plant the first");
    std::fs::write(&second, b"nor is this one, and it is longer").expect("plant the second");
    let out = scratch.join("merged.pdf");
    let writer = FakeWriter::writing(Ok(b"%PDF-1.7 the merge the worker built".to_vec()));

    let merged = write_merged(
        &at,
        &plan,
        &[first.clone(), second.clone()],
        &out,
        None,
        &writer,
    )
    .expect("the writer's answer is it");

    assert_eq!(
        std::fs::read(&out).expect("the merge landed"),
        b"%PDF-1.7 the merge the worker built",
        "the file is what the writer wrote and nothing this process made"
    );
    // **The page count is the writer's, not a recount here.** Counting it in
    // this process would mean parsing the merged document, which is the
    // parse that moved; the fake answers 7 for any merge, so a coordinator
    // that recounted would report 0 or refuse.
    assert_eq!(
        merged.pages, 7,
        "the page count crossed back from the writer"
    );
    assert_eq!(merged.files, 2, "and the reader chose two files");

    // **The files the reader chose crossed, in order and whole.** A length
    // alone is equally satisfied by the same file twice, and a merge that
    // appended one file twice would produce a document with the right number
    // of pages in it.
    let asked = writer.merges.borrow();
    let (bytes, incoming) = asked.first().expect("asked exactly once");
    assert_eq!(asked.len(), 1, "and asked once, not once per file");
    assert_eq!(
        incoming,
        &vec![
            Incoming {
                at: 0,
                len: 16,
                label: "first.pdf".to_string(),
            },
            Incoming {
                at: 16,
                len: 33,
                label: "second.pdf".to_string(),
            },
        ],
        "each document is named by where it begins, how long it is and what to call it"
    );
    assert_eq!(
        &bytes[incoming[0].at..incoming[0].at + incoming[0].len],
        b"not a PDF either",
        "the first file's own bytes"
    );
    assert_eq!(
        &bytes[incoming[1].at..incoming[1].at + incoming[1].len],
        b"nor is this one, and it is longer",
        "and the second's, which is a different length"
    );
}

#[test]
fn a_merge_of_nothing_is_refused_in_the_worker_too() {
    // **The guard on the far side of the pipe**, which `write_merged`'s own
    // refusal makes unreachable from the application --- so it is reached
    // here directly, which is the only way it can be. It exists because the
    // worker must not trust the coordinator's request, and it says so in
    // words about the request rather than in the reader's.
    let scratch = Scratch::new("merge-nothing");
    let (at, plan) = staging_subject(&scratch, "one-page.pdf").expect("a subject");
    let base = std::fs::read(&at).expect("read the base");

    let why = merge_update(
        &base,
        &plan,
        Inputs {
            whole: &[],
            each: &[],
        },
        None,
    )
    .expect_err("a merge of nothing must be refused");

    assert!(
        why.message.contains("no documents to merge in"),
        "and the refusal is about the request, not about the reader: {}",
        why.message
    );
}

#[test]
fn a_merge_that_overstates_what_it_wrote_is_refused() {
    // `landed_is` on the third caller. What it stands between here is a
    // short write in another process and a merged document that is missing
    // whatever came last --- which the page count cannot see, because the
    // count comes from the same writer that reported the length.
    let scratch = Scratch::new("merge-overstated");
    let (at, plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
    let first = scratch.join("first.pdf");
    std::fs::write(&first, b"not a PDF either").expect("plant it");
    let out = scratch.join("merged.pdf");
    let mut writer = FakeWriter::writing(Ok(b"%PDF-1.7 short".to_vec()));
    writer.overstate_by = 4_096;

    let why = write_merged(&at, &plan, &[first], &out, None, &writer).expect_err("must refuse");

    assert!(
        why.message.contains("was not completed"),
        "the length the writer claimed is checked against the file: {}",
        why.message
    );
    assert!(
        !out.exists(),
        "and the destination is untouched --- the staged file is what was written"
    );
}

#[test]
fn a_save_tells_the_writer_it_is_a_save() {
    // The control for the job assertion above, and it is what stops that one
    // being satisfied by a writer told `Print` for everything. The three save
    // paths differ from the print one in exactly this value.
    let scratch = Scratch::new("save-job");
    let (at, plan) = staging_subject(&scratch, "unparseable.pdf").expect("a subject");
    let out = scratch.join("copy.pdf");
    let writer = FakeWriter::writing(Ok(b"%PDF-1.7 written".to_vec()));

    stage_in_place(&at, &plan, None, &writer).expect("stage");
    write_copy(&at, &plan, &out, None, &writer).expect("copy");

    assert_eq!(
        writer.jobs.borrow().as_slice(),
        &[Job::Save, Job::Save],
        "a save is a save, however it is written"
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
    let want = usize::try_from(appended.was).expect("a length that fits") + appended.update.len();
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
        let built = rewritten_here(&path, &plan, OnChange::Refuse);
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
            built.len(),
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
            let whole = rewritten_here(&at, &plan, OnChange::Proceed);
            let wrote = whole.len();
            written_atomically(&out, &whole);
            rewrites.push((clock.elapsed().as_secs_f64() * 1000.0, wrote));

            let _ = std::fs::remove_file(&fresh);
        }

        // The fingerprint on its own, because it is most of both numbers and
        // is the finding rather than an aside.
        let clock = std::time::Instant::now();
        let _ = crate::fingerprint::Fingerprint::of(&at).expect("fingerprint");
        let hashing = clock.elapsed().as_secs_f64() * 1000.0;

        let best = |runs: &[(f64, usize)]| runs.iter().map(|run| run.0).fold(f64::MAX, f64::min);
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

        copy_here(&source, &plan_with_mark(one_quad()), &out, None)
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
    copy_here(&source, &plan_with_mark(one_quad()), &out, None).expect("save");

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
        copy_here(source, &plan, out, None).expect("save");
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
                source: PageSource::Baseline(0),
                turns: 0,
                crop: None,
            },
            PageView {
                id: 2,
                source: PageSource::Baseline(1),
                turns: 0,
                crop: None,
            },
        ],
        redactions: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
        marks: vec![PlannedMark {
            kind: MarkKind::Highlight,
            stamp: None,
            reply_to: None,
            at: 0,
            quads: one_quad(),
            strokes: Vec::new(),
            color: [1.0, 0.9, 0.2],
            width: crate::docmodel::INK_WIDTH,
            author: String::new(),
            note: String::new(),
            made: "D:20260818120000Z".to_string(),
        }],
    };
    let why = copy_here(&source, &plan, &out, None).expect_err("a shared page must be refused");
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
                source: PageSource::Baseline(source),
                turns: 0,
                crop: None,
            })
            .collect(),
        redactions: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
        marks: vec![PlannedMark {
            kind: MarkKind::Highlight,
            stamp: None,
            reply_to: None,
            at: 2,
            quads: one_quad(),
            strokes: Vec::new(),
            color: [1.0, 0.9, 0.2],
            width: crate::docmodel::INK_WIDTH,
            author: String::new(),
            note: String::new(),
            made: "D:20260818120000Z".to_string(),
        }],
    };
    copy_here(&source, &plan, &out, None).expect("a mark on the unshared page is fine");
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
    let why = copy_here(&source, &plan_with_mark(flat), &out, None)
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
            source: PageSource::Baseline(0),
            turns: 0,
            crop: None,
        }],
        redactions: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
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
            source: PageSource::Baseline(0),
            turns: 0,
            crop: None,
        }],
        redactions: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
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
    let Ok(Object::Array(entries)) = doc.get_dictionary(page).and_then(|d| d.get(b"Annots")) else {
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
    copy_here(&source, &plan, &out, None).expect("save");
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
    let why = copy_here(&source, &plan_with_mark(one_quad()), &out, None)
        .expect_err("an encrypted source must still be refused");
    assert!(why.message.contains("encrypted"), "{why}");
}

/// The written annotation for a mark of `kind`, reopened from the file.
fn written_mark(kind: MarkKind, scratch: &Scratch) -> Dictionary {
    let source = scratch.join("in.pdf");
    let out = scratch.join("out.pdf");
    std::fs::write(&source, document_with_annots(AnnotShape::Absent)).expect("write fixture");
    copy_here(&source, &plan_of_kind(kind, one_quad()), &out, None).expect("save");
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
    copy_here(&source, plan, &out, None).expect("save");
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
fn the_stroke_width_is_the_nib_the_reader_chose() {
    // **The number that reaches every foreign reader.** The overlay draws ink
    // from `MarkView::width` and the file draws it from this `w`; a writer
    // reaching for `INK_WIDTH` here would show a broad drawing on screen and
    // save a thin one, and nothing would say so until the file was reopened.
    //
    // A width no constant in the tree holds, so the wide assertion cannot be
    // satisfied by the default --- and the default is checked too, because a
    // writer that always wrote the mark's field would pass the first half
    // whether or not the field carried anything.
    let scratch = Scratch::new("annots-ink-nib");
    let broad = 8.0_f64;
    assert_ne!(
        broad,
        crate::docmodel::INK_WIDTH,
        "the fixture discriminates"
    );

    let mut plan = plan_with_ink();
    plan.marks[0].width = broad;
    let content = appearance_of_plan(&plan, &scratch);
    assert!(
        content.contains(&format!("{broad} w")),
        "the chosen nib is not the width written: {content}"
    );

    // The control. `plan_with_ink` leaves the default in place, so this is
    // the same code path answering a different question, and without it the
    // check above passes for a writer with `8.0` hard-coded in it.
    let content = appearance_of_plan(&plan_with_ink(), &scratch);
    assert!(
        content.contains(&format!("{} w", crate::docmodel::INK_WIDTH)),
        "the default nib is not written either: {content}"
    );
    assert!(
        !content.contains(&format!("{broad} w")),
        "and the wide one leaked into a default drawing: {content}"
    );
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
    copy_here(&source, &plan_with_ink(), &out, None).expect("save");
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
    copy_here(&source, &plan_with_ink(), &out, None).expect("save");
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
    copy_here(&source, &plan_with_mark(one_quad()), &out2, None).expect("save");
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
    copy_here(&source, plan, &out, None).expect("save");
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
        reply_to: None,
        at: 0,
        quads: vec![quad],
        strokes: Vec::new(),
        color: [1.0, 0.9, 0.2],
        width: crate::docmodel::INK_WIDTH,
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
