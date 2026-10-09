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
