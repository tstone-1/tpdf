//! Building the PDF that gets handed to the printer.
//!
//! Printing hands a **PDF** to the operating system, never pixels. That is not a
//! convenience: measured on macOS, `cupsfilter -d <queue>` against a configured
//! AirPrint device returned output byte-identical to the input, so the document
//! reaches the printer untouched and anything we rendered first could only throw
//! information away. Where a printer is not PDF-native the conversion is CUPS's
//! own, and pre-rasterising at a resolution we guessed would be strictly worse
//! than letting it choose (docs/PLAN.md, Phase 1 → Print).
//!
//! So the only question left is *which* PDF. Three cases:
//!
//! - **Everything, unrotated.** The file itself, byte for byte. Rewriting a
//!   document in order to change nothing about it is pure risk: `lopdf` drops
//!   encryption silently (AGENTS.md), a rewrite reflows structure, and the
//!   printer was going to receive these exact bytes anyway.
//! - **A page range.** Pages are deleted **in place** rather than re-parented
//!   under a fresh `/Pages`. That matters: `/Resources`, `/MediaBox`, `/CropBox`
//!   and `/Rotate` are *inheritable*, so a page moved out from under its parent
//!   loses whatever it was inheriting --- and a page that has lost its resources
//!   still counts as a page, still opens, and prints blank.
//! - **A rotated view.** The reader asked for what they are looking at, so the
//!   view rotation is composed onto each page's own `/Rotate` --- the effective
//!   one, resolved up the `/Parent` chain, not the literal one, which is absent
//!   on exactly the documents that inherit it.
//!
//! The outline is dropped whenever pages are. Its destinations name pages that
//! are no longer in the file, and a table of contents that points at nothing is
//! worse than none --- the same reason a bounded outline walk reports what it cut
//! rather than presenting a partial tree as whole.

use std::collections::HashSet;
use std::path::Path;

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId};

use crate::sweep;

/// Cap on a single decompressed stream, matching the sanitizing rewrite.
///
/// A print job parses attacker-controlled input like everything else here, and
/// spike 0.4 measured a 2,879-byte input inflating to 1 GiB.
pub(crate) const MAX_DECODE: usize = 64 * 1024 * 1024;

/// Which pages to print.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pages {
    /// Every page, in document order.
    All,
    /// Exactly these, one-based --- and printed in **document order**, not in
    /// the order they are listed here.
    ///
    /// [`build`] produces a subset by *deleting* the pages nobody asked for, so
    /// the survivors keep the positions they already had; `[3, 1]` prints page 1
    /// then page 3. Assembling a new page tree in the listed order is a
    /// different operation and this is not it. Said here rather than left to be
    /// discovered because nothing downstream would report it: [`expect_pages`]
    /// compares how many pages came out, never which, so a reordering that the
    /// caller expected and did not get is invisible until it is on paper.
    Only(Vec<u32>),
}

/// What to print, and how it should be oriented.
#[derive(Clone, Debug)]
pub struct Job {
    pub pages: Pages,
    /// Quarter-turns clockwise the *view* is rotated by, 0 to 3.
    pub turns: u8,
}

impl Job {
    /// Whether this job is the document exactly as it already is on disk.
    #[must_use]
    pub fn is_passthrough(&self) -> bool {
        self.pages == Pages::All && self.turns % 4 == 0
    }
}

/// Produces the bytes to hand to the print system.
///
/// # Errors
///
/// A page number outside the document, or an empty selection. Both are refused
/// rather than repaired: a range the reader typed is an instruction, and
/// silently printing the pages that happened to exist --- or nothing at all ---
/// is the kind of plausible wrong answer that only shows up on paper.
pub fn build(source: &Path, job: &Job) -> Result<Vec<u8>, String> {
    if job.is_passthrough() {
        return std::fs::read(source).map_err(|e| format!("could not read {source:?}: {e}"));
    }

    let mut doc = Document::load_with_options(
        source,
        LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            ..Default::default()
        },
    )
    .map_err(|e| format!("could not parse {source:?}: {e}"))?;

    let present: Vec<u32> = doc.get_pages().keys().copied().collect();
    let wanted = resolve(&job.pages, &present)?;

    if wanted.len() != present.len() {
        let dropped: Vec<u32> = present
            .iter()
            .copied()
            .filter(|number| !wanted.contains(number))
            .collect();
        drop_pages(&mut doc, &dropped)?;
        // Destinations into pages that are no longer here.
        doc.catalog_mut()
            .map_err(|e| format!("no document catalog: {e}"))?
            .remove(b"Outlines");
    }

    let turns = job.turns % 4;
    if turns != 0 {
        // Distinct objects, not page numbers. A `/Kids` array may name one page
        // twice and `lopdf`'s page walk keeps no visited set, so composing once
        // per number turns a shared page twice --- the second visit reads what
        // the first wrote. Every page takes the same turn here, so there is
        // nothing to reconcile, only a second application to avoid; that is why
        // this deduplicates where `save::agreed_turns` has to refuse. See the trap.
        let mut seen: HashSet<ObjectId> = HashSet::new();
        let ids: Vec<_> = doc
            .get_pages()
            .values()
            .copied()
            .filter(|id| seen.insert(*id))
            .collect();
        for id in ids {
            let composed = (effective_rotation(&doc, id) + i64::from(turns) * 90).rem_euclid(360);
            doc.get_object_mut(id)
                .and_then(Object::as_dict_mut)
                .map_err(|e| format!("page {id:?} is not a dictionary: {e}"))?
                .set("Rotate", composed);
        }
    }

    sweep::collect(&mut doc)?;

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| format!("could not serialise the print job: {e}"))?;
    Ok(out)
}

/// Removes pages, and every reference to them, in a single pass.
///
/// `lopdf::delete_pages` does exactly this and does not scale: it calls
/// `delete_object` per page, and `delete_object` calls `traverse_objects` ---
/// the quadratic walk AGENTS.md already records for `prune_objects`, here run
/// once *per deleted page*. Measured release-profile on the 775-page corpus,
/// keeping two pages: **620 ms** against **1.2 ms** here, and the two produce
/// byte-identical output on every fixture and corpus
/// (`control_page_deletion_matches_lopdf_byte_for_byte`).
///
/// Same shape as the mark-and-sweep, and the same conclusion --- use `lopdf`
/// for the object model, write the graph walks ourselves.
///
/// # Errors
///
/// An object nesting deeper than [`sweep::MAX_NESTING`]. Refused rather than
/// walked as far as it goes, for the same reason the sweep refuses: a pass that
/// stopped early would leave a reference to a deleted page in the file, and a
/// page tree naming an object that is gone is a document that opens and prints
/// blank pages.
fn drop_pages(doc: &mut Document, numbers: &[u32]) -> Result<(), String> {
    let pages = doc.get_pages();
    // A page object can answer to more than one page number: `/Kids` may list it
    // twice, and `lopdf`'s page walk keeps no visited set. An object that a KEPT
    // number also names must survive --- otherwise printing "page 1" of such a
    // document deletes the object page 1 *is*, and prints a blank sheet. This is
    // the damaging member of the family in the trap: the other two turn a page
    // twice, this one removes the page that was asked for.
    let kept: HashSet<ObjectId> = pages
        .iter()
        .filter(|(number, _)| !numbers.contains(number))
        .map(|(_, id)| *id)
        .collect();
    let doomed: HashSet<ObjectId> = numbers
        .iter()
        .filter_map(|number| pages.get(number).copied())
        .filter(|id| !kept.contains(id))
        .collect();
    if doomed.is_empty() {
        return Ok(());
    }

    // Every ancestor of every doomed page, collected before anything moves, so
    // that a `/Count` is decremented once per page beneath it. A page tree is
    // usually two levels deep and may be many.
    //
    // Walked per page NUMBER rather than per object, because `/Count` counts
    // entries in the tree: two numbers naming one doomed object remove two of
    // them. Iterating the object set decremented once per object, which is right
    // only while objects and page numbers are the same thing.
    let mut decrements = Vec::new();
    for number in numbers {
        let Some(id) = pages.get(number).copied() else {
            continue;
        };
        if !doomed.contains(&id) {
            continue;
        }
        let mut at = parent_of(doc, id);
        // Same `/Parent`-cycle bound as `effective_rotation`, same reason: this
        // runs on input we did not write.
        for _ in 0..64 {
            let Some(parent) = at else { break };
            decrements.push(parent);
            at = parent_of(doc, parent);
        }
    }
    for parent in decrements {
        if let Ok(tree) = doc.get_object_mut(parent).and_then(Object::as_dict_mut) {
            if let Ok(count) = tree.get(b"Count").and_then(Object::as_i64) {
                tree.set("Count", count - 1);
            }
        }
    }

    // One pass over the whole graph, dropping array entries and dictionary keys
    // that name a doomed page --- the `/Kids` entry that removes it from the
    // tree, and anything else pointing at it. The trailer is not in `objects`
    // and has to be walked in its own right.
    forget_in_dictionary(&mut doc.trailer, &doomed, 0)?;
    for object in doc.objects.values_mut() {
        forget_in_object(object, &doomed, 0)?;
    }
    for id in &doomed {
        doc.objects.remove(id);
    }
    Ok(())
}

/// The `/Parent` of an object, if it names one.
fn parent_of(doc: &Document, id: ObjectId) -> Option<ObjectId> {
    doc.get_object(id)
        .and_then(Object::as_dict)
        .and_then(|dict| dict.get(b"Parent"))
        .and_then(Object::as_reference)
        .ok()
}

/// Drops references to any doomed object, recursively.
///
/// Depth-bounded by [`sweep::MAX_NESTING`], for the reason recorded there: this
/// runs on a document we did not write, in the app process, and the recursion is
/// otherwise unbounded.
fn forget_in_object(
    object: &mut Object,
    doomed: &HashSet<ObjectId>,
    depth: usize,
) -> Result<(), String> {
    if depth > sweep::MAX_NESTING {
        return Err(format!(
            "an object nests deeper than {} levels",
            sweep::MAX_NESTING
        ));
    }
    match object {
        Object::Array(items) => {
            items.retain(|item| !matches!(item, Object::Reference(id) if doomed.contains(id)));
            for item in items.iter_mut() {
                forget_in_object(item, doomed, depth + 1)?;
            }
        }
        Object::Dictionary(dictionary) => forget_in_dictionary(dictionary, doomed, depth + 1)?,
        Object::Stream(stream) => forget_in_dictionary(&mut stream.dict, doomed, depth + 1)?,
        _ => {}
    }
    Ok(())
}

/// Drops keys whose value names a doomed object, then recurses.
fn forget_in_dictionary(
    dictionary: &mut Dictionary,
    doomed: &HashSet<ObjectId>,
    depth: usize,
) -> Result<(), String> {
    if depth > sweep::MAX_NESTING {
        return Err(format!(
            "an object nests deeper than {} levels",
            sweep::MAX_NESTING
        ));
    }
    let dead: Vec<Vec<u8>> = dictionary
        .iter()
        .filter(|(_, value)| matches!(value, Object::Reference(id) if doomed.contains(id)))
        .map(|(key, _)| key.clone())
        .collect();
    for key in dead {
        dictionary.remove(&key);
    }
    for (_, value) in dictionary.iter_mut() {
        forget_in_object(value, doomed, depth + 1)?;
    }
    Ok(())
}

/// Checks a built job against what was asked for, before it reaches paper.
///
/// `found` comes from an independent parser, never from the writer that
/// produced the bytes --- see `print_macos::read` for why that is the whole
/// point. `expected` is `None` for "everything", where there is no count to
/// compare against and the only wrong answer that can be recognised is nothing
/// at all.
///
/// # Errors
///
/// A count that disagrees, or an empty job.
pub fn expect_pages(found: usize, expected: Option<usize>) -> Result<(), String> {
    match expected {
        Some(expected) if found != expected => Err(format!(
            "the print job has {found} pages, not the {expected} asked for"
        )),
        None if found == 0 => Err("the print job came out empty".into()),
        _ => Ok(()),
    }
}

/// The page numbers to keep, validated against what the document has.
fn resolve(pages: &Pages, present: &[u32]) -> Result<Vec<u32>, String> {
    match pages {
        Pages::All => Ok(present.to_vec()),
        Pages::Only(wanted) => {
            if wanted.is_empty() {
                return Err("no pages selected".into());
            }
            for number in wanted {
                if !present.contains(number) {
                    return Err(format!(
                        "page {number} is not in this document, which has {}",
                        present.len()
                    ));
                }
            }
            Ok(wanted.clone())
        }
    }
}

/// A page's `/Rotate` including anything it inherits.
///
/// Absent means "ask the parent", and only a page with no ancestor carrying one
/// is really at zero. Composing against the literal value instead would be
/// correct on every document that states it and wrong on every document that
/// does not --- which is the half nobody has a fixture for.
pub(crate) fn effective_rotation(doc: &Document, page: lopdf::ObjectId) -> i64 {
    let mut at = page;
    // Bounded: a `/Parent` cycle in a malformed file would otherwise spin here,
    // and this runs on input we did not write.
    for _ in 0..64 {
        let Ok(dictionary) = doc.get_object(at).and_then(Object::as_dict) else {
            break;
        };
        if let Ok(value) = dictionary.get(b"Rotate").and_then(Object::as_i64) {
            return value;
        }
        match dictionary.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent) => at = parent,
            Err(_) => break,
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::{build, drop_pages, effective_rotation, Job, Pages};
    use lopdf::{dictionary, Document, Object, ObjectId, Stream};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    /// The operating system's own PDF parser, whichever platform this is.
    ///
    /// PDFKit on macOS, `Windows.Data.Pdf` on Windows. Both are the platform's own
    /// PDF stack, so both are independent of the `lopdf` that writes these jobs and
    /// of the PDFium that draws what the reader sees --- which is the whole property
    /// these checks are built on. The two modules deliberately expose the same
    /// `Reading` shape so that one set of expectations covers both.
    ///
    /// **The four checks below were macOS-only until 2026-07-30, and three of them
    /// had no reason to be.** They were written when PDFKit was the only independent
    /// parser available, and the gate then said "no third parser here" rather than
    /// anything about the property under test. Printing is the one subsystem whose
    /// output leaves the process, so the platform without a third parser was also
    /// the platform where nothing checked that its jobs were readable at all.
    #[cfg(target_os = "macos")]
    use crate::print_macos as os_pdf;
    #[cfg(windows)]
    use crate::print_win as os_pdf;

    /// Whether the OS parser on this platform can extract text.
    ///
    /// PDFKit can; `Windows.Data.Pdf` renders and reports geometry and has no text
    /// API at all. A runtime constant rather than a `cfg` on the test, so the check
    /// that needs text still *runs* on Windows --- it asserts everything it can and
    /// prints a `[SKIP]` naming what it could not. `BUILD.md`'s rule: a check that
    /// quietly stops existing on one platform is worse than one that skips out loud,
    /// because a vanished check and a passing one look identical in a summary.
    const OS_PARSER_HAS_TEXT: bool = cfg!(target_os = "macos");

    /// A scratch directory that removes itself.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("tpdf-print-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("scratch");
            Self(path)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A document whose pages inherit `/Resources` and `/Rotate` from the tree.
    ///
    /// Inherited on purpose: a page re-parented under a fresh `/Pages` loses
    /// both, and the result still opens and still counts the right number of
    /// pages. Each page's content names its own number, so a subset can be
    /// checked for *which* pages it kept rather than only how many.
    fn fixture(path: &Path, pages: usize, tree_rotate: i64) {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });

        let mut kids = Vec::new();
        for number in 1..=pages {
            let content = format!("BT /F1 24 Tf 72 700 Td (page {number}) Tj ET");
            let contents_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
            kids.push(Object::Reference(doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => contents_id,
            })));
        }

        let mut tree = dictionary! {
            "Type" => "Pages",
            "Count" => pages as i64,
            "Kids" => kids,
            // Inheritable, and deliberately only here.
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        };
        if tree_rotate != 0 {
            tree.set("Rotate", tree_rotate);
        }
        doc.objects.insert(pages_id, Object::Dictionary(tree));

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.save(path).expect("fixture");

        // Trailing bytes past `%%EOF`, and they are load-bearing. Without them
        // the fixture is a file lopdf itself wrote, so loading and saving it
        // reproduces it byte for byte --- and "the file was handed over
        // untouched" becomes true of a full rewrite too. Both passthrough
        // mutations survived until this was here. Readers tolerate the tail
        // (`hostile-trailing` exists for exactly that), lopdf does not emit it.
        let mut bytes = std::fs::read(path).expect("read back");
        bytes.extend_from_slice(b"\n% a tail no serialiser would reproduce\n");
        std::fs::write(path, bytes).expect("retag");
    }

    /// A document whose page tree has an intermediate level.
    ///
    /// `fixture` above builds every page directly under the root, which is what
    /// a generator does and not what a producer does --- real documents balance
    /// the tree, so a page's `/Parent` chain is two or more nodes long. Deleting
    /// a page has to decrement `/Count` on **every** ancestor, and with a flat
    /// tree "the page's parent" and "the whole chain" are the same thing, so
    /// nothing can tell the two apart. Found by a mutation that survived
    /// (`D4`), not by reading the code.
    ///
    /// Returns the root and the intermediate node ids, so a check can name the
    /// level it is asserting about.
    fn nested_fixture(path: &Path, groups: usize, per_group: usize) -> (ObjectId, Vec<ObjectId>) {
        let mut doc = Document::with_version("1.7");
        let root_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });

        let mut number = 0;
        let mut middles = Vec::new();
        let mut root_kids = Vec::new();
        for _ in 0..groups {
            let middle_id = doc.new_object_id();
            let mut kids = Vec::new();
            for _ in 0..per_group {
                number += 1;
                let content = format!("BT /F1 24 Tf 72 700 Td (page {number}) Tj ET");
                let contents_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
                kids.push(Object::Reference(doc.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => middle_id,
                    "Contents" => contents_id,
                })));
            }
            doc.objects.insert(
                middle_id,
                Object::Dictionary(dictionary! {
                    "Type" => "Pages",
                    "Parent" => root_id,
                    "Count" => per_group as i64,
                    "Kids" => kids,
                }),
            );
            middles.push(middle_id);
            root_kids.push(Object::Reference(middle_id));
        }

        doc.objects.insert(
            root_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Count" => number as i64,
                "Kids" => root_kids,
                // Inheritable, and two levels above the pages on purpose.
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => root_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.save(path).expect("fixture");
        (root_id, middles)
    }

    /// The `/Count` a page-tree node declares.
    fn declared_count(doc: &Document, id: ObjectId) -> i64 {
        doc.get_object(id)
            .and_then(Object::as_dict)
            .and_then(|node| node.get(b"Count"))
            .and_then(Object::as_i64)
            .expect("a page tree node must declare a count")
    }

    /// Reloads built bytes.
    fn reload(bytes: &[u8]) -> Document {
        Document::load_mem(bytes).expect("the print job must be a readable document")
    }

    /// The text drawn on each page, in order, so a subset can be identified.
    fn page_labels(doc: &Document) -> Vec<String> {
        doc.get_pages()
            .values()
            .map(|id| String::from_utf8_lossy(&doc.get_page_content(*id)).into_owned())
            .collect()
    }

    #[test]
    fn printing_everything_unrotated_hands_over_the_file_itself() {
        // Not "an equivalent document". Rewriting to change nothing is pure
        // risk --- lopdf drops encryption silently, and a rewrite reflows
        // structure --- and the printer was going to get these bytes anyway.
        let dir = TempDir::new("passthrough");
        let path = dir.file("in.pdf");
        fixture(&path, 3, 0);
        let original = std::fs::read(&path).expect("read");

        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 0,
            },
        )
        .expect("build");
        assert_eq!(out, original);
    }

    #[test]
    fn a_turn_of_four_quarters_is_still_the_file_itself() {
        let dir = TempDir::new("fullturn");
        let path = dir.file("in.pdf");
        fixture(&path, 2, 0);
        let original = std::fs::read(&path).expect("read");
        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 4,
            },
        )
        .expect("build");
        assert_eq!(out, original);
    }

    #[test]
    fn a_page_range_keeps_exactly_the_pages_asked_for() {
        let dir = TempDir::new("range");
        let path = dir.file("in.pdf");
        fixture(&path, 5, 0);

        let out = build(
            &path,
            &Job {
                pages: Pages::Only(vec![2, 4]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        let labels = page_labels(&printed);
        assert_eq!(labels.len(), 2);
        // Which pages, not merely how many: a subset that kept the wrong two
        // has the right count and is entirely wrong.
        assert!(labels[0].contains("page 2"), "{labels:?}");
        assert!(labels[1].contains("page 4"), "{labels:?}");
    }

    #[test]
    fn a_kept_page_still_inherits_its_resources() {
        // The trap this module exists to avoid. A page that lost `/Resources`
        // still opens, still counts, and prints blank --- so the assertion has
        // to reach the font through the page, which is what a renderer does.
        let dir = TempDir::new("inherit");
        let path = dir.file("in.pdf");
        fixture(&path, 4, 0);

        let out = build(
            &path,
            &Job {
                pages: Pages::Only(vec![3]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        let page = *printed.get_pages().values().next().expect("one page");

        // Not `get_page_fonts`, which was the first version of this and could
        // not fail. lopdf collects the page's resources *and* every ancestor's
        // and merges them, so a page carrying its own empty `/Resources` still
        // reports the inherited font --- while a renderer, following PDF
        // 32000-1 §7.7.3.4, would see the page's own dictionary replace what it
        // inherits and draw nothing. The oracle was more forgiving than the
        // thing it stands in for, and the mutation modelling that failure
        // survived it.
        //
        // So: the page must still be inheriting (no `/Resources` of its own),
        // and an ancestor must still supply the font.
        let dictionary = printed.get_dictionary(page).expect("page");
        assert!(
            dictionary.get(b"Resources").is_err(),
            "the page carries its own /Resources, which replaces what it inherits"
        );

        let mut at = page;
        let mut found = None;
        for _ in 0..64 {
            let Ok(node) = printed.get_dictionary(at) else {
                break;
            };
            if let Ok(resources) = node
                .get(b"Resources")
                .and_then(|r| printed.dereference(r).map(|(_, o)| o))
                .and_then(Object::as_dict)
            {
                found = resources
                    .get(b"Font")
                    .and_then(|f| printed.dereference(f).map(|(_, o)| o))
                    .and_then(Object::as_dict)
                    .ok()
                    .and_then(|fonts| fonts.get(b"F1").ok())
                    .map(|_| ());
                break;
            }
            match node.get(b"Parent").and_then(Object::as_reference) {
                Ok(parent) => at = parent,
                Err(_) => break,
            }
        }
        assert!(
            found.is_some(),
            "no ancestor supplies the font the page draws with"
        );
    }

    #[test]
    fn a_rotation_composes_with_one_the_page_inherits() {
        // The document is already sideways and the reader has turned the view
        // another quarter. Composing against the literal `/Rotate` --- absent on
        // every page here --- would print 90 instead of 180.
        let dir = TempDir::new("compose");
        let path = dir.file("in.pdf");
        fixture(&path, 2, 90);

        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 1,
            },
        )
        .expect("build");
        let printed = reload(&out);
        for id in printed.get_pages().values() {
            assert_eq!(effective_rotation(&printed, *id), 180);
        }
    }

    /// Three page numbers over two objects: `/Kids` names the first page twice.
    ///
    /// `fixture` above gives every page an object of its own, which is what a
    /// generator does. A `/Kids` array can name one page twice, and `lopdf`'s page
    /// walk keeps no visited set, so `get_pages` then maps numbers 1 and 2 onto one
    /// object. Nothing else in the tree is shaped this way.
    ///
    /// **Three rather than two, and that is what makes it useful.** With only the
    /// shared page there is no selection that drops both of its numbers and keeps
    /// something --- an empty selection is refused before `drop_pages` is reached
    /// --- so the `/Count` arithmetic would have been unreachable. Page 3 is the
    /// page that lets it be asked for.
    ///
    /// Returns the root `/Pages` id, so a check can read its `/Count`.
    fn shared_fixture(path: &Path) -> ObjectId {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let page_of = |doc: &mut Document, label: &str| {
            let content = format!("BT /F1 24 Tf 72 700 Td ({label}) Tj ET");
            let contents_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
            doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => contents_id,
            })
        };
        let shared = page_of(&mut doc, "page 1");
        let only = page_of(&mut doc, "page 3");
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Count" => 3i64,
                "Kids" => vec![
                    Object::Reference(shared),
                    Object::Reference(shared),
                    Object::Reference(only),
                ],
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.save(path).expect("fixture");
        pages_id
    }

    /// Asserted rather than assumed, for the reason `save.rs` gives at length: a
    /// future `lopdf` that deduplicates its page walk would leave both guards
    /// below reachable by nothing while their outcomes kept passing.
    #[test]
    fn the_shared_fixture_really_does_present_one_object_under_two_numbers() {
        let dir = TempDir::new("shared-pre");
        let path = dir.file("in.pdf");
        let _ = shared_fixture(&path);

        let doc = Document::load(&path).expect("load");
        let ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
        assert_eq!(ids.len(), 3, "three page numbers");
        assert_eq!(
            ids[0], ids[1],
            "and the first two resolve to ONE object --- if this fails, lopdf now \
             deduplicates and the guards below are dead code"
        );
        assert_ne!(
            ids[1], ids[2],
            "the control: the third is a different object, so the assertion above \
             is about a shared page rather than about every id being equal"
        );
    }

    #[test]
    fn a_page_named_twice_is_turned_once() {
        let dir = TempDir::new("shared-turn");
        let path = dir.file("in.pdf");
        let _ = shared_fixture(&path);

        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 1,
            },
        )
        .expect("build");
        let printed = reload(&out);
        for id in printed.get_pages().values() {
            assert_eq!(
                effective_rotation(&printed, *id),
                90,
                "one quarter-turn. Composing once per page number reads back the \
                 90 it just wrote and leaves 180 --- on paper, sideways"
            );
        }
    }

    /// The damaging member of the family: not a page turned twice, but a page
    /// deleted that the reader asked to print.
    #[test]
    fn a_page_a_kept_number_also_names_is_not_dropped() {
        let dir = TempDir::new("shared-drop");
        let path = dir.file("in.pdf");
        let _ = shared_fixture(&path);

        // Print page 1 only. Page 2 is the same object, so building `doomed` from
        // the dropped numbers alone deletes the page that was asked for and prints
        // a blank sheet.
        let out = build(
            &path,
            &Job {
                pages: Pages::Only(vec![1]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        let labels = page_labels(&printed);
        assert!(
            labels.iter().any(|label| label.contains("page 1")),
            "the page asked for is still there and still draws its text: {labels:?}"
        );
        assert!(
            !labels.iter().any(|label| label.contains("page 3")),
            "the control: the page that only a dropped number names really was \
             dropped, so the assertion above is not satisfied by a build that \
             deletes nothing: {labels:?}"
        );
    }

    /// `/Count` counts entries in the tree, and two numbers naming one doomed
    /// object remove two of them.
    ///
    /// Reachable only because the fixture has a third page: dropping both numbers
    /// of the shared page needs something left to keep, and an empty selection is
    /// refused long before `drop_pages`.
    #[test]
    fn a_shared_page_costs_the_tree_one_count_per_number_it_answered_to() {
        let dir = TempDir::new("shared-count");
        let path = dir.file("in.pdf");
        let pages_id = shared_fixture(&path);

        let before = reload(&std::fs::read(&path).expect("read"));
        assert_eq!(
            declared_count(&before, pages_id),
            3,
            "the control: the tree starts out claiming three pages"
        );

        // Keep page 3 only, so both of the shared page's numbers go.
        let out = build(
            &path,
            &Job {
                pages: Pages::Only(vec![3]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        assert_eq!(
            page_labels(&printed).len(),
            1,
            "one page is left in the tree"
        );
        assert_eq!(
            declared_count(&printed, pages_id),
            1,
            "and `/Count` says so. Decrementing once per doomed OBJECT leaves 2 \
             here --- a tree that claims a page it does not have"
        );
    }

    #[test]
    fn a_rotation_wraps_rather_than_growing() {
        let dir = TempDir::new("wrap");
        let path = dir.file("in.pdf");
        fixture(&path, 1, 270);

        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 2,
            },
        )
        .expect("build");
        let printed = reload(&out);
        let id = *printed.get_pages().values().next().expect("one page");
        assert_eq!(effective_rotation(&printed, id), 90);
    }

    #[test]
    fn a_page_the_document_does_not_have_is_refused() {
        let dir = TempDir::new("range-error");
        let path = dir.file("in.pdf");
        fixture(&path, 3, 0);
        let error = build(
            &path,
            &Job {
                pages: Pages::Only(vec![2, 9]),
                turns: 0,
            },
        )
        .expect_err("must refuse");
        assert!(error.contains('9'), "{error}");
    }

    #[test]
    fn an_empty_selection_is_refused() {
        // Printing nothing is a bug in whatever built the job, not a job.
        let dir = TempDir::new("empty");
        let path = dir.file("in.pdf");
        fixture(&path, 2, 0);
        assert!(build(
            &path,
            &Job {
                pages: Pages::Only(vec![]),
                turns: 0,
            },
        )
        .is_err());
    }

    #[test]
    fn a_subset_drops_the_objects_it_orphaned() {
        // "Fewer objects than before" is not this test: deleting a page removes
        // the page object itself, so the count falls whether or not anything
        // was collected, and the mutation that deletes the sweep survived that
        // assertion. What only the sweep can remove is the *content stream* a
        // deleted page pointed at, so that is what is named and looked for.
        let dir = TempDir::new("collect");
        let path = dir.file("in.pdf");
        fixture(&path, 8, 0);

        let source = Document::load(&path).expect("load");
        let orphaned: Vec<_> = source
            .get_pages()
            .iter()
            .filter(|(number, _)| **number != 1)
            .flat_map(|(_, id)| source.get_page_contents(*id))
            .collect();
        assert!(!orphaned.is_empty(), "the fixture has no streams to orphan");

        let out = build(
            &path,
            &Job {
                pages: Pages::Only(vec![1]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        // Object numbers are deliberately not made contiguous, so an id here
        // still names what it named in the source.
        let left: Vec<_> = orphaned
            .iter()
            .filter(|id| printed.objects.contains_key(id))
            .collect();
        assert!(
            left.is_empty(),
            "{} content stream(s) of dropped pages are still in the file: {left:?}",
            left.len()
        );
    }

    #[test]
    fn an_outline_naming_pages_that_are_gone_is_dropped() {
        let dir = TempDir::new("outline");
        let path = dir.file("in.pdf");
        fixture(&path, 4, 0);

        // Give it an outline pointing at a page the subset will not keep.
        let mut doc = Document::load(&path).expect("load");
        let last = *doc.get_pages().get(&4).expect("page 4");
        let item = doc.add_object(dictionary! {
            "Title" => Object::string_literal("The end"),
            "Dest" => vec![Object::Reference(last), "Fit".into()],
        });
        let outlines = doc.add_object(dictionary! {
            "Type" => "Outlines", "First" => item, "Last" => item, "Count" => 1,
        });
        doc.catalog_mut()
            .expect("catalog")
            .set("Outlines", outlines);
        doc.save(&path).expect("save");

        let out = build(
            &path,
            &Job {
                pages: Pages::Only(vec![1]),
                turns: 0,
            },
        )
        .expect("build");
        let printed = reload(&out);
        assert!(printed
            .catalog()
            .expect("catalog")
            .get(b"Outlines")
            .is_err());
    }

    /// What the OS's own PDF parser makes of built bytes.
    ///
    /// A **third** parser: independent of `lopdf`, which wrote the job, and of
    /// PDFium, which drew what the reader was looking at. Every other check in this
    /// module asks `lopdf` to read back a file `lopdf` produced, which cannot
    /// distinguish "the document says this" from "our serialiser and our loader
    /// agree about this" --- and it is the second that a printer does not care
    /// about. Neither platform's choice is a neutral third party either, which is
    /// the point: each is the parser that platform's own print path uses.
    fn read_back(bytes: &[u8]) -> os_pdf::Reading {
        // macOS asks for text as well, because the checks below assert *which*
        // pages survived and PDFKit can say. The print path deliberately does not
        // pay for that (see `print_macos::read`), and Windows cannot supply it at
        // any price.
        #[cfg(target_os = "macos")]
        return crate::print_macos::read_with_text(bytes)
            .expect("the OS parser could not read the print job");
        #[cfg(windows)]
        return crate::print_win::read(bytes).expect("the OS parser could not read the print job");
    }

    #[test]
    fn a_third_parser_reads_back_exactly_the_pages_that_were_kept() {
        let dir = TempDir::new("pdfkit-range");
        let path = dir.file("in.pdf");
        fixture(&path, 5, 0);

        let out = build(
            &path,
            &Job {
                pages: Pages::Only(vec![2, 4]),
                turns: 0,
            },
        )
        .expect("build");

        let reading = read_back(&out);
        // The portable half: *how many* pages a parser that did not write them can
        // find. Asserted on both platforms.
        assert_eq!(reading.pages.len(), 2, "{reading:?}");

        if !OS_PARSER_HAS_TEXT {
            // Said out loud rather than gated away. The count above is a real check
            // and this is a real hole in it: a subset that kept pages 1 and 3 would
            // satisfy everything asserted here. `a_third_parser_checks_a_job_built_-`
            // `from_a_document_we_did_not_write` covers the same property on both
            // platforms by using per-page rotation instead of text, which is why
            // this gap is acceptable rather than merely admitted.
            println!(
                "[SKIP] which pages survived: this platform's OS parser extracts no \
                 text, so only the count is pinned here"
            );
            return;
        }
        // Which pages, read by something that did not write them.
        assert!(
            reading.pages[0]
                .text
                .as_deref()
                .unwrap_or_default()
                .contains("page 2"),
            "{reading:?}"
        );
        assert!(
            reading.pages[1]
                .text
                .as_deref()
                .unwrap_or_default()
                .contains("page 4"),
            "{reading:?}"
        );
    }

    #[test]
    fn a_third_parser_sees_the_rotation_the_page_inherited_and_the_one_we_added() {
        // The pair is the point. `effective_rotation` returning 0 instead of
        // reading the tree writes 90 here where 180 is correct, and only the
        // second case can tell --- the first is 90 either way.
        for (inherited, expected) in [(0, 90), (90, 180)] {
            let dir = TempDir::new(&format!("pdfkit-turn-{inherited}"));
            let path = dir.file("in.pdf");
            fixture(&path, 2, inherited);

            let out = build(
                &path,
                &Job {
                    pages: Pages::All,
                    turns: 1,
                },
            )
            .expect("build");

            let reading = read_back(&out);
            for page in &reading.pages {
                assert_eq!(
                    page.rotation, expected,
                    "inherited {inherited}: {reading:?}"
                );
            }
        }
    }

    #[test]
    fn a_third_parser_accepts_the_handed_over_file_tail_and_all() {
        // The passthrough fixture carries bytes past `%%EOF` so a rewrite is
        // distinguishable from a copy. That trick is only legitimate because
        // readers tolerate the tail --- asserted here rather than assumed, by a
        // reader that is not the one which wrote it.
        let dir = TempDir::new("pdfkit-tail");
        let path = dir.file("in.pdf");
        fixture(&path, 3, 0);

        let out = build(
            &path,
            &Job {
                pages: Pages::All,
                turns: 0,
            },
        )
        .expect("build");

        assert_eq!(out, std::fs::read(&path).expect("source"));
        assert_eq!(read_back(&out).pages.len(), 3);
    }

    /// `build` fed documents that no Rust code in this repository wrote.
    ///
    /// Every other check in this module builds its input with `fixture`, which
    /// is `lopdf`'s own serialiser --- so the module tests a writer against its
    /// own reader, and `read_back` makes only the *output* side independent.
    /// A defect the writer and the loader share is invisible to that, which is
    /// the trap `docs/TRAPS.md` records as "a writer and its own reader agree
    /// about a document that is wrong", and printing is the one subsystem here
    /// whose output leaves the process.
    ///
    /// So both ends are independent here. The inputs come from the hand-rolled
    /// generators under `testdata/`, which assemble PDF bytes directly and
    /// share no code with anything under test; the page list, the expected
    /// rotations and the verdict all come from PDFKit. `lopdf` appears only as
    /// the thing being tested --- deliberately, because a check that derives
    /// its expectations from the library under test agrees with itself by
    /// construction.
    ///
    /// The subset keeps the first and last page, so the dropped set is neither
    /// a prefix nor a suffix and the `/Kids` surgery lands in the middle, and
    /// it adds a quarter turn, so each surviving page's rotation has to be
    /// resolved up its own `/Parent` chain and composed rather than written.
    ///
    /// What makes *which* pages survived observable is per-page rotation:
    /// `rotated.pdf` carries 0/90/180/270 on four otherwise byte-identical
    /// pages, so keeping the wrong two is a different rotation pair. On a
    /// fixture whose pages all share one rotation only the count and the
    /// composition are pinned, and the run says which case each fixture was
    /// rather than leaving that to be assumed.
    #[test]
    fn a_third_parser_checks_a_job_built_from_a_document_we_did_not_write() {
        let mut examined = 0;
        for name in [
            "rotated.pdf",
            "text-heavy.pdf",
            "vector-multi.pdf",
            "outline-hostile.pdf",
            "incr-scan-5p.pdf",
            "hostile-filters.pdf",
        ] {
            let path = Path::new("../testdata").join(name);
            if !path.exists() {
                println!("[SKIP] {name}: fixture not generated");
                continue;
            }
            let source = std::fs::read(&path).expect("read source");

            // The baseline, from the parser that will read the print job --- not
            // from the one that builds it.
            let Some(before) = os_pdf::read(&source) else {
                println!("[SKIP] {name}: the OS parser refused the source document");
                continue;
            };
            let count = before.pages.len();
            if count < 3 {
                println!("[SKIP] {name}: {count} pages, too few to drop a middle");
                continue;
            }

            let keep = [1u32, u32::try_from(count).expect("page count")];
            let expected: Vec<i64> = keep
                .iter()
                .map(|number| {
                    let at = usize::try_from(*number).expect("page number") - 1;
                    (before.pages[at].rotation + 90).rem_euclid(360)
                })
                .collect();

            let out = build(
                &path,
                &Job {
                    pages: Pages::Only(keep.to_vec()),
                    turns: 1,
                },
            )
            .unwrap_or_else(|e| panic!("{name}: build failed: {e}"));

            let after = os_pdf::read(&out)
                .unwrap_or_else(|| panic!("{name}: the OS parser could not read the built job"));

            assert_eq!(
                after.pages.len(),
                keep.len(),
                "{name}: page count, {after:?}"
            );
            let got: Vec<i64> = after
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect();
            assert_eq!(got, expected, "{name}: rotations");

            // Says which of the two cases this fixture was, because a fixture
            // with one rotation throughout cannot report a wrong *choice* of
            // pages and a run that does not say so reads as if it had.
            let distinct: HashSet<i64> = before
                .pages
                .iter()
                .map(|page| page.rotation.rem_euclid(360))
                .collect();
            let discriminating = if distinct.len() > 1 {
                "pins which pages survived"
            } else {
                "pins the count and the composition only"
            };
            println!("[OK] {name:20} {count} pages, rotations {distinct:?} --- {discriminating}");
            examined += 1;
        }

        // A run where every fixture was absent prints six SKIP lines and
        // otherwise looks exactly like a run where every one passed.
        assert!(
            examined > 0,
            "no fixture was examined --- generate testdata/ (BUILD.md, Test fixtures)"
        );
    }

    #[test]
    fn a_job_of_the_wrong_size_is_refused_before_it_reaches_paper() {
        use super::expect_pages;
        assert!(expect_pages(2, Some(2)).is_ok());
        assert!(expect_pages(1, Some(2)).is_err());
        assert!(expect_pages(3, Some(2)).is_err());
        // "Everything" has no count to check against, so the only recognisable
        // wrong answer is nothing at all.
        assert!(expect_pages(5, None).is_ok());
        assert!(expect_pages(0, None).is_err());
        // And an empty selection is refused earlier, by `resolve` --- so a
        // zero-page job with a zero expectation is a state nothing can reach,
        // and this pins which of the two guards is doing the work.
        assert!(expect_pages(0, Some(0)).is_ok());
    }

    #[test]
    fn every_level_of_the_page_tree_learns_it_lost_a_page() {
        // Three groups of two. Dropping one page from the first group and both
        // from the last means the root must fall by three while the middles
        // fall by different amounts --- so a walk that stops at the page's own
        // parent, and one that decrements the root once per *group* rather than
        // once per page, are both wrong here and in different directions.
        let dir = TempDir::new("nested");
        let path = dir.file("in.pdf");
        let (root, middles) = nested_fixture(&path, 3, 2);

        let mut doc = Document::load(&path).expect("load");
        assert_eq!(declared_count(&doc, root), 6);
        drop_pages(&mut doc, &[1, 5, 6]).expect("drop");

        assert_eq!(declared_count(&doc, root), 3, "root");
        assert_eq!(declared_count(&doc, middles[0]), 1, "first group");
        assert_eq!(declared_count(&doc, middles[1]), 2, "untouched group");
        assert_eq!(declared_count(&doc, middles[2]), 0, "emptied group");
        // And the tree agrees with itself: what the root claims is what a
        // reader walking `/Kids` actually finds.
        assert_eq!(doc.get_pages().len(), 3);
    }

    /// The control for replacing `lopdf::delete_pages` with `drop_pages`.
    ///
    /// A refactor claiming to change nothing has to be shown to change nothing,
    /// so both routes run on the same input and their bytes are compared. Same
    /// procedure as the mark-and-sweep move, which was verified by running the
    /// pre-move code as a control rather than by reading it.
    ///
    /// The 775-page corpora are deliberately **not** in this list even though
    /// they are the interesting case: `lopdf`'s side of the comparison is the
    /// quadratic one, and in the debug profile the gate runs in it costs 20 s.
    /// They were checked once, by hand, at 775 -> 2 pages --- identical bytes,
    /// 620.5 ms against 1.2 ms and 663.1 ms against 1.0 ms (docs/PLAN.md).
    #[test]
    fn control_page_deletion_matches_lopdf_byte_for_byte() {
        use std::time::Instant;

        let save = |doc: &mut Document| {
            super::sweep::collect(doc).expect("collect");
            let mut out = Vec::new();
            doc.save_to(&mut out).expect("save");
            out
        };
        let load = |path: &Path| {
            Document::load_with_options(
                path,
                lopdf::LoadOptions {
                    max_decompressed_size: Some(super::MAX_DECODE),
                    ..Default::default()
                },
            )
            .expect("load")
        };

        let dir = TempDir::new("control");
        let synthetic = dir.file("in.pdf");
        fixture(&synthetic, 6, 90);

        let mut cases: Vec<(String, PathBuf)> = vec![("synthetic-6p".into(), synthetic)];
        for name in [
            "vector-multi.pdf",
            "rotated.pdf",
            "outline-hostile.pdf",
            "incr-scan-5p.pdf",
        ] {
            let path = Path::new("../testdata").join(name);
            if path.exists() {
                cases.push((name.into(), path));
            } else {
                println!("[SKIP] {name}: fixture not generated");
            }
        }

        for (name, path) in cases {
            let present: Vec<u32> = load(&path).get_pages().keys().copied().collect();
            // Keep the first and the last, so the dropped set is neither a
            // prefix nor a suffix and the `/Kids` surgery has to be right in
            // the middle of the array.
            let keep = [1, *present.last().expect("pages")];
            let dropped: Vec<u32> = present
                .iter()
                .copied()
                .filter(|n| !keep.contains(n))
                .collect();

            let mut theirs = load(&path);
            let t = Instant::now();
            theirs.delete_pages(&dropped);
            let their_ms = t.elapsed().as_secs_f64() * 1e3;
            let their_bytes = save(&mut theirs);

            let mut ours = load(&path);
            let t = Instant::now();
            drop_pages(&mut ours, &dropped).expect("drop");
            let our_ms = t.elapsed().as_secs_f64() * 1e3;
            let our_bytes = save(&mut ours);

            println!(
                "[{}] {name:22} {:>4} -> {:>2} pages   lopdf {:>9.1} ms   ours {:>7.1} ms   {:>6.0}x",
                if our_bytes == their_bytes { "OK" } else { "DIFF" },
                present.len(),
                keep.len(),
                their_ms,
                our_ms,
                their_ms / our_ms.max(1e-6),
            );
            assert_eq!(our_bytes, their_bytes, "{name}: bytes differ");
        }
    }

    #[test]
    fn a_parent_cycle_does_not_hang_the_rotation_walk() {
        // `effective_rotation` runs on input we did not write. A `/Parent` loop
        // is exactly the shape the outline walk already has to defend against.
        let dir = TempDir::new("cycle");
        let path = dir.file("in.pdf");
        fixture(&path, 1, 0);
        let mut doc = Document::load(&path).expect("load");
        let page = *doc.get_pages().values().next().expect("one page");
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("page")
            .set("Parent", Object::Reference(page));
        assert_eq!(effective_rotation(&doc, page), 0);
    }
}
