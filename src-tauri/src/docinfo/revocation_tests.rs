//! Revocation and the attested moment, through the whole scan.
//!
//! Each test signs the plain test document with a certificate a test
//! authority issued, timestamps it with the test timestamp authority, appends
//! a `/DSS` the way a PAdES B-LT writer does, and reads the result with
//! [`scan_at`] --- the test authorities as the only anchors, so no keychain or
//! certificate store is touched. The trust answers need an operating system
//! store to ask, so on a platform with none those tests return early.

use super::*;
use crate::integrity::test_tsa::{
    mint_ocsp, mint_with, with_dss, Faults, Imprint, Issued, OcspFaults, Responder, Status as Says,
    TestCa, TestTsa,
};
use crate::revocation::{Basis, Gap, Status};
use crate::sign_cms::testkeys::{plain_pdf, Soft, NOW as SIGNED};
use crate::trust::{Doubt, Standing};

/// 2026-09-01 00:00:00 UTC: the time the tokens attest.
const G: u64 = 1_788_220_800;
/// 2026-09-28: the present the scans run at.
const NOW: u64 = 1_790_553_600;
const DAY: u64 = 86_400;

fn has_store() -> bool {
    cfg!(any(target_os = "macos", windows))
}

struct World {
    ca: TestCa,
    signer: Issued,
    tsa: TestTsa,
    tsa_root: TestCa,
}

fn world() -> World {
    let ca = TestCa::new("tpdf test signing authority", 0x61);
    let signer = ca.issue("tpdf test signer", 0x62, 5, None);
    World {
        ca,
        signer,
        tsa: TestTsa::new(),
        tsa_root: TestCa::of_tsa(),
    }
}

/// The plain document signed by `signer` with `chain` in the signature, and
/// a token over the signature at `gen_time`.
fn signed(signer: &Issued, chain: &[Vec<u8>], gen_time: u64, tsa: &TestTsa) -> Vec<u8> {
    let original = plain_pdf();
    let key = Soft::p256(signer.seed);
    let unsigned = crate::sign_prepare::prepare(original.clone(), SIGNED, None).expect("prepared");
    let made = crate::sign_cms::sign(original, unsigned, SIGNED, &signer.certificate, chain, &key)
        .expect("made");
    let value = made.value().expect("a value");
    let token = mint_with(
        Imprint::Sha256,
        &Imprint::Sha256.digest(&value),
        None,
        gen_time,
        tsa,
        &Faults::default(),
    );
    let stamped = made.stamped(&token).expect("stamped");
    made.seal(Some(stamped)).expect("sealed")
}

/// A response from `issuer` about `certificate`, fresh for `G`.
fn ocsp(certificate: &[u8], issuer: &TestCa, says: Says) -> Vec<u8> {
    mint_ocsp(
        certificate,
        issuer,
        says,
        G - 3_600,
        Some(G + 7 * DAY),
        Responder::Issuer,
        &OcspFaults::default(),
    )
}

/// The one signature, read with `anchors` as the only roots.
fn read(bytes: &[u8], anchors: &[Vec<u8>]) -> Signature {
    let found = scan_at(bytes, crate::trust::Anchors::Only(anchors), NOW).expect("scanned");
    let mut signed: Vec<Signature> = found.signatures.into_iter().filter(|s| s.signed).collect();
    assert_eq!(signed.len(), 1);
    signed.remove(0)
}

fn anchors(w: &World) -> Vec<Vec<u8>> {
    vec![w.ca.certificate.clone(), w.tsa.root.clone()]
}

/// The document signed, stamped at `G`, and carrying OCSP responses for the
/// signer and the authority.
fn b_lt(w: &World, signer_says: Says, authority_says: Says) -> Vec<u8> {
    let bytes = signed(&w.signer, &[], G, &w.tsa);
    // Both issuers, as a B-LT writer adds them: the token carries only the
    // authority's own certificate, so without its root here the authority's
    // revocation data could not be checked (a test below holds that).
    with_dss(
        &bytes,
        &[w.ca.certificate.clone(), w.tsa.root.clone()],
        &[
            ocsp(&w.signer.certificate, &w.ca, signer_says),
            ocsp(&w.tsa.certificate, &w.tsa_root, authority_says),
        ],
        &[],
    )
}

#[test]
fn a_b_lt_signature_is_judged_at_the_attested_time_with_the_documents_own_data() {
    if !has_store() {
        return;
    }
    let w = world();
    let signature = read(&b_lt(&w, Says::Good, Says::Good), &anchors(&w));
    assert_eq!(
        signature.integrity.as_ref().map(|i| i.verdict),
        Some(crate::integrity::Verdict::Intact)
    );
    let trust = signature.trust.expect("a standing");
    assert_eq!(trust.standing, Standing::TrustedAtTimestamp, "{trust:?}");
    assert_eq!(trust.attested_at, "2026-09-01 00:00:00 UTC");
    let revocation = signature.revocation.expect("a revocation answer");
    assert_eq!(
        (revocation.standing, revocation.basis),
        (Status::Good, Basis::Attested),
        "{revocation:?}"
    );
    let stamp = signature.timestamp.expect("a timestamp");
    let authority = stamp.revocation.expect("the authority's revocation");
    assert_eq!(
        (authority.standing, authority.basis),
        (Status::Good, Basis::Stated),
        "{authority:?}"
    );
}

#[test]
fn a_document_carrying_no_revocation_data_reads_none_and_still_judges_at_the_timestamp() {
    if !has_store() {
        return;
    }
    let w = world();
    let signature = read(&signed(&w.signer, &[], G, &w.tsa), &anchors(&w));
    assert_eq!(
        signature.trust.as_ref().map(|t| t.standing),
        Some(Standing::TrustedAtTimestamp)
    );
    let revocation = signature.revocation.expect("an answer");
    assert_eq!(revocation.standing, Status::None);
    assert!(!revocation.undoes());
    let stamp = signature.timestamp.expect("a timestamp");
    assert_eq!(
        stamp.revocation.map(|r| r.standing),
        Some(Status::None),
        "the authority is asked about too"
    );
}

#[test]
fn an_authority_this_computer_does_not_trust_leaves_the_signer_judged_now() {
    // The token is intact; its authority is not anchored. Its time is then
    // only the authority's word, so the signer is judged at the present and
    // its revocation at the signer's own claimed date.
    if !has_store() {
        return;
    }
    let w = world();
    let bytes = b_lt(&w, Says::Good, Says::Good);
    let signature = read(&bytes, std::slice::from_ref(&w.ca.certificate));
    let trust = signature.trust.expect("a standing");
    assert_eq!(trust.standing, Standing::Trusted, "{trust:?}");
    assert!(trust.attested_at.is_empty());
    let revocation = signature.revocation.expect("an answer");
    assert_eq!(revocation.basis, Basis::Claimed, "{revocation:?}");
    // And the control: the same bytes with the authority anchored.
    assert_eq!(
        read(&bytes, &anchors(&w)).trust.map(|t| t.standing),
        Some(Standing::TrustedAtTimestamp)
    );
}

#[test]
fn an_authority_the_document_shows_revoked_leaves_the_signer_judged_now() {
    if !has_store() {
        return;
    }
    let w = world();
    let bytes = b_lt(
        &w,
        Says::Good,
        Says::Revoked {
            at: G - DAY,
            reason: Some(1),
        },
    );
    let signature = read(&bytes, &anchors(&w));
    let stamp = signature.timestamp.as_ref().expect("a timestamp");
    assert_eq!(
        stamp.revocation.as_ref().map(|r| r.standing),
        Some(Status::Revoked)
    );
    let trust = signature.trust.expect("a standing");
    assert_eq!(
        (trust.standing, trust.attested_at.as_str()),
        (Standing::Trusted, "")
    );
    assert_eq!(signature.revocation.map(|r| r.basis), Some(Basis::Claimed));
}

#[test]
fn an_authority_whose_issuer_is_not_in_the_document_has_revocation_data_that_cannot_be_checked() {
    // The token carries the authority's certificate alone; a /DSS with the
    // response and without the root that signed it cannot be checked.
    if !has_store() {
        return;
    }
    let w = world();
    let bytes = with_dss(
        &signed(&w.signer, &[], G, &w.tsa),
        std::slice::from_ref(&w.ca.certificate),
        &[ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good)],
        &[],
    );
    let stamp = read(&bytes, &anchors(&w)).timestamp.expect("a timestamp");
    let revocation = stamp.revocation.expect("an answer");
    assert_eq!(
        (revocation.standing, revocation.why),
        (Status::Unchecked, Some(Gap::Issuer))
    );
}

#[test]
fn a_time_outside_the_authoritys_own_dates_is_not_used() {
    // 2017, before the test authority's certificate begins: a trusted
    // authority stating a time it was not in force at. Judged now, the signer
    // is trusted; judged at that time it would not be in force either.
    if !has_store() {
        return;
    }
    let w = world();
    let bytes = signed(&w.signer, &[], 1_500_000_000, &w.tsa);
    let trust = read(&bytes, &anchors(&w)).trust.expect("a standing");
    assert_eq!(
        (trust.standing, trust.attested_at.as_str()),
        (Standing::Trusted, "")
    );
}

#[test]
fn a_signer_not_in_force_at_the_attested_time_is_not_trusted() {
    if !has_store() {
        return;
    }
    let w = world();
    // In force from 2026-09-20, which covers the signing and not the time
    // the timestamp attests.
    let signer = w.ca.issue_dated(
        "tpdf test late signer",
        0x69,
        9,
        G + 19 * DAY,
        NOW + 365 * DAY,
    );
    let bytes = signed(&signer, &[], G, &w.tsa);
    let trust = read(&bytes, &anchors(&w)).trust.expect("a standing");
    assert_eq!(
        (trust.standing, trust.why),
        (Standing::Untrusted, Some(Doubt::NotInForce)),
        "{trust:?}"
    );
    assert_eq!(trust.attested_at, "2026-09-01 00:00:00 UTC");
}

#[test]
fn a_revocation_after_the_attested_time_does_not_fail_strict_and_one_before_does() {
    if !has_store() {
        return;
    }
    let w = world();
    for (at, after) in [(G + DAY, true), (G - DAY, false)] {
        let bytes = b_lt(
            &w,
            Says::Revoked {
                at,
                reason: Some(1),
            },
            Says::Good,
        );
        let signature = read(&bytes, &anchors(&w));
        let revocation = signature.revocation.clone().expect("an answer");
        assert_eq!(revocation.standing, Status::Revoked);
        assert_eq!(revocation.after_moment, after, "{revocation:?}");
        let report = crate::cli::verify::signature_report(&signature);
        assert_eq!(crate::cli::verify::passes_strict(&report), after);
    }
}

/// The signer under an intermediate `w.ca` issued, the intermediate in the
/// signature, and a `/DSS` holding a good response for the signer, one saying
/// `middle` for the intermediate when given, and a good one for the authority.
fn b_lt_under_an_intermediate(w: &World, middle: Option<Says>) -> Vec<u8> {
    let issuing = w.ca.intermediate("tpdf test issuing authority", 0x6c, 0x31);
    let signer = issuing.issue("tpdf test signer under it", 0x6d, 8, None);
    let bytes = signed(
        &signer,
        std::slice::from_ref(&issuing.certificate),
        G,
        &w.tsa,
    );
    let mut responses = vec![
        ocsp(&signer.certificate, &issuing, Says::Good),
        ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good),
    ];
    if let Some(says) = middle {
        responses.push(ocsp(&issuing.certificate, &w.ca, says));
    }
    with_dss(
        &bytes,
        &[w.ca.certificate.clone(), w.tsa.root.clone()],
        &responses,
        &[],
    )
}

#[test]
fn the_certificate_above_the_signers_is_judged_at_the_same_moment() {
    if !has_store() {
        return;
    }
    let w = world();
    let signature = read(
        &b_lt_under_an_intermediate(&w, Some(Says::Good)),
        &anchors(&w),
    );
    assert_eq!(
        signature.trust.as_ref().map(|t| t.standing),
        Some(Standing::TrustedAtTimestamp)
    );
    let chain = signature.revocation_chain.clone().expect("a chain");
    assert_eq!(
        (chain.standing, chain.decided_by, chain.certificates.len()),
        (Status::Good, None, 2),
        "{chain:?}"
    );
    assert_eq!(
        chain.certificates[1].subject_cn,
        "tpdf test issuing authority"
    );
    assert_eq!(chain.certificates[1].revocation.basis, Basis::Attested);
    // Its first certificate is the answer beside it, not a second judgement.
    assert_eq!(
        Some(&chain.certificates[0].revocation),
        signature.revocation.as_ref()
    );
    // And the authority's, issued straight by its root: a chain of one.
    let stamp = signature.timestamp.expect("a timestamp");
    let above = stamp.revocation_chain.expect("the authority's chain");
    assert_eq!(
        (above.standing, above.certificates.len()),
        (Status::Good, 1)
    );
}

#[test]
fn a_certificate_above_the_signers_revoked_before_the_attested_time_fails_strict() {
    if !has_store() {
        return;
    }
    let w = world();
    for (at, after) in [(G + DAY, true), (G - DAY, false)] {
        let bytes = b_lt_under_an_intermediate(
            &w,
            Some(Says::Revoked {
                at,
                reason: Some(2),
            }),
        );
        let signature = read(&bytes, &anchors(&w));
        assert_eq!(
            signature.revocation.as_ref().map(|r| r.standing),
            Some(Status::Good),
            "the signer's own certificate is good: only the chain can fail it"
        );
        let chain = signature.revocation_chain.clone().expect("a chain");
        assert_eq!(
            (chain.standing, chain.decided_by, chain.after_moment),
            (Status::Revoked, Some(1), after),
            "{chain:?}"
        );
        let report = crate::cli::verify::signature_report(&signature);
        assert_eq!(crate::cli::verify::passes_strict(&report), after, "{at}");
    }
}

#[test]
fn a_certificate_above_the_signers_with_no_data_is_not_read_as_good() {
    if !has_store() {
        return;
    }
    let w = world();
    let signature = read(&b_lt_under_an_intermediate(&w, None), &anchors(&w));
    let chain = signature.revocation_chain.clone().expect("a chain");
    assert_eq!(
        (chain.standing, chain.decided_by),
        (Status::None, Some(1)),
        "{chain:?}"
    );
    // Not checked is not a failure: `--strict` asks what speaks against.
    let report = crate::cli::verify::signature_report(&signature);
    assert!(crate::cli::verify::passes_strict(&report));
}

#[test]
fn the_dss_certificates_complete_a_chain_the_signature_does_not_carry() {
    if !has_store() {
        return;
    }
    let w = world();
    let middle = w.ca.intermediate("tpdf test intermediate", 0x6a, 0x30);
    let signer = middle.issue("tpdf test signer under an intermediate", 0x6b, 7, None);
    let bytes = signed(&signer, &[], G, &w.tsa);
    let without = read(&bytes, &anchors(&w)).trust.expect("a standing");
    assert_eq!(
        (without.standing, without.why),
        (Standing::Untrusted, Some(Doubt::Incomplete)),
        "the control: the signature alone does not reach the root"
    );
    let with = with_dss(
        &bytes,
        std::slice::from_ref(&middle.certificate),
        &[ocsp(&signer.certificate, &middle, Says::Good)],
        &[],
    );
    let signature = read(&with, &anchors(&w));
    assert_eq!(
        signature.trust.map(|t| t.standing),
        Some(Standing::TrustedAtTimestamp)
    );
    // The issuer the response names is found in the /DSS too.
    assert_eq!(signature.revocation.map(|r| r.standing), Some(Status::Good));
}

#[test]
fn a_dss_stream_that_inflates_past_its_bound_is_counted_and_turns_none_into_not_checked() {
    use std::io::Write as _;
    let w = world();
    let bytes = signed(&w.signer, &[], G, &w.tsa);
    let mut deflated = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    deflated
        .write_all(&vec![0u8; crate::revocation::MAX_RESPONSE_BYTES * 4])
        .expect("deflated");
    let bomb = deflated.finish().expect("deflated");
    let with = append_dss_stream(&bytes, bomb);
    let found = scan_at(&with, crate::trust::Anchors::Only(&anchors(&w)), NOW).expect("scanned");
    assert_eq!(found.limits.revocation_unread, 1);
    assert!(found.limits.any());
    if has_store() {
        let signature = found.signatures.iter().find(|s| s.signed).expect("signed");
        let revocation = signature.revocation.as_ref().expect("an answer");
        assert_eq!(
            (revocation.standing, revocation.why),
            (Status::Unchecked, Some(Gap::Unreadable))
        );
    }
}

/// `bytes` with a `/DSS` whose one `/OCSPs` entry is a Flate stream of
/// `deflated`.
fn append_dss_stream(bytes: &[u8], deflated: Vec<u8>) -> Vec<u8> {
    use lopdf::{dictionary, IncrementalDocument, Object, Stream};
    let prev = Document::load_mem(bytes).expect("a document");
    let root = prev
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("a catalog");
    let mut catalog = prev
        .get_object(root)
        .and_then(Object::as_dict)
        .expect("a catalog")
        .clone();
    let mut incremental = IncrementalDocument::create_from(bytes.to_vec(), prev);
    let doc = &mut incremental.new_document;
    let stream = doc.add_object(Stream::new(
        dictionary! { "Filter" => "FlateDecode" },
        deflated,
    ));
    let dss = doc.add_object(dictionary! { "OCSPs" => vec![Object::Reference(stream)] });
    catalog.set("DSS", dss);
    doc.set_object(root, catalog);
    let mut out = Vec::new();
    incremental.save_to(&mut out).expect("saved");
    out
}

#[test]
fn a_vri_entry_is_read_beside_the_top_level_arrays() {
    // A /VRI entry naming a response the top level does not.
    use lopdf::{dictionary, IncrementalDocument, Object, Stream};
    let w = world();
    let bytes = signed(&w.signer, &[], G, &w.tsa);
    let prev = Document::load_mem(&bytes).expect("a document");
    let root = prev
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("a catalog");
    let mut catalog = prev
        .get_object(root)
        .and_then(Object::as_dict)
        .expect("a catalog")
        .clone();
    let mut incremental = IncrementalDocument::create_from(bytes.clone(), prev);
    let doc = &mut incremental.new_document;
    let response = doc.add_object(Stream::new(
        dictionary! {},
        ocsp(
            &w.signer.certificate,
            &w.ca,
            Says::Revoked {
                at: G - DAY,
                reason: None,
            },
        ),
    ));
    let issuer = doc.add_object(Stream::new(dictionary! {}, w.ca.certificate.clone()));
    let entry = dictionary! {
        "OCSP" => vec![Object::Reference(response)],
        "Cert" => vec![Object::Reference(issuer)],
    };
    let dss = doc.add_object(dictionary! {
        "VRI" => dictionary! { "0123456789ABCDEF0123456789ABCDEF01234567" => entry },
    });
    catalog.set("DSS", dss);
    doc.set_object(root, catalog);
    let mut with = Vec::new();
    incremental.save_to(&mut with).expect("saved");
    let found = read_dss(&crate::encoding::load(&with, None).expect("loads"));
    assert_eq!((found.responses.len(), found.certificates.len()), (1, 1));
    if has_store() {
        assert_eq!(
            read(&with, &anchors(&w)).revocation.map(|r| r.standing),
            Some(Status::Revoked)
        );
    }
}

// ------------------------------------------------ the attested moment, alone

fn timestamp(verdict: crate::integrity::Verdict, standing: Standing, revoked: bool) -> Timestamp {
    Timestamp {
        integrity: Some(crate::integrity::Integrity {
            verdict,
            ..Default::default()
        }),
        trust: Some(crate::trust::Trust {
            standing,
            ..Default::default()
        }),
        attested: true,
        revocation: Some(crate::revocation::Revocation {
            standing: if revoked {
                Status::Revoked
            } else {
                Status::None
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn only_an_intact_token_from_a_trusted_unrevoked_authority_attests_a_moment() {
    use crate::integrity::Verdict;
    let tsa = TestTsa::new();
    let token = mint_with(Imprint::Sha256, &[0; 32], None, G, &tsa, &Faults::default());
    // The control: everything holds.
    assert_eq!(
        attested_moment(
            &timestamp(Verdict::Intact, Standing::Trusted, false),
            &token,
            G
        ),
        Some(G)
    );
    for (verdict, standing, revoked) in [
        (Verdict::Weak, Standing::Trusted, false),
        (Verdict::Broken, Standing::Trusted, false),
        (Verdict::Intact, Standing::Untrusted, false),
        (Verdict::Intact, Standing::Expired, false),
        (Verdict::Intact, Standing::Trusted, true),
    ] {
        assert_eq!(
            attested_moment(&timestamp(verdict, standing, revoked), &token, G),
            None,
            "{verdict:?} {standing:?} revoked {revoked}"
        );
    }
    // A certificate above the authority's, revoked: it vouches for nothing
    // below it, so the token attests no moment --- and the control, a chain
    // that is good.
    for (standing, attests) in [(Status::Revoked, None), (Status::Good, Some(G))] {
        let above = Timestamp {
            revocation_chain: Some(crate::revocation::chain::Chain {
                standing,
                decided_by: Some(1),
                ..Default::default()
            }),
            ..timestamp(Verdict::Intact, Standing::Trusted, false)
        };
        assert_eq!(attested_moment(&above, &token, G), attests, "{standing:?}");
    }
    // And a moment outside the authority's certificate's dates.
    assert_eq!(
        attested_moment(
            &timestamp(Verdict::Intact, Standing::Trusted, false),
            &token,
            crate::integrity::test_tsa::FROM - 1
        ),
        None
    );
}

#[test]
fn a_pdf_date_is_read_as_the_moment_it_names() {
    assert_eq!(pdf_date_seconds("D:20260901000000Z"), Some(G));
    assert_eq!(pdf_date_seconds("D:20260901020000+02'00'"), Some(G));
    assert_eq!(pdf_date_seconds("D:20260831220000-02'00"), Some(G));
    assert_eq!(pdf_date_seconds("D:2026"), Some(1_767_225_600));
    assert_eq!(pdf_date_seconds("D:20261301"), None, "month 13");
    assert_eq!(pdf_date_seconds("yesterday"), None);
}

// ------------------------------------------ a B-LT document another tool wrote

#[test]
fn a_b_lt_document_pyhanko_wrote_reads_good_at_the_attested_time() {
    // `incr-lt.pdf`: pyHanko's /DSS, responses and a list `cryptography` made.
    // Its root is carried in the /DSS and anchored here, as a reader who
    // trusted it would have it; through the system store it is a stranger.
    let Ok(bytes) = std::fs::read("../testdata/incr-lt.pdf") else {
        crate::fixtures::missing("[SKIP] incr-lt.pdf: not generated");
        return;
    };
    let material = read_dss(&crate::encoding::load(&bytes, None).expect("loads"));
    assert!(
        material.responses.len() >= 2 && !material.lists.is_empty(),
        "the /DSS carries responses and a list: {} and {}",
        material.responses.len(),
        material.lists.len()
    );
    assert_eq!((material.unread, material.dropped), (0, 0));
    let roots: Vec<Vec<u8>> = material
        .certificates
        .iter()
        .filter(|der| {
            <x509_cert::Certificate as der::Decode>::from_der(der).is_ok_and(|c| {
                use der::Encode;
                c.tbs_certificate.subject.to_der().ok() == c.tbs_certificate.issuer.to_der().ok()
            })
        })
        .cloned()
        .collect();
    assert_eq!(roots.len(), 1, "one self-issued root in the /DSS");
    let now = now_seconds();
    let found = scan_at(&bytes, crate::trust::Anchors::Only(&roots), now).expect("scanned");
    assert_eq!(
        found.limits.timestamps_unread, 0,
        "pyHanko's token, microseconds and all"
    );
    let signature = found.signatures.iter().find(|s| s.signed).expect("signed");
    let revocation = signature.revocation.as_ref().expect("an answer");
    assert_eq!(revocation.standing, Status::Good, "{revocation:?}");
    let stamp = signature.timestamp.as_ref().expect("a timestamp");
    assert_eq!(
        stamp.revocation.as_ref().map(|r| (r.standing, r.basis)),
        Some((Status::Good, Basis::Stated)),
        "{stamp:?}"
    );
    if has_store() {
        let trust = signature.trust.as_ref().expect("a standing");
        assert_eq!(trust.standing, Standing::TrustedAtTimestamp, "{trust:?}");
        assert_eq!(revocation.basis, Basis::Attested);
    }
    // Through the system store the root is nobody's: the authority is not
    // trusted, so the signer's revocation is judged at its own claimed date.
    let system = scan_at(&bytes, crate::trust::Anchors::System, now).expect("scanned");
    let signature = system.signatures.iter().find(|s| s.signed).expect("signed");
    assert_eq!(
        signature.revocation.as_ref().map(|r| (r.standing, r.basis)),
        Some((Status::Good, Basis::Claimed))
    );
}

#[test]
fn a_gen_time_with_fractional_seconds_is_read_to_the_second() {
    // RFC 3161 permits a fraction; `der`'s GeneralizedTime does not, and the
    // token was counted unreadable until 2026-09-28.
    let tst_info = |time: &[u8]| {
        let mut fields = Vec::new();
        fields.extend([0x02, 0x01, 0x01]); // version
        fields.extend([0x06, 0x03, 0x2a, 0x03, 0x04]); // policy
        fields.extend([0x30, 0x00]); // imprint, opaque here
        fields.extend([0x02, 0x01, 0x07]); // serial
        fields.push(0x18);
        fields.push(time.len() as u8);
        fields.extend_from_slice(time);
        let mut out = vec![0x30, fields.len() as u8];
        out.extend(fields);
        out
    };
    let at = |time: &[u8]| gen_time_of(&tst_info(time)).map(|t| t.unix_duration().as_secs());
    assert_eq!(at(b"20260901000000Z"), Some(G));
    assert_eq!(at(b"20260901000000.638502Z"), Some(G));
    assert_eq!(at(b"20260901000000.5Z"), Some(G));
    // Not DER: a trailing zero, an empty fraction, no zone, a local offset.
    for refused in [
        &b"20260901000000.50Z"[..],
        b"20260901000000.Z",
        b"20260901000000",
        b"20260901000000+0200",
        b"202609010000Z",
    ] {
        assert_eq!(at(refused), None, "{}", String::from_utf8_lossy(refused));
    }
}

/// Writes B-LT documents with minted revocation data, and the two roots, into
/// `TPDF_REVOCATION_OUT`, for pyHanko's long-term validation to judge beside
/// tpdf's answers (`BUILD.md`, *Revocation*). An instrument, not a gate.
#[test]
#[ignore = "an instrument: writes documents for pyHanko, run by hand"]
fn write_b_lt_documents_for_pyhanko() {
    let Ok(out) = std::env::var("TPDF_REVOCATION_OUT") else {
        println!("[SKIP] set TPDF_REVOCATION_OUT to a directory");
        return;
    };
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out).expect("the directory");
    let w = world();
    std::fs::write(out.join("root-signer.der"), &w.ca.certificate).expect("written");
    std::fs::write(out.join("root-tsa.der"), &w.tsa.root).expect("written");
    let revoked = |at| Says::Revoked {
        at,
        reason: Some(1),
    };
    let stale = {
        let bytes = signed(&w.signer, &[], G, &w.tsa);
        with_dss(
            &bytes,
            &[w.ca.certificate.clone(), w.tsa.root.clone()],
            &[
                mint_ocsp(
                    &w.signer.certificate,
                    &w.ca,
                    Says::Good,
                    G - 9 * DAY,
                    Some(G - 2 * DAY),
                    Responder::Issuer,
                    &OcspFaults::default(),
                ),
                ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good),
            ],
            &[],
        )
    };
    for (name, bytes) in [
        ("good", b_lt(&w, Says::Good, Says::Good)),
        ("revoked-before", b_lt(&w, revoked(G - DAY), Says::Good)),
        ("revoked-after", b_lt(&w, revoked(G + DAY), Says::Good)),
        ("stale", stale),
        ("none", signed(&w.signer, &[], G, &w.tsa)),
        (
            "intermediate-good",
            b_lt_under_an_intermediate(&w, Some(Says::Good)),
        ),
        (
            "intermediate-revoked-before",
            b_lt_under_an_intermediate(&w, Some(revoked(G - DAY))),
        ),
        (
            "intermediate-revoked-after",
            b_lt_under_an_intermediate(&w, Some(revoked(G + DAY))),
        ),
    ] {
        std::fs::write(out.join(format!("{name}.pdf")), &bytes).expect("written");
        let anchors = anchors(&w);
        let signature = read(&bytes, &anchors);
        println!(
            "{name}: tpdf trust {:?} revocation {:?}",
            signature.trust.map(|t| t.standing),
            signature
                .revocation
                .map(|r| (r.standing, r.why, r.after_moment))
        );
        println!(
            "{name}: tpdf chain {:?}",
            signature
                .revocation_chain
                .map(|c| (c.standing, c.decided_by, c.after_moment))
        );
    }
}

#[test]
fn a_compressed_dss_stream_is_decoded_and_a_vri_past_its_bound_is_counted() {
    use lopdf::{dictionary, Dictionary, IncrementalDocument, Object, Stream};
    use std::io::Write as _;
    let w = world();
    let bytes = signed(&w.signer, &[], G, &w.tsa);
    let response = ocsp(&w.signer.certificate, &w.ca, Says::Good);
    let mut deflated = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    deflated.write_all(&response).expect("deflated");
    let deflated = deflated.finish().expect("deflated");

    let prev = Document::load_mem(&bytes).expect("a document");
    let root = prev
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("a catalog");
    let mut catalog = prev
        .get_object(root)
        .and_then(Object::as_dict)
        .expect("a catalog")
        .clone();
    let mut incremental = IncrementalDocument::create_from(bytes.clone(), prev);
    let doc = &mut incremental.new_document;
    let stream = doc.add_object(Stream::new(
        dictionary! { "Filter" => "FlateDecode" },
        deflated,
    ));
    // One /VRI entry more than is walked, each naming nothing.
    let mut vri = Dictionary::new();
    for n in 0..=crate::revocation::MAX_VRI {
        vri.set(format!("{n:040X}"), dictionary! {});
    }
    let dss = doc.add_object(dictionary! {
        "OCSPs" => vec![Object::Reference(stream)],
        "VRI" => vri,
    });
    catalog.set("DSS", dss);
    doc.set_object(root, catalog);
    let mut with = Vec::new();
    incremental.save_to(&mut with).expect("saved");
    let found = read_dss(&crate::encoding::load(&with, None).expect("loads"));
    assert_eq!(
        found.responses.len(),
        1,
        "the Flate stream is decoded, not read raw"
    );
    assert_eq!((found.unread, found.dropped), (0, 1));
}

/// `plain_pdf()` with a `/DSS` whose `/CRLs` names `items` --- each the index
/// of one of `streams` --- and whose one `/VRI` entry names them again.
fn with_crls(streams: &[Vec<u8>], items: &[usize]) -> Document {
    use lopdf::{dictionary, Object, Stream};
    let mut document = Document::load_mem(&plain_pdf()).expect("a document");
    let ids: Vec<lopdf::ObjectId> = streams
        .iter()
        .map(|deflated| {
            document.add_object(Stream::new(
                dictionary! { "Filter" => "FlateDecode" },
                deflated.clone(),
            ))
        })
        .collect();
    let named: Vec<Object> = items.iter().map(|&n| Object::Reference(ids[n])).collect();
    let dss = document.add_object(dictionary! {
        "CRLs" => named.clone(),
        "VRI" => dictionary! { "0000" => dictionary! { "CRL" => named } },
    });
    let root = document
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("a catalog");
    document
        .get_object_mut(root)
        .and_then(Object::as_dict_mut)
        .expect("a catalog")
        .set("DSS", dss);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("saved");
    crate::encoding::load(&bytes, None).expect("loads")
}

/// `bytes` of zeros, deflated: a stream of a few kilobytes that inflates to
/// `bytes`.
fn inflating(bytes: usize) -> Vec<u8> {
    use std::io::Write as _;
    let mut deflated = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    deflated.write_all(&vec![0; bytes]).expect("deflated");
    deflated.finish().expect("deflated")
}

/// [`read_dss`], counting the streams decoded and the bytes they inflated to.
fn decoding(document: &Document) -> (crate::revocation::Material, usize, usize) {
    let (mut calls, mut inflated) = (0, 0);
    let material = read_dss_with(document, &mut |stream, bound| {
        calls += 1;
        let content = dss_content(stream, bound);
        inflated += content.as_ref().map_or(0, Vec::len);
        content
    });
    (material, calls, inflated)
}

/// The counts bound the decoding, not only what is kept: one stream a
/// thousand times over is decoded once, and past the most lists kept nothing
/// more is decoded --- each counted dropped, which the scan reports.
#[test]
fn the_dss_counts_bound_the_decoding_and_what_they_drop_is_counted() {
    const MIB: usize = 1024 * 1024;
    // One small stream inflating to a mebibyte, named a thousand times in
    // /CRLs and a thousand more under /VRI.
    let bomb = with_crls(&[inflating(MIB)], &[0; 1000]);
    let (material, calls, inflated) = decoding(&bomb);
    assert_eq!((calls, inflated), (1, MIB), "decoded once");
    assert_eq!(
        (material.lists.len(), material.unread, material.dropped),
        (1, 0, 0),
        "kept once, and nothing dropped: the rest are the same object"
    );

    // Forty distinct streams: no more are decoded than lists are kept.
    let many: Vec<Vec<u8>> = (0..40).map(|n| inflating(1024 + n)).collect();
    let items: Vec<usize> = (0..40).collect();
    let document = with_crls(&many, &items);
    let (material, calls, _) = decoding(&document);
    let kept = crate::revocation::MAX_LISTS;
    assert_eq!(calls, kept);
    assert_eq!(
        (material.lists.len(), material.unread, material.dropped),
        (kept, 0, 40 - kept)
    );
    // And the scan's limits say so.
    let mut limits = Limits::default();
    let _ = Judging::of(
        &document,
        &mut limits,
        crate::trust::Anchors::Only(&[]),
        NOW,
    );
    assert_eq!(
        (limits.revocation_unread, limits.revocation_dropped),
        (0, 40 - kept)
    );
}

// ------------------------------------------------ archive timestamps (B-LTA)

/// `bytes` with a document timestamp appended, its token minted by `tsa` at
/// `at` --- over the range, or over other bytes when `broken`.
fn archived(bytes: &[u8], tsa: &TestTsa, at: u64, broken: bool) -> Vec<u8> {
    let unsigned =
        crate::sign_prepare::prepare_document_timestamp(bytes.to_vec(), None).expect("prepared");
    let imprint = if broken {
        Imprint::Sha256.digest(b"not this document")
    } else {
        unsigned.digest.clone()
    };
    let token = mint_with(Imprint::Sha256, &imprint, None, at, tsa, &Faults::default());
    if broken {
        // `seal_document_timestamp` refuses what it would not call intact, so
        // the damaged one is spliced by hand, as a hostile writer would.
        let mut update = unsigned.update.clone();
        let hole = usize::try_from(unsigned.range[1]).expect("offset") + 1 - bytes.len();
        for (index, byte) in token.iter().enumerate() {
            update[hole + index * 2..hole + index * 2 + 2]
                .copy_from_slice(format!("{byte:02X}").as_bytes());
        }
        let mut out = bytes.to_vec();
        out.extend_from_slice(&update);
        return out;
    }
    crate::sign_cms::seal_document_timestamp(bytes.to_vec(), unsigned, &token).expect("sealed")
}

/// Every signed field, in the document's order.
fn read_all(bytes: &[u8], anchors: &[Vec<u8>]) -> Vec<Signature> {
    scan_at(bytes, crate::trust::Anchors::Only(anchors), NOW)
        .expect("scanned")
        .signatures
        .into_iter()
        .filter(|s| s.signed)
        .collect()
}

/// A signature timestamped at `G` by an authority whose certificate ran out
/// ten days later --- before `NOW`.
fn stamped_by_a_lapsed_authority(w: &World) -> Vec<u8> {
    let lapsed = TestTsa::dated(crate::integrity::test_tsa::FROM, G + 10 * DAY);
    signed(&w.signer, &[], G, &lapsed)
}

#[test]
fn a_lapsed_authority_attests_nothing_without_an_archive_timestamp() {
    // The control for the tests below: B-T, read after the authority's
    // certificate ran out.
    if !has_store() {
        return;
    }
    let w = world();
    let signature = read(&stamped_by_a_lapsed_authority(&w), &anchors(&w));
    let stamp = signature.timestamp.expect("a timestamp");
    assert_eq!(
        stamp.trust.map(|t| t.standing),
        Some(Standing::Expired),
        "judged now, the authority's certificate has run out"
    );
    assert_eq!(
        signature.revocation.map(|r| r.basis),
        Some(Basis::Claimed),
        "so the signer is judged at its own date"
    );
}

#[test]
fn an_archive_timestamp_lets_a_lapsed_authority_attest_the_time_it_stated() {
    if !has_store() {
        return;
    }
    let w = world();
    let bytes = archived(
        &stamped_by_a_lapsed_authority(&w),
        &w.tsa,
        G + 5 * DAY,
        false,
    );
    let [signature, archive] = read_all(&bytes, &anchors(&w))
        .try_into()
        .expect("two fields");
    assert_eq!(archive.kind, "ETSI.RFC3161");
    assert_eq!(
        archive.trust.as_ref().map(|t| t.standing),
        Some(Standing::Trusted),
        "the archive's own authority, now"
    );
    let stamp = signature.timestamp.as_ref().expect("a timestamp");
    let trust = stamp.trust.as_ref().expect("the authority's standing");
    assert_eq!(
        (trust.standing, trust.attested_at.as_str()),
        (Standing::TrustedAtTimestamp, "2026-09-06 00:00:00 UTC"),
        "{trust:?}"
    );
    // And so the signer is judged at the time the token states, as B-T was
    // before its authority ran out.
    assert_eq!(
        signature
            .trust
            .as_ref()
            .map(|t| (t.standing, t.attested_at.as_str())),
        Some((Standing::TrustedAtTimestamp, "2026-09-01 00:00:00 UTC"))
    );
    assert_eq!(signature.revocation.map(|r| r.basis), Some(Basis::Attested));
}

#[test]
fn an_archive_timestamp_made_after_the_authority_lapsed_proves_nothing_for_it() {
    // Twenty days on, the authority's certificate had already run out: the
    // archive shows the token existed then, and at that moment nothing vouched
    // for it.
    if !has_store() {
        return;
    }
    let w = world();
    let bytes = archived(
        &stamped_by_a_lapsed_authority(&w),
        &w.tsa,
        G + 20 * DAY,
        false,
    );
    let [signature, _] = read_all(&bytes, &anchors(&w))
        .try_into()
        .expect("two fields");
    let stamp = signature.timestamp.expect("a timestamp");
    assert_ne!(
        stamp.trust.as_ref().map(|t| t.standing),
        Some(Standing::TrustedAtTimestamp),
        "{:?}",
        stamp.trust
    );
    assert_eq!(signature.revocation.map(|r| r.basis), Some(Basis::Claimed));
}

#[test]
fn an_archive_timestamp_that_does_not_attest_is_no_archive() {
    if !has_store() {
        return;
    }
    let w = world();
    let lapsed = stamped_by_a_lapsed_authority(&w);
    // Its token over other bytes; a sound one from an authority whose
    // certificate is not issued for timestamping; and a weak one, stamping a
    // SHA-1 of the range, which does not show the time belongs to it.
    let unfit = TestTsa::with_purposes(None);
    for (what, bytes) in [
        ("broken", archived(&lapsed, &w.tsa, G + 5 * DAY, true)),
        ("unfit", archived(&lapsed, &unfit, G + 5 * DAY, false)),
        ("weak", archived_weak(&lapsed, &w.tsa, G + 5 * DAY)),
    ] {
        let [signature, archive] = read_all(&bytes, &anchors(&w))
            .try_into()
            .expect("two fields");
        assert!(archive.integrity.is_some(), "{what}: the archive was read");
        assert_eq!(
            signature.revocation.map(|r| r.basis),
            Some(Basis::Claimed),
            "{what}: judged at its own date"
        );
    }
}

/// [`archived`], its token stamping the range's SHA-1: sound, and weak.
fn archived_weak(bytes: &[u8], tsa: &TestTsa, at: u64) -> Vec<u8> {
    let unsigned =
        crate::sign_prepare::prepare_document_timestamp(bytes.to_vec(), None).expect("prepared");
    let mut whole = bytes.to_vec();
    whole.extend_from_slice(&unsigned.update);
    let [_, hole, rest, len] = unsigned.range.map(|n| usize::try_from(n).expect("offset"));
    let mut covered = whole[..hole].to_vec();
    covered.extend_from_slice(&whole[rest..rest + len]);
    let token = mint_with(
        Imprint::Sha1,
        &Imprint::Sha1.digest(&covered),
        None,
        at,
        tsa,
        &Faults::default(),
    );
    let mut update = unsigned.update.clone();
    let start = hole + 1 - bytes.len();
    for (index, byte) in token.iter().enumerate() {
        update[start + index * 2..start + index * 2 + 2]
            .copy_from_slice(format!("{byte:02X}").as_bytes());
    }
    let mut out = bytes.to_vec();
    out.extend_from_slice(&update);
    out
}

#[test]
fn an_archive_moment_is_the_earliest_covering_one() {
    let archives = [
        Archive { end: 900, at: 30 },
        Archive { end: 500, at: 10 },
        Archive { end: 700, at: 20 },
    ];
    // A range ending at 600 is covered by the two reaching further; the
    // earlier of them is the moment.
    assert_eq!(archive_moment(&archives, 600), Some(20));
    // One reaching exactly as far covers nothing past it: it is that range.
    assert_eq!(archive_moment(&archives, 900), None);
    assert_eq!(archive_moment(&archives, 100), Some(10));
}

#[test]
fn a_date_this_module_wrote_reads_back_as_the_moment_it_names() {
    assert_eq!(utc_seconds("2026-09-01 00:00:00 UTC"), Some(G));
    assert_eq!(
        utc_seconds(&crate::revocation::format_time(G + 3_723)),
        Some(G + 3_723)
    );
    for bad in [
        "",
        "2026-09-01 00:00:00",
        "2026-13-01 00:00:00 UTC",
        "2026-09-01 UTC",
    ] {
        assert_eq!(utc_seconds(bad), None, "{bad}");
    }
}

// ---- A certificate added to the unsigned set does not choose the chain ----

/// The name and key of `real`, issued by `theirs`, as somebody holding the file
/// adds it to the signature's unsigned certificate set.
fn twins(theirs: &TestCa, real: &TestCa) -> [Vec<u8>; 2] {
    [theirs.cross(real, 1), theirs.cross_no_check(real, 2)]
}

/// The signer under `issuing`, with `added` in the signature beside it, and a
/// `/DSS` holding every response in `extra` and the signer's and authority's.
fn with_added(
    w: &World,
    issuing: &TestCa,
    signer: &Issued,
    added: &[Vec<u8>],
    middle: Says,
    extra: &[Vec<u8>],
) -> Vec<u8> {
    let mut chain = added.to_vec();
    chain.push(issuing.certificate.clone());
    let bytes = signed(signer, &chain, G, &w.tsa);
    let mut responses = vec![
        ocsp(&signer.certificate, issuing, Says::Good),
        ocsp(&issuing.certificate, &w.ca, middle),
        ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good),
    ];
    responses.extend_from_slice(extra);
    with_dss(
        &bytes,
        &[w.ca.certificate.clone(), w.tsa.root.clone()],
        &responses,
        &[],
    )
}

#[test]
fn a_twin_of_a_revoked_issuer_added_to_the_signature_does_not_hide_the_revocation() {
    if !has_store() {
        return;
    }
    let w = world();
    let issuing = w.ca.intermediate("tpdf test issuing authority", 0x6c, 0x31);
    let signer = issuing.issue("tpdf test signer under it", 0x6d, 8, None);
    let theirs = TestCa::new("tpdf test injected root", 0x5a);
    let revoked = Says::Revoked {
        at: G - DAY,
        reason: Some(2),
    };
    for twin in twins(&theirs, &issuing) {
        // Their own root says their twin is good, which it is theirs to say.
        let bytes = with_added(
            &w,
            &issuing,
            &signer,
            &[twin.clone(), theirs.certificate.clone()],
            revoked,
            &[ocsp(&twin, &theirs, Says::Good)],
        );
        let signature = read(&bytes, &anchors(&w));
        assert_eq!(
            signature.trust.as_ref().map(|t| t.standing),
            Some(Standing::TrustedAtTimestamp),
            "the attack's premise: the store still builds the real chain"
        );
        let chain = signature.revocation_chain.clone().expect("a chain");
        assert_eq!(
            (chain.standing, chain.after_moment, chain.decided_by),
            (Status::Revoked, false, Some(1)),
            "{chain:?}"
        );
        assert_eq!(chain.certificates[1].serial, "31");
        let report = crate::cli::verify::signature_report(&signature);
        assert!(!crate::cli::verify::passes_strict(&report));
    }
}

#[test]
fn a_twin_added_to_a_clean_signature_changes_nothing_it_reports() {
    if !has_store() {
        return;
    }
    let w = world();
    let issuing = w.ca.intermediate("tpdf test issuing authority", 0x6c, 0x31);
    let signer = issuing.issue("tpdf test signer under it", 0x6d, 8, None);
    let theirs = TestCa::new("tpdf test injected root", 0x5a);
    let plain = read(
        &with_added(&w, &issuing, &signer, &[], Says::Good, &[]),
        &anchors(&w),
    );
    for twin in twins(&theirs, &issuing) {
        let added = [twin, theirs.certificate.clone()];
        let bytes = with_added(&w, &issuing, &signer, &added, Says::Good, &[]);
        let signature = read(&bytes, &anchors(&w));
        assert_eq!(signature.revocation_chain, plain.revocation_chain);
    }
}

#[test]
fn a_twin_added_to_a_token_does_not_hide_a_revoked_issuer_above_the_authority() {
    if !has_store() {
        return;
    }
    use crate::integrity::test_tsa::Published;
    let w = world();
    let root = TestCa::of_tsa();
    let issuing = root.intermediate("tpdf test timestamp issuing authority", 0x54, 4);
    let genuine = TestTsa::under(&issuing, &Published::default());
    let theirs = TestCa::new("tpdf test injected root", 0x5a);
    let revoked = Says::Revoked {
        at: G - DAY,
        reason: Some(2),
    };
    for twin in twins(&theirs, &issuing) {
        let mut tampered = genuine.clone();
        tampered.carried.insert(0, theirs.certificate.clone());
        tampered.carried.insert(0, twin.clone());
        let bytes = signed(&w.signer, &[], G, &tampered);
        let bytes = with_dss(
            &bytes,
            &[w.ca.certificate.clone(), root.certificate.clone()],
            &[
                ocsp(&w.signer.certificate, &w.ca, Says::Good),
                ocsp(&genuine.certificate, &issuing, Says::Good),
                ocsp(&issuing.certificate, &root, revoked),
                ocsp(&twin, &theirs, Says::Good),
            ],
            &[],
        );
        let signature = read(
            &bytes,
            &[w.ca.certificate.clone(), root.certificate.clone()],
        );
        let stamp = signature.timestamp.as_ref().expect("a timestamp");
        assert_eq!(
            stamp.trust.as_ref().map(|t| t.standing),
            Some(Standing::Trusted),
            "the attack's premise: the store still builds the real chain"
        );
        let chain = stamp.revocation_chain.clone().expect("a chain");
        assert_eq!(
            (chain.standing, chain.decided_by),
            (Status::Revoked, Some(1)),
            "{chain:?}"
        );
        // So the time it states attests nothing: the signer is judged now.
        assert_eq!(
            signature.trust.as_ref().map(|t| t.standing),
            Some(Standing::Trusted)
        );
    }
}

/// `real` with the second key that verifies the ECDSA signature on `issued`,
/// recoverable by anybody from that signature: `real`'s name, not `real`'s key,
/// and not `real`'s signature, which nothing asks about.
fn second_key_twin(real: &[u8], issued: &[u8]) -> Vec<u8> {
    use der::{Decode as _, Encode as _};
    use p256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    use sha2::{Digest as _, Sha256};
    let subject = x509_cert::Certificate::from_der(issued).expect("the certificate");
    let tbs = subject.tbs_certificate.to_der().expect("its body");
    let digest = Sha256::digest(&tbs);
    let signature =
        Signature::from_der(subject.signature.as_bytes().expect("bits")).expect("a signature");
    let mut twin = x509_cert::Certificate::from_der(real).expect("the issuer");
    let genuine = twin
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .raw_bytes()
        .to_vec();
    let other = [0u8, 1]
        .into_iter()
        .filter_map(|id| {
            VerifyingKey::recover_from_prehash(
                &digest,
                &signature,
                RecoveryId::from_byte(id).expect("an id"),
            )
            .ok()
        })
        .map(|key| key.to_sec1_point(false).as_bytes().to_vec())
        .find(|key| *key != genuine)
        .expect("a second key");
    twin.tbs_certificate
        .subject_public_key_info
        .subject_public_key = der::asn1::BitString::from_bytes(&other).expect("bits");
    twin.to_der().expect("encoded")
}

#[test]
fn a_second_key_twin_does_not_lose_a_revocation_of_the_signers_own_certificate() {
    if !has_store() {
        return;
    }
    let w = world();
    let issuing = w.ca.intermediate("tpdf test issuing authority", 0x6c, 0x31);
    let signer = issuing.issue("tpdf test signer under it", 0x6d, 8, None);
    let twin = second_key_twin(&issuing.certificate, &signer.certificate);
    let bytes = signed(&signer, &[twin, issuing.certificate.clone()], G, &w.tsa);
    let bytes = with_dss(
        &bytes,
        &[w.ca.certificate.clone(), w.tsa.root.clone()],
        &[
            ocsp(
                &signer.certificate,
                &issuing,
                Says::Revoked {
                    at: G - DAY,
                    reason: Some(1),
                },
            ),
            ocsp(&issuing.certificate, &w.ca, Says::Good),
            ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good),
        ],
        &[],
    );
    let signature = read(&bytes, &anchors(&w));
    assert_eq!(
        signature.trust.as_ref().map(|t| t.standing),
        Some(Standing::TrustedAtTimestamp),
        "the attack's premise: the store still builds the real chain"
    );
    assert_eq!(
        signature.revocation.as_ref().map(|r| r.standing),
        Some(Status::Revoked)
    );
    let report = crate::cli::verify::signature_report(&signature);
    assert!(!crate::cli::verify::passes_strict(&report));
}

// ---- The rules of judging along the store's chain ----

/// A chain under `w.ca` with `depth` authorities between it and the signer,
/// all carried by the signature, with good answers for the signer and the
/// authority and none for the authorities in between.
fn deep(w: &World, depth: u8) -> Vec<u8> {
    let mut authorities = vec![w.ca.intermediate("tpdf test authority 0", 0x70, 0x40)];
    for at in 1..depth {
        let above = authorities.last().expect("one");
        let name = format!("tpdf test authority {at}");
        authorities.push(above.intermediate(&name, 0x70 + at, 0x40 + at));
    }
    let lowest = authorities.last().expect("one");
    let signer = lowest.issue("tpdf test signer deep down", 0x6d, 8, None);
    let carried: Vec<Vec<u8>> = authorities.iter().map(|a| a.certificate.clone()).collect();
    let bytes = signed(&signer, &carried, G, &w.tsa);
    with_dss(
        &bytes,
        &[w.ca.certificate.clone(), w.tsa.root.clone()],
        &[
            ocsp(&signer.certificate, lowest, Says::Good),
            ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good),
        ],
        &[],
    )
}

#[test]
fn a_chain_past_the_bound_is_counted_and_cannot_read_good() {
    if !has_store() {
        return;
    }
    let w = world();
    let signature = read(&deep(&w, 9), &anchors(&w));
    assert_eq!(
        signature.trust.as_ref().map(|t| t.standing),
        Some(Standing::TrustedAtTimestamp)
    );
    let chain = signature.revocation_chain.expect("a chain");
    assert_eq!(
        chain.certificates.len(),
        crate::revocation::chain::MAX_CHAIN
    );
    assert_eq!(chain.dropped, 2, "{chain:?}");
    assert_eq!(chain.standing, Status::Unchecked);
}

#[test]
fn an_authority_marked_no_check_on_the_stores_chain_is_not_asked_about() {
    if !has_store() {
        return;
    }
    let w = world();
    let issuing =
        w.ca.intermediate_no_check("tpdf test issuing authority", 0x6c, 0x31);
    let signer = issuing.issue("tpdf test signer under it", 0x6d, 8, None);
    let bytes = with_added(&w, &issuing, &signer, &[], Says::Good, &[]);
    let chain = read(&bytes, &anchors(&w))
        .revocation_chain
        .expect("a chain");
    assert_eq!(
        (chain.standing, chain.certificates.len(), chain.end),
        (Status::Good, 1, crate::revocation::chain::End::Root),
        "{chain:?}"
    );
}

#[test]
fn the_anchor_the_store_ended_at_is_not_asked_about_though_it_issued_nothing_to_itself() {
    if !has_store() {
        return;
    }
    // The issuing authority itself is the only root here, so the store's chain
    // ends at a certificate that is not self-issued and has no data about it.
    let w = world();
    let issuing = w.ca.intermediate("tpdf test issuing authority", 0x6c, 0x31);
    let signer = issuing.issue("tpdf test signer under it", 0x6d, 8, None);
    let bytes = with_added(&w, &issuing, &signer, &[], Says::Good, &[]);
    let only = [issuing.certificate.clone(), w.tsa.root.clone()];
    let signature = read(&bytes, &only);
    assert_eq!(
        signature.trust.as_ref().map(|t| t.standing),
        Some(Standing::TrustedAtTimestamp)
    );
    let chain = signature.revocation_chain.expect("a chain");
    assert_eq!(
        (chain.standing, chain.certificates.len()),
        (Status::Good, 1),
        "{chain:?}"
    );
}

#[test]
fn a_cross_certificate_of_a_root_the_document_carries_ends_the_stores_chain_too() {
    if !has_store() {
        return;
    }
    // The store goes through the cross-certificate to the root it trusts; the
    // document carries the self-issued twin, as DigiCert's and Sectigo's do.
    let w = world();
    let their_root = TestCa::new("tpdf test second root", 0x75);
    let cross = w.ca.cross(&their_root, 0x32);
    let signer = their_root.issue("tpdf test signer under it", 0x6d, 8, None);
    let bytes = signed(&signer, &[cross, their_root.certificate.clone()], G, &w.tsa);
    let bytes = with_dss(
        &bytes,
        std::slice::from_ref(&w.tsa.root),
        &[
            ocsp(&signer.certificate, &their_root, Says::Good),
            ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good),
        ],
        &[],
    );
    let signature = read(&bytes, &anchors(&w));
    assert_eq!(
        signature.trust.as_ref().map(|t| t.standing),
        Some(Standing::TrustedAtTimestamp)
    );
    let chain = signature.revocation_chain.expect("a chain");
    assert_eq!(
        (chain.standing, chain.certificates.len()),
        (Status::Good, 1),
        "{chain:?}"
    );
}

#[test]
fn an_issuer_only_the_store_holds_supplies_no_key_and_a_same_named_key_is_no_issuer() {
    if !has_store() {
        return;
    }
    use crate::revocation::chain::End;
    // The root is an anchor and the document leaves it out, so a response about
    // the authority below it cannot be checked on any computer. A certificate
    // with the root's name and another key changes nothing.
    let w = world();
    let issuing = w.ca.intermediate("tpdf test issuing authority", 0x6c, 0x31);
    let signer = issuing.issue("tpdf test signer under it", 0x6d, 8, None);
    let impostor = TestCa::new("tpdf test signing authority", 0x76);
    for added in [vec![], vec![impostor.certificate.clone()]] {
        let mut chain = added;
        chain.push(issuing.certificate.clone());
        let bytes = signed(&signer, &chain, G, &w.tsa);
        let bytes = with_dss(
            &bytes,
            std::slice::from_ref(&w.tsa.root),
            &[
                ocsp(&signer.certificate, &issuing, Says::Good),
                ocsp(&issuing.certificate, &w.ca, Says::Good),
                ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good),
            ],
            &[],
        );
        let signature = read(&bytes, &anchors(&w));
        assert_eq!(
            signature.trust.as_ref().map(|t| t.standing),
            Some(Standing::TrustedAtTimestamp)
        );
        let chain = signature.revocation_chain.expect("a chain");
        assert_eq!(
            (chain.standing, chain.end, chain.decided_by),
            (Status::Unchecked, End::NoIssuer, Some(1)),
            "{chain:?}"
        );
        assert_eq!(chain.certificates[1].revocation.why, Some(Gap::Issuer));
    }
}

#[test]
fn a_cross_certificate_stands_in_for_a_root_the_document_leaves_out() {
    if !has_store() {
        return;
    }
    // The root's name and key under another authority: data the root signed
    // checks out under it, as it does under the root.
    let w = world();
    let issuing = w.ca.intermediate("tpdf test issuing authority", 0x6c, 0x31);
    let signer = issuing.issue("tpdf test signer under it", 0x6d, 8, None);
    let other = TestCa::new("tpdf test other root", 0x77);
    let stand_in = other.cross(&w.ca, 0x33);
    let bytes = signed(&signer, &[issuing.certificate.clone(), stand_in], G, &w.tsa);
    let bytes = with_dss(
        &bytes,
        std::slice::from_ref(&w.tsa.root),
        &[
            ocsp(&signer.certificate, &issuing, Says::Good),
            ocsp(&issuing.certificate, &w.ca, Says::Good),
            ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good),
        ],
        &[],
    );
    let chain = read(&bytes, &anchors(&w))
        .revocation_chain
        .expect("a chain");
    assert_eq!(
        (chain.standing, chain.end, chain.certificates.len()),
        (Status::Good, crate::revocation::chain::End::Root, 2),
        "{chain:?}"
    );
}

#[test]
fn a_trusted_signer_that_issued_itself_is_judged_by_its_own_data() {
    if !has_store() {
        return;
    }
    let w = world();
    let own = TestCa::new("tpdf test signer of its own", 0x78);
    let signer = own.as_signer(0x78);
    let bytes = signed(&signer, &[], G, &w.tsa);
    let bytes = with_dss(
        &bytes,
        std::slice::from_ref(&w.tsa.root),
        &[
            ocsp(&own.certificate, &own, Says::Good),
            ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good),
        ],
        &[],
    );
    let roots = [own.certificate.clone(), w.tsa.root.clone()];
    let signature = read(&bytes, &roots);
    assert_eq!(
        signature.trust.as_ref().map(|t| t.standing),
        Some(Standing::TrustedAtTimestamp)
    );
    assert_eq!(signature.revocation.map(|r| r.standing), Some(Status::Good));
}

#[test]
fn a_chain_the_store_refused_is_not_followed() {
    if !has_store() {
        return;
    }
    // The store stops at the signer, so its chain is one certificate long and
    // says nothing about who issued it; the data about the signer cannot be
    // checked. Followed, the signer would read as having issued itself.
    let w = world();
    let issuing = w.ca.intermediate("tpdf test issuing authority", 0x6c, 0x31);
    let signer = issuing.issue("tpdf test signer under it", 0x6d, 8, None);
    let bytes = signed(&signer, &[], G, &w.tsa);
    let bytes = with_dss(
        &bytes,
        std::slice::from_ref(&w.tsa.root),
        &[
            ocsp(&signer.certificate, &issuing, Says::Good),
            ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good),
        ],
        &[],
    );
    let signature = read(&bytes, &anchors(&w));
    assert_eq!(
        signature.trust.as_ref().map(|t| (t.standing, t.why)),
        Some((Standing::Untrusted, Some(Doubt::Incomplete)))
    );
    let revocation = signature.revocation.expect("an answer");
    assert_eq!(
        (revocation.standing, revocation.why),
        (Status::Unchecked, Some(Gap::Issuer))
    );
}

#[test]
fn a_signer_read_as_out_of_its_dates_is_still_judged_along_the_stores_chain() {
    if !has_store() {
        return;
    }
    use crate::integrity::test_tsa::{FROM, UNTIL};
    let w = world();
    let issuing = w.ca.intermediate("tpdf test issuing authority", 0x6c, 0x31);
    let signer = issuing.issue("tpdf test signer under it", 0x6d, 8, None);
    let theirs = TestCa::new("tpdf test injected root", 0x5a);
    let twin = theirs.cross(&issuing, 1);
    let bytes = with_added(
        &w,
        &issuing,
        &signer,
        &[twin.clone(), theirs.certificate.clone()],
        Says::Revoked {
            at: G - DAY,
            reason: Some(2),
        },
        &[ocsp(&twin, &theirs, Says::Good)],
    );
    for (now, standing) in [
        (UNTIL + DAY, Standing::Expired),
        (FROM - DAY, Standing::NotYetValid),
    ] {
        let found = scan_at(&bytes, crate::trust::Anchors::Only(&anchors(&w)), now).expect("read");
        let signature = found
            .signatures
            .into_iter()
            .find(|s| s.signed)
            .expect("the signature");
        assert_eq!(signature.trust.map(|t| t.standing), Some(standing));
        let chain = signature.revocation_chain.expect("a chain");
        // The real authority, serial 31, whatever the data says at that moment.
        assert_eq!(chain.certificates[1].serial, "31", "{chain:?}");
    }
}

#[test]
fn a_twin_that_may_not_sign_lists_does_not_stop_the_real_issuers_list_being_read() {
    if !has_store() {
        return;
    }
    use crate::integrity::test_tsa::{mint_crl, CrlFaults, Listed};
    use der::{Decode as _, Encode as _};
    let w = world();
    let issuing = w.ca.intermediate("tpdf test issuing authority", 0x6c, 0x31);
    let signer = issuing.issue("tpdf test signer under it", 0x6d, 8, None);
    // The root's name and key, stating a key usage without `cRLSign`, ahead of
    // the root in the signature's set.
    let mut twin = x509_cert::Certificate::from_der(&w.ca.certificate).expect("the root");
    let usage =
        x509_cert::ext::pkix::KeyUsage(x509_cert::ext::pkix::KeyUsages::DigitalSignature.into());
    twin.tbs_certificate.extensions = Some(vec![x509_cert::ext::Extension {
        extn_id: der::oid::ObjectIdentifier::new_unwrap("2.5.29.15"),
        critical: true,
        extn_value: der::asn1::OctetString::new(usage.to_der().expect("usage")).expect("octets"),
    }]);
    let twin = twin.to_der().expect("encoded");
    let list = mint_crl(
        &w.ca,
        &[Listed {
            certificate: &issuing.certificate,
            at: G - DAY,
            reason: Some(2),
        }],
        G - 3_600,
        Some(G + 7 * DAY),
        &CrlFaults::default(),
    );
    let bytes = signed(&signer, &[twin, issuing.certificate.clone()], G, &w.tsa);
    let bytes = with_dss(
        &bytes,
        &[w.ca.certificate.clone(), w.tsa.root.clone()],
        &[
            ocsp(&signer.certificate, &issuing, Says::Good),
            ocsp(&w.tsa.certificate, &w.tsa_root, Says::Good),
        ],
        &[list],
    );
    let chain = read(&bytes, &anchors(&w))
        .revocation_chain
        .expect("a chain");
    assert_eq!(
        (chain.standing, chain.decided_by),
        (Status::Revoked, Some(1)),
        "{chain:?}"
    );
}
