//! The `/DSS` revision, built over signed test documents.

use super::*;
use crate::integrity::test_tsa::{
    mint_crl, mint_ocsp, CrlFaults, OcspFaults, Responder, Status, TestCa,
};
use crate::sign_cms::testkeys::{plain_pdf, Soft, NOW};

/// The plain document signed by a test authority's signer, and the data a
/// B-LT writer would add about it.
fn world() -> (Vec<u8>, Gathered) {
    let ca = TestCa::new("tpdf test DSS authority", 0x41);
    let signer = ca.issue("tpdf test DSS signer", 0x42, 3, None);
    let bytes = crate::sign_cms::testkeys::signed(
        &plain_pdf(),
        &signer.certificate,
        std::slice::from_ref(&ca.certificate),
        &Soft::p256(signer.seed),
    )
    .expect("signed");
    let response = mint_ocsp(
        &signer.certificate,
        &ca,
        Status::Good,
        NOW - 60,
        Some(NOW + 86_400),
        Responder::Issuer,
        &OcspFaults::default(),
    );
    let list = mint_crl(
        &ca,
        &[],
        NOW - 60,
        Some(NOW + 86_400),
        &CrlFaults::default(),
    );
    (
        bytes,
        Gathered {
            certificates: vec![ca.certificate, signer.certificate],
            responses: vec![response],
            lists: vec![list],
        },
    )
}

fn dss_of(bytes: &[u8]) -> Dictionary {
    let document = Document::load_mem(bytes).expect("parses");
    let catalog = document.catalog().expect("a catalog");
    resolve(&document, catalog.get(b"DSS").expect("a /DSS"))
        .as_dict()
        .expect("a dictionary")
        .clone()
}

fn count(dss: &Dictionary, key: &[u8]) -> usize {
    dss.get(key)
        .ok()
        .and_then(|o| o.as_array().ok())
        .map_or(0, Vec::len)
}

#[test]
fn the_revision_is_appended_and_the_signature_still_covers_what_it_did() {
    let (bytes, gathered) = world();
    let extended = extend(&bytes, &gathered, 1).expect("extended");
    assert_eq!(extended.built_against, bytes.len());
    let mut whole = bytes.clone();
    whole.extend_from_slice(&extended.update);
    let dss = dss_of(&whole);
    assert_eq!(
        (
            count(&dss, b"Certs"),
            count(&dss, b"OCSPs"),
            count(&dss, b"CRLs")
        ),
        (2, 1, 1)
    );
    assert!(!dss.has(b"VRI"), "no /VRI, by decision");
    // The scan the worker answers is of the whole file.
    let ours = extended
        .signatures
        .iter()
        .find(|s| s.signed)
        .expect("the signature");
    assert_eq!(
        ours.integrity.as_ref().map(|i| i.verdict),
        Some(crate::integrity::Verdict::Intact)
    );
    assert!(
        ours.appended_bytes > 0,
        "the /DSS is after the signed range"
    );
    assert_eq!(
        ours.revocation.as_ref().map(|r| r.standing),
        Some(crate::revocation::Status::Good)
    );
}

#[test]
fn an_earlier_dss_is_kept_and_added_to() {
    let (bytes, gathered) = world();
    let first = append(&bytes, &gathered).expect("first");
    let mut once = bytes.clone();
    once.extend_from_slice(&first);
    let more = Gathered {
        certificates: vec![b"not really a certificate".to_vec()],
        responses: vec![b"not really a response".to_vec()],
        lists: Vec::new(),
    };
    let second = append(&once, &more).expect("second");
    let mut twice = once.clone();
    twice.extend_from_slice(&second);
    let dss = dss_of(&twice);
    assert_eq!(
        (
            count(&dss, b"Certs"),
            count(&dss, b"OCSPs"),
            count(&dss, b"CRLs")
        ),
        (3, 2, 1)
    );
}

/// An earlier `/DSS` is carried into the new one key for key, and it is the
/// document's own. With a `/Linearized` key in it, `lopdf`'s incremental
/// writer would leave the new `/DSS` out and write a catalog pointing at an
/// object that is not there; that is refused.
#[test]
fn a_dss_the_writer_would_leave_out_is_a_refusal() {
    let (bytes, gathered) = world();
    let first = append(&bytes, &gathered).expect("first");
    let mut once = bytes.clone();
    once.extend_from_slice(&first);
    let with_key = {
        let mut doc = Document::load_mem(&once).expect("parses");
        let dss = doc
            .catalog()
            .expect("a catalog")
            .get(b"DSS")
            .and_then(Object::as_reference)
            .expect("a /DSS of its own");
        doc.get_dictionary_mut(dss)
            .expect("a dictionary")
            .set("Xinearized", Object::Integer(1));
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("saved");
        out
    };
    let more = Gathered {
        certificates: Vec::new(),
        responses: vec![b"not really a response".to_vec()],
        lists: Vec::new(),
    };
    let why = append(&crate::save::marked_left_out(with_key.clone()), &more).expect_err("refused");
    assert!(
        why.contains("(/Linearized)") && why.contains("tpdf does not write one"),
        "{why}"
    );
    // The control: the same `/DSS` with the key under any other name is
    // carried over and added to.
    let update = append(&with_key, &more).expect("appended");
    let mut whole = with_key;
    whole.extend_from_slice(&update);
    assert_eq!(count(&dss_of(&whole), b"OCSPs"), 2);
}

#[test]
fn nothing_to_add_or_too_much_is_refused() {
    let (bytes, gathered) = world();
    let empty = Gathered {
        certificates: gathered.certificates.clone(),
        ..Gathered::default()
    };
    assert!(append(&bytes, &empty).is_err());
    let huge = Gathered {
        lists: vec![vec![0x30; MAX_BYTES + 1]],
        ..Gathered::default()
    };
    let why = append(&bytes, &huge).expect_err("refused");
    assert!(why.contains("more than"), "{why}");
}

// ------------------------------------------- a document already signed

/// The signer's certificate of a CMS, DER.
fn signer_in(cms: &[u8]) -> Vec<u8> {
    use der::{Decode as _, Encode as _};
    let info = cms::content_info::ContentInfo::from_der(cms).expect("a ContentInfo");
    let signed = info
        .content
        .decode_as::<cms::signed_data::SignedData>()
        .expect("SignedData");
    let (certificate, identified) = crate::docinfo::signer_certificate(&signed).expect("a signer");
    assert!(identified);
    certificate.to_der().expect("DER")
}

#[test]
fn the_survey_hands_over_each_signature_s_value_and_what_the_dss_carries() {
    let (bytes, gathered) = world();
    let found = survey(&bytes, 1);
    assert_eq!(found.refused, None);
    assert!(found.complete);
    assert_eq!(found.signatures.iter().filter(|s| s.signed).count(), 1);
    let [(name, value)] = found.values.as_slice() else {
        panic!("one value: {:?}", found.values.len());
    };
    assert_eq!(name, "Signature1");
    // The value is the signature's own CMS, ended where it ends: its signer
    // is the certificate the document was signed with.
    assert_eq!(signer_in(value), gathered.certificates[1]);
    assert!(found.store.is_empty(), "no /DSS yet");

    // With a `/DSS`, the store is what it carries, for the walk to have as
    // the reader of the result will.
    let mut whole = bytes.clone();
    whole.extend_from_slice(&extend(&bytes, &gathered, 1).expect("extended").update);
    let found = survey(&whole, 1);
    assert_eq!(found.refused, None);
    assert_eq!(found.store, gathered.certificates);
    assert_eq!(found.values.len(), 1);
}

#[test]
fn a_value_that_cannot_be_read_makes_the_survey_incomplete() {
    let (mut bytes, _) = world();
    let at = bytes
        .windows(10)
        .position(|w| w == b"/Contents<")
        .expect("the value")
        + 10;
    assert_eq!(&bytes[at..at + 2], b"30", "a SEQUENCE");
    // A tag no CMS begins with, and a length nothing satisfies.
    bytes[at..at + 4].copy_from_slice(b"05ff");
    let found = survey(&bytes, 1);
    assert_eq!(found.refused, None, "it still parses as a document");
    assert!(!found.complete);
    assert!(found.values.iter().all(|(_, value)| value.is_empty()));
    // The control: untouched, it is complete.
    assert!(survey(&world().0, 1).complete);
}

#[test]
fn a_document_that_can_take_no_revision_is_refused_in_the_answer() {
    let found = survey(b"%PDF-1.7 and nothing else", 1);
    let why = found.refused.expect("refused");
    assert!(
        why.starts_with("this document could not be parsed"),
        "{why}"
    );
    assert!(found.values.is_empty() && found.signatures.is_empty());

    // More DER than the answer carries: the bound, reached with a small one.
    let (bytes, _) = world();
    let size = survey(&bytes, 1).bytes();
    assert!(size > 0);
    assert_eq!(survey_within(&bytes, 1, size).refused, None);
    let why = survey_within(&bytes, 1, size - 1).refused.expect("refused");
    assert!(why.contains("more than the"), "{why}");
}

use lopdf::dictionary;

/// A document whose form holds `fields` as its `/Fields`.
fn form_of(fields: Vec<Object>, document: &mut Document) {
    let pages = document.new_object_id();
    let page = document.add_object(dictionary! { "Type" => "Page", "Parent" => pages });
    document.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let form = document.add_object(dictionary! { "Fields" => fields });
    let catalog = document
        .add_object(dictionary! { "Type" => "Catalog", "Pages" => pages, "AcroForm" => form });
    document.trailer.set("Root", catalog);
}

/// A signed signature field called `name`, whose value is a five-byte
/// SEQUENCE: enough to be read as a value, which is all the walk asks.
fn signed_field(document: &mut Document, name: &str) -> Object {
    let value = dictionary! {
        "Type" => "Sig",
        "Contents" => Object::String(vec![0x30, 3, 2, 1, 1], lopdf::StringFormat::Hexadecimal),
    };
    document
        .add_object(dictionary! {
            "FT" => "Sig",
            "T" => Object::string_literal(name),
            "V" => value,
        })
        .into()
}

/// The values are complete only when every signature field was reached: the
/// walk's three bounds each say so rather than answering a shorter list.
#[test]
fn values_cut_short_by_a_bound_of_the_walk_are_not_complete() {
    let with = |count: usize| {
        let mut document = Document::with_version("1.7");
        let fields = (0..count)
            .map(|n| signed_field(&mut document, &format!("S{n}")))
            .collect();
        form_of(fields, &mut document);
        crate::docinfo::signature_values(&document)
    };
    // As many signatures as the scan reads, and one more.
    let most = with(32);
    assert_eq!((most.values.len(), most.complete), (32, true));
    assert_eq!(most.values[0], ("S0".to_string(), vec![0x30, 3, 2, 1, 1]));
    let over = with(33);
    assert_eq!((over.values.len(), over.complete), (32, false));
    // Exactly as many signatures as are read, and another field after them
    // that is no signature: nothing was left out.
    let mut document = Document::with_version("1.7");
    let mut fields: Vec<Object> = (0..32)
        .map(|n| signed_field(&mut document, &format!("S{n}")))
        .collect();
    fields.push(
        document
            .add_object(dictionary! { "FT" => "Tx", "T" => Object::string_literal("Name") })
            .into(),
    );
    form_of(fields, &mut document);
    let found = crate::docinfo::signature_values(&document);
    assert_eq!((found.values.len(), found.complete), (32, true));

    // An entry of `/Fields` that is no field at all.
    let mut document = Document::with_version("1.7");
    let field = signed_field(&mut document, "S0");
    form_of(vec![field, Object::Reference((9_999, 0))], &mut document);
    let found = crate::docinfo::signature_values(&document);
    assert_eq!((found.values.len(), found.complete), (1, false));

    // A signature under more groups than the walk descends.
    let mut document = Document::with_version("1.7");
    let mut node = signed_field(&mut document, "deep");
    for _ in 0..12 {
        node = document
            .add_object(dictionary! { "Kids" => vec![node] })
            .into();
    }
    form_of(vec![node], &mut document);
    let found = crate::docinfo::signature_values(&document);
    assert!(found.values.is_empty() && !found.complete);
}

/// A second revision of the same data writes nothing the `/DSS` holds, and
/// one that is partly new writes the new part: no entry twice, none dropped.
#[test]
fn an_entry_the_dss_already_holds_is_not_written_again() {
    let (bytes, gathered) = world();
    let mut once = bytes.clone();
    once.extend_from_slice(&append(&bytes, &gathered).expect("appended"));
    let mut twice = once.clone();
    twice.extend_from_slice(&append(&once, &gathered).expect("appended again"));
    let dss = dss_of(&twice);
    assert_eq!(
        (
            count(&dss, b"Certs"),
            count(&dss, b"OCSPs"),
            count(&dss, b"CRLs")
        ),
        (2, 1, 1),
        "the same data added twice is there once"
    );
    // Partly new, and one entry twice in what was gathered.
    let fresh = vec![0x30, 3, 2, 1, 7];
    let more = Gathered {
        certificates: vec![
            gathered.certificates[0].clone(),
            fresh.clone(),
            fresh.clone(),
        ],
        ..gathered.clone()
    };
    let mut thrice = twice.clone();
    thrice.extend_from_slice(&append(&twice, &more).expect("appended"));
    let document = Document::load_mem(&thrice).expect("parses");
    let dss = dss_of(&thrice);
    let certs: Vec<Vec<u8>> = dss
        .get(b"Certs")
        .and_then(Object::as_array)
        .expect("certificates")
        .iter()
        .map(|item| {
            resolve(&document, item)
                .as_stream()
                .expect("a stream")
                .content
                .clone()
        })
        .collect();
    assert_eq!(
        certs,
        [
            gathered.certificates[0].clone(),
            gathered.certificates[1].clone(),
            fresh
        ],
        "what was there, in its order, and the new one once"
    );
    // And the survey says how many entries the arrays list.
    assert_eq!(survey(&thrice, 1).held, [3, 1, 1]);
    assert_eq!(survey(&bytes, 1).held, [0, 0, 0]);
}

/// A `/DSS` the writer could not extend is refused in the survey, before
/// anything is fetched for the document, and one tpdf's reader does not take
/// whole is said to be cut.
#[test]
fn a_dss_that_cannot_be_extended_or_read_whole_is_said_in_the_survey() {
    let with_dss = |dss: Object| {
        let mut document = Document::load_mem(&plain_pdf()).expect("parses");
        let root = document
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .expect("a catalog");
        document
            .get_dictionary_mut(root)
            .expect("the catalog")
            .set("DSS", dss);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("saved");
        bytes
    };
    let why = survey(&with_dss(Object::Integer(7)), 1)
        .refused
        .expect("refused");
    assert_eq!(why, "this document's /DSS is not a dictionary");
    let why = survey(
        &with_dss(Object::Dictionary(dictionary! { "OCSPs" => 7 })),
        1,
    )
    .refused
    .expect("refused");
    assert_eq!(why, "this document's /DSS /OCSPs is not an array");

    // More certificates than the reader takes: read, counted, and cut.
    let certificates = |count: usize| {
        let mut document = Document::load_mem(&plain_pdf()).expect("parses");
        let streams: Vec<Object> = (0..count)
            .map(|n| {
                let n = u8::try_from(n).expect("small");
                document
                    .add_object(Stream::new(Dictionary::new(), vec![0x30, 1, n]))
                    .into()
            })
            .collect();
        let root = document
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .expect("a catalog");
        document
            .get_dictionary_mut(root)
            .expect("the catalog")
            .set("DSS", dictionary! { "Certs" => streams });
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("saved");
        survey(&bytes, 1)
    };
    let most = crate::revocation::MAX_DSS_CERTIFICATES;
    let whole = certificates(most);
    assert_eq!(
        (whole.refused, whole.held, whole.store_cut),
        (None, [most, 0, 0], false)
    );
    let cut = certificates(most + 1);
    assert_eq!((cut.held, cut.store_cut), ([most + 1, 0, 0], true));
    // An entry that is no stream is one the reader could not read.
    let unread = survey(
        &with_dss(Object::Dictionary(
            dictionary! { "Certs" => vec![Object::Integer(7)] },
        )),
        1,
    );
    assert_eq!((unread.refused, unread.store_cut), (None, true));
}
