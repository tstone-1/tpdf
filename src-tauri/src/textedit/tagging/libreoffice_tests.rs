//! A list in paragraph styles of the document's own, as LibreOffice exports it.
//!
//! `fixtures/libreoffice-list.pdf` is the unchanged tagged export of
//! `testdata/textedit-producer-list-styles.fodt`, a synthetic document whose
//! every name is invented, made with LibreOffice 26.8.0.3 by the command in
//! `docs/VERIFICATION.md` (*A LibreOffice list in paragraph styles of its own*). It is
//! committed because a hosted runner has no LibreOffice, and the digest below
//! is what ties these tests to that one export. Its fonts are subsets of
//! Liberation Sans (SIL Open Font License 1.1).
//!
//! What it holds that the synthetic tests beside this one build by hand:
//! `LI > LBody > paragraph` under the style's name, a sublist in a body beside
//! its paragraph, a `Span` naming the language around every paragraph's words
//! with a `Span` for each hyphen inside it, centred lines, a justified
//! paragraph, a table that states its bounds, a header and a footer no element
//! reaches, and a bullet in the font the body is set in.
use crate::textedit::{self, Change, EditFont, Layout};
use lopdf::{Document, Object};
use sha2::{Digest, Sha256};

const EXPORT: &[u8] = include_bytes!("fixtures/libreoffice-list.pdf");

const OFFERED: [&str; 27] = [
    "Example Declaration",
    "Synthetic sample for testing, issued by ACME Ltd",
    "This invented document declares nothing. Every name in it is made up.",
    "1.",
    "Sample maker:",
    " ACME Ltd, Sample Street 1, 00000 Exampletown, which manufactures demonstration wid",
    "gets for representative documentation and characterisation purposes, straightforwardly and comprehen",
    "sively, without exception.",
    "2.",
    "Sample product:",
    " Example Widget, model EX-1.",
    "3.",
    "The widget described above meets the invented requirements listed here:",
    "Rule one",
    " of the Example Widget Rulebook, first edition.",
    "Rule two of the same rulebook.",
    "4.",
    "Further remarks:",
    " none, because this is a sample.",
    "A separate bulleted list follows the numbered one.",
    "Its second item ends the lists.",
    "Place of issue:",
    "Exampletown",
    "Name:",
    "Jane Example",
    "Signature:",
    "______________________",
];

fn export() -> Document {
    Document::load_mem(EXPORT).unwrap()
}

fn offered(doc: &Document) -> Vec<String> {
    textedit::scan(doc, 0)
        .unwrap()
        .runs
        .into_iter()
        .map(|run| run.text)
        .collect()
}

#[test]
fn textedit_a_libreoffice_list_in_its_own_paragraph_styles_is_offered() {
    let digest = Sha256::digest(EXPORT)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest, "8f7e3f4514c568989892003a3170af06ff9134c7d826347067777e95a5fe62fb",
        "the committed export changed; see docs/VERIFICATION.md before replacing it"
    );
    let doc = export();
    assert_eq!(offered(&doc), OFFERED);
    // Read-only, and counted so that a run going missing is not a run offered:
    // four bullets and two hyphens, the two lines of the justified paragraph,
    // and the footer's two shows, its link and the words before it.
    let page = textedit::inspect(&doc, 0).unwrap();
    let kept: Vec<&str> = page
        .preserved
        .iter()
        .map(|run| run.text.trim())
        .filter(|text| !text.is_empty())
        .collect();
    assert_eq!(
        kept.iter().filter(|text| **text == "-").count(),
        2,
        "{kept:?}"
    );
    assert_eq!(
        kept.iter()
            .filter(|text| text.chars().all(|ch| ch == textedit::fonts::OPAQUE))
            .count(),
        4,
        "{kept:?}"
    );
    assert_eq!(
        kept.iter()
            .filter(|text| text.contains("Signed for") || text.contains("words to be set"))
            .count(),
        2,
        "{kept:?}"
    );
    assert!(kept.contains(&"example.com"), "{kept:?}");
    assert!(
        kept.iter().any(|text| text.starts_with("ACME Ltd ")),
        "{kept:?}"
    );
    assert_eq!(kept.len(), 10, "{kept:?}");
    // An item is one block: its label, its bold first words and the rest of
    // its paragraph. The sublist's items are blocks of their own.
    let block = |text: &str| {
        let run = page.runs.runs.iter().find(|run| run.text == text).unwrap();
        page.blocks[&run.operator]
    };
    assert_eq!(block("1."), block("Sample maker:"));
    assert_eq!(block("1."), block("sively, without exception."));
    assert_ne!(block("1."), block("2."));
    assert_eq!(block("3."), block(OFFERED[12]));
    assert_ne!(block("3."), block("Rule one"));
    assert_eq!(block("Rule one"), block(OFFERED[14]));
    assert_ne!(block("Rule one"), block(OFFERED[15]));
}

#[test]
fn textedit_a_libreoffice_list_is_edited_and_its_structure_written_back() {
    let mut doc = export();
    let before = textedit::scan(&doc, 0).unwrap();
    // A list paragraph, an item's bold first words, a sublist's bullet, and a
    // centred line, which is edited about its centre and so needs a layout.
    let edits = [
        (6, "representative", "illustrative", false),
        (4, "maker", "rules", false),
        (15, "two", "six", false),
        (0, "Declaration", "Exception", true),
    ];
    let changes: Vec<Change> = edits
        .iter()
        .map(|&(index, old, new, centred)| {
            let run = &before.runs[index];
            assert!(run.text.contains(old), "{}", run.text);
            Change {
                layout: centred.then(|| Layout::opened(run, EditFont::Original)),
                page: 0,
                revision: before.revision.clone(),
                operator: run.operator,
                original: run.text.clone(),
                replacement: run.text.replacen(old, new, 1),
            }
        })
        .collect();
    let objects = doc.objects.clone();
    textedit::write(&mut doc, &changes).unwrap();
    let after = textedit::scan(&doc, 0).unwrap();
    assert_eq!(after.runs.len(), before.runs.len());
    for (index, (was, is)) in before.runs.iter().zip(&after.runs).enumerate() {
        match edits.iter().find(|edit| edit.0 == index) {
            Some(&(_, old, new, centred)) => {
                assert_eq!(is.text, was.text.replacen(old, new, 1));
                // A centred line moves to stay centred; the others start
                // where they did.
                assert_eq!(is.matrix[4] == was.matrix[4], !centred, "{index}");
                assert_eq!(is.matrix[5], was.matrix[5], "{index}");
            }
            None => {
                assert_eq!(is.text, was.text, "{index}");
                assert_eq!(is.matrix, was.matrix, "{index}");
                assert_eq!(is.display_rect, was.display_rect, "{index}");
            }
        }
    }
    // The centred line is still centred: both ends moved by the same amount.
    let (was, is) = (&before.runs[0], &after.runs[0]);
    let shift = is.matrix[4] - was.matrix[4];
    assert!(shift > 0.);
    // The page position is an f32 and the run's own start is rounded.
    assert!(
        ((was.advance - is.advance) / 2. - shift).abs() < 0.05,
        "{shift}"
    );
    // Every structure element, the role map and the parent tree are the ones
    // that were read, and what was read-only is shown by the same bytes.
    let structure: Vec<_> = objects
        .iter()
        .filter(|(_, object)| {
            object
                .as_dict()
                .is_ok_and(|dict| dict.has(b"S") || dict.has(b"Nums") || dict.has(b"RoleMap"))
        })
        .collect();
    assert!(structure.len() > 80, "{}", structure.len());
    for (id, object) in structure {
        assert_eq!(&doc.objects[id], object, "{id:?}");
    }
    let kept = |doc: &Document| {
        textedit::inspect(doc, 0)
            .unwrap()
            .preserved
            .iter()
            .map(|run| (run.text.clone(), run.matrix))
            // A layout ends with an empty show that puts the cursor back.
            .filter(|(text, _)| !text.is_empty())
            .collect::<Vec<_>>()
    };
    assert_eq!(kept(&doc), kept(&export()));
    // The fonts are written back untouched: nothing was embedded.
    for (id, object) in &objects {
        if matches!(object, Object::Stream(stream) if stream.dict.has(b"Length1")) {
            assert_eq!(&doc.objects[id], object, "{id:?}");
        }
    }
}

// A change with no layout fits the source's own advance, so a longer word in a
// list paragraph is refused whole rather than run into the rest of its line,
// and a centred line refuses a change that would be written at its origin.
#[test]
fn textedit_a_libreoffice_list_refuses_what_it_cannot_place() {
    let doc = export();
    let before = textedit::scan(&doc, 0).unwrap();
    for (index, old, new) in [
        (4, "maker", "manufacturer"),
        (0, "Declaration", "Exception"),
        // The subset has no such letter, and no layout to bring a font.
        (4, "maker", "maze"),
    ] {
        let mut edited = doc.clone();
        let run = &before.runs[index];
        let refused = textedit::write(
            &mut edited,
            &[Change {
                layout: None,
                page: 0,
                revision: before.revision.clone(),
                operator: run.operator,
                original: run.text.clone(),
                replacement: run.text.replacen(old, new, 1),
            }],
        );
        assert!(refused.is_err(), "{new}");
        assert_eq!(edited.objects, doc.objects, "{new}");
    }
}

// The same export after Acrobat's Fill & Sign has put a signature picture on
// it: the addition is not the document's content and no element owns it, like
// anything appended to a tagged page without marks, so the page's own text is
// offered as before. Any other tag in that place is still refused.
#[test]
fn textedit_a_libreoffice_list_stays_editable_under_a_fill_and_sign_addition() {
    let added = |tag: &str| {
        let mut doc = export();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let own = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .clone();
        let addition = doc.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            format!("/{tag} BMC q 1 g 500 60 40 20 re f Q EMC").into_bytes(),
        ));
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", vec![own, Object::Reference(addition)]);
        doc
    };
    assert_eq!(offered(&added("ADBE_FillSign")), OFFERED);
    assert_eq!(offered(&added("Artifact")), OFFERED);
    assert_eq!(
        textedit::scan(&added("ADBE_Other"), 0).unwrap_err(),
        "marked content without MCID must be an Artifact"
    );
}
