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

use std::path::Path;

use lopdf::{Document, LoadOptions, Object};

use crate::sweep;

/// Cap on a single decompressed stream, matching the sanitizing rewrite.
///
/// A print job parses attacker-controlled input like everything else here, and
/// spike 0.4 measured a 2,879-byte input inflating to 1 GiB.
const MAX_DECODE: usize = 64 * 1024 * 1024;

/// Which pages to print.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pages {
    /// Every page, in document order.
    All,
    /// Exactly these, one-based, in the order given.
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
        doc.delete_pages(&dropped);
        // Destinations into pages that are no longer here.
        doc.catalog_mut()
            .map_err(|e| format!("no document catalog: {e}"))?
            .remove(b"Outlines");
    }

    let turns = job.turns % 4;
    if turns != 0 {
        let ids: Vec<_> = doc.get_pages().values().copied().collect();
        for id in ids {
            let composed = (effective_rotation(&doc, id) + i64::from(turns) * 90).rem_euclid(360);
            doc.get_object_mut(id)
                .and_then(Object::as_dict_mut)
                .map_err(|e| format!("page {id:?} is not a dictionary: {e}"))?
                .set("Rotate", composed);
        }
    }

    sweep::collect(&mut doc);

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| format!("could not serialise the print job: {e}"))?;
    Ok(out)
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
fn effective_rotation(doc: &Document, page: lopdf::ObjectId) -> i64 {
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
    use super::{build, effective_rotation, Job, Pages};
    use lopdf::{dictionary, Document, Object, Stream};
    use std::path::{Path, PathBuf};

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
