//! Whether a save over the reader's file can land, asked before the document
//! is closed.
//!
//! A module of its own rather than more of `save/tests.rs`, which is ten
//! thousand lines; still under `save::`, which is what `scripts/mutate_rust.py`
//! selects tests by.
//!
//! **The refusals are asked of the two halves that run while the reader still
//! has the document**: [`stage_in_place`] for a rewrite and [`append_ready`]
//! for an append. Everything after them runs past the close, where a refusal
//! costs the edits.

use super::*;

/// A one-page document of this test's own, and a plan fingerprinted against
/// it that keeps the page.
fn one_page(dir: &crate::testutil::TempDir) -> (PathBuf, Plan) {
    use lopdf::{dictionary, Object};
    let path = dir.join("open.pdf");
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page.into()],
            "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    doc.save(&path).expect("write the document");
    let plan = Plan {
        field_edits: Vec::new(),
        tab_order: false,
        opened_as: Some(Fingerprint::of(&path).expect("fingerprint")),
        baseline: 1,
        pages: vec![crate::edits::PageView {
            id: 1,
            source: PageSource::Baseline(0),
            turns: 1,
            crop: None,
        }],
        redactions: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
        sources: Vec::new(),
        forms: Vec::new(),
        text_edits: Vec::new(),
        text_layers: Vec::new(),
        added_text: Vec::new(),
        protection: Default::default(),
        compress: Default::default(),
        new_fields: Vec::new(),
        marks: Vec::new(),
    };
    (path, plan)
}

/// The same plan with a highlight on the page and no turn, which an append
/// writes.
fn marked(plan: Plan) -> Plan {
    use crate::docmodel::{MarkKind, Quad};
    Plan {
        pages: plan
            .pages
            .iter()
            .map(|page| crate::edits::PageView { turns: 0, ..*page })
            .collect(),
        marks: vec![crate::edits::PlannedMark {
            text_size: None,
            kind: MarkKind::Highlight,
            stamp: None,
            field: None,
            image: None,
            reply_to: None,
            at: 0,
            quads: vec![Quad {
                left: 10.0,
                top: 10.0,
                right: 20.0,
                bottom: 20.0,
            }],
            strokes: Vec::new(),
            color: [1.0, 0.9, 0.2],
            width: 1.0,
            author: "tpdf".to_string(),
            note: String::new(),
            made: "D:20261119000000Z".to_string(),
        }],
        ..plan
    }
}

/// The names in `dir` other than `keep`: what a refused save left behind.
/// Only the Windows refusals ask, since only there is a rewrite refused.
#[cfg(windows)]
fn others(keep: &Path) -> Vec<PathBuf> {
    let root = keep.parent().expect("a parent");
    std::fs::read_dir(root)
        .expect("list the folder")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path != keep)
        .collect()
}

/// Marks `path` read-only for the length of `body`, and clears it after.
fn read_only<T>(path: &Path, body: impl FnOnce() -> T) -> T {
    let mut permissions = std::fs::metadata(path).expect("stat").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(path, permissions.clone()).expect("mark it read-only");
    let answer = body();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(path, permissions).expect("clear the attribute");
    answer
}

/// An append to a file that cannot be written is refused by the half that runs
/// before the close, and the file is not touched.
#[test]
fn an_append_to_a_read_only_file_is_refused_before_the_close() {
    let dir = crate::testutil::TempDir::new("landing-append-readonly");
    let (path, plan) = one_page(&dir);
    let plan = marked(plan);
    assert_eq!(mode_for_source(&plan, &path), Mode::Append, "the control");
    let before = std::fs::read(&path).expect("read");

    let why = read_only(&path, || append_ready(&path, &plan)).expect_err("must refuse");

    assert!(
        why.message.contains("cannot save over"),
        "the refusal names the file it cannot write: {}",
        why.message
    );
    assert_eq!(std::fs::read(&path).expect("read"), before);
}

/// The check before an append opens the file and writes nothing: the bytes
/// and the modification time are what they were.
///
/// The control for the refusal above, and the half the check's safety rests
/// on: a check that truncated, or that touched the time, would make the
/// file disagree with the fingerprint the landing compares it against.
#[test]
fn the_check_before_an_append_changes_nothing_in_the_file() {
    let dir = crate::testutil::TempDir::new("landing-append-untouched");
    let (path, plan) = one_page(&dir);
    let plan = marked(plan);
    let before = std::fs::read(&path).expect("read");
    let was = std::fs::metadata(&path)
        .expect("stat")
        .modified()
        .expect("mtime");

    append_ready(&path, &plan).expect("a writable file passes");

    assert_eq!(std::fs::read(&path).expect("read"), before);
    assert_eq!(
        std::fs::metadata(&path)
            .expect("stat")
            .modified()
            .expect("mtime"),
        was
    );
}

/// A rewrite over a file Windows would refuse to replace is refused before
/// anything is staged.
///
/// The rename past the close fails on a read-only file with *Access is
/// denied*; asked here, the reader keeps the document and nothing is left in
/// the folder.
#[test]
#[cfg(windows)]
fn a_rewrite_over_a_read_only_file_is_refused_before_the_close() {
    let dir = crate::testutil::TempDir::new("landing-rewrite-readonly");
    let (path, plan) = one_page(&dir);
    let before = std::fs::read(&path).expect("read");

    let why =
        read_only(&path, || stage_in_place(&path, &plan, None, &Here)).expect_err("must refuse");

    assert!(
        why.message.contains("read-only"),
        "the refusal says why: {}",
        why.message
    );
    assert_eq!(std::fs::read(&path).expect("read"), before);
    assert_eq!(others(&path), Vec::<PathBuf>::new(), "nothing staged");
}

/// A rewrite over a file another program holds without delete sharing is
/// refused before the close.
///
/// That holder makes the rename fail for as long as it holds the file. The
/// check waits as the rename would, about one and a half seconds, and then
/// refuses with the document still open.
#[test]
#[cfg(windows)]
fn a_rewrite_over_a_file_held_without_delete_sharing_is_refused_before_the_close() {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_SHARE_READ | FILE_SHARE_WRITE`, without `FILE_SHARE_DELETE`.
    const READ_WRITE_SHARING: u32 = 0x1 | 0x2;
    let dir = crate::testutil::TempDir::new("landing-rewrite-held");
    let (path, plan) = one_page(&dir);
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(READ_WRITE_SHARING)
        .open(&path)
        .expect("hold the file");

    let why = stage_in_place(&path, &plan, None, &Here).expect_err("must refuse");

    drop(held);
    assert!(
        why.message.contains("cannot save over"),
        "the refusal names the file: {}",
        why.message
    );
    assert_eq!(others(&path), Vec::<PathBuf>::new(), "nothing staged");
}

/// The control for the two refusals above: the same document, writable and
/// held by nobody, is staged.
#[test]
fn a_rewrite_over_a_free_file_is_staged() {
    let dir = crate::testutil::TempDir::new("landing-rewrite-free");
    let (path, plan) = one_page(&dir);

    let staged = stage_in_place(&path, &plan, None, &Here).expect("stage");

    assert!(staged.path.exists());
    let _ = std::fs::remove_file(&staged.path);
}
