//! Saving a document that holds pages of another file.
//!
//! A module of its own rather than more of `save/tests.rs`, which is ten
//! thousand lines; still under `save::`, which is what `scripts/mutate_rust.py`
//! selects tests by.
//!
//! **Every fixture is built here, from labels, and every assertion reads the
//! written file back through `lopdf`'s own page walk** rather than through
//! anything this module wrote. The labels are synthetic and uncompressed, so a
//! page's identity is a string in its content stream and a leak is a string in
//! the file's bytes.

use lopdf::{dictionary, Stream};

use super::*;
use crate::docmodel::{MarkKind, Quad, SourceId};
use crate::edits::{PageView, PlannedMark, PlannedSource};

/// A document with one page per label, each drawing its label in Helvetica.
///
/// The content streams are not compressed, so the label is findable in the
/// bytes of any file that carries the stream.
fn labelled(labels: &[&str]) -> Vec<u8> {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let font = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let mut kids = Vec::new();
    for label in labels {
        let content = document.add_object(Stream::new(
            dictionary! {},
            format!("BT /F1 12 Tf 72 720 Td ({label}) Tj ET").into_bytes(),
        ));
        kids.push(Object::from(document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            "Contents" => content,
        })));
    }
    let count = kids.len() as i64;
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => count,
        }),
    );
    let catalog = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("the fixture must save");
    bytes
}

/// A scratch directory of this test's own, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "tpdf-import-{name}-{}-{serial}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir)
    }

    /// Writes `bytes` under `name` and answers the path.
    fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).expect("write a fixture");
        path
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

/// A page of the opened file.
fn own(page: u32) -> PageView {
    PageView {
        id: u64::from(page) + 1,
        source: PageSource::Baseline(page),
        turns: 0,
        crop: None,
    }
}

/// A page of the other file the plan names as source 1.
fn theirs(id: u64, page: u32) -> PageView {
    PageView {
        id,
        source: PageSource::Imported {
            source: SourceId::from_raw(1),
            page,
        },
        turns: 0,
        crop: None,
    }
}

/// A plan over a `baseline`-page file, placing `pages`, with the other file
/// at `other` fingerprinted as it is now.
fn plan_with(baseline: u32, pages: Vec<PageView>, other: &Path) -> Plan {
    Plan {
        text_edits: Vec::new(),
        forms: Vec::new(),
        baseline,
        opened_as: None,
        pages,
        marks: Vec::new(),
        redactions: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
        sources: vec![PlannedSource {
            id: 1,
            path: other.to_path_buf(),
            opened_as: Some(Fingerprint::of(other).expect("fingerprint the other file")),
        }],
    }
}

/// A highlight on the plan's page `at`.
fn highlight(at: u32) -> PlannedMark {
    PlannedMark {
        kind: MarkKind::Highlight,
        stamp: None,
        image: None,
        reply_to: None,
        at,
        quads: vec![Quad {
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
        made: "D:20260919120000Z".into(),
    }
}

/// Each page's label, read from its content stream, in the written order.
fn labels_of(path: &Path) -> Vec<String> {
    let after = Document::load(path).expect("the written file must parse");
    ordered_pages(&after)
        .into_iter()
        .map(|id| {
            let content = after.get_page_content(id);
            let text = String::from_utf8_lossy(&content).into_owned();
            let open = text.find('(').expect("a label");
            let close = text[open..].find(')').expect("a closed label") + open;
            text[open + 1..close].to_string()
        })
        .collect()
}

/// The pages of the other file land where the plan puts them, in the plan's
/// order rather than the file's, between the opened file's own pages --- and
/// the page nobody asked for is not in the file at all.
#[test]
fn a_rewrite_places_pages_of_another_file_where_the_plan_puts_them() {
    let scratch = Scratch::new("places");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A", "OWN-B"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1", "THEIR-2", "THEIR-3"]));
    let out = scratch.join("out.pdf");
    let plan = plan_with(2, vec![own(0), theirs(3, 2), theirs(4, 0), own(1)], &other);

    write_copy(&source, &plan, &out, None, &Here).expect("the copy");

    assert_eq!(
        labels_of(&out),
        vec!["OWN-A", "THEIR-3", "THEIR-1", "OWN-B"],
        "the reader's order, both files interleaved"
    );
    let bytes = std::fs::read(&out).expect("read back");
    let holds = |needle: &str| bytes.windows(needle.len()).any(|w| w == needle.as_bytes());
    assert!(
        holds("THEIR-3"),
        "the control: a label is findable in the bytes"
    );
    assert!(
        !holds("THEIR-2"),
        "the page nobody asked for is not carried in the file"
    );
}

/// A plan carrying a page of another file is never an append, even beside a
/// mark that would otherwise be one --- and that is asked of the one plan
/// shape where only the imported page can answer: as many pages as the file,
/// its own pages at their own positions.
#[test]
fn a_plan_with_a_page_of_another_file_is_never_appendable() {
    let scratch = Scratch::new("appendable");
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1"]));
    let mut plan = plan_with(2, vec![own(0), own(1)], &other);
    plan.marks = vec![highlight(0)];
    plan.sources.clear();
    assert!(
        plan.is_appendable(),
        "the control: the same pages and mark with no import are an append"
    );

    let mut plan = plan_with(2, vec![own(0), theirs(3, 0)], &other);
    plan.marks = vec![highlight(0)];
    assert!(
        !plan.is_appendable(),
        "a page of another file cannot be appended"
    );
    assert!(!plan.is_identity());
    assert_eq!(mode_for(&plan, 1), Mode::Rewrite);
}

/// A file changed since its pages were inserted is refused, and nothing is
/// written --- changed **without changing its length**, which is the case only
/// the digest can see.
#[test]
fn a_source_changed_since_its_pages_were_inserted_is_refused() {
    let scratch = Scratch::new("changed");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1"]));
    let out = scratch.join("out.pdf");
    let plan = plan_with(1, vec![own(0), theirs(2, 0)], &other);

    let before = std::fs::read(&other).expect("read");
    let after = labelled(&["THEIR-X"]);
    assert_eq!(before.len(), after.len(), "the premise: the same length");
    std::fs::write(&other, &after).expect("change it");

    let why = write_copy(&source, &plan, &out, None, &Here).expect_err("a changed file");
    assert!(
        why.message
            .contains("other.pdf has changed since its pages were inserted"),
        "{why}"
    );
    assert!(
        !why.changed,
        "not the opened document's change: Reload would throw the journal away for nothing"
    );
    assert!(!out.exists(), "nothing was written");

    std::fs::write(&other, &before).expect("put it back");
    write_copy(&source, &plan, &out, None, &Here)
        .expect("the control: the same plan over the original bytes is written");
}

/// A file whose fingerprint was never taken is refused rather than trusted.
#[test]
fn a_source_with_no_fingerprint_is_refused() {
    let scratch = Scratch::new("unprinted");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1"]));
    let out = scratch.join("out.pdf");
    let mut plan = plan_with(1, vec![own(0), theirs(2, 0)], &other);
    plan.sources[0].opened_as = None;

    let why = write_copy(&source, &plan, &out, None, &Here).expect_err("unfingerprinted");
    assert!(why.message.contains("did not record"), "{why}");
    assert!(!out.exists());
}

/// A mark, a turn and a crop on a page of another file land on that page and
/// no other.
#[test]
fn a_mark_a_turn_and_a_crop_on_an_imported_page_land_on_it() {
    let scratch = Scratch::new("marks");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A", "OWN-B"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1", "THEIR-2"]));
    let out = scratch.join("out.pdf");
    let want = [20.0, 30.0, 400.0, 500.0];
    let mut imported = theirs(3, 1);
    imported.turns = 1;
    imported.crop = Some(want);
    let mut plan = plan_with(2, vec![own(0), imported, own(1)], &other);
    plan.marks = vec![highlight(1)];

    write_copy(&source, &plan, &out, None, &Here).expect("the copy");

    assert_eq!(labels_of(&out), vec!["OWN-A", "THEIR-2", "OWN-B"]);
    let after = Document::load(&out).expect("parse");
    let pages = ordered_pages(&after);
    let annots = |id| {
        after
            .get_dictionary(id)
            .ok()
            .and_then(|page| page.get(b"Annots").ok())
            .and_then(|annots| match annots {
                Object::Array(list) => Some(list.len()),
                Object::Reference(r) => after.get_object(*r).ok()?.as_array().ok().map(Vec::len),
                _ => None,
            })
            .unwrap_or(0)
    };
    assert_eq!(annots(pages[1]), 1, "the mark is on the imported page");
    assert_eq!(annots(pages[0]) + annots(pages[2]), 0, "and on no other");
    assert_eq!(
        crate::pagetree::effective_rotation(&after, pages[1]),
        90,
        "the turn is on the imported page"
    );
    assert_eq!(crate::pagetree::effective_rotation(&after, pages[0]), 0);
    let crop = |id| crate::pagetree::box_on(&after, id, b"CropBox").map(|b| b.map(f64::from));
    assert_eq!(
        crop(pages[1]),
        Some(want),
        "the crop is on the imported page"
    );
    assert_eq!(crop(pages[0]), None);
    assert_eq!(crop(pages[2]), None);
}

/// The same page of the other file placed twice becomes two pages, each of
/// which takes a mark of its own.
///
/// **This is the rounds in `import_pages`**: one `merge::import` call cannot
/// take a page twice, so a writer that asked for both in one call would be
/// refused, and one that reused the first object would put the mark on both.
#[test]
fn the_same_page_placed_twice_is_two_pages() {
    let scratch = Scratch::new("twice");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1", "THEIR-2"]));
    let out = scratch.join("out.pdf");
    let mut plan = plan_with(1, vec![theirs(2, 0), own(0), theirs(3, 0)], &other);
    plan.marks = vec![highlight(2)];

    write_copy(&source, &plan, &out, None, &Here).expect("the copy");

    assert_eq!(labels_of(&out), vec!["THEIR-1", "OWN-A", "THEIR-1"]);
    let after = Document::load(&out).expect("parse");
    let pages = ordered_pages(&after);
    assert_ne!(pages[0], pages[2], "two objects, not one object twice");
    let marked = |id| {
        after
            .get_dictionary(id)
            .map(|page| page.has(b"Annots"))
            .unwrap_or(false)
    };
    assert!(
        marked(pages[2]) && !marked(pages[0]),
        "the mark is on one of them"
    );
}

/// The coordinator hands the rewriter the other file's bytes, exactly as they
/// are on disk --- and hands nothing for a plan that names no other file.
#[test]
fn the_other_file_crosses_to_the_rewriter_as_bytes() {
    let scratch = Scratch::new("crosses");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1"]));
    let out = scratch.join("out.pdf");
    let plan = plan_with(1, vec![own(0), theirs(2, 0)], &other);

    let writer = super::tests::fake_writer(Ok(b"%PDF-1.7".to_vec()));
    write_copy(&source, &plan, &out, None, &writer).expect("the copy");
    let handed = super::tests::merges_of(&writer);
    assert_eq!(handed.len(), 1, "one rewrite, handed the other file");
    let (whole, each) = &handed[0];
    assert_eq!(each.len(), 1);
    assert_eq!(each[0].label, "other.pdf");
    assert_eq!(
        &whole[each[0].at..each[0].at + each[0].len],
        std::fs::read(&other).expect("read").as_slice()
    );

    let mut own_only = plan_with(1, vec![own(0)], &other);
    own_only.sources.clear();
    let writer = super::tests::fake_writer(Ok(b"%PDF-1.7".to_vec()));
    let out = scratch.join("own-only.pdf");
    write_copy(&source, &own_only, &out, None, &writer).expect("the control");
    assert!(super::tests::merges_of(&writer).is_empty());
}

/// A plan placing pages of another file, handed no files, is refused in the
/// words of the plan rather than written short.
#[test]
fn a_rewrite_handed_no_other_file_is_refused() {
    let scratch = Scratch::new("unhanded");
    let original = labelled(&["OWN-A"]);
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1"]));
    let plan = plan_with(1, vec![own(0), theirs(2, 0)], &other);
    let why = rewrite_update(&original, &plan, Job::Save, None).expect_err("no inputs");
    assert!(why.message.contains("was not handed to the save"), "{why}");
}

/// A print job carries the imported pages: it is built by the same rewrite.
#[test]
fn a_print_job_carries_the_imported_pages() {
    let _serial = print_lock();
    let scratch = Scratch::new("print");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1"]));
    let plan = plan_with(1, vec![theirs(2, 0), own(0)], &other);

    let bytes = print_bytes(&source, &plan, 0, None, &Here).expect("a print job");
    let job = scratch.put("job.pdf", &bytes);
    assert_eq!(labels_of(&job), vec!["THEIR-1", "OWN-A"]);
}

/// A file whose encryption an empty password opens --- a permission-restricted
/// one, which every reader opens without asking --- is refused rather than
/// imported in the clear.
///
/// **That is the case only `was_encrypted` sees**: `lopdf` authenticates it
/// with the empty password unprompted and removes the trailer's `/Encrypt`, so
/// `is_encrypted` answers no. `checked` records the same trap for the opened
/// document.
#[test]
fn a_source_an_empty_password_opens_is_refused_rather_than_decrypted() {
    let Some(encrypted) =
        Some(Path::new("../testdata/incr-encrypted-open.pdf")).filter(|path| path.exists())
    else {
        println!("[SKIP] incr-encrypted-open.pdf not generated (BUILD.md)");
        return;
    };
    let scratch = Scratch::new("encrypted");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A"]));
    let other = scratch.put(
        "locked.pdf",
        &std::fs::read(encrypted).expect("read the fixture"),
    );
    let out = scratch.join("out.pdf");
    let plan = plan_with(1, vec![own(0), theirs(2, 0)], &other);

    let why = write_copy(&source, &plan, &out, None, &Here).expect_err("encrypted");
    assert!(why.message.contains("locked.pdf is encrypted"), "{why}");
    assert!(!out.exists());
}

/// A page past the end of the other file is refused in the half that has
/// written nothing, in words about the plan.
#[test]
fn a_page_the_other_file_does_not_have_is_refused() {
    let scratch = Scratch::new("past");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1"]));
    let out = scratch.join("out.pdf");
    let plan = plan_with(1, vec![own(0), theirs(2, 4)], &other);

    let why = write_copy(&source, &plan, &out, None, &Here).expect_err("past the end");
    assert!(
        why.message
            .contains("the edits place page 5 of a document with 1 page(s)"),
        "{why}"
    );
}

/// A merge of a document that holds pages of another file is refused in the
/// words of the merge, rather than in the rewrite's sentence about a file
/// the save was not given.
#[test]
fn a_merge_of_a_document_holding_imported_pages_says_why_it_is_refused() {
    let scratch = Scratch::new("merge");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1"]));
    let third = scratch.put("third.pdf", &labelled(&["THIRD-1"]));
    let out = scratch.join("out.pdf");
    let plan = plan_with(1, vec![own(0), theirs(2, 0)], &other);

    let why = write_merged(&source, &plan, &[third], &out, None, &Here).expect_err("refused");
    assert!(
        why.message.contains("save this document before merging it"),
        "{why}"
    );
}

/// A replacement on page `page` of the file at `path`, read from that file's
/// own scan --- which is where a reader's edit comes from too: the other
/// file's worker, addressed by its page number there.
fn edit_of(path: &Path, page: u32, replacement: &str) -> crate::textedit::Change {
    let document = Document::load(path).expect("the other file must parse");
    let runs = crate::textedit::scan(&document, page).expect("scan the other file's page");
    let run = runs
        .runs
        .first()
        .expect("one run on a labelled page")
        .clone();
    crate::textedit::Change {
        layout: None,
        page,
        revision: runs.revision,
        operator: run.operator,
        original: run.text,
        replacement: replacement.into(),
    }
}

/// A replacement on an inserted page is written into that page, and the
/// opened file's page of the same number is untouched by it.
///
/// **The second half is the assertion that bites.** Both edits address page 0
/// with the same operator, so a writer that applied them to one document
/// would produce a file where one label is right by accident and the other is
/// wrong --- and a test that checked only the inserted page would pass. The
/// labels are literal strings in uncompressed content streams, so this reads
/// the words out of the written file rather than out of anything the writer
/// reported.
#[test]
fn a_rewrite_writes_a_replacement_into_the_page_it_imports() {
    let scratch = Scratch::new("text");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A", "OWN-B"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1", "THEIR-2"]));
    let out = scratch.join("out.pdf");
    let mut plan = plan_with(2, vec![own(0), theirs(3, 0), own(1)], &other);
    plan.text_edits = vec![crate::textedit::Edit::imported(
        1,
        edit_of(&other, 0, "EDITED"),
    )];

    write_copy(&source, &plan, &out, None, &Here).expect("the copy");

    assert_eq!(
        labels_of(&out),
        vec!["OWN-A", "EDITED", "OWN-B"],
        "the inserted page carries the reader's words, and only that page"
    );
    let bytes = std::fs::read(&out).expect("read back");
    let holds = |needle: &str| bytes.windows(needle.len()).any(|w| w == needle.as_bytes());
    assert!(!holds("THEIR-1"), "the replaced words are not in the file");
    assert!(!holds("THEIR-2"), "the page nobody asked for is not here");
}

/// Both documents can be edited in one save, and each replacement reaches the
/// document its page number belongs to.
#[test]
fn one_save_edits_the_opened_file_and_the_inserted_page() {
    let scratch = Scratch::new("both");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A", "OWN-B"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1"]));
    let out = scratch.join("out.pdf");
    let mut plan = plan_with(2, vec![own(0), theirs(3, 0), own(1)], &other);
    plan.text_edits = vec![
        crate::textedit::Edit::opened(edit_of(&source, 0, "OWN")),
        crate::textedit::Edit::imported(1, edit_of(&other, 0, "EDITED")),
    ];

    write_copy(&source, &plan, &out, None, &Here).expect("the copy");

    assert_eq!(
        labels_of(&out),
        vec!["OWN", "EDITED", "OWN-B"],
        "each replacement landed on its own page"
    );
}

/// The three ways a plan can address a replacement at a page this save will
/// not place, each refused before the graph is touched and in its own words.
///
/// A plan reaches the writer from outside the process --- across the worker
/// boundary, out of a restored session, out of the fuzz target --- so the
/// model's refusals are claims to check here rather than to rely on.
#[test]
fn replacements_a_save_cannot_place_are_refused_with_the_reason() {
    let scratch = Scratch::new("refuse-text");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1", "THEIR-2"]));
    let edit = edit_of(&other, 0, "THEIR-EDIT");

    // A file the plan does not list at all.
    let out = scratch.join("unknown.pdf");
    let mut plan = plan_with(1, vec![own(0), theirs(2, 0)], &other);
    plan.text_edits = vec![crate::textedit::Edit::imported(9, edit.clone())];
    let why = write_copy(&source, &plan, &out, None, &Here).expect_err("no such file");
    assert!(
        why.message
            .contains("from document 9, which the save was not given"),
        "{why}"
    );
    assert!(!out.exists());

    // A page of a file the plan lists, at a position the plan does not place.
    let out = scratch.join("unplaced.pdf");
    let mut plan = plan_with(1, vec![own(0), theirs(2, 1)], &other);
    plan.text_edits = vec![crate::textedit::Edit::imported(1, edit.clone())];
    let why = write_copy(&source, &plan, &out, None, &Here).expect_err("not placed");
    assert!(
        why.message
            .contains("text on page 1 of another document, which this save does not place"),
        "{why}"
    );
    assert!(!out.exists());

    // One page of one file in two positions: the edit would appear in both.
    let out = scratch.join("twice.pdf");
    let mut plan = plan_with(1, vec![own(0), theirs(2, 0), theirs(3, 0)], &other);
    plan.text_edits = vec![crate::textedit::Edit::imported(1, edit)];
    let why = write_copy(&source, &plan, &out, None, &Here).expect_err("placed twice");
    assert!(
        why.message
            .contains("page 1 of another document is in this one 2 times"),
        "{why}"
    );
    assert!(!out.exists());
}

/// A replacement addressed at the opened document while its page is really a
/// page of the other file is refused by the digest, not silently applied ---
/// the writer never reads a change against a document it was not validated
/// in.
#[test]
fn a_replacement_addressed_at_the_wrong_document_is_refused_by_its_digest() {
    let scratch = Scratch::new("digest");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1"]));
    let out = scratch.join("out.pdf");
    let mut plan = plan_with(1, vec![own(0), theirs(2, 0)], &other);
    // The other file's own change, wrongly claimed to be the opened file's.
    plan.text_edits = vec![crate::textedit::Edit::opened(edit_of(&other, 0, "EDIT"))];

    let why = write_copy(&source, &plan, &out, None, &Here).expect_err("wrong document");
    assert!(
        why.message
            .contains("text changed since this run was inspected"),
        "{why}"
    );
    assert!(!out.exists());
}

/// A removal takes the words off the reader's own page while the pages of
/// another file are written into the same file, in one rewrite.
///
/// **The step this was written to pin is an ordering one, and the comment above
/// `apply_redactions` already claimed it.** A redaction's ordinals are worked
/// out against the opened file's objects, and `rewrite` runs the removal last
/// on the grounds that nothing above it touches a content stream. `import_pages`
/// sits above it and copies another file's objects in --- so this asserts the
/// claim rather than trusting it: the words go from the page they were marked
/// on, both inserted pages arrive whole, and the opened file's other page keeps
/// its own.
///
/// `text_objects: 1` is what the fixture has --- one `Tj` per page --- so
/// `redact::remove_shows` is being given the truthful count it refuses to work
/// without.
///
/// The three labels are the discrimination. A writer that redacted by output
/// slot rather than by baseline page would take `THEIR-1`, which sits at the
/// slot the marked page used to occupy; a writer that lost the import would
/// leave a two-page file. Neither is caught by asking only whether `OWN-A` is
/// gone.
#[test]
fn a_rewrite_removes_from_the_reader_s_page_and_carries_the_inserted_ones_whole() {
    let scratch = Scratch::new("redact-beside-import");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A", "OWN-B"]));
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1", "THEIR-2"]));
    let out = scratch.join("out.pdf");

    // The inserted pages go in *front* of the marked one, so the page's number
    // in the file and its slot in the output are different numbers.
    let mut plan = plan_with(2, vec![theirs(3, 0), theirs(4, 1), own(0), own(1)], &other);
    plan.redactions = vec![crate::edits::PlannedRedaction {
        // Baseline page 0 --- `OWN-A` --- which is slot 2 of the output.
        source: 0,
        shows: vec![0],
        text_objects: 1,
        areas: vec![[0.0, 700.0, 595.0, 780.0]],
        taking: vec!["OWN-A".to_string()],
        form_shows: Vec::new(),
        form_text_objects: Vec::new(),
        images: Vec::new(),
        image_objects: 0,
    }];

    write_copy(&source, &plan, &out, None, &Here).expect("the copy");

    // A tolerant reader, because the marked page has no label left to find and
    // `labels_of` requires one --- which is itself the first thing this asserts.
    let after: Vec<String> = {
        let document = Document::load(&out).expect("the written file must parse");
        ordered_pages(&document)
            .into_iter()
            .map(|id| {
                let text = String::from_utf8_lossy(&document.get_page_content(id)).into_owned();
                match (text.find('('), text.find(')')) {
                    (Some(open), Some(close)) if close > open => text[open + 1..close].to_string(),
                    _ => String::new(),
                }
            })
            .collect()
    };
    assert_eq!(
        after.len(),
        4,
        "four pages, so the import happened at all: {after:?}"
    );
    assert_eq!(
        &after[..2],
        &["THEIR-1".to_string(), "THEIR-2".to_string()],
        "both inserted pages came across with their own text"
    );
    assert_eq!(
        after[3], "OWN-B",
        "and the opened file's other page is untouched"
    );
    assert_eq!(
        after[2], "",
        "the marked page's show operator is gone from the page it was marked on"
    );

    // And the bytes, which is the reader `docs/PLAN.md` §6 actually requires:
    // a label still in the file somewhere is a label that was not removed.
    let bytes = std::fs::read(&out).expect("read back");
    let holds = |needle: &str| bytes.windows(needle.len()).any(|w| w == needle.as_bytes());
    assert!(!holds("OWN-A"), "the marked words are not in the file");
    assert!(
        holds("THEIR-1") && holds("THEIR-2") && holds("OWN-B"),
        "and the control says the scan can see this file at all"
    );
}

/// The inserted page prints the very word the region covered, and the scan says
/// which page that is.
///
/// **The case the 2026-09-20 narrowing could only disclaim.** `verify::scan`
/// read the whole output and reported *"OWN-A is still in the file"*, which a
/// reader cannot tell from a removal that did not take --- so
/// `redact::inserted_pages_note` was added to say the scan could not tell
/// either. Since 2026-09-21 it can: the per-page walk places the surviving word
/// on slot 0, which is the inserted page, and `redact::marked_pages_note` reads
/// that against the slot the region was actually on.
///
/// Two things make this the real subject rather than a restatement of
/// `verify`'s own unit tests. The pages come from **two different files**, so
/// the carrier is an object `merge::import` renumbered into the output rather
/// than one the fixture builder placed; and the marked page's number in the
/// base file (0) and its slot in the output (1) are different numbers, so a
/// walk that answered in the wrong space would name the inserted page for the
/// marked one and vice versa --- which is the mistake that would read as
/// success.
///
/// The verdict is **unchanged and must be**: the word is in the file, so the
/// file is not verified. What the attribution buys is which half of that the
/// reader has to act on.
#[test]
fn a_word_surviving_on_an_inserted_page_is_placed_there_and_not_on_the_marked_one() {
    let scratch = Scratch::new("redact-import-attribution");
    let source = scratch.put("own.pdf", &labelled(&["OWN-A", "OWN-B"]));
    // The other file prints the same label the region covers.
    let other = scratch.put("other.pdf", &labelled(&["OWN-A"]));
    let out = scratch.join("out.pdf");

    let mut plan = plan_with(2, vec![theirs(3, 0), own(0), own(1)], &other);
    plan.redactions = vec![crate::edits::PlannedRedaction {
        source: 0,
        shows: vec![0],
        text_objects: 1,
        areas: vec![[0.0, 700.0, 595.0, 780.0]],
        taking: vec!["OWN-A".to_string()],
        form_shows: Vec::new(),
        form_text_objects: Vec::new(),
        images: Vec::new(),
        image_objects: 0,
    }];
    write_copy(&source, &plan, &out, None, &Here).expect("the copy");

    let bytes = std::fs::read(&out).expect("read back");
    let needles = vec!["OWN-A".to_string(), "OWN-B".to_string()];
    let report = crate::verify::scan(&bytes, &needles, None);

    assert!(
        report.found.contains("OWN-A"),
        "the inserted page still prints it, which is the whole fixture: {:?}",
        report.found
    );
    assert_eq!(
        report.located.get("OWN-A"),
        Some(&crate::verify::Located::Pages(crate::verify::Placed {
            pages: vec![0],
            more: 0,
        })),
        "on the inserted page, which is slot 0 --- and on no other"
    );
    // The control, and it is what says the walk is answering in output slots
    // rather than in baseline page numbers: `OWN-B` is baseline page 1 and
    // output slot 2, and the two answers have to differ.
    assert_eq!(
        report.located.get("OWN-B"),
        Some(&crate::verify::Located::Pages(crate::verify::Placed {
            pages: vec![2],
            more: 0,
        })),
        "the untouched page is placed where the output puts it, not where the file had it"
    );

    assert!(
        matches!(report.verdict(), crate::verify::Verdict::NotVerified(_)),
        "a word still in the file is still a leak, however well placed"
    );
    // Slot 1 is where the region was: baseline page 0, one page further down
    // because of the insert.
    assert_eq!(
        crate::redact::marked_pages_note(&report, &[1]).as_deref(),
        Some(
            "every word reported above is on a page no region was marked on --- page 2 carried \
             the regions, and none of them carries any of those words"
        ),
    );
    assert_eq!(
        crate::redact::inserted_pages_note(1, &report.found, report.placed()),
        None,
        "and the sentence that exists to disclaim an answer stays quiet now there is one"
    );
}

/// The image-only route is the one step an inserted page really blocks, and it
/// says so itself rather than being refused for the whole document.
///
/// **Through `rewrite_update_with`, which is where that job is refused outside
/// the render worker** --- `raster_redact::rewrite` is only reachable with a
/// PDFium document in hand. What this pins is the pairing: the ordinary rewrite
/// above accepts exactly the plan this refuses, so the two sentences cannot
/// drift into refusing the same thing twice or neither.
#[test]
fn the_image_only_route_refuses_a_plan_that_places_another_file_s_pages() {
    let scratch = Scratch::new("raster-import");
    let other = scratch.put("other.pdf", &labelled(&["THEIR-1"]));
    let plan = plan_with(1, vec![theirs(2, 0), own(0)], &other);
    let refused = rewrite_update(&labelled(&["OWN-A"]), &plan, Job::RasterRedact, None)
        .expect_err("the image-only job is not served here");
    assert!(
        refused.message.contains("sandboxed rendering worker"),
        "{:?}",
        refused.message
    );
}
