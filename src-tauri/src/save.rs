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
//!  - Writing **over the source**. The baseline the model replays against is the
//!    file on disk; replacing it would leave every journalled command describing
//!    a document that is gone. Saving in place is a different operation with its
//!    own rebase, and §5 has it.
//!
//! **The write is atomic**: the bytes go to a sibling temporary file and are
//! renamed over the destination, so an interrupted save leaves either the old
//! file or the new one. A partially written PDF is the worst of the three
//! outcomes --- it opens, and it is missing pages.
//!
//! The page-tree surgery itself is `pagetree.rs`, shared with the print path,
//! which needs every one of the same operations for the same reasons.

use std::path::{Path, PathBuf};

use lopdf::Document;

use crate::edits::Plan;
use crate::pagetree::{
    agreed_turns, apply_turns, drop_outline, drop_pages, ordered_pages, reorder_pages,
};
use crate::print::MAX_DECODE;

/// Extension of the file the bytes are written to before the rename.
///
/// Sibling rather than in the system temp directory, because a rename across
/// filesystems is not atomic and the temp directory is routinely on another
/// one.
const PARTIAL: &str = "tpdf-partial";

/// Writes the pages `plan` keeps, each with its own turn, from `source` to `out`.
///
/// # Errors
///
/// The source cannot be read or parsed; it is encrypted; it has a different
/// number of pages than the plan's baseline; the plan is empty or names a page
/// the file does not have; two of its pages are one object and disagree about
/// the turn, or one of them is dropped without the other; `out` is the source;
/// or the write fails.
/// The temporary file is removed on every failing path that created one.
pub fn write_copy(source: &Path, plan: &Plan, out: &Path) -> Result<(), String> {
    if plan.pages.is_empty() {
        return Err("a document must keep at least one page".into());
    }
    if same_file(source, out) {
        return Err(
            "tpdf cannot save over the document it is reading --- choose another name".into(),
        );
    }

    let mut doc = Document::load_with_options(
        source,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            ..Default::default()
        },
    )
    .map_err(|e| format!("could not parse {source:?}: {e}"))?;

    // Before anything is written, and before the page walk: a refusal that
    // arrives after a temporary file exists has to clean up, and a refusal that
    // arrives after a rename has nothing to clean up at all.
    if doc.trailer.has(b"Encrypt") {
        return Err(
            "This document is encrypted, and saving a copy would silently remove that. \
             tpdf will not write it."
                .into(),
        );
    }

    let pages = ordered_pages(&doc);
    if pages.len() != plan.baseline as usize {
        return Err(format!(
            "the document on disk has {} page(s) and the edits were made against {} --- it has \
             changed since it was opened, so reopen it before saving",
            pages.len(),
            plan.baseline
        ));
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
        return Err(format!(
            "the edits name page {past}, which this document does not have"
        ));
    }

    let turns: Vec<(lopdf::ObjectId, u8)> = plan
        .pages
        .iter()
        .filter_map(|page| Some((*pages.get(page.source as usize)?, page.turns)))
        .collect();

    if kept.len() != pages.len() {
        let dropped: Vec<u32> = (1..=pages.len() as u32)
            .filter(|number| !kept.contains(number))
            .collect();
        unshared(&pages, &kept, &dropped)?;
        drop_pages(&mut doc, &dropped)?;
        // Its destinations name pages that are no longer in the file. Dropped
        // whole rather than repaired --- `pagetree::drop_outline` carries what
        // repairing it would take, and it is its own piece of work.
        drop_outline(&mut doc)?;
    }

    // After the deletion, so that the tree written here holds exactly the pages
    // that survived it. The outline is *not* dropped for a move: a destination
    // names a page object, and the object is still there --- a bookmark follows
    // its page to wherever the reader put it, which is what a reader who
    // rearranged a document means.
    if moved {
        let order: Vec<lopdf::ObjectId> = turns.iter().map(|(id, _)| *id).collect();
        reorder_pages(&mut doc, &order)?;
    }

    // After the deletion, and it has to be: `drop_pages` removes objects, and a
    // rotation written onto a page that is about to go is work thrown away. The
    // ids are unaffected --- the survivors are the same objects they were.
    apply_turns(&mut doc, &agreed_turns(&turns)?)?;

    let mut bytes = Vec::new();
    doc.save_to(&mut bytes)
        .map_err(|e| format!("could not serialise the document: {e}"))?;

    write_atomically(out, &bytes)
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

/// Writes `bytes` to `out` via a sibling temporary file and a rename.
fn write_atomically(out: &Path, bytes: &[u8]) -> Result<(), String> {
    let partial = out.with_extension(PARTIAL);
    std::fs::write(&partial, bytes).map_err(|e| {
        // Nothing to remove: the failure is the write itself, and a file that
        // may or may not exist is removed below rather than guessed about here.
        let _ = std::fs::remove_file(&partial);
        format!("could not write {partial:?}: {e}")
    })?;
    std::fs::rename(&partial, out).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
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
    use lopdf::Object;
    use std::collections::HashSet;

    /// A plan that keeps every page of an `n`-page document, turning each by
    /// `turns[i]`.
    ///
    /// The ids are the model's own numbering --- one per baseline page, from 1 ---
    /// and nothing here reads them: a plan is addressed by `source`, and the id
    /// travels only so that this is the shape the model really produces.
    fn plan_of(turns: &[u8]) -> Plan {
        Plan {
            baseline: turns.len() as u32,
            pages: turns
                .iter()
                .enumerate()
                .map(|(at, &turns)| PageView {
                    id: at as u64 + 1,
                    source: at as u32,
                    turns,
                })
                .collect(),
        }
    }

    /// A plan over a `baseline`-page document that keeps only `kept`.
    ///
    /// `kept` is `(source, turns)`, zero-based, in the order the pages are to
    /// come out, which need not be the order the file has them.
    fn keeping(baseline: u32, kept: &[(u32, u8)]) -> Plan {
        Plan {
            baseline,
            pages: kept
                .iter()
                .map(|&(source, turns)| PageView {
                    id: u64::from(source) + 1,
                    source,
                    turns,
                })
                .collect(),
        }
    }

    #[cfg(target_os = "macos")]
    use crate::print_macos as os_pdf;
    #[cfg(not(target_os = "macos"))]
    use crate::print_win as os_pdf;

    /// A scratch directory that removes itself.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("tpdf-save-{name}-{}", std::process::id()));
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
            write_copy(&path, &plan_of(&turns), &out).unwrap_or_else(|e| panic!("{name}: {e}"));

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
        write_copy(&path, &plan_of(&turns), &out).expect("write");

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
        write_copy(&source, &plan_of(&[0, 1]), &out).expect("write");

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
        write_copy(&source, &plan_of(&[1, 1]), &out).expect("agreeing turns are honoured");

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

        let why = write_copy(&source, &plan_of(&[1, 2]), &out).expect_err("must refuse");
        assert!(
            why.contains("same page"),
            "the message says why rather than naming an internal id: {why}"
        );
        assert!(
            why.contains('1') && why.contains('2'),
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

        write_copy(&source, &plan_of(&[0, 0]), &out).expect("an unedited document still saves");
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
        write_copy(&path, &keeping(4, &[(0, 0), (2, 0), (3, 0)]), &out).expect("write");

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
        write_copy(&path, &keeping(4, &[(0, 0), (2, 0), (3, 1)]), &out).expect("write");

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
        write_copy(&path, &keeping(count as u32, &kept), &trimmed).expect("write");
        assert!(
            !has_outline(&Document::load(&trimmed).expect("load written")),
            "a page was dropped, so its destinations are gone"
        );

        // The control. Without it this check passes for a save that drops every
        // outline it ever sees, which is a different and much worse rule.
        let whole = scratch.join("whole.pdf");
        write_copy(&path, &plan_of(&vec![0u8; count]), &whole).expect("write");
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

        let why = write_copy(&source, &keeping(2, &[(0, 0)]), &out).expect_err("must refuse");
        assert!(
            why.contains("same page") && why.contains("on its own"),
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
        write_copy(&source, &keeping(3, &[(2, 0)]), &out).expect("write");
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
        write_copy(&path, &keeping(4, &[(2, 0), (0, 0), (3, 0), (1, 0)]), &out).expect("write");

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
        write_copy(&path, &keeping(4, &[(3, 1), (0, 0), (2, 0)]), &out).expect("write");

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
        write_copy(&path, &keeping(count as u32, &kept), &out).expect("write");

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
        let why = write_copy(&path, &keeping(4, &[(0, 0), (9, 0)]), &out).expect_err("must refuse");
        assert!(why.contains("does not have"), "{why}");
        assert!(!out.exists());
    }

    #[test]
    fn an_encrypted_document_is_refused_rather_than_quietly_decrypted() {
        let scratch = Scratch::new("encrypted");
        let source = scratch.join("locked.pdf");
        std::fs::write(&source, encrypted_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        let why = write_copy(&source, &plan_of(&[0]), &out).expect_err("must refuse");
        assert!(
            why.contains("encrypted"),
            "the message names the reason: {why}"
        );
        assert!(
            !out.exists(),
            "a refusal writes nothing, not even a temporary"
        );
        assert!(!out.with_extension(PARTIAL).exists());
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
            write_copy(&path, &plan_of(&vec![0u8; count + 1]), &out).expect_err("must refuse");
        assert!(why.contains("changed since it was opened"), "{why}");
        assert!(!out.exists());

        // And the matching plan is accepted, so the refusal is about the
        // mismatch rather than about this document.
        write_copy(&path, &plan_of(&vec![0u8; count]), &out).expect("the matching plan writes");
        assert!(out.exists());
    }

    #[test]
    fn an_empty_plan_is_refused() {
        let scratch = Scratch::new("empty");
        let out = scratch.join("out.pdf");
        let why = write_copy(Path::new("../testdata/rotated.pdf"), &plan_of(&[]), &out)
            .expect_err("must refuse");
        assert!(why.contains("at least one page"), "{why}");
    }

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

        let why = write_copy(&copy, &plan_of(&[1, 0, 0, 0]), &copy).expect_err("must refuse");
        assert!(why.contains("save over"), "{why}");
        assert_eq!(
            std::fs::read(&copy).expect("read"),
            before,
            "the document is untouched"
        );

        // The same file reached by a different spelling of the path is still the
        // same file --- a comparison of the strings would let this through.
        let indirect = scratch.join(".").join("copy.pdf");
        assert!(write_copy(&copy, &plan_of(&[1, 0, 0, 0]), &indirect).is_err());
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
        write_copy(&path, &plan_of(&[0, 0, 0, 0]), &out).expect("a fresh destination is accepted");
        assert!(out.exists());
    }

    #[test]
    fn nothing_of_the_partial_file_survives_a_successful_write() {
        let Some(path) = fixture("rotated.pdf") else {
            println!("[SKIP] rotated.pdf not generated");
            return;
        };
        let scratch = Scratch::new("partial");
        let out = scratch.join("done.pdf");
        write_copy(&path, &plan_of(&[1, 1, 1, 1]), &out).expect("write");
        assert!(out.exists());
        assert!(
            !out.with_extension(PARTIAL).exists(),
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

        write_copy(&path, &plan_of(&[0, 0, 0, 0]), &out).expect("write");

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
}
