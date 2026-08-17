//! Writing the working document to a file.
//!
//! One operation so far --- **save a copy with each page's turn applied** --- and
//! the signature is what says so. It takes one turn per page, in order, so
//! "every page, in the order the file already has them" is structural rather
//! than a precondition somebody has to check: there is no way to spell a plan
//! that drops or moves a page. Deleting and reordering are different operations
//! and will need a different signature, which is the point --- a general "plan"
//! parameter would need a guard for the shapes this code cannot honour, and
//! `docs/TRAPS.md` is clear about what an unreachable guard is worth beside a
//! type that carries the same thing.
//!
//! **Three refusals, and none of them is defensive.**
//!
//!  - An **encrypted** document. `docs/TRAPS.md` records that `lopdf` silently
//!    drops encryption on save, so writing one produces a file whose restrictions
//!    are gone and whose reader has no way to know. 3 of the 39 PDFs in a real
//!    Downloads folder carry `/Encrypt` (measured for `progressive::open_failure`),
//!    so this is a case a reader meets, not a hypothetical.
//!  - A **page count that disagrees** with the plan. That is the external
//!    modification §5 of `docs/PLAN.md` is about: the file changed under the open
//!    document, and the turns the reader applied no longer name the pages they
//!    were applied to.
//!  - Writing **over the source**. The baseline the model replays against is the
//!    file on disk; replacing it would leave every journalled command describing
//!    a document that is gone. Saving in place is a different operation with its
//!    own rebase, and §5 has it.
//!
//! **The write is atomic**: the bytes go to a sibling temporary file and are
//! renamed over the destination, so an interrupted save leaves either the old
//! file or the new one. A partially written PDF is the worst of the three
//! outcomes --- it opens, and it is missing pages.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lopdf::{Document, Object};

use crate::print::{effective_rotation, MAX_DECODE};

/// Extension of the file the bytes are written to before the rename.
///
/// Sibling rather than in the system temp directory, because a rename across
/// filesystems is not atomic and the temp directory is routinely on another
/// one.
const PARTIAL: &str = "tpdf-partial";

/// Writes `source` to `out` with `turns[i]` extra quarter-turns on page `i`.
///
/// # Errors
///
/// The source cannot be read or parsed; it is encrypted; it has a different
/// number of pages than the plan; `out` is the source; or the write fails. The
/// temporary file is removed on every failing path that created one.
pub fn write_copy(source: &Path, turns: &[u8], out: &Path) -> Result<(), String> {
    if turns.is_empty() {
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

    let pages: Vec<lopdf::ObjectId> = {
        let table = doc.get_pages();
        let mut numbers: Vec<u32> = table.keys().copied().collect();
        numbers.sort_unstable();
        numbers
            .iter()
            .map(|number| table[number])
            .collect::<Vec<_>>()
    };
    if pages.len() != turns.len() {
        return Err(format!(
            "the document on disk has {} page(s) and the edits describe {} --- it has changed \
             since it was opened, so reopen it before saving",
            pages.len(),
            turns.len()
        ));
    }

    for (id, extra) in agreed_turns(&pages, turns)? {
        let extra = i64::from(extra);
        if extra == 0 {
            // Deliberately not written. A page the reader did not turn must come
            // out of a save exactly as it went in, and writing the composed value
            // would replace its inheritance with whatever the walk answered ---
            // which is 0 whenever `effective_rotation`'s 64-hop bound gives up.
            // See the trap; its first version had this reason wrong.
            continue;
        }
        let composed = (effective_rotation(&doc, id) + extra * 90).rem_euclid(360);
        doc.get_object_mut(id)
            .and_then(Object::as_dict_mut)
            .map_err(|e| format!("page {id:?} is not a dictionary: {e}"))?
            .set("Rotate", composed);
    }

    let mut bytes = Vec::new();
    doc.save_to(&mut bytes)
        .map_err(|e| format!("could not serialise the document: {e}"))?;

    write_atomically(out, &bytes)
}

/// One turn per distinct page *object*, or a refusal naming the pages that disagree.
///
/// `pages[i]` supplies page `i + 1`, and a well-formed document gives every page
/// an object of its own. A malformed one need not: `/Kids` may name the same page
/// twice, and `lopdf`'s page walk returns it twice because it keeps no visited
/// set --- so two page numbers share one `/Rotate`. Composing the turn once per
/// *number* would then turn that page twice, because the second visit reads what
/// the first wrote. See the trap; the same shape was live in `print.rs` in two
/// places, one of them since printing landed.
///
/// **Refused only where the plan genuinely cannot be honoured.** Turns that agree
/// can be: the object turns once and both page numbers show it, which is what was
/// asked for. Turns that differ cannot be by any output --- page 3 cannot be at 90
/// and page 7 at 180 when they are one object --- so that is refused rather than
/// resolved by picking one and handing back a file the reader would have to check.
/// A blanket refusal was the obvious move and is wrong for the case that
/// dominates: a document nobody edited, where every turn is zero and there is
/// nothing to reconcile.
fn agreed_turns(
    pages: &[lopdf::ObjectId],
    turns: &[u8],
) -> Result<Vec<(lopdf::ObjectId, u8)>, String> {
    let mut order: Vec<lopdf::ObjectId> = Vec::with_capacity(pages.len());
    let mut chosen: HashMap<lopdf::ObjectId, (u8, usize)> = HashMap::new();
    for (at, (id, extra)) in pages.iter().zip(turns).enumerate() {
        let extra = extra % 4;
        match chosen.get(id) {
            None => {
                chosen.insert(*id, (extra, at));
                order.push(*id);
            }
            Some(&(first, first_at)) if first != extra => {
                return Err(format!(
                    "pages {} and {} are the same page in this file, so they cannot be turned \
                     differently. Turn them the same way, or leave both as they are.",
                    first_at + 1,
                    at + 1
                ));
            }
            Some(_) => {}
        }
    }
    Ok(order.into_iter().map(|id| (id, chosen[&id].0)).collect())
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
    use std::collections::HashSet;

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
            write_copy(&path, &turns, &out).unwrap_or_else(|e| panic!("{name}: {e}"));

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
        write_copy(&path, &turns, &out).expect("write");

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

    fn ordered_pages(doc: &Document) -> Vec<lopdf::ObjectId> {
        let table = doc.get_pages();
        let mut numbers: Vec<u32> = table.keys().copied().collect();
        numbers.sort_unstable();
        numbers.iter().map(|n| table[n]).collect()
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
        write_copy(&source, &[0, 1], &out).expect("write");

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
        write_copy(&source, &[1, 1], &out).expect("agreeing turns are honoured");

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

        let why = write_copy(&source, &[1, 2], &out).expect_err("must refuse");
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

        write_copy(&source, &[0, 0], &out).expect("an unedited document still saves");
        assert!(out.exists());
    }

    #[test]
    fn an_encrypted_document_is_refused_rather_than_quietly_decrypted() {
        let scratch = Scratch::new("encrypted");
        let source = scratch.join("locked.pdf");
        std::fs::write(&source, encrypted_document()).expect("write fixture");
        let out = scratch.join("out.pdf");

        let why = write_copy(&source, &[0], &out).expect_err("must refuse");
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

        let why = write_copy(&path, &vec![0u8; count + 1], &out).expect_err("must refuse");
        assert!(why.contains("changed since it was opened"), "{why}");
        assert!(!out.exists());

        // And the matching plan is accepted, so the refusal is about the
        // mismatch rather than about this document.
        write_copy(&path, &vec![0u8; count], &out).expect("the matching plan writes");
        assert!(out.exists());
    }

    #[test]
    fn an_empty_plan_is_refused() {
        let scratch = Scratch::new("empty");
        let out = scratch.join("out.pdf");
        let why =
            write_copy(Path::new("../testdata/rotated.pdf"), &[], &out).expect_err("must refuse");
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

        let why = write_copy(&copy, &[1, 0, 0, 0], &copy).expect_err("must refuse");
        assert!(why.contains("save over"), "{why}");
        assert_eq!(
            std::fs::read(&copy).expect("read"),
            before,
            "the document is untouched"
        );

        // The same file reached by a different spelling of the path is still the
        // same file --- a comparison of the strings would let this through.
        let indirect = scratch.join(".").join("copy.pdf");
        assert!(write_copy(&copy, &[1, 0, 0, 0], &indirect).is_err());
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
        write_copy(&path, &[0, 0, 0, 0], &out).expect("a fresh destination is accepted");
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
        write_copy(&path, &[1, 1, 1, 1], &out).expect("write");
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

        write_copy(&path, &[0, 0, 0, 0], &out).expect("write");

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
