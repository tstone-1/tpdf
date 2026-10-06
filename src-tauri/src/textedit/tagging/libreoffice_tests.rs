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
//! reaches, and a bullet in the font the body is set in. Every page's content
//! is drawn under a clip the size of the page, and its fifth item is a
//! paragraph whose first line ends at a space and whose second ends at a
//! hyphen: the shapes a wrap of a list item meets (`docs/TEXTEDIT.md`, *What
//! moves with a wrap on such a page*).
use crate::textedit::{self, Change, EditFont, Layout};
use lopdf::{Document, Object};
use sha2::{Digest, Sha256};

const EXPORT: &[u8] = include_bytes!("fixtures/libreoffice-list.pdf");

const OFFERED: [&str; 32] = [
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
    "5.",
    "The demonstration widget is described here for illustration only and the sample maker gives no undertaking;",
    " ",
    "the characterisation that comes with it was written for representative documentation purposes, and it repre",
    "sents nothing at all.",
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
        digest, "558ff52a4bc29f8f4f07b320af70a699ff302a966f15b4a812f517e007ae5930",
        "the committed export changed; see docs/VERIFICATION.md before replacing it"
    );
    let doc = export();
    assert_eq!(offered(&doc), OFFERED);
    // Read-only, and counted so that a run going missing is not a run offered:
    // four bullets and three hyphens, the two lines of the justified paragraph,
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
        3,
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
    assert_eq!(kept.len(), 11, "{kept:?}");
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

/// Every show with text on the page, offered or read-only, in the order the
/// stream draws them: its text and where it starts.
fn shown(doc: &Document) -> Vec<(String, f64, f64)> {
    let page = textedit::inspect(doc, 0).unwrap();
    let mut all: Vec<_> = page
        .runs
        .runs
        .iter()
        .chain(&page.preserved)
        .filter(|run| !run.text.trim().is_empty())
        .map(|run| (run.operator, run.text.clone(), run.matrix[4], run.matrix[5]))
        .collect();
    all.sort_by_key(|(operator, ..)| *operator);
    all.into_iter()
        .map(|(_, text, x, y)| (text, x, y))
        .collect()
}

/// The export with `added` typed at the end of the run `text`, in the box the
/// editor opens, set in the document's own font.
fn typed(text: &str, added: &str) -> Result<Document, String> {
    typed_in(export(), text, added)
}

/// The same in a document that may have been edited before: `text` is the
/// whole of the run as it now reads.
fn typed_in(mut doc: Document, text: &str, added: &str) -> Result<Document, String> {
    let before = textedit::scan(&doc, 0).unwrap();
    let run = before.runs.iter().find(|run| run.text == text).unwrap();
    textedit::write(
        &mut doc,
        &[Change {
            layout: Some(Layout::opened(run, EditFont::Original)),
            page: 0,
            revision: before.revision.clone(),
            operator: run.operator,
            original: run.text.clone(),
            replacement: format!("{text}{added}"),
        }],
    )?;
    Ok(doc)
}

// A list item that is full to the page takes more words: it wraps, and what is
// below goes down with it, read-only or not. Three items, each with something
// under it the editor cannot rewrite: the bullets of a sublist, the hyphen its
// producer set at a line end, the justified paragraph. Each of those is drawn
// from the bytes it had, one distance lower, beside the text it belongs to.
// The table, which states its bounds, and the footer stay where they were.
#[test]
fn textedit_a_libreoffice_list_item_wraps_and_carries_what_is_below() {
    let source = shown(&export());
    let at = |shows: &[(String, f64, f64)], text: &str| {
        let found: Vec<_> = shows.iter().filter(|(shown, ..)| shown == text).collect();
        assert_eq!(found.len(), 1, "{text}");
        (found[0].1, found[0].2)
    };
    let table = at(&source, "Place of issue:").1;
    // (the run, what is typed after it, the new line, how far a line is)
    for (text, added, tail, pitch) in [
        (
            // One line, so the pitch is the nearest paragraph's: item 1's,
            // and not the editor's own 1.25 em, which is 11.25 here.
            // And no measure of its own: it breaks where the items beside
            // it end their lines, not at the paper's edge two words later.
            " Example Widget, model EX-1.",
            " It is described here for illustration only and it represents nothing at all, as the \
             sample maker gives no undertaking.",
            "nothing at all, as the sample maker gives no undertaking.",
            10.35,
        ),
        (
            OFFERED[12],
            " The same rulebook is described here for illustration only and represents nothing.",
            "here for illustration only and represents nothing.",
            10.35,
        ),
        (
            // Three lines, at the paragraph's own pitch.
            OFFERED[20],
            " the sample maker gives no undertaking for the demonstration widget described.",
            "the sample maker gives no undertaking for the demonstration widget described.",
            10.35,
        ),
    ] {
        let (_, line) = at(&source, text);
        let saved = typed(text, added).unwrap_or_else(|error| panic!("{text}: {error}"));
        let after = shown(&saved);
        // The new line starts under the item's words, where its producer
        // hangs a second line, and not under its number.
        let item = source
            .iter()
            .filter(|(_, x, y)| *y == line && *x > 80.)
            .map(|(_, x, _)| *x)
            .fold(f64::INFINITY, f64::min);
        assert!((item - 90.1).abs() < 0.001, "{text}: {item}");
        let (x, y) = at(&after, tail);
        assert!(
            (x - item).abs() < 0.001 && (y - (line - pitch)).abs() < 0.001,
            "{text}: the new line is at {x} {y}"
        );
        // Everything else, in the order it was drawn: where it was, or one
        // line lower when it was between the edited line and the table.
        let rest: Vec<_> = after
            .iter()
            .filter(|(shown, ..)| shown != tail && !shown.starts_with(text))
            .collect();
        let was: Vec<_> = source.iter().filter(|(shown, ..)| shown != text).collect();
        assert_eq!(rest.len(), was.len(), "{text}");
        let mut moved = Vec::new();
        for ((old, x0, y0), (new, x1, y1)) in was.iter().zip(&rest) {
            assert_eq!(old, new, "{text}");
            assert!(
                (x0 - x1).abs() < 0.001,
                "{text}: {old} moved along its line"
            );
            let below = *y0 < line - 1. && *y0 > table + 1.;
            let down = if below { pitch } else { 0. };
            assert!(
                (y0 - y1 - down).abs() < 0.001,
                "{text}: {old} went from {y0} to {y1}"
            );
            if below {
                moved.push(old.as_str());
            }
        }
        // What moved holds what the editor cannot rewrite: bullets, and both
        // lines of the justified paragraph, and a hyphen for the last item.
        let bullets = moved
            .iter()
            .filter(|text| text.chars().all(|ch| ch == textedit::fonts::OPAQUE))
            .count();
        assert!(bullets >= 2, "{text}: {moved:?}");
        assert!(moved.iter().any(|text| text.contains("Signed for")));
        assert!(moved.iter().any(|text| text.contains("words to be set")));
        if text == OFFERED[20] {
            // The hyphen is still at the end of its line, on that line.
            let hyphens: Vec<_> = rest.iter().filter(|(shown, ..)| shown == "-").collect();
            let (_, y) = at(&after, OFFERED[22]);
            assert_eq!(hyphens.len(), 3);
            assert!((hyphens[2].2 - y).abs() < 0.001 && (y - (line - 2. * pitch)).abs() < 0.001);
            assert_eq!(moved.iter().filter(|text| **text == "-").count(), 1);
        }
        // And the page is still the tagged page it was: scanned again, it
        // reads, and every run it offered is offered still.
        assert!(offered(&saved).len() > OFFERED.len(), "{text}");
    }
}

/// The same in a box the reader widened by `wider` and did not let grow: the
/// text stays on its line, whatever the paragraph's measure.
fn typed_wide(mut doc: Document, text: &str, added: &str, wider: f64) -> Result<Document, String> {
    let before = textedit::scan(&doc, 0).unwrap();
    let run = before.runs.iter().find(|run| run.text == text).unwrap();
    let opened = Layout::opened(run, EditFont::Original);
    textedit::write(
        &mut doc,
        &[Change {
            layout: Some(Layout {
                width: opened.width + wider,
                grow: false,
                ..opened
            }),
            page: 0,
            revision: before.revision.clone(),
            operator: run.operator,
            original: run.text.clone(),
            replacement: format!("{text}{added}"),
        }],
    )?;
    Ok(doc)
}

fn lines(doc: &Document) -> Vec<String> {
    shown(doc).into_iter().map(|(text, ..)| text).collect()
}

// A one-line item breaks where the page's paragraphs end their lines, and that
// is read from paragraphs of several lines only. Item 3 is first made longer
// on its line, into the right margin, in a box the reader widened. Item 2,
// wrapped after that, still breaks where it broke before: a line that long is
// no measure. And item 3 itself, made longer again until the page ends it, is
// past what the paragraphs show, so its first line keeps what it holds and
// only the new words go to the next line.
#[test]
fn textedit_a_one_line_item_wraps_at_the_measure_of_the_paragraphs_beside_it() {
    let long = format!("{} The same rulebook is described here", OFFERED[12]);
    let grown = typed_wide(
        export(),
        OFFERED[12],
        " The same rulebook is described here",
        200.,
    )
    .unwrap();
    assert!(lines(&grown).contains(&long));
    let second = typed_in(
        grown.clone(),
        " Example Widget, model EX-1.",
        " It is described here for illustration only and it represents nothing at all, as the \
         sample maker gives no undertaking.",
    )
    .unwrap();
    let second = lines(&second);
    assert!(second.contains(
        &" Example Widget, model EX-1. It is described here for illustration only and it represents "
            .to_string()
    ));
    assert!(
        second.contains(&"nothing at all, as the sample maker gives no undertaking.".to_string())
    );
    let third = typed_in(
        grown,
        &long,
        " for illustration only and represents nothing.",
    )
    .unwrap();
    let third = lines(&third);
    assert!(third.contains(&format!("{long} for illustration ")));
    assert!(third.contains(&"only and represents nothing.".to_string()));
}

// A few words that take a line past its paragraph's measure and not as far as
// the edge of the sheet wrap at the measure too: the line was set on, through
// the right margin, since only a line the page had filled wrapped. Words that
// stay within the measure are set on the line as before. And where the wrap
// cannot be made, here because the table below states its bounds and three
// lines of room above it are spent, the words stay on their line as they did:
// only a line that is full is refused for what a wrap cannot do.
#[test]
fn textedit_a_line_grown_past_its_measure_wraps_there_or_stays() {
    let within = typed(OFFERED[12], " The same").unwrap();
    assert!(lines(&within).contains(&format!("{} The same", OFFERED[12])));
    // Written as it is in a box the reader widened, which is never wrapped:
    // the run's own items kept, and no line laid out afresh.
    let saved = |doc: &Document| {
        let mut out = Vec::new();
        doc.clone().save_to(&mut out).unwrap();
        out
    };
    let boxed = typed_wide(export(), OFFERED[12], " The same", 60.).unwrap();
    assert!(saved(&within) == saved(&boxed));
    // A block of one line, at the measure it borrows.
    let added = " The same rulebook is described here";
    let past = lines(&typed(OFFERED[12], added).unwrap());
    assert!(past.contains(&format!("{} The same rulebook is described ", OFFERED[12])));
    assert!(past.contains(&"here".to_string()));
    // The last line of a paragraph of several, at the paragraph's own.
    let last = lines(
        &typed(
            OFFERED[7],
            " It is described here for illustration only and it represents nothing at all, as \
             said here.",
        )
        .unwrap(),
    );
    assert!(last.contains(&format!(
        "{} It is described here for illustration only and it represents nothing at all, as said ",
        OFFERED[7]
    )));
    assert!(last.contains(&"here.".to_string()));
    // A box the reader asked to wrap within is theirs, and is not wrapped at
    // the measure for them: the words stay on the line it grew along.
    let mut ticked = export();
    let before = textedit::scan(&ticked, 0).unwrap();
    let run = before
        .runs
        .iter()
        .find(|run| run.text == OFFERED[12])
        .unwrap();
    textedit::write(
        &mut ticked,
        &[Change {
            layout: Some(Layout {
                wrap: true,
                ..Layout::opened(run, EditFont::Original)
            }),
            page: 0,
            revision: before.revision.clone(),
            operator: run.operator,
            original: run.text.clone(),
            replacement: format!("{}{added}", run.text),
        }],
    )
    .unwrap();
    assert!(lines(&ticked).contains(&format!("{}{added}", OFFERED[12])));
    let long = " the sample maker gives no undertaking for the demonstration widget described.";
    let spent = typed(OFFERED[20], &long.repeat(3)).unwrap();
    let stays = lines(&typed_in(spent, OFFERED[12], added).unwrap());
    assert!(stays.contains(&format!("{}{added}", OFFERED[12])));
}

// What a wrap may not move still refuses it, each in its own words. The table
// states its bounds, and its rows do not leave them: the space above it and
// inside its bounds takes three more lines of the item and not a fourth. And a line that ends at a hyphen its producer
// added has that hyphen after it, which is read-only and moves with a line,
// never along one: such a line takes no more words.
#[test]
fn textedit_a_libreoffice_list_refuses_a_wrap_it_cannot_carry() {
    let long = " the sample maker gives no undertaking for the demonstration widget described.";
    assert!(typed(OFFERED[20], &long.repeat(3)).is_ok());
    let refused = typed(OFFERED[20], &long.repeat(4)).unwrap_err();
    assert_eq!(
        refused,
        "The table states its bounds, and this text would leave them. Reduce the box or font \
         size to keep the text inside."
    );
    for text in [OFFERED[5], OFFERED[6], OFFERED[22]] {
        let refused = typed(text, long).unwrap_err();
        assert!(
            refused.starts_with("There is no room for more text on this line: ")
                && (refused.contains("other text follows it")
                    || refused.contains("the text after it cannot be moved")),
            "{text}: {refused}"
        );
    }
}
