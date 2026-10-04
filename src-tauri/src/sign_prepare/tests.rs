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

// ------------------------------------------------ the invisible path, pinned

/// SHA-256 of the update [`prepare`] builds for each synthetic document, as it
/// was built before visible signatures existed (`26b911e`, 2026-09-27).
///
/// Measured by running the unchanged builder over these inputs, not derived:
/// a digest written from the new code would pin the new code to itself.
/// Synthetic inputs only, because `lopdf` writes them the same way every run;
/// a generated fixture is regenerated by a script with a clock in it.
const INVISIBLE: [(&str, usize, &str); 4] = [
    (
        "plain",
        66141,
        "0147e8a5e2d6cdc922b6b631d6d18e2e8cf200449881b4ff7aca50c2352201ec",
    ),
    (
        "annots object, form object, fields object",
        66085,
        "ecca0102e3e28b5a580d5a094c4b7baefeb80b73ec25fbcacc9feaadd69e1075",
    ),
    (
        "annots inline, form inline, fields inline",
        66155,
        "0372886f3f56f49fa47f1a9c64be9ea644af8935fe96f9cb7f78adc61c937e41",
    ),
    (
        "no annots, no form",
        66143,
        "07eecccda400b8b02cb865b1836c8ed5e5292f6612529d86a0aff6f1e16c3cae",
    ),
];

#[test]
fn the_invisible_revision_is_byte_for_byte_what_it_was() {
    let inputs = [
        testkeys::plain_pdf(),
        shaped(Held::Object, Some((Held::Object, Held::Object)), None),
        shaped(Held::Inline, Some((Held::Inline, Held::Inline)), None),
        shaped(Held::Absent, None, None),
    ];
    for ((what, len, digest), original) in INVISIBLE.iter().zip(inputs) {
        let unsigned = prepare(original, NOW, None).expect(what);
        let found: String = Sha256::digest(&unsigned.update)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            (unsigned.update.len(), found.as_str()),
            (*len, *digest),
            "{what}"
        );
    }
}

// ------------------------------------------------------ a visible signature

/// Two pages: a plain 200 x 200 first page, and a second whose crop box is
/// inset from its media box --- `[10 20 310 420]` inside `[0 0 320 440]` ---
/// turned `rotate` degrees. The inset is what makes a missing origin visible:
/// on a page whose crop box starts at zero, forgetting it changes nothing.
fn two_pages(rotate: i64) -> Vec<u8> {
    use lopdf::{Object, Stream};
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let content = doc.add_object(Stream::new(dictionary! {}, b"0 0 m 1 1 l S".to_vec()));
    let first = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        "Contents" => content,
    });
    let second = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 320.into(), 440.into()],
        "CropBox" => vec![10.into(), 20.into(), 310.into(), 420.into()],
        "Rotate" => rotate,
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![first.into(), second.into()],
            "Count" => 2,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// A 4 x 2 raster: opaque red on the left half, transparent on the right.
fn raster() -> crate::signature::Image {
    let mut rgba = Vec::new();
    for _ in 0..2 {
        for x in 0..4 {
            rgba.extend_from_slice(if x < 2 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 0, 0]
            });
        }
    }
    crate::signature::Image {
        width: 4,
        height: 2,
        rgba,
    }
}

fn visible(page: u32, rect: [f32; 4], image: Option<crate::signature::Image>) -> Visible {
    Visible {
        page,
        rect,
        name: "A. Signer".into(),
        image,
        options: Options::default(),
    }
}

/// The form's own box for a widget whose `/Rect` is `rect`: the same size, at
/// the origin, which is where the appearance draws.
fn local(rect: &[f64]) -> Vec<f64> {
    vec![0.0, 0.0, rect[2] - rect[0], rect[3] - rect[1]]
}

/// The page-space numbers of an array of numbers.
fn numbers_of(object: &Object) -> Vec<f64> {
    object
        .as_array()
        .expect("an array")
        .iter()
        .map(|o| o.as_float().map(f64::from).expect("a number"))
        .collect()
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

/// The annotation references on page `at`, or none.
fn annots_on(after: &Document, at: usize) -> Vec<ObjectId> {
    let page = ordered_pages(after)[at];
    let dict = after.get_dictionary(page).expect("page");
    dict.get(b"Annots")
        .ok()
        .map(|annots| {
            resolve(after, annots)
                .as_array()
                .expect("array")
                .iter()
                .map(|o| o.as_reference().expect("a reference"))
                .collect()
        })
        .unwrap_or_default()
}

/// The widget's normal appearance: its dictionary and its decoded operations.
fn appearance_of(
    after: &Document,
    widget: &Dictionary,
) -> (Dictionary, Vec<lopdf::content::Operation>) {
    let ap = widget.get(b"AP").and_then(Object::as_dict).expect("an /AP");
    let form = after
        .get_object(ap.get(b"N").and_then(Object::as_reference).expect("/N"))
        .and_then(Object::as_stream)
        .expect("a stream");
    let content = lopdf::content::Content::decode(&form.content).expect("content");
    (form.dict.clone(), content.operations)
}

#[test]
fn a_visible_widget_has_the_rectangle_the_reader_placed_on_the_page_they_chose() {
    let original = two_pages(0);
    let placed = visible(1, [20.0, 30.0, 170.0, 90.0], None);
    let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("prepared");
    let after = reread(&original, &unsigned);
    let (id, widget) = new_widget(&after, &unsigned);
    let pages = ordered_pages(&after);
    assert_eq!(
        widget.get(b"P").and_then(Object::as_reference).expect("/P"),
        pages[1],
        "the page the reader chose"
    );
    assert_eq!(annots_on(&after, 1), [id]);
    assert!(
        annots_on(&after, 0).is_empty(),
        "the first page is untouched"
    );
    // Display [20 30 170 90] on a 300 x 400 displayed page whose crop box sits
    // at (10, 20): left and right move by 10, and top and bottom flip about the
    // displayed height before moving by 20.
    let rect = numbers_of(widget.get(b"Rect").expect("/Rect"));
    assert!(close(&rect, &[30.0, 330.0, 180.0, 390.0]), "{rect:?}");
    assert_eq!(widget.get(b"F").and_then(Object::as_i64).expect("/F"), 132);
    let (form, _) = appearance_of(&after, &widget);
    assert_eq!(
        form.get(b"Subtype")
            .and_then(Object::as_name)
            .expect("subtype"),
        b"Form"
    );
    // The form is drawn at the origin and placed by `/Rect` alone.
    assert!(close(
        &numbers_of(form.get(b"BBox").expect("/BBox")),
        &local(&rect)
    ));
    assert_eq!(fields_of(&after), [id]);
    assert_eq!(sig_flags(&after), 3);
}

/// What the appearance draws: the clip, every text origin, every string and
/// every image placement, in page space.
struct Drawn {
    clip: Vec<f64>,
    origins: Vec<(f64, f64)>,
    matrices: Vec<[f64; 4]>,
    strings: Vec<String>,
    images: Vec<[f64; 6]>,
    size: f64,
}

fn drawn(operations: &[lopdf::content::Operation]) -> Drawn {
    let number = |o: &Object| o.as_float().map(f64::from).expect("a number");
    let mut out = Drawn {
        clip: Vec::new(),
        origins: Vec::new(),
        matrices: Vec::new(),
        strings: Vec::new(),
        images: Vec::new(),
        size: 0.0,
    };
    for (at, op) in operations.iter().enumerate() {
        let n: Vec<f64> = op
            .operands
            .iter()
            .filter(|o| o.as_float().is_ok())
            .map(number)
            .collect();
        match op.operator.as_str() {
            "re" if out.clip.is_empty() => {
                assert_eq!(operations[at + 1].operator, "W", "the first path clips");
                out.clip = n;
            }
            "Tm" => {
                out.matrices.push([n[0], n[1], n[2], n[3]]);
                out.origins.push((n[4], n[5]));
            }
            "Tf" => out.size = n[0],
            "Tj" => out.strings.push(
                op.operands[0]
                    .as_str()
                    .expect("a string")
                    .iter()
                    .map(|b| char::from(*b))
                    .collect(),
            ),
            "cm" => out.images.push([n[0], n[1], n[2], n[3], n[4], n[5]]),
            _ => {}
        }
    }
    out
}

fn inside(rect: &[f64], (x, y): (f64, f64)) -> bool {
    let e = 1e-6;
    x >= rect[0] - e && x <= rect[2] + e && y >= rect[1] - e && y <= rect[3] + e
}

#[test]
fn the_appearance_names_the_signer_and_the_date_inside_its_rectangle() {
    let original = two_pages(0);
    let placed = visible(1, [20.0, 30.0, 170.0, 90.0], None);
    let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("prepared");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    let rect = local(&numbers_of(widget.get(b"Rect").expect("/Rect")));
    let (_, operations) = appearance_of(&after, &widget);
    let seen = drawn(&operations);
    assert_eq!(
        seen.clip,
        [rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]],
        "the rectangle is the clip"
    );
    // The time is the signature dictionary's /M, D:20260926000000Z.
    assert_eq!(
        seen.strings,
        [
            "Digitally signed by",
            "A. Signer",
            "Date: 2026-09-26 00:00:00 UTC"
        ]
    );
    assert!(seen.size > 0.0 && seen.size <= appearance::MAX_SIZE);
    for origin in &seen.origins {
        assert!(inside(&rect, *origin), "{origin:?} outside {rect:?}");
    }
    assert!(seen.images.is_empty(), "no saved image, so none drawn");
}

#[test]
fn a_saved_image_is_drawn_beside_the_words_and_text_alone_without_one() {
    let original = two_pages(0);
    let rect_display = [20.0, 30.0, 220.0, 90.0];
    let with = visible(1, rect_display, Some(raster()));
    let unsigned = prepare_visible(original.clone(), NOW, None, &with).expect("prepared");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    let rect = local(&numbers_of(widget.get(b"Rect").expect("/Rect")));
    let (form, operations) = appearance_of(&after, &widget);
    let resources = form
        .get(b"Resources")
        .and_then(Object::as_dict)
        .expect("resources");
    let image = after
        .get_object(
            resources
                .get(b"XObject")
                .and_then(Object::as_dict)
                .and_then(|x| x.get(b"Signature"))
                .and_then(Object::as_reference)
                .expect("the image"),
        )
        .and_then(Object::as_stream)
        .expect("a stream");
    assert_eq!(
        image.dict.get(b"Width").and_then(Object::as_i64).ok(),
        Some(4)
    );
    assert_eq!(
        image.dict.get(b"Height").and_then(Object::as_i64).ok(),
        Some(2)
    );
    assert!(image.dict.get(b"SMask").is_ok(), "transparency is kept");
    let seen = drawn(&operations);
    assert_eq!(seen.images.len(), 1);
    let [a, b, c, d, e, f] = seen.images[0];
    // The unit square's four corners, mapped: all inside the rectangle, and on
    // the left half of it, since the box is wider than tall.
    let middle = (rect[0] + rect[2]) / 2.0;
    for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let corner = (a * u + c * v + e, b * u + d * v + f);
        assert!(inside(&rect, corner), "{corner:?} outside {rect:?}");
        assert!(corner.0 <= middle + 1e-6, "{corner:?} is not on the left");
    }
    // Its proportions are the raster's: 4 wide, 2 high.
    assert!((a / d - 2.0).abs() < 1e-6, "{:?}", seen.images[0]);
    for origin in &seen.origins {
        assert!(
            origin.0 >= middle - 1e-6,
            "the words are on the right: {origin:?}"
        );
    }

    // Without an image the words take the whole box, starting at its left.
    let without = visible(1, rect_display, None);
    let unsigned = prepare_visible(original.clone(), NOW, None, &without).expect("prepared");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    let (form, operations) = appearance_of(&after, &widget);
    let seen = drawn(&operations);
    assert!(seen.images.is_empty());
    assert!(form
        .get(b"Resources")
        .and_then(Object::as_dict)
        .expect("resources")
        .get(b"XObject")
        .is_err());
    assert!(
        seen.origins.iter().all(|o| o.0 < middle),
        "{:?}",
        seen.origins
    );
}

#[test]
fn a_turned_page_gets_an_appearance_that_reads_upright() {
    for rotate in [0, 90, 180, 270] {
        let original = two_pages(rotate);
        // Displayed, the page is 300 x 400 upright and 400 x 300 on a quarter
        // turn; this rectangle fits both.
        let placed = visible(1, [20.0, 30.0, 170.0, 90.0], Some(raster()));
        let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("prepared");
        let after = reread(&original, &unsigned);
        let (_, widget) = new_widget(&after, &unsigned);
        let rect = numbers_of(widget.get(b"Rect").expect("/Rect"));
        let turns = (rotate / 90) as u8;
        // The reader dragged 150 wide and 60 high; on a quarter turn the
        // page's own axes swap those.
        let (w, h) = (rect[2] - rect[0], rect[3] - rect[1]);
        let expected = if turns % 2 == 0 {
            (150.0, 60.0)
        } else {
            (60.0, 150.0)
        };
        assert!(
            (w - expected.0).abs() < 1e-3 && (h - expected.1).abs() < 1e-3,
            "{rotate}: {rect:?}"
        );
        assert!(
            rect[0] >= 10.0 && rect[1] >= 20.0 && rect[2] <= 310.0 && rect[3] <= 420.0,
            "{rotate}: {rect:?} is not inside the crop box"
        );
        let rect = local(&rect);
        let (_, operations) = appearance_of(&after, &widget);
        let seen = drawn(&operations);
        // The baseline runs the reader's way: along x upright, up the page on
        // a quarter turn clockwise, and so on.
        let along = match turns {
            0 => [1.0, 0.0, 0.0, 1.0],
            1 => [0.0, 1.0, -1.0, 0.0],
            2 => [-1.0, 0.0, 0.0, -1.0],
            _ => [0.0, -1.0, 1.0, 0.0],
        };
        assert_eq!(seen.matrices.len(), 3, "{rotate}");
        for matrix in &seen.matrices {
            assert!(close(matrix, &along), "{rotate}: {matrix:?}");
        }
        for origin in &seen.origins {
            assert!(
                inside(&rect, *origin),
                "{rotate}: {origin:?} outside {rect:?}"
            );
        }
        let [a, b, c, d, e, f] = seen.images[0];
        for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let corner = (a * u + c * v + e, b * u + d * v + f);
            assert!(
                inside(&rect, corner),
                "{rotate}: {corner:?} outside {rect:?}"
            );
        }
        // The image's own x axis runs the reader's way too, so it is upright.
        let length = a.hypot(b);
        assert!(
            close(&[a / length, b / length], &along[..2]),
            "{rotate}: {:?}",
            seen.images[0]
        );
    }
}

/// Every combination of the five lines: bit 0 the label, 1 the name, 2 the
/// date, 3 a reason, 4 a location.
fn options_for(mask: u32, reason: &str, location: &str) -> Options {
    Options {
        label: mask & 1 != 0,
        name: mask & 2 != 0,
        date: mask & 4 != 0,
        reason: if mask & 8 != 0 {
            reason.into()
        } else {
            String::new()
        },
        location: if mask & 16 != 0 {
            location.into()
        } else {
            String::new()
        },
        ..Options::default()
    }
}

#[test]
fn every_layout_keeps_its_ink_inside_the_box_and_apart_from_the_image() {
    let long = "W".repeat(appearance::MAX_NAME_CHARS);
    let note = "M".repeat(appearance::MAX_NOTE_CHARS - "Reason: ".len());
    let inset = crate::textbox::INSET;
    let mut checked = 0;
    // For each number of lines, whether some box was bound by its height and
    // some by its width: a grid in which one of the two never happens for a
    // count cannot tell a layout that ignores it from a correct one.
    let mut bound = [(false, false, false); 6];
    for (name, reason, location) in [
        ("A. Signer", "Approved", "Hamburg"),
        (
            "Ålfhild Ærøskøbing-Überschär",
            "Geprüft und freigegeben, Änderungsstand C",
            "Köln-Mülheim",
        ),
        (long.as_str(), note.as_str(), "x"),
    ] {
        for mask in 0..32 {
            let options = options_for(mask, reason, location);
            let lines = appearance::words(name, "D:20260926000000Z", &options);
            assert_eq!(lines.len(), mask.count_ones() as usize, "{mask:05b}");
            for (width, height) in [
                (24.0, 24.0),
                (40.0, 24.0),
                (150.0, 60.0),
                (60.0, 150.0),
                (600.0, 200.0),
                (24.0, 400.0),
                // Wide and short, so the height rather than the widest line sets
                // the size: every box above is width-bound, and a layout that
                // ignored the height passed all of them.
                (600.0, 24.0),
                (400.0, 30.0),
            ] {
                for image in [None, Some((4, 2)), Some((2, 400)), Some((400, 2))] {
                    let layout = appearance::layout(width, height, image, &lines);
                    let e = 1e-9;
                    if lines.is_empty() {
                        assert!(layout.lines.is_empty() && layout.size == 0.0);
                    } else {
                        assert!(layout.size > 0.0 && layout.size <= appearance::MAX_SIZE);
                        assert_eq!(layout.lines.len(), lines.len());
                        // Which limit set the size, from the text's own share
                        // of the box as the module note states it.
                        let [tw, th] = match image {
                            None => [width, height],
                            Some(_) if width >= height => [width / 2.0, height],
                            Some(_) => [width, height / 2.0],
                        };
                        let widest = lines
                            .iter()
                            .map(|l| crate::textbox::advance(l, 1.0))
                            .fold(0.0_f64, f64::max);
                        let count = lines.len();
                        let by_height = (th - inset * 2.0) / (count as f64 * appearance::LEADING);
                        let by_width = (tw - inset * 2.0) / (widest + appearance::BEARING * 2.0);
                        bound[count].0 |= (layout.size - by_height).abs() < e;
                        bound[count].1 |= (layout.size - by_width).abs() < e;
                        bound[count].2 |= (layout.size - appearance::MAX_SIZE).abs() < e;
                    }
                    for (u, v, text) in &layout.lines {
                        let [u0, v0, u1, v1] = appearance::line_extent(layout.size, *u, *v, text);
                        assert!(
                            u0 >= -e && v0 >= -e && u1 <= width + e && v1 <= height + e,
                            "{name} {mask:05b} {width}x{height} {image:?}: {text} at {:?}",
                            [u0, v0, u1, v1]
                        );
                        if let Some([iu, iv, iw, ih]) = layout.image {
                            let apart = u1 <= iu + e
                                || u0 >= iu + iw - e
                                || v1 <= iv + e
                                || v0 >= iv + ih - e;
                            assert!(
                                apart,
                                "{name} {mask:05b} {width}x{height} {image:?}: {text} over the image"
                            );
                        }
                    }
                    // Lines are drawn top to bottom in the order given.
                    for pair in layout.lines.windows(2) {
                        assert!(pair[0].1 < pair[1].1, "{mask:05b}: out of order");
                    }
                    if let Some([iu, iv, iw, ih]) = layout.image {
                        assert!(
                            iu >= -e && iv >= -e && iu + iw <= width + e && iv + ih <= height + e
                        );
                        let (pw, ph) = image
                            .map(|(w, h)| (f64::from(w), f64::from(h)))
                            .expect("an image");
                        assert!((iw / ih - pw / ph).abs() < 1e-6, "proportions kept");
                        // With no words the image has the whole box: it meets
                        // two opposite insets.
                        if lines.is_empty() {
                            let fills = (iw - (width - inset * 2.0)).abs() < 1e-6
                                || (ih - (height - inset * 2.0)).abs() < 1e-6;
                            assert!(fills, "{width}x{height} {image:?}: {:?}", layout.image);
                        }
                    } else {
                        assert!(image.is_none(), "an image that fits was not placed");
                    }
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 3 * 32 * 8 * 4);
    // One line is never bound by its height here, and in practice cannot be:
    // one line at the cap needs 12 points plus the insets, the shortest box is
    // 24, and the only share of a box shorter than 16 is the lower half of a
    // tall box under 32 points wide, too narrow for any line to reach the cap.
    // So for one line the limits that bind are the width and the cap; from two
    // lines on, the height and the width both do.
    for (count, (height, width, cap)) in bound.iter().enumerate().skip(1) {
        let seen = if count == 1 {
            !*height && *width && *cap
        } else {
            *height && *width
        };
        assert!(
            seen,
            "{count} line(s): bound by height {height}, by width {width}, by the cap {cap}"
        );
    }
}

/// The strings an appearance draws, as text.
fn strings_of(after: &Document, widget: &Dictionary) -> Vec<String> {
    let (_, operations) = appearance_of(after, widget);
    drawn(&operations).strings
}

#[test]
fn each_line_is_drawn_only_when_it_is_on() {
    let original = two_pages(0);
    let every = [
        "Digitally signed by",
        "A. Signer",
        "Date: 2026-09-26 00:00:00 UTC",
        "Reason: Approved",
        "Location: Hamburg",
    ];
    let mut drawn_somewhere = 0;
    for mask in 1..32 {
        let mut placed = visible(1, [20.0, 30.0, 220.0, 130.0], None);
        placed.options = options_for(mask, "Approved", "Hamburg");
        let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("prepared");
        let after = reread(&original, &unsigned);
        let (_, widget) = new_widget(&after, &unsigned);
        let expected: Vec<&str> = every
            .iter()
            .enumerate()
            .filter(|(at, _)| mask & (1 << at) != 0)
            .map(|(_, line)| *line)
            .collect();
        assert_eq!(strings_of(&after, &widget), expected, "{mask:05b}");
        // And absent from the stream's bytes altogether, not merely from what
        // the decoder above recognises as a line.
        let ap = widget.get(b"AP").and_then(Object::as_dict).expect("/AP");
        let form = after
            .get_object(ap.get(b"N").and_then(Object::as_reference).expect("/N"))
            .and_then(Object::as_stream)
            .expect("stream");
        let content = String::from_utf8_lossy(&form.content).to_string();
        for (at, line) in every.iter().enumerate() {
            let hex = crate::save::winansi_hex(line);
            assert_eq!(
                content.contains(&hex),
                mask & (1 << at) != 0,
                "{mask:05b}: {line}"
            );
        }
        drawn_somewhere += 1;
    }
    assert_eq!(drawn_somewhere, 31);
}

#[test]
fn a_visible_signature_that_would_draw_nothing_is_refused() {
    let original = two_pages(0);
    let mut placed = visible(1, [20.0, 30.0, 170.0, 90.0], None);
    placed.options = options_for(0, "", "");
    let refused = prepare_visible(original.clone(), NOW, None, &placed).expect_err("nothing");
    assert!(refused.contains("has to show something"), "{refused}");
    // Blank text is nothing too.
    placed.options.reason = "   ".into();
    placed.options.location = "\t".into();
    let refused = prepare_visible(original.clone(), NOW, None, &placed).expect_err("blank");
    assert!(refused.contains("has to show something"), "{refused}");

    // The controls: the image alone is something, and it is drawn over the
    // whole box with no font and no words.
    placed.image = Some(raster());
    let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("image alone");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    let rect = local(&numbers_of(widget.get(b"Rect").expect("/Rect")));
    let (form, operations) = appearance_of(&after, &widget);
    let seen = drawn(&operations);
    assert!(seen.strings.is_empty());
    let resources = form
        .get(b"Resources")
        .and_then(Object::as_dict)
        .expect("resources");
    assert!(resources.get(b"Font").is_err(), "no font for no words");
    let [a, _, _, d, e, _] = seen.images[0];
    // A 4 x 2 raster in a 150 x 60 box: its height fills the box less the
    // insets, and it is centred, so it passes the middle.
    assert!(
        (d - (rect[3] - 2.0 * crate::textbox::INSET)).abs() < 1e-6,
        "{:?}",
        seen.images[0]
    );
    assert!(
        e < rect[2] / 2.0 && e + a > rect[2] / 2.0,
        "{:?}",
        seen.images[0]
    );
    // And one line alone is something.
    placed.image = None;
    placed.options = options_for(4, "", "");
    assert!(prepare_visible(original, NOW, None, &placed).is_ok());
}

#[test]
fn a_reason_or_location_that_cannot_be_drawn_is_refused() {
    let original = two_pages(0);
    for (reason, location, why) in [
        ("审核通过", "", "the reason"),
        ("", "Москва", "the location"),
        ("Appr\noved", "", "the reason"),
        ("", "Ham\u{7}burg", "the location"),
        ("€ 12", "", "the reason"),
        (
            &*"a".repeat(appearance::MAX_NOTE_CHARS + 1),
            "",
            "longer than",
        ),
        (
            "",
            &*"b".repeat(appearance::MAX_NOTE_CHARS + 1),
            "longer than",
        ),
    ] {
        let mut placed = visible(1, [20.0, 30.0, 170.0, 90.0], None);
        placed.options.reason = reason.into();
        placed.options.location = location.into();
        let refused = prepare_visible(original.clone(), NOW, None, &placed).expect_err(why);
        assert!(refused.contains(why), "{reason:?} {location:?}: {refused}");
    }
    // The control: Latin-1 beyond ASCII is drawn, and written.
    let mut placed = visible(1, [20.0, 30.0, 220.0, 130.0], None);
    placed.options.reason = "Geprüft".into();
    placed.options.location = "Köln".into();
    let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("Latin-1");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    let strings = strings_of(&after, &widget);
    assert_eq!(&strings[3..], ["Reason: Geprüft", "Location: Köln"]);
}

// `sign --hide`: the dictionary carries the text and the appearance does not.
#[test]
fn a_hidden_reason_or_location_is_written_and_not_drawn() {
    let original = two_pages(0);
    let latin = |text: &str| text.chars().map(|ch| ch as u8).collect::<Vec<u8>>();
    for (hide_reason, hide_location, drawn) in [
        // Control: what every line without `--hide` draws.
        (false, false, vec!["Reason: Approved", "Location: Hamburg"]),
        (true, false, vec!["Location: Hamburg"]),
        (false, true, vec!["Reason: Approved"]),
        (true, true, vec![]),
    ] {
        let mut placed = visible(1, [20.0, 30.0, 220.0, 130.0], None);
        placed.options.reason = "Approved".into();
        placed.options.location = "Hamburg".into();
        placed.options.hide_reason = hide_reason;
        placed.options.hide_location = hide_location;
        let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("prepared");
        let after = reread(&original, &unsigned);
        let (_, widget) = new_widget(&after, &unsigned);
        assert_eq!(
            &strings_of(&after, &widget)[3..],
            drawn.as_slice(),
            "{hide_reason} {hide_location}"
        );
        let signature = signature_of(&after, &widget);
        assert_eq!(bytes_of(&signature, b"Reason"), Some(latin("Approved")));
        assert_eq!(bytes_of(&signature, b"Location"), Some(latin("Hamburg")));
    }
    // Hidden text is not something shown: with no line and no image there is
    // nothing to draw, and with an image the image is the whole appearance.
    let mut placed = visible(1, [20.0, 30.0, 220.0, 130.0], None);
    placed.options = Options {
        label: false,
        name: false,
        date: false,
        reason: "Approved".into(),
        hide_reason: true,
        ..Options::default()
    };
    assert!(prepare_visible(original.clone(), NOW, None, &placed)
        .expect_err("nothing drawn")
        .contains("has to show something"));
    placed.options.hide_reason = false;
    assert!(prepare_visible(original.clone(), NOW, None, &placed).is_ok());
    placed.options.hide_reason = true;
    placed.image = Some(raster());
    let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("image alone");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    assert!(strings_of(&after, &widget).is_empty());
    assert_eq!(
        bytes_of(&signature_of(&after, &widget), b"Reason"),
        Some(latin("Approved"))
    );
    // A hidden text is still held to what the dictionary may carry.
    placed.options.reason = "Appr\noved".into();
    assert!(prepare_visible(original, NOW, None, &placed).is_err());
}

// `sign --text`: the wording is the caller's, and the signature's own values
// are filled in where it asks for them.
#[test]
fn a_text_is_drawn_in_place_of_the_standard_lines() {
    let original = two_pages(0);
    let latin = |text: &str| text.chars().map(|ch| ch as u8).collect::<Vec<u8>>();
    let mut placed = visible(1, [20.0, 30.0, 220.0, 130.0], None);
    placed.options = Options {
        // The three switches are not read with a text: all off, and the
        // lines are still drawn.
        label: false,
        name: false,
        date: false,
        reason: "Approved".into(),
        location: "Hamburg".into(),
        text: vec![
            "Digitally signed".into(),
            "{date}".into(),
            "by {name} in {location} ({reason}) {{x}}".into(),
            "Geprüft".into(),
        ],
        date_format: "DD.MM.YYYY".into(),
        ..Options::default()
    };
    let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("prepared");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    assert_eq!(
        strings_of(&after, &widget),
        [
            "Digitally signed",
            "26.09.2026",
            "by A. Signer in Hamburg (Approved) {x}",
            "Geprüft"
        ]
    );
    // A reason the text does not ask for is written and not drawn; no
    // standard "Reason:" line appears beside the text.
    placed.options.text = vec!["Signed {date}".into()];
    placed.options.date_format = String::new();
    let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("prepared");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    assert_eq!(
        strings_of(&after, &widget),
        ["Signed 2026-09-26 00:00:00 UTC"]
    );
    let signature = signature_of(&after, &widget);
    assert_eq!(bytes_of(&signature, b"Reason"), Some(latin("Approved")));
    assert_eq!(bytes_of(&signature, b"Location"), Some(latin("Hamburg")));

    // Without a text, the format writes the standard date line.
    let mut dated = visible(1, [20.0, 30.0, 220.0, 130.0], None);
    dated.options.date_format = "YYYY/MM/DD".into();
    let unsigned = prepare_visible(original.clone(), NOW, None, &dated).expect("prepared");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    assert_eq!(strings_of(&after, &widget)[2], "Date: 2026/09/26");

    // A name tpdf cannot draw is refused only when the text asks for it.
    let mut named = visible(1, [20.0, 30.0, 220.0, 130.0], None);
    named.name = "张伟".into();
    named.options.text = vec!["Signed by {name}".into()];
    assert!(prepare_visible(original.clone(), NOW, None, &named)
        .expect_err("the name is drawn")
        .contains("the certificate's name"));
    named.options.text = vec!["Signed".into()];
    assert!(prepare_visible(original.clone(), NOW, None, &named).is_ok());

    for (text, format, why) in [
        (vec!["by {who}".to_string()], "", "tpdf fills in"),
        (vec!["a { b".to_string()], "", "never closed"),
        (vec!["a } b".to_string()], "", "closes nothing"),
        (vec!["审核通过".to_string()], "", "the text"),
        (vec!["Appr\u{7}oved".to_string()], "", "the text"),
        (vec![" ".to_string(), String::new()], "", "no words"),
        (
            vec!["x".to_string(); appearance::MAX_TEXT_LINES + 1],
            "",
            "more than",
        ),
        (
            vec!["a".repeat(appearance::MAX_NOTE_CHARS + 1)],
            "",
            "longer than",
        ),
        (vec!["{date}".to_string()], "YYYY\u{7}", "the date format"),
        (vec!["{date}".to_string()], "年YYYY", "the date format"),
        (
            vec!["{date}".to_string()],
            &*"Y".repeat(appearance::MAX_FORMAT_CHARS + 1),
            "longer than",
        ),
    ] {
        let mut placed = visible(1, [20.0, 30.0, 220.0, 130.0], None);
        placed.options.text = text.clone();
        placed.options.date_format = format.into();
        let refused = prepare_visible(original.clone(), NOW, None, &placed).expect_err(why);
        assert!(refused.contains(why), "{text:?} {format:?}: {refused}");
    }
    // The bounds themselves are allowed.
    let mut placed = visible(1, [20.0, 30.0, 220.0, 130.0], None);
    placed.options.text = vec!["x".to_string(); appearance::MAX_TEXT_LINES];
    placed.options.date_format = "Y".repeat(appearance::MAX_FORMAT_CHARS);
    assert!(prepare_visible(original, NOW, None, &placed).is_ok());
}

#[test]
fn a_date_is_written_token_by_token() {
    let at = "D:20260926134507Z";
    for (format, written) in [
        ("", "2026-09-26 13:45:07 UTC"),
        (" ", "2026-09-26 13:45:07 UTC"),
        ("DD/MM/YYYY ss:mm:HH", "26/09/2026 07:45:13"),
        ("YYYYMMDD", "20260926"),
        ("HH h mm", "13 h 45"),
        // One letter is not a token, and neither is the wrong case.
        ("Y M D H m s yyyy", "Y M D H m s yyyy"),
        ("Día DD", "Día 26"),
    ] {
        assert_eq!(appearance::date_text(at, format), written, "{format:?}");
    }
    assert_eq!(appearance::DATE_FORMAT, "YYYY-MM-DD HH:mm:ss UTC");
    // A date too short to hold a part says so in that part.
    assert_eq!(
        appearance::date_text("D:2026", ""),
        "2026-??-?? ??:??:?? UTC"
    );
}

#[test]
fn the_name_is_checked_only_when_it_is_drawn() {
    let original = two_pages(0);
    let mut placed = visible(1, [20.0, 30.0, 170.0, 90.0], None);
    placed.name = "张伟".into();
    assert!(prepare_visible(original.clone(), NOW, None, &placed)
        .expect_err("drawn")
        .contains("turn the name line off"));
    placed.options.name = false;
    let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("not drawn");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    assert_eq!(
        strings_of(&after, &widget),
        ["Digitally signed by", "Date: 2026-09-26 00:00:00 UTC"]
    );
}

/// The signature dictionary the update added.
fn signature_of(after: &Document, widget: &Dictionary) -> Dictionary {
    after
        .get_dictionary(widget.get(b"V").and_then(Object::as_reference).expect("/V"))
        .expect("the signature dictionary")
        .clone()
}

/// A string entry's bytes.
fn bytes_of(dict: &Dictionary, key: &[u8]) -> Option<Vec<u8>> {
    match dict.get(key).ok()? {
        Object::String(bytes, _) => Some(bytes.clone()),
        _ => None,
    }
}

/// UTF-16BE with its byte-order mark.
fn utf16(text: &str) -> Vec<u8> {
    let mut out = vec![0xFE, 0xFF];
    out.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    out
}

// `sign --reason`, `--location` and `--contact`: what the dictionary carries
// with nothing drawn.
#[test]
fn an_invisible_signature_carries_its_notes_and_a_visible_one_its_contact() {
    let original = two_pages(0);
    let latin = |text: &str| text.chars().map(|ch| ch as u8).collect::<Vec<u8>>();
    let notes = Notes {
        field: String::new(),
        reason: " Approved ".into(),
        location: "東京".into(),
        contact: "jane@example.com".into(),
    };
    let unsigned = prepare_noted(original.clone(), NOW, None, None, &notes).expect("invisible");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    assert!(widget.get(b"AP").is_err(), "nothing is drawn");
    let signature = signature_of(&after, &widget);
    assert_eq!(bytes_of(&signature, b"Reason"), Some(latin("Approved")));
    assert_eq!(bytes_of(&signature, b"Location"), Some(utf16("東京")));
    assert_eq!(
        bytes_of(&signature, b"ContactInfo"),
        Some(latin("jane@example.com"))
    );

    // Control: with no note, the invisible revision is `prepare`'s, byte for
    // byte, and its dictionary has none of the three.
    let bare = prepare_noted(original.clone(), NOW, None, None, &Notes::default()).expect("bare");
    assert_eq!(bare, prepare(original.clone(), NOW, None).expect("prepare"));
    let after = reread(&original, &bare);
    let (_, widget) = new_widget(&after, &bare);
    let signature = signature_of(&after, &widget);
    for key in [&b"Reason"[..], b"Location", b"ContactInfo"] {
        assert_eq!(bytes_of(&signature, key), None);
    }

    // A visible signature's reason is the one it draws, and the note's is not
    // read; its contact is the note's, and is not drawn.
    let mut placed = visible(1, [20.0, 30.0, 220.0, 130.0], None);
    placed.options.reason = "Drawn".into();
    let unsigned =
        prepare_noted(original.clone(), NOW, None, Some(&placed), &notes).expect("visible");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    let signature = signature_of(&after, &widget);
    assert_eq!(bytes_of(&signature, b"Reason"), Some(latin("Drawn")));
    assert_eq!(bytes_of(&signature, b"Location"), None);
    assert_eq!(
        bytes_of(&signature, b"ContactInfo"),
        Some(latin("jane@example.com"))
    );
    let drawn = strings_of(&after, &widget);
    assert!(drawn.contains(&"Reason: Drawn".to_string()), "{drawn:?}");
    assert!(!drawn.iter().any(|line| line.contains("jane")), "{drawn:?}");
    // And a visible signature is still held to what it can draw.
    placed.name = "张伟".into();
    assert!(prepare_noted(original.clone(), NOW, None, Some(&placed), &notes).is_err());

    for (reason, location, contact, why) in [
        ("Appr\noved", "", "", "the reason"),
        ("", "Ham\u{7}burg", "", "the location"),
        ("", "", "a\tb", "the contact"),
        (&*"a".repeat(MAX_NOTE_CHARS + 1), "", "", "the reason"),
        ("", &*"a".repeat(MAX_NOTE_CHARS + 1), "", "the location"),
        ("", "", &*"a".repeat(MAX_NOTE_CHARS + 1), "the contact"),
    ] {
        let notes = Notes {
            field: String::new(),
            reason: reason.into(),
            location: location.into(),
            contact: contact.into(),
        };
        let refused = prepare_noted(original.clone(), NOW, None, None, &notes).expect_err(why);
        assert!(refused.contains(why), "{refused}");
    }
    // The bound itself is carried.
    let notes = Notes {
        contact: "a".repeat(MAX_NOTE_CHARS),
        ..Notes::default()
    };
    assert!(prepare_noted(original, NOW, None, None, &notes).is_ok());
}

#[test]
fn a_reason_and_location_are_text_strings_inside_the_range() {
    let original = two_pages(0);
    let latin = |text: &str| text.chars().map(|ch| ch as u8).collect::<Vec<u8>>();
    for (reason, location, reason_bytes, location_bytes) in [
        ("Approved", "Hamburg", latin("Approved"), latin("Hamburg")),
        // Umlauts and ß have the same byte in PDFDocEncoding as in Latin-1.
        (
            "Geprüft, Größe",
            "Köln-Mülheim",
            latin("Geprüft, Größe"),
            latin("Köln-Mülheim"),
        ),
        // Outside PDFDocEncoding: UTF-16BE behind its mark. The euro sign has a
        // PDFDocEncoding byte (0xA0), which is not its Latin-1 one; it goes to
        // UTF-16 rather than to a table.
        (
            "审核通过",
            "東京都 港区",
            utf16("审核通过"),
            utf16("東京都 港区"),
        ),
        ("€ 12", "Zürich", utf16("€ 12"), latin("Zürich")),
        // Latin-1 whose first bytes would read as a byte-order mark.
        ("þÿ ok", "ï»¿ ok", utf16("þÿ ok"), utf16("ï»¿ ok")),
    ] {
        let details = Details {
            field: None,
            reason: Some(reason.into()),
            location: Some(location.into()),
            contact: None,
            document_timestamp: false,
        };
        let unsigned = build(original.clone(), NOW, None, None, &details).expect(reason);
        let after = reread(&original, &unsigned);
        let (_, widget) = new_widget(&after, &unsigned);
        let signature = signature_of(&after, &widget);
        assert_eq!(
            bytes_of(&signature, b"Reason"),
            Some(reason_bytes),
            "{reason}"
        );
        assert_eq!(
            bytes_of(&signature, b"Location"),
            Some(location_bytes),
            "{location}"
        );
        // Read back by the decoder that reads comments, through lopdf's table.
        for (key, text) in [(&b"Reason"[..], reason), (b"Location", location)] {
            let back = crate::annots::decode_text_string(&bytes_of(&signature, key).expect("set"));
            assert_eq!(back, text);
        }
        // Inside the byte range: each entry sits in one of the two covered
        // pieces, not in the hole.
        let whole = [original.as_slice(), unsigned.update.as_slice()].concat();
        for key in [&b"/Reason<"[..], b"/Location<"] {
            let at = only(&whole, key).expect("written once, in hexadecimal") as u64;
            let end = at
                + whole[at as usize..]
                    .iter()
                    .position(|b| *b == b'>')
                    .expect(">") as u64;
            let [_, hole, hole_end, rest] = unsigned.range;
            let covered = end < hole || (at >= hole_end && end < hole_end + rest);
            assert!(
                covered,
                "{reason}: {at}..{end} against {:?}",
                unsigned.range
            );
        }
    }
    // Through the visible path, the entries it drew are the entries it wrote.
    let mut placed = visible(1, [20.0, 30.0, 220.0, 130.0], None);
    placed.options.reason = "  Geprüft ".into();
    placed.options.location = "Köln".into();
    let unsigned = prepare_visible(original.clone(), NOW, None, &placed).expect("visible");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    let signature = signature_of(&after, &widget);
    assert_eq!(bytes_of(&signature, b"Reason"), Some(latin("Geprüft")));
    assert_eq!(bytes_of(&signature, b"Location"), Some(latin("Köln")));
    // And none is written when none was given.
    let unsigned = prepare_visible(
        original.clone(),
        NOW,
        None,
        &visible(1, [20.0, 30.0, 170.0, 90.0], None),
    )
    .expect("plain");
    let after = reread(&original, &unsigned);
    let (_, widget) = new_widget(&after, &unsigned);
    let signature = signature_of(&after, &widget);
    assert!(signature.get(b"Reason").is_err() && signature.get(b"Location").is_err());
}

#[test]
fn every_character_reads_back_from_its_text_string() {
    // Enumerated, not sampled: every scalar value, alone and between two
    // letters, through the decoder that reads comments with lopdf's own
    // PDFDocEncoding table.
    let mut pdfdoc = 0;
    for code in 0..=0x10FFFF_u32 {
        let Some(ch) = char::from_u32(code) else {
            continue;
        };
        for text in [ch.to_string(), format!("a{ch}b")] {
            let Object::String(bytes, _) = text_string(&text) else {
                panic!("not a string")
            };
            if !bytes.starts_with(&[0xFE, 0xFF]) {
                pdfdoc += 1;
            }
            assert_eq!(
                crate::annots::decode_text_string(&bytes),
                text,
                "U+{code:04X}"
            );
        }
    }
    // Printable ASCII and U+00A1..U+00FF less the soft hyphen, each twice ---
    // less the two sequences that would read as a byte-order mark, which are
    // longer than one character and so are not in this count.
    assert_eq!(pdfdoc, (95 + 94) * 2);
}

#[test]
fn a_preview_draws_the_stream_the_signing_writes() {
    let original = two_pages(0);
    let mut compared = 0;
    for (image, mask) in [
        (None, 0b00111),
        (Some(raster()), 0b11111),
        (Some(raster()), 0b01101),
        (None, 0b10000),
    ] {
        for (width, height) in [(240.0_f32, 80.0), (100.0, 150.0)] {
            for (left, top) in [(0.0_f32, 0.0), (20.0, 30.0), (37.5, 211.25)] {
                let mut placed = visible(1, [left, top, left + width, top + height], image.clone());
                placed.options = options_for(mask, "Geprüft", "Köln");
                let unsigned =
                    prepare_visible(original.clone(), NOW, None, &placed).expect("signed");
                let after = reread(&original, &unsigned);
                let (_, widget) = new_widget(&after, &unsigned);
                let written = form_of(&after, &widget);

                let previewed = preview(NOW, &placed).expect("preview");
                let shown = Document::load_mem(&previewed).expect("the preview parses");
                let page = ordered_pages(&shown)[0];
                let media = numbers_of(
                    shown
                        .get_dictionary(page)
                        .and_then(|p| p.get(b"MediaBox"))
                        .expect("a media box"),
                );
                assert!(
                    close(&media, &[0.0, 0.0, f64::from(width), f64::from(height)]),
                    "{media:?}"
                );
                let widget = shown
                    .objects
                    .values()
                    .filter_map(|o| o.as_dict().ok())
                    .find(|d| d.get(b"FT").and_then(Object::as_name).ok() == Some(b"Sig"))
                    .expect("the preview's widget")
                    .clone();
                let seen = form_of(&shown, &widget);
                assert_eq!(seen, written, "{mask:05b} {width}x{height} at {left},{top}");
                compared += 1;
            }
        }
    }
    assert_eq!(compared, 24);
}

/// A widget's appearance as bytes that must match: the stream, its box, and
/// every image and font it draws with, decoded.
fn form_of(doc: &Document, widget: &Dictionary) -> (Vec<u8>, Vec<f64>, Vec<Vec<u8>>, Vec<String>) {
    let ap = widget.get(b"AP").and_then(Object::as_dict).expect("/AP");
    let form = doc
        .get_object(ap.get(b"N").and_then(Object::as_reference).expect("/N"))
        .and_then(Object::as_stream)
        .expect("stream");
    let resources = form
        .dict
        .get(b"Resources")
        .and_then(Object::as_dict)
        .expect("resources");
    let mut images = Vec::new();
    if let Ok(xobjects) = resources.get(b"XObject").and_then(Object::as_dict) {
        for (_, reference) in xobjects.iter() {
            let image = doc
                .get_object(reference.as_reference().expect("a reference"))
                .and_then(Object::as_stream)
                .expect("an image");
            images.push(image.decompressed_content().expect("decoded"));
            let mask = image
                .dict
                .get(b"SMask")
                .and_then(Object::as_reference)
                .expect("a mask");
            let mask = doc
                .get_object(mask)
                .and_then(Object::as_stream)
                .expect("mask stream");
            images.push(mask.decompressed_content().expect("decoded"));
        }
    }
    let fonts = resources
        .get(b"Font")
        .and_then(Object::as_dict)
        .map(|fonts| {
            fonts
                .iter()
                .map(|(name, reference)| {
                    let font = doc
                        .get_dictionary(reference.as_reference().expect("a reference"))
                        .expect("a font");
                    format!(
                        "{} {:?} {:?}",
                        String::from_utf8_lossy(name),
                        font.get(b"BaseFont").and_then(Object::as_name).ok(),
                        font.get(b"Encoding").and_then(Object::as_name).ok()
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    (
        form.content.clone(),
        numbers_of(form.dict.get(b"BBox").expect("/BBox")),
        images,
        fonts,
    )
}

#[test]
fn a_preview_is_refused_where_the_signing_would_be() {
    for (rect, why) in [
        ([0.0, 0.0, 20.0, 80.0], "between"),
        ([0.0, 0.0, 240.0, 800.0], "between"),
        ([0.0, 0.0, f32::NAN, 80.0], "between"),
    ] {
        let refused = preview(NOW, &visible(0, rect, None)).expect_err(why);
        assert!(refused.contains(why), "{rect:?}: {refused}");
    }
    // The signing's own refusals, word for word.
    let original = two_pages(0);
    let mut placed = visible(1, [20.0, 30.0, 170.0, 90.0], None);
    placed.name = "Иван Петров".into();
    let signing = prepare_visible(original.clone(), NOW, None, &placed).expect_err("name");
    assert_eq!(preview(NOW, &placed).expect_err("name"), signing);
    placed.options = options_for(0, "", "");
    let signing = prepare_visible(original, NOW, None, &placed).expect_err("nothing");
    assert_eq!(preview(NOW, &placed).expect_err("nothing"), signing);
}

#[test]
fn a_name_that_cannot_be_drawn_honestly_is_refused() {
    let original = two_pages(0);
    for (name, why) in [
        ("张伟", "cannot draw"),
        ("Иван Петров", "cannot draw"),
        ("A.\u{2009}Signer", "cannot draw"),
        ("tab\there", "cannot draw"),
        ("line\nbreak", "cannot draw"),
        ("   ", "names nobody"),
        (&*"a".repeat(appearance::MAX_NAME_CHARS + 1), "longer than"),
    ] {
        let mut placed = visible(1, [20.0, 30.0, 170.0, 90.0], None);
        placed.name = name.to_string();
        let refused = prepare_visible(original.clone(), NOW, None, &placed).expect_err(name);
        assert!(refused.contains(why), "{name}: {refused}");
    }
    // The control: Latin-1 beyond ASCII is drawn, not refused.
    let mut placed = visible(1, [20.0, 30.0, 170.0, 90.0], None);
    placed.name = "Jürgen Müller-Øster".into();
    assert!(prepare_visible(original, NOW, None, &placed).is_ok());
}

#[test]
fn a_rectangle_off_its_page_too_small_or_on_no_page_is_refused() {
    let original = two_pages(0);
    let mut damaged = raster();
    damaged.rgba.pop();
    for (placed, why) in [
        (
            visible(2, [20.0, 30.0, 170.0, 90.0], None),
            "not in this document",
        ),
        (
            visible(1, [20.0, 30.0, 400.0, 90.0], None),
            "not on its page",
        ),
        (
            visible(1, [-5.0, 30.0, 170.0, 90.0], None),
            "not on its page",
        ),
        (
            visible(1, [20.0, 30.0, 170.0, 500.0], None),
            "not on its page",
        ),
        (
            visible(1, [20.0, f32::NAN, 170.0, 90.0], None),
            "not on its page",
        ),
        (visible(1, [20.0, 30.0, 25.0, 90.0], None), "smaller than"),
        (visible(1, [20.0, 30.0, 170.0, 35.0], None), "smaller than"),
        (visible(1, [170.0, 30.0, 20.0, 90.0], None), "smaller than"),
        (
            visible(1, [20.0, 30.0, 170.0, 90.0], Some(damaged)),
            "damaged",
        ),
    ] {
        let refused = prepare_visible(original.clone(), NOW, None, &placed).expect_err(why);
        assert!(refused.contains(why), "{placed:?}: {refused}");
    }
    // The controls: the whole displayed page, and the smallest side allowed.
    for rect in [[0.0, 0.0, 300.0, 400.0], [20.0, 30.0, 44.0, 54.0]] {
        assert!(prepare_visible(original.clone(), NOW, None, &visible(1, rect, None)).is_ok());
    }
}

#[test]
fn a_visible_signature_after_a_certification_is_refused_and_an_invisible_one_is_not() {
    // Measured with pyHanko: a visible signature field appended to a certified
    // document is read as a change the certification forbids, at /P 2 and 3
    // alike, and the same revision without an appearance as form filling.
    let placed = visible(0, [20.0, 30.0, 170.0, 90.0], None);
    let mut examined = 0;
    for name in ["incr-certified-2.pdf", "incr-certified-3.pdf"] {
        let Some(bytes) = fixture(name) else { continue };
        let refused = prepare_visible(bytes.clone(), NOW, None, &placed).expect_err(name);
        assert!(refused.contains("is certified"), "{name}: {refused}");
        assert!(prepare(bytes, NOW, None).is_ok(), "{name}: invisible");
        examined += 1;
    }
    // The control: an approval signature is not a certification, and a visible
    // signature after one is accepted.
    if let Some(bytes) = fixture("incr-signed.pdf") {
        assert!(prepare_visible(bytes, NOW, None, &placed).is_ok());
        examined += 1;
    }
    println!("examined {examined} fixtures");
}

// ------------------------------------------------- a field the document has

/// [`two_pages`] with a form: empty signature fields `Approved` on the first
/// page and `Witness` on the second, and a text field `Name`. Each signature
/// field is 150 by 60 points at display `[20, 30]`.
fn with_fields(rotate: i64) -> Vec<u8> {
    use crate::formfields::{add, Kind, NewField};
    let mut doc = Document::load_mem(&two_pages(rotate)).expect("loads");
    let field = |name: &str, kind: Kind, page: u32, rect: [f64; 4]| NewField {
        name: name.into(),
        kind,
        page,
        rect,
        tooltip: None,
        required: false,
        max_length: None,
        border: kind == Kind::Signature,
        options: Vec::new(),
        text_size: None,
        default_value: None,
    };
    add(
        &mut doc,
        &[
            field("Approved", Kind::Signature, 0, [20.0, 30.0, 150.0, 60.0]),
            field("Witness", Kind::Signature, 1, [20.0, 30.0, 150.0, 60.0]),
            field("Name", Kind::Text, 0, [20.0, 120.0, 150.0, 20.0]),
        ],
    )
    .expect("fields");
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

fn into(field: &str) -> Notes {
    Notes {
        field: field.into(),
        ..Notes::default()
    }
}

/// The form's widget called `name`, as `forms::scan` reads it.
fn scanned(after: &Document, name: &str) -> crate::forms::Widget {
    crate::forms::scan(after)
        .expect("a form")
        .widgets
        .into_iter()
        .find(|w| w.name == name)
        .expect("the field")
}

#[test]
fn a_named_empty_field_takes_the_signature_and_nothing_is_added() {
    let original = with_fields(0);
    let before = Document::load_mem(&original).expect("loads");
    let was = scanned(&before, "Approved");
    let unsigned =
        prepare_noted(original.clone(), NOW, None, None, &into("Approved")).expect("prepared");
    assert_eq!(unsigned.field, "Approved");
    let after = reread(&original, &unsigned);
    // The same fields and the same annotations: the signature went into one
    // the document had.
    assert_eq!(fields_of(&after), fields_of(&before));
    assert_eq!(annots_of(&after), annots_of(&before));
    let now = scanned(&after, "Approved");
    assert!(matches!(
        now.control,
        crate::forms::Control::Signature { signed: true }
    ));
    assert!(matches!(
        scanned(&after, "Witness").control,
        crate::forms::Control::Signature { signed: false }
    ));
    let id = (was.widget.0, was.widget.1);
    let widget = after.get_dictionary(id).expect("the field");
    let signature = signature_of(&after, widget);
    assert_eq!(
        signature
            .get(b"SubFilter")
            .and_then(Object::as_name)
            .expect("subfilter"),
        b"ETSI.CAdES.detached"
    );
    // Print and Locked over what it had, where it was, drawn as it was.
    assert_eq!(widget.get(b"F").and_then(Object::as_i64).expect("/F"), 132);
    assert_eq!(now.rect, was.rect);
    let before_widget = before.get_dictionary(id).expect("the field");
    assert_eq!(widget.get(b"AP").ok(), before_widget.get(b"AP").ok());
    assert_eq!(sig_flags(&after), 3);
    // The update holds the signature, the field and the form, and no page.
    let touched = written(&original, &unsigned);
    assert!(touched.contains(&id));
    assert!(ordered_pages(&after)
        .iter()
        .all(|page| !touched.contains(page)));
}

#[test]
fn a_visible_signature_in_a_field_is_drawn_in_the_fields_rectangle() {
    for rotate in [0, 90, 180, 270] {
        let original = with_fields(rotate);
        let before = Document::load_mem(&original).expect("loads");
        let was = scanned(&before, "Witness");
        // A page and a rectangle that are not the field's: neither is read.
        let elsewhere = visible(0, [0.0, 0.0, 30.0, 30.0], Some(raster()));
        let unsigned = prepare_noted(
            original.clone(),
            NOW,
            None,
            Some(&elsewhere),
            &into("Witness"),
        )
        .unwrap_or_else(|why| panic!("{rotate}: {why}"));
        let after = reread(&original, &unsigned);
        let id = (was.widget.0, was.widget.1);
        let widget = after.get_dictionary(id).expect("the field").clone();
        assert_eq!(scanned(&after, "Witness").rect, was.rect, "{rotate}");
        assert_eq!(annots_on(&after, 1), annots_on(&before, 1));
        assert!(annots_on(&after, 0) == annots_on(&before, 0));

        // What signing draws when a reader drags that same rectangle on that
        // page of a document with no field: the same form, to the byte.
        let plain = two_pages(rotate);
        let dragged = visible(1, was.display_rect, Some(raster()));
        let apart = prepare_visible(plain.clone(), NOW, None, &dragged).expect("prepared");
        let apart_doc = reread(&plain, &apart);
        let (_, apart_widget) = new_widget(&apart_doc, &apart);
        let (form, operations) = appearance_of(&after, &widget);
        let (apart_form, apart_operations) = appearance_of(&apart_doc, &apart_widget);
        assert_eq!(
            format!("{operations:?}"),
            format!("{apart_operations:?}"),
            "{rotate}"
        );
        for key in [b"BBox".as_slice(), b"Matrix"] {
            assert_eq!(
                form.get(key).ok().map(numbers_of),
                apart_form.get(key).ok().map(numbers_of),
                "{rotate}"
            );
        }
        assert!(close(
            &numbers_of(widget.get(b"Rect").expect("/Rect")),
            &numbers_of(apart_widget.get(b"Rect").expect("/Rect"))
        ));
    }
}

#[test]
fn a_field_that_cannot_be_signed_is_refused_and_says_why() {
    let original = with_fields(0);
    let refused = |bytes: &[u8], visible: Option<&Visible>, field: &str| {
        prepare_noted(bytes.to_vec(), NOW, None, visible, &into(field)).expect_err("refused")
    };
    assert!(refused(&original, None, "Missing").contains("has no field called `Missing`"));
    assert!(refused(&original, None, "Name").contains("`Name` is not a signature field"));

    // Signed once, it is not signed again; the other field still is.
    let first = prepare_noted(original.clone(), NOW, None, None, &into("Approved")).expect("once");
    let signed = [original.as_slice(), first.update.as_slice()].concat();
    assert!(refused(&signed, None, "Approved").contains("`Approved` already holds a signature"));
    prepare_noted(signed.clone(), NOW, None, None, &into("Witness")).expect("the other one");

    // Too small to draw in: refused with an appearance, signed without one.
    let small = {
        let mut doc = Document::load_mem(&original).expect("loads");
        let id = scanned(&doc, "Approved").widget;
        doc.get_dictionary_mut((id.0, id.1))
            .expect("field")
            .set("Rect", vec![20.into(), 110.into(), 170.into(), 130.into()]);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("saved");
        bytes
    };
    let shown = visible(0, [0.0, 0.0, 100.0, 100.0], None);
    assert!(refused(&small, Some(&shown), "Approved").contains("sign it without an appearance"));
    prepare_noted(small, NOW, None, None, &into("Approved")).expect("without one");

    // A field that refuses an answer refuses a signature.
    let locked = {
        let mut doc = Document::load_mem(&original).expect("loads");
        let id = scanned(&doc, "Approved").widget;
        doc.get_dictionary_mut((id.0, id.1))
            .expect("field")
            .set("Ff", 1);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("saved");
        bytes
    };
    assert!(refused(&locked, None, "Approved").contains("read-only"));
}

#[test]
fn with_no_field_named_the_signature_still_makes_its_own() {
    let original = with_fields(0);
    let before = Document::load_mem(&original).expect("loads");
    let unsigned =
        prepare_noted(original.clone(), NOW, None, None, &Notes::default()).expect("prepared");
    let after = reread(&original, &unsigned);
    assert_eq!(fields_of(&after).len(), fields_of(&before).len() + 1);
    assert_eq!(unsigned.field, "Signature1");
    assert!(matches!(
        scanned(&after, "Approved").control,
        crate::forms::Control::Signature { signed: false }
    ));
}
