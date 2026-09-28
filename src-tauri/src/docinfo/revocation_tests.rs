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
        println!("[SKIP] incr-lt.pdf: not generated");
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
