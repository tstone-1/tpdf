use super::*;
use crate::textedit;
use lopdf::{Dictionary, Object, Stream};

#[test]
fn textedit_stream_conversion_is_limited_to_patched_text_shows() {
    let bytes = b"BT (FIRST) Tj (SECOND) Tj ET";
    let mut content = Content::decode_strict(bytes).unwrap();
    content.operations[1].operator = "TJ".into();
    content.operations[1].operands = vec![Object::Array(vec![
        Object::string_literal("FI"),
        (-100).into(),
    ])];
    assert!(rewrite(bytes, &content, &BTreeSet::new()).is_err());
    let saved = rewrite(bytes, &content, &BTreeSet::from([1])).unwrap();
    assert!(saved.ends_with(b" (SECOND) Tj ET"));
    content.operations[1].operator = "q".into();
    assert!(rewrite(bytes, &content, &BTreeSet::from([1])).is_err());
}

#[test]
fn textedit_stream_patch_preserves_coordinates_comments_and_untouched_text_bytes() {
    let source = b"% untouched decimals\r\n.23999999 0 0 .23999999 0 0 cm\nBT /F1 12 Tf 1 0 0 1 40 180 Tm\n(SYNTHETIC FIRST) Tj\n1 0 0 1 40 140.000001 Tm [(SECOND) 0 ( LINE)] TJ ET % tail";
    let mut doc = textedit::tests::fixture();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), source.to_vec()));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    let edit = textedit::tests::change(&doc);
    textedit::write(&mut doc, &[edit]).unwrap();
    let after = textedit::page_content(&doc, page).unwrap();
    let expected = String::from_utf8(source.to_vec())
        .unwrap()
        .replace("(SYNTHETIC FIRST) Tj", "(EDITED FIRST) Tj");
    // The source merger contributes one newline on each read.
    assert_eq!(after, format!("{expected}\n\n").as_bytes());
}

#[test]
fn textedit_stream_patch_tracks_nested_tokens_and_multiple_edits() {
    for bytes in [
        b"% Tj\nBT /F#31 12 Tf (a\\) Tj (nested)\\\r\n) Tj ET".as_slice(),
        b"/P << /MCID 0 /Hidden [(Tj) <544a>] >> BDC BT [<41> 0 (B)] TJ ET EMC",
        b"BT (a(b)c\\\\)Tj (untouched)Tj ET% trailing\n",
    ] {
        let mut content = Content::decode_strict(bytes)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(bytes)));
        let edits: BTreeSet<_> = content
            .operations
            .iter()
            .enumerate()
            .filter_map(|(i, o)| matches!(o.operator.as_str(), "Tj" | "TJ").then_some(i))
            .collect();
        assert!(!edits.is_empty());
        let original = rewrite(bytes, &content, &BTreeSet::new()).unwrap();
        assert_eq!(original, bytes);
        for &index in &edits {
            let op = &mut content.operations[index];
            op.operands[0] = if op.operator == "TJ" {
                Object::Array(vec![Object::string_literal("X")])
            } else {
                Object::string_literal("X")
            };
        }
        let result = rewrite(bytes, &content, &edits).unwrap();
        assert_eq!(
            Content::decode_strict(&result).unwrap().encode().unwrap(),
            content.encode().unwrap()
        );
    }
}

#[test]
fn textedit_stream_patch_refuses_disagreement_and_bounds_work() {
    let original = b"BT (A) Tj ET";
    let mut content = Content::decode_strict(original).unwrap();
    content.operations[1].operands[0] = Object::string_literal("B");
    assert!(rewrite(original, &content, &BTreeSet::new())
        .unwrap_err()
        .contains("untouched"));
    content.operations.pop();
    assert!(rewrite(original, &content, &BTreeSet::from([1])).is_err());
    let content = Content::decode_strict(b"q Q").unwrap();
    assert!(rewrite(b"q Q 1", &content, &BTreeSet::new()).is_err());
    assert!(rewrite(b"q Q Q", &content, &BTreeSet::new()).is_err());
    assert!(rewrite(b"q q", &content, &BTreeSet::new()).is_err());
    assert!(rewrite(b"q Q", &content, &BTreeSet::from([0])).is_err());
    for bytes in [
        b"(".as_slice(),
        b"(\\)",
        b"<",
        b"[",
        b"<<",
        b"]",
        b">",
        b")",
    ] {
        assert!(token(bytes, &mut 0, 0).is_err());
    }
    for (open, close, allowed) in [(b'[', b']', 33), (b'(', b')', 32)] {
        for n in [allowed, allowed + 1] {
            let bytes = [vec![open; n], vec![close; n]].concat();
            assert_eq!(token(&bytes, &mut 0, 0).is_ok(), n == allowed);
        }
    }
    let content = Content::decode_strict(original).unwrap();
    let bytes = [original.as_slice(), &vec![b' '; MAX_CONTENT]].concat();
    assert!(rewrite(&bytes, &content, &BTreeSet::new()).is_err());
}

#[test]
fn textedit_stream_discovery_refuses_unpatchable_nesting() {
    for depth in [32, 33] {
        let source = [
            b"BT /F1 12 Tf 40 180 Td ".as_slice(),
            &vec![b'('; depth],
            b"A",
            &vec![b')'; depth],
            b" Tj ET",
        ]
        .concat();
        let mut doc = textedit::tests::fixture();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let stream = doc.add_object(Stream::new(Dictionary::new(), source));
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), depth == 32);
        if depth == 32 {
            let mut edit = textedit::tests::change(&doc);
            edit.replacement.clear();
            textedit::write(&mut doc, &[edit]).unwrap();
        }
    }
}

#[test]
fn textedit_stream_patch_opening_with_a_number_is_kept_apart_from_the_operator_before() {
    // xdvipdfmx writes a show's operand straight after the operator before it.
    let bytes = b"BT /F1 12 Tf 10 0 Td[(A)]TJ ET";
    let content = Content::decode_strict(bytes).unwrap();
    let show = 3;
    assert_eq!(content.operations[show].operator, "TJ");
    let moved = vec![
        lopdf::content::Operation::new(
            "Tm",
            [1, 0, 0, 1, 20, 30].into_iter().map(Object::from).collect(),
        ),
        content.operations[show].clone(),
    ];
    let saved = rewrite_expanded(
        bytes,
        &content,
        &BTreeSet::from([show]),
        &std::collections::BTreeMap::from([(show, moved)]),
    )
    .unwrap();
    // Read back the way a reopened document is: by the scan.
    let mut doc = textedit::tests::fixture();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), saved));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    let runs = textedit::scan(&doc, 0).unwrap().runs;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].matrix[4..], [20., 30.]);
}

/// A painted path moved with its line is accepted only as `wrap::translated`
/// writes it: a pure translation after a saved state, and every opening
/// restored once before the next opens and before the page ends.
#[test]
fn a_moved_path_is_accepted_only_as_a_paired_pure_translation() {
    use lopdf::content::Operation;
    let bytes = b"0 g 20 170 21.6 0.8 re f 20 150 21.6 0.8 re f";
    let content = Content::decode_strict(bytes).unwrap();
    let op = |index: usize| content.operations[index].clone();
    let cm =
        |values: [f32; 6]| Operation::new("cm", values.into_iter().map(Object::Real).collect());
    let open = |index: usize, transform: Operation| {
        (
            index,
            vec![Operation::new("q", vec![]), transform, op(index)],
        )
    };
    let close = |index: usize| (index, vec![op(index), Operation::new("Q", vec![])]);
    let down = cm([1., 0., 0., 1., 0., -14.]);
    let apply = |moves: Vec<(usize, Vec<Operation>)>| {
        let edits: BTreeSet<usize> = moves.iter().map(|(index, _)| *index).collect();
        rewrite_expanded(bytes, &content, &edits, &moves.into_iter().collect())
    };
    // The control: both paths moved, each bracket closed before the next.
    let saved = apply(vec![
        open(1, down.clone()),
        close(2),
        open(3, down.clone()),
        close(4),
    ])
    .unwrap();
    assert_eq!(Content::decode_strict(&saved).unwrap().operations.len(), 11);
    // A transform that scales or turns the path is not a move.
    for turned in [[2., 0., 0., 2., 0., -14.], [0., 1., -1., 0., 0., -14.]] {
        assert!(
            apply(vec![open(1, cm(turned)), close(2)]).is_err(),
            "{turned:?}"
        );
    }
    // An opening with no restore, a restore with no opening, and a second
    // opening inside the first.
    assert!(apply(vec![open(1, down.clone())]).is_err());
    assert!(apply(vec![close(2)]).is_err());
    assert!(apply(vec![open(1, down.clone()), open(3, down.clone()), close(4)]).is_err());
}
