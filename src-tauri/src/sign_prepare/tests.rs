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
    }
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
    assert!(close(&numbers_of(form.get(b"BBox").expect("/BBox")), &rect));
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
    let rect = numbers_of(widget.get(b"Rect").expect("/Rect"));
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
    let rect = numbers_of(widget.get(b"Rect").expect("/Rect"));
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

#[test]
fn every_layout_keeps_its_ink_inside_the_box_and_apart_from_the_image() {
    let long = "W".repeat(appearance::MAX_NAME_CHARS);
    let mut checked = 0;
    for name in ["A. Signer", "Ålfhild Ærøskøbing-Überschär", long.as_str()] {
        let lines = appearance::words(name, "D:20260926000000Z");
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
                assert!(layout.size > 0.0 && layout.size <= appearance::MAX_SIZE);
                assert_eq!(layout.lines.len(), 3);
                let e = 1e-9;
                for (u, v, text) in &layout.lines {
                    let [u0, v0, u1, v1] = appearance::line_extent(layout.size, *u, *v, text);
                    assert!(
                        u0 >= -e && v0 >= -e && u1 <= width + e && v1 <= height + e,
                        "{name} {width}x{height} {image:?}: {text} at {:?}",
                        [u0, v0, u1, v1]
                    );
                    if let Some([iu, iv, iw, ih]) = layout.image {
                        let apart =
                            u1 <= iu + e || u0 >= iu + iw - e || v1 <= iv + e || v0 >= iv + ih - e;
                        assert!(
                            apart,
                            "{name} {width}x{height} {image:?}: {text} over the image"
                        );
                    }
                }
                if let Some([iu, iv, iw, ih]) = layout.image {
                    assert!(iu >= -e && iv >= -e && iu + iw <= width + e && iv + ih <= height + e);
                    let (pw, ph) = image
                        .map(|(w, h)| (f64::from(w), f64::from(h)))
                        .expect("an image");
                    assert!((iw / ih - pw / ph).abs() < 1e-6, "proportions kept");
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 96);
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
