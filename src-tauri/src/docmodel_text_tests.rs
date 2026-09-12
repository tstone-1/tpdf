use super::*;

fn change(page: u32, operator: u32, replacement: &str) -> crate::textedit::Change {
    crate::textedit::Change {
        page,
        revision: vec![1; 32],
        operator,
        original: "SYNTHETIC ORIGINAL TEXT".into(),
        replacement: replacement.into(),
    }
}

#[test]
fn textedit_journal_replays_replacements_across_snapshots() {
    let mut doc = Doc::open(2);
    let page = doc.working().order()[0];
    let count = SNAPSHOT_EVERY + 3;
    for i in 0..count {
        doc.replace_text(page, change(0, 3, &format!("EDIT {i}")))
            .unwrap();
    }
    assert_eq!(doc.text_changes().len(), 1);
    for i in (0..count).rev() {
        assert_eq!(doc.text_changes()[0].replacement, format!("EDIT {i}"));
        assert!(doc.undo());
    }
    assert!(doc.text_changes().is_empty());
    assert!(!doc.can_undo());
    for i in 0..count {
        assert!(doc.redo());
        assert_eq!(doc.text_changes()[0].replacement, format!("EDIT {i}"));
    }
    assert!(!doc.can_redo());
}

#[test]
fn textedit_journal_restores_original_and_discards_abandoned_bodies() {
    let mut doc = Doc::open(2);
    let page = doc.working().order()[0];
    for i in 0..8 {
        doc.replace_text(page, change(0, 3, &format!("EDIT {i}")))
            .unwrap();
    }
    for _ in 0..5 {
        assert!(doc.undo());
    }
    doc.replace_text(page, change(0, 3, "BRANCH")).unwrap();
    assert_eq!(doc.text_versions.len(), 4);
    assert!(!doc.can_redo());
    let mut restore = change(0, 3, "");
    restore.replacement.clone_from(&restore.original);
    doc.replace_text(page, restore.clone()).unwrap();
    assert!(doc.text_changes().is_empty());
    let depth = doc.depth();
    doc.replace_text(page, restore).unwrap();
    assert_eq!(
        doc.depth(),
        depth,
        "restoring unchanged text adds no undo entry"
    );
    assert!(doc.undo());
    assert_eq!(doc.text_changes()[0].replacement, "BRANCH");
    assert!(doc.redo());
    assert!(doc.text_changes().is_empty());
}

#[test]
fn textedit_journal_refuses_stale_or_unbounded_input_atomically() {
    let mut doc = Doc::open(2);
    let page = doc.working().order()[0];
    doc.replace_text(page, change(0, 3, "FIRST")).unwrap();
    let baseline = doc.clone();
    let mut bad_digest = change(0, 3, "SECOND");
    bad_digest.revision[0] = 2;
    let mut bad_original = change(0, 3, "SECOND");
    bad_original.original = "STALE".into();
    let mut oversized_original = change(0, 4, "SECOND");
    oversized_original.original = "x".repeat(crate::textedit::MAX_TEXT + 1);
    let mut short_digest = change(0, 3, "SECOND");
    short_digest.revision.pop();
    for invalid in [
        bad_digest,
        bad_original,
        short_digest,
        oversized_original,
        change(1, 3, "SECOND"),
        change(0, 3, &"x".repeat(crate::textedit::MAX_TEXT + 1)),
    ] {
        assert!(doc.replace_text(page, invalid).is_err());
        assert_eq!(doc.working(), baseline.working());
        assert_eq!(doc.depth(), baseline.depth());
        assert_eq!(doc.text_versions, baseline.text_versions);
        assert_eq!(doc.next_text_version, baseline.next_text_version);
    }
}

#[test]
fn textedit_journal_bounds_history_but_reclaims_the_redo_tail() {
    let mut doc = Doc::open(1);
    let page = doc.working().order()[0];
    for i in 0..MAX_TEXT_VERSIONS {
        doc.replace_text(page, change(0, 3, &format!("EDIT {i}")))
            .unwrap();
    }
    assert!(doc.replace_text(page, change(0, 3, "OVER LIMIT")).is_err());
    assert_eq!(doc.text_versions.len(), MAX_TEXT_VERSIONS);
    for _ in 0..100 {
        assert!(doc.undo());
    }
    doc.replace_text(page, change(0, 3, "BRANCH")).unwrap();
    assert_eq!(doc.text_versions.len(), MAX_TEXT_VERSIONS - 99);
    assert_eq!(doc.text_changes()[0].replacement, "BRANCH");
    assert!(!doc.can_redo());
}

#[test]
fn textedit_journal_bounds_active_operands_and_reuses_restored_slots() {
    let mut doc = Doc::open(1);
    let page = doc.working().order()[0];
    for operator in 0..crate::textedit::MAX_CHANGES as u32 {
        doc.replace_text(page, change(0, operator, "EDIT")).unwrap();
    }
    assert!(doc.replace_text(page, change(0, 999, "NEW")).is_err());
    let mut restore = change(0, 0, "");
    restore.replacement.clone_from(&restore.original);
    doc.replace_text(page, restore).unwrap();
    doc.replace_text(page, change(0, 999, "NEW")).unwrap();
    assert_eq!(doc.text_changes().len(), crate::textedit::MAX_CHANGES);
}

#[test]
fn textedit_journal_and_redaction_refuse_each_other_without_retaining_bodies() {
    let mut doc = Doc::open(2);
    let page = doc.working().order()[0];
    let redaction = Redaction {
        page,
        area: Quad {
            left: 0.,
            top: 0.,
            right: 10.,
            bottom: 10.,
        },
    };
    doc.replace_text(page, change(0, 3, "EDIT")).unwrap();
    assert!(doc.redact(redaction).is_err());
    assert_eq!(doc.redaction_bodies(), 0);
    doc.undo();
    doc.redact(redaction).unwrap();
    assert!(doc.replace_text(page, change(0, 3, "EDIT")).is_err());
    assert!(doc.text_versions.is_empty());
}

#[test]
fn textedit_plans_follow_page_identity_and_filter_deleted_or_extracted_pages() {
    let edits = crate::edits::Edits::default();
    edits.open(7, 3, None);
    let state = edits.state(7).unwrap();
    let first = state.pages[0].id;
    let second = state.pages[1].id;
    let third = state.pages[2].id;
    edits.replace_text(7, first, change(0, 3, "FIRST")).unwrap();
    edits
        .replace_text(7, second, change(1, 3, "SECOND"))
        .unwrap();
    edits.move_page(7, first, Some(third)).unwrap();
    let plan = edits.plan(7).unwrap();
    assert!(!plan.is_identity());
    assert!(!plan.is_appendable());
    assert_eq!(
        plan.text_edits,
        vec![change(0, 3, "FIRST"), change(1, 3, "SECOND")]
    );
    assert_eq!(
        edits.plan_subset(7, &[0]).unwrap().text_edits,
        vec![change(1, 3, "SECOND")]
    );
    edits.delete(7, first).unwrap();
    assert_eq!(
        edits.plan(7).unwrap().text_edits,
        vec![change(1, 3, "SECOND")]
    );
    assert!(edits.replace_text(7, first, change(0, 3, "STALE")).is_err());
    edits.undo(7).unwrap();
    assert_eq!(edits.plan(7).unwrap().text_edits.len(), 2);
    let state = edits.undo(7).unwrap();
    assert!(state.dirty && state.can_undo && state.can_redo);
}

#[test]
fn textedit_journal_reaches_the_writer_after_page_moves_and_extraction() {
    let mut source = crate::textedit::tests::fixture();
    let edit = crate::textedit::tests::change(&source);
    let mut bytes = Vec::new();
    source.save_to(&mut bytes).unwrap();
    let edits = crate::edits::Edits::default();
    edits.open(7, 2, None);
    let pages = edits.state(7).unwrap().pages;
    edits.replace_text(7, pages[0].id, edit).unwrap();
    edits.move_page(7, pages[0].id, Some(pages[1].id)).unwrap();
    for (plan, expected) in [
        (
            edits.plan(7).unwrap(),
            vec!["SYNTHETIC FIRST", "EDITED FIRST"],
        ),
        (edits.plan_subset(7, &[0]).unwrap(), vec!["SYNTHETIC FIRST"]),
        (edits.plan_subset(7, &[1]).unwrap(), vec!["EDITED FIRST"]),
    ] {
        let saved =
            crate::save::rewrite_update(&bytes, &plan, crate::save::Job::Save, None).unwrap();
        let document = crate::encoding::load(&saved, None).unwrap();
        let actual: Vec<_> = (0..expected.len())
            .map(|page| {
                crate::textedit::scan(&document, page as u32).unwrap().runs[0]
                    .text
                    .clone()
            })
            .collect();
        assert_eq!(actual, expected);
    }
    edits.undo(7).unwrap();
    edits.undo(7).unwrap();
    assert!(edits.plan(7).unwrap().is_identity());
    assert!(!edits.state(7).unwrap().dirty);
}
