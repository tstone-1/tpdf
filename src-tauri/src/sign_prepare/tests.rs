use super::*;
use crate::integrity::Verdict;
use crate::sign_cms::testkeys::{self, certificate, signed, verdicts, Soft, Spec, NOW};
use lopdf::dictionary;

/// A generated fixture, or `None` with a `[SKIP]` line when it is absent.
///
/// `integrity.rs`'s rule: CI builds every one with `ci_fixtures.py --signed`
/// and fails at that step when it cannot, which is what makes skipping safe.
fn fixture(name: &str) -> Option<Vec<u8>> {
    match std::fs::read(std::path::Path::new("../testdata").join(name)) {
        Ok(bytes) => Some(bytes),
        Err(_) => {
            println!("[SKIP] {name}: not generated");
            None
        }
    }
}

/// How a synthetic document holds one of its arrays.
#[derive(Clone, Copy, Debug)]
enum Held {
    Absent,
    Inline,
    Object,
}

/// A one-page document whose `/Annots`, form and `/Fields` are held as asked.
///
/// `form` is `None` for a document with no `/AcroForm`; otherwise how the form
/// dictionary is held (inline or its own object) and how its `/Fields` is. An
/// existing text field and an existing annotation are there whenever their
/// array is, so appending can be told apart from replacing.
fn shaped(annots: Held, form: Option<(Held, Held)>, flags: Option<i64>) -> Vec<u8> {
    use lopdf::{Object, Stream};
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let content = doc.add_object(Stream::new(dictionary! {}, b"0 0 m 1 1 l S".to_vec()));
    let note = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text",
        "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
    });
    let text = doc.add_object(dictionary! {
        "FT" => "Tx",
        "T" => Object::string_literal("Signature1"),
    });
    let mut page = dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
        "Contents" => content,
    };
    match annots {
        Held::Absent => {}
        Held::Inline => page.set("Annots", vec![note.into()]),
        Held::Object => {
            let array = doc.add_object(Object::Array(vec![note.into()]));
            page.set("Annots", array);
        }
    }
    let page = doc.add_object(page);
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages };
    if let Some((held, fields)) = form {
        let mut acro = dictionary! {};
        if let Some(flags) = flags {
            acro.set("SigFlags", flags);
        }
        match fields {
            Held::Absent => {}
            Held::Inline => acro.set("Fields", vec![text.into()]),
            Held::Object => {
                let array = doc.add_object(Object::Array(vec![text.into()]));
                acro.set("Fields", array);
            }
        }
        match held {
            Held::Object => {
                let id = doc.add_object(acro);
                catalog.set("AcroForm", id);
            }
            _ => catalog.set("AcroForm", acro),
        }
    }
    let catalog = doc.add_object(catalog);
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// The document after the update, parsed strictly as a reader would.
fn reread(original: &[u8], unsigned: &Unsigned) -> Document {
    let whole = [original, unsigned.update.as_slice()].concat();
    Document::load_mem_with_options(
        &whole,
        lopdf::LoadOptions {
            strict: true,
            ..Default::default()
        },
    )
    .expect("the signed revision parses strictly")
}

/// The ids of the objects the update section rewrote or added.
fn written(original: &[u8], unsigned: &Unsigned) -> Vec<ObjectId> {
    let prior = Document::load_mem(original).expect("prior");
    let after = reread(original, unsigned);
    after
        .objects
        .keys()
        .copied()
        .filter(|id| {
            let text = format!("{} {} obj", id.0, id.1);
            unsigned
                .update
                .windows(text.len())
                .any(|w| w == text.as_bytes())
                || !prior.objects.contains_key(id)
        })
        .collect()
}

/// The widget the update added, read back from the reread document.
fn new_widget(after: &Document, unsigned: &Unsigned) -> (ObjectId, Dictionary) {
    after
        .objects
        .iter()
        .find_map(|(id, object)| {
            let dict = object.as_dict().ok()?;
            let named = dict.get(b"T").ok()?.as_str().ok()? == unsigned.field.as_bytes();
            (named && dict.get(b"FT").ok()?.as_name().ok()? == b"Sig").then(|| (*id, dict.clone()))
        })
        .expect("the new field")
}

fn fields_of(after: &Document) -> Vec<ObjectId> {
    let catalog = after.catalog().expect("catalog");
    let form = resolve(after, catalog.get(b"AcroForm").expect("a form"))
        .as_dict()
        .expect("form");
    resolve(after, form.get(b"Fields").expect("fields"))
        .as_array()
        .expect("array")
        .iter()
        .map(|o| o.as_reference().expect("a reference"))
        .collect()
}

fn annots_of(after: &Document) -> Vec<ObjectId> {
    let page = ordered_pages(after)[0];
    let dict = after.get_dictionary(page).expect("page");
    resolve(after, dict.get(b"Annots").expect("annots"))
        .as_array()
        .expect("array")
        .iter()
        .map(|o| o.as_reference().expect("a reference"))
        .collect()
}

fn sig_flags(after: &Document) -> i64 {
    let catalog = after.catalog().expect("catalog");
    let form = resolve(after, catalog.get(b"AcroForm").expect("a form"))
        .as_dict()
        .expect("form");
    form.get(b"SigFlags")
        .expect("flags")
        .as_i64()
        .expect("an integer")
}

// ---------------------------------------------------- the range and the hole

#[test]
fn the_range_frames_exactly_the_reserved_hole_and_the_digest_is_its_bytes() {
    let original = testkeys::plain_pdf();
    let unsigned = prepare(original.clone(), NOW, None).expect("prepared");
    let whole = [original.as_slice(), unsigned.update.as_slice()].concat();
    let [start, first, second, last] = unsigned.range.map(|n| n as usize);
    assert_eq!(start, 0);
    assert_eq!(second + last, whole.len(), "the range reaches the end");
    assert_eq!(
        second - first,
        RESERVED * 2 + 2,
        "the hole is the reservation"
    );
    assert_eq!(whole[first], b'<');
    assert_eq!(whole[second - 1], b'>');
    assert!(whole[first + 1..second - 1].iter().all(|b| *b == b'0'));
    assert_eq!(unsigned.built_against, original.len());

    // Recomputed here from the joined file, by a different route from the
    // one `covered_digest` takes over two buffers.
    let digest = Sha256::new()
        .chain_update(&whole[..first])
        .chain_update(&whole[second..])
        .finalize();
    assert_eq!(unsigned.digest, digest.to_vec());

    // The written range is the one reported, as the document spells it.
    let text = format!("/ByteRange[0 {first} {second} {last}");
    let at = unsigned
        .update
        .windows(text.len())
        .position(|w| w == text.as_bytes())
        .expect("the range as written");
    let end = at + RANGE_TEXT.len();
    assert_eq!(unsigned.update[end - 1], b']');
    assert!(unsigned.update[at + text.len()..end - 1]
        .iter()
        .all(|b| *b == b' '));
}

#[test]
fn the_revision_is_appended_and_the_document_before_it_untouched() {
    // Nothing `prepare` returns can rewrite the original: it hands back an
    // update, and the file is the original's bytes followed by it. What this
    // pins is that the update starts where an update may --- not with a second
    // header --- and ends as a revision does.
    let original = testkeys::plain_pdf();
    let unsigned = prepare(original.clone(), NOW, None).expect("prepared");
    assert!(!unsigned.update.starts_with(b"%PDF"), "no second header");
    assert!(unsigned.update.ends_with(b"%%EOF"));
    let after = reread(&original, &unsigned);
    assert_eq!(ordered_pages(&after).len(), 1);
}

#[test]
fn the_signature_dictionary_is_pades_and_dated_by_the_caller() {
    let original = testkeys::plain_pdf();
    let unsigned = prepare(original.clone(), NOW, None).expect("prepared");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    let sig = after
        .get_dictionary(widget.get(b"V").and_then(Object::as_reference).expect("/V"))
        .expect("the signature dictionary");
    let name = |key: &[u8]| {
        sig.get(key)
            .and_then(Object::as_name)
            .expect("a name")
            .to_vec()
    };
    assert_eq!(name(b"Type"), b"Sig");
    assert_eq!(name(b"Filter"), b"Adobe.PPKLite");
    assert_eq!(name(b"SubFilter"), b"ETSI.CAdES.detached");
    assert_eq!(
        sig.get(b"M").and_then(Object::as_str).expect("/M"),
        b"D:20260926000000Z"
    );
    assert_eq!(
        sig.get(b"Contents")
            .and_then(Object::as_str)
            .expect("/Contents")
            .len(),
        RESERVED
    );
}

#[test]
fn the_widget_is_an_invisible_locked_signature_field_on_the_first_page() {
    let original = testkeys::plain_pdf();
    let unsigned = prepare(original.clone(), NOW, None).expect("prepared");
    let after = reread(&original, &unsigned);
    let (id, widget) = new_widget(&after, &unsigned);
    assert_eq!(unsigned.field, "Signature1");
    assert_eq!(
        widget
            .get(b"Subtype")
            .and_then(Object::as_name)
            .expect("subtype"),
        b"Widget"
    );
    assert_eq!(widget.get(b"F").and_then(Object::as_i64).expect("/F"), 132);
    let rect: Vec<i64> = widget
        .get(b"Rect")
        .and_then(Object::as_array)
        .expect("rect")
        .iter()
        .map(|o| o.as_i64().expect("integer"))
        .collect();
    assert_eq!(rect, [0, 0, 0, 0]);
    assert_eq!(
        widget.get(b"P").and_then(Object::as_reference).expect("/P"),
        ordered_pages(&after)[0]
    );
    assert_eq!(annots_of(&after), [id]);
    assert_eq!(fields_of(&after), [id]);
    assert_eq!(sig_flags(&after), 3);
}

// ------------------------------------------ each shape the lists come in

#[test]
fn a_list_that_is_its_own_object_is_rewritten_and_its_owner_is_not() {
    let original = shaped(Held::Object, Some((Held::Object, Held::Object)), None);
    let prior = Document::load_mem(&original).expect("prior");
    let unsigned = prepare(original.clone(), NOW, None).expect("prepared");
    let after = reread(&original, &unsigned);
    let (id, _) = new_widget(&after, &unsigned);

    let page = ordered_pages(&prior)[0];
    let annots = prior
        .get_dictionary(page)
        .and_then(|p| p.get(b"Annots"))
        .and_then(Object::as_reference)
        .expect("annots object");
    let rewritten = written(&original, &unsigned);
    assert!(
        rewritten.contains(&annots),
        "the annotation array is rewritten"
    );
    assert!(!rewritten.contains(&page), "the page is not: {rewritten:?}");
    assert_eq!(annots_of(&after).len(), 2, "appended, not replaced");
    assert_eq!(*annots_of(&after).last().expect("one"), id);
    assert_eq!(fields_of(&after).len(), 2);
    assert_eq!(*fields_of(&after).last().expect("one"), id);
    // A field already called Signature1, so the new one is the next free name.
    assert_eq!(unsigned.field, "Signature2");
    let catalog = prior
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("root");
    assert!(
        !rewritten.contains(&catalog),
        "the catalog is not rewritten"
    );
}

#[test]
fn every_shape_of_the_two_lists_gains_the_widget() {
    for annots in [Held::Absent, Held::Inline, Held::Object] {
        for form in [
            None,
            Some((Held::Inline, Held::Absent)),
            Some((Held::Inline, Held::Inline)),
            Some((Held::Inline, Held::Object)),
            Some((Held::Object, Held::Absent)),
            Some((Held::Object, Held::Inline)),
            Some((Held::Object, Held::Object)),
        ] {
            let case = format!("annots {annots:?}, form {form:?}");
            let original = shaped(annots, form, None);
            let unsigned = prepare(original.clone(), NOW, None).expect(&case);
            let after = reread(&original, &unsigned);
            let (id, _) = new_widget(&after, &unsigned);
            let had_annot = !matches!(annots, Held::Absent);
            let had_field = matches!(form, Some((_, Held::Inline | Held::Object)));
            assert_eq!(
                annots_of(&after).len(),
                1 + usize::from(had_annot),
                "{case}"
            );
            assert_eq!(annots_of(&after).last(), Some(&id), "{case}");
            assert_eq!(
                fields_of(&after).len(),
                1 + usize::from(had_field),
                "{case}"
            );
            assert_eq!(fields_of(&after).last(), Some(&id), "{case}");
            assert_eq!(sig_flags(&after), 3, "{case}");
            let expected = if had_field {
                "Signature2"
            } else {
                "Signature1"
            };
            assert_eq!(unsigned.field, expected, "{case}");
        }
    }
}

#[test]
fn existing_sig_flags_are_kept_and_the_two_bits_added() {
    for (held, flags) in [(Held::Inline, 4), (Held::Object, 4), (Held::Inline, 1)] {
        let original = shaped(Held::Absent, Some((held, Held::Inline)), Some(flags));
        let unsigned = prepare(original.clone(), NOW, None).expect("prepared");
        assert_eq!(
            sig_flags(&reread(&original, &unsigned)),
            flags | 3,
            "{held:?} {flags}"
        );
    }
}

// ------------------------------------------------------------- refusals

#[test]
fn a_placeholder_the_document_already_spells_is_a_refusal_not_a_guess() {
    use lopdf::Object;
    let original = testkeys::plain_pdf();
    let mut doc = Document::load_mem(&original).expect("parsed");
    let page = ordered_pages(&doc)[0];
    // A literal string, which `lopdf` writes back verbatim: nothing in it
    // needs escaping, so the page's own bytes spell the placeholder.
    doc.get_dictionary_mut(page).expect("page").set(
        "Spelled",
        Object::String(RANGE_TEXT.to_vec(), lopdf::StringFormat::Literal),
    );
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    let why = prepare(bytes, NOW, None).expect_err("refused");
    assert!(why.contains("byte range exactly once"), "{why}");
}

#[test]
fn a_file_whose_offsets_outgrow_the_placeholder_is_refused() {
    let mut update = b"/ByteRange[0 9999999999 9999999999 9999999999]/Contents<".to_vec();
    update.extend(std::iter::repeat_n(b'0', RESERVED * 2));
    update.extend(b">>>");
    let mut copy = update.clone();
    assert!(
        fill_range(&mut copy, 1_000).is_ok(),
        "the control: a small file fits"
    );
    // Past ten digits a number still fits, as long as the three together fit
    // the thirty the placeholders took; sixteen-digit offsets do not.
    assert!(
        fill_range(&mut update.clone(), 99_999_999_999).is_ok(),
        "eleven digits fit"
    );
    let why = fill_range(&mut update, 1_000_000_000_000_000).expect_err("refused");
    assert!(why.contains("too large"), "{why}");
}

#[test]
fn an_encrypted_document_is_refused_whether_or_not_it_was_opened() {
    for (name, password) in [
        ("incr-encrypted-open.pdf", None),
        ("incr-encrypted-pw.pdf", Some("swordfish")),
    ] {
        let Some(bytes) = fixture(name) else { continue };
        let why = prepare(bytes, NOW, password).expect_err(name);
        assert!(why.contains("encrypted"), "{name}: {why}");
    }
}

#[test]
fn a_certification_that_permits_no_change_is_refused_and_the_others_are_not() {
    if let Some(bytes) = fixture("incr-certified-1.pdf") {
        let why = prepare(bytes, NOW, None).expect_err("P 1");
        assert!(why.contains("no changes permitted"), "{why}");
    }
    for name in ["incr-certified-2.pdf", "incr-certified-3.pdf"] {
        let Some(bytes) = fixture(name) else { continue };
        assert!(prepare(bytes, NOW, None).is_ok(), "{name}");
    }
}

// ---------------------------------- earlier signatures, through the whole path

#[test]
fn signing_a_signed_document_leaves_every_earlier_signature_intact() {
    let key = Soft::p256(41);
    let cert = certificate(&key, &Spec::new("Second signer"));
    let mut examined = 0;
    for (name, earlier, field) in [
        ("incr-signed.pdf", 1, "Signature2"),
        ("incr-two-signers.pdf", 2, "Signature3"),
        ("incr-certified-2.pdf", 1, "Signature2"),
        ("incr-xrefstream.pdf", 0, "Signature1"),
    ] {
        let Some(original) = fixture(name) else {
            continue;
        };
        let bytes = signed(&original, &cert, &[], &key).expect(name);
        assert_eq!(&bytes[..original.len()], &original[..], "{name}: appended");
        let found = verdicts(&bytes);
        assert_eq!(found.len(), earlier + 1, "{name}: {found:?}");
        for (at, integrity) in &found {
            assert_eq!(
                integrity.verdict,
                Verdict::Intact,
                "{name}: {at}: {integrity:?}"
            );
        }
        assert!(found.iter().any(|(at, _)| at == field), "{name}: {field}");
        examined += 1;
    }
    println!("examined {examined} fixtures");
}
