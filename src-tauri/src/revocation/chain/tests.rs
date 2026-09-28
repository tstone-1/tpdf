//! The chain above a certificate: the walk, and the rule that combines the
//! answers along it.
//!
//! Every chain is minted by `integrity/test_tsa.rs`: a root, one or more
//! intermediates, a signer. Each test asserts the chain's standing **and**
//! which certificate decides it, so a chain that reached the right standing
//! through the wrong certificate cannot pass.

use der::{Decode as _, Encode as _};

use super::*;
use crate::integrity::test_tsa::{
    mint_crl, mint_ocsp, CrlFaults, Listed, OcspFaults, Responder, Status as Says, TestCa,
};
use crate::revocation::{Basis, Gap, Material};

/// 2026-09-01 00:00:00 UTC: the moment judged.
const T: u64 = 1_788_220_800;
/// 2026-09-28: the present these tests run at.
const NOW: u64 = 1_790_553_600;
const DAY: u64 = 86_400;

fn attested() -> Moment {
    Moment {
        basis: Basis::Attested,
        at: T,
    }
}

fn parsed(der: &[u8]) -> Certificate {
    Certificate::from_der(der).expect("a certificate")
}

/// A root, an intermediate it issued, and a signer the intermediate issued.
struct Pki {
    root: TestCa,
    intermediate: TestCa,
    signer: Vec<u8>,
}

fn pki() -> Pki {
    let root = TestCa::new("tpdf test chain root", 0x41);
    let intermediate = root.intermediate("tpdf test issuing authority", 0x42, 7);
    let signer = intermediate
        .issue("tpdf test chain signer", 0x43, 9, None)
        .certificate;
    Pki {
        root,
        intermediate,
        signer,
    }
}

/// A response from `issuer` about `certificate`, fresh for `T`.
fn ocsp(certificate: &[u8], issuer: &TestCa, says: Says) -> Vec<u8> {
    mint_ocsp(
        certificate,
        issuer,
        says,
        T - 3_600,
        Some(T + 7 * DAY),
        Responder::Issuer,
        &OcspFaults::default(),
    )
}

fn revoked(at: u64) -> Says {
    Says::Revoked {
        at,
        reason: Some(1),
    }
}

/// The chain above `leaf`, with `certificates` as the signature's own set and
/// `responses` and `lists` as the document's data --- the leaf judged first,
/// as the scan judges it.
fn ask(leaf: &[u8], certificates: &[&[u8]], responses: &[Vec<u8>], lists: &[Vec<u8>]) -> Chain {
    let mut material = Material::default();
    for response in responses {
        material.response(response);
    }
    for list in lists {
        material.list(list.clone());
    }
    let pool = Pool::new(&material);
    let candidates: Vec<Certificate> = certificates.iter().map(|der| parsed(der)).collect();
    let leaf = parsed(leaf);
    let mut budget = crate::integrity::MAX_HASHED;
    let own = judge(&leaf, &candidates, &[&pool], attested(), NOW, &mut budget);
    chain(
        &leaf,
        &own,
        &candidates,
        &[&pool],
        attested(),
        NOW,
        &mut budget,
    )
}

/// The ordinary chain: the signature carries the intermediate and the root,
/// the document a response for the signer and `intermediate` for the
/// intermediate.
fn with_intermediate(pki: &Pki, intermediate: Option<Says>) -> Chain {
    let mut responses = vec![ocsp(&pki.signer, &pki.intermediate, Says::Good)];
    if let Some(says) = intermediate {
        responses.push(ocsp(&pki.intermediate.certificate, &pki.root, says));
    }
    ask(
        &pki.signer,
        &[&pki.intermediate.certificate, &pki.root.certificate],
        &responses,
        &[],
    )
}

fn standings(chain: &Chain) -> Vec<Status> {
    chain
        .certificates
        .iter()
        .map(|c| c.revocation.standing)
        .collect()
}

#[test]
fn an_intermediate_the_document_says_is_good_makes_the_chain_good() {
    // The control for every refusal below.
    let pki = pki();
    let chain = with_intermediate(&pki, Some(Says::Good));
    assert_eq!(standings(&chain), [Status::Good, Status::Good], "{chain:?}");
    assert_eq!(
        (chain.standing, chain.decided_by, chain.end, chain.dropped),
        (Status::Good, None, End::Root, 0)
    );
    assert_eq!(
        chain.certificates[1].subject_cn,
        "tpdf test issuing authority"
    );
    assert_eq!(chain.certificates[1].serial, "07");
    // The root is not judged: nobody revokes an anchor through its own data.
    assert_eq!(chain.certificates.len(), 2);
    assert!(!chain.undoes());
}

#[test]
fn an_intermediate_revoked_before_the_moment_revokes_the_chain_and_is_named() {
    let pki = pki();
    let chain = with_intermediate(&pki, Some(revoked(T - DAY)));
    assert_eq!(standings(&chain), [Status::Good, Status::Revoked]);
    assert_eq!(
        (chain.standing, chain.decided_by, chain.after_moment),
        (Status::Revoked, Some(1), false),
        "{chain:?}"
    );
    assert!(chain.undoes());
    assert_eq!(
        chain.deciding().map(|c| c.subject_cn.as_str()),
        Some("tpdf test issuing authority")
    );
}

#[test]
fn an_intermediate_revoked_after_the_moment_does_not_undo_the_chain() {
    let pki = pki();
    let chain = with_intermediate(&pki, Some(revoked(T + DAY)));
    assert_eq!(
        (chain.standing, chain.decided_by, chain.after_moment),
        (Status::Revoked, Some(1), true),
        "{chain:?}"
    );
    assert!(!chain.undoes());
}

#[test]
fn an_intermediate_the_document_says_nothing_about_is_not_read_as_good() {
    let pki = pki();
    let chain = with_intermediate(&pki, None);
    assert_eq!(standings(&chain), [Status::Good, Status::None]);
    assert_eq!(
        (chain.standing, chain.decided_by),
        (Status::None, Some(1)),
        "{chain:?}"
    );
}

#[test]
fn data_about_an_intermediate_that_does_not_check_out_leaves_the_chain_unchecked() {
    let pki = pki();
    let damaged = mint_ocsp(
        &pki.intermediate.certificate,
        &pki.root,
        Says::Good,
        T - 3_600,
        Some(T + 7 * DAY),
        Responder::Issuer,
        &OcspFaults {
            corrupt_signature: true,
            ..OcspFaults::default()
        },
    );
    let chain = ask(
        &pki.signer,
        &[&pki.intermediate.certificate, &pki.root.certificate],
        &[ocsp(&pki.signer, &pki.intermediate, Says::Good), damaged],
        &[],
    );
    assert_eq!(
        (chain.standing, chain.decided_by),
        (Status::Unchecked, Some(1)),
        "{chain:?}"
    );
    assert_eq!(
        chain.certificates[1].revocation.why,
        Some(Gap::Signature),
        "unchecked for the reason the data gives, not another"
    );
}

#[test]
fn an_intermediate_revoked_by_its_list_revokes_the_chain() {
    let pki = pki();
    let list = mint_crl(
        &pki.root,
        &[Listed {
            certificate: &pki.intermediate.certificate,
            at: T - DAY,
            reason: Some(2),
        }],
        T - 3_600,
        Some(T + 7 * DAY),
        &CrlFaults::default(),
    );
    let chain = ask(
        &pki.signer,
        &[&pki.intermediate.certificate, &pki.root.certificate],
        &[ocsp(&pki.signer, &pki.intermediate, Says::Good)],
        &[list],
    );
    assert_eq!(
        (chain.standing, chain.decided_by),
        (Status::Revoked, Some(1)),
        "{chain:?}"
    );
}

#[test]
fn a_chain_whose_root_is_not_in_the_document_ends_at_the_certificate_below_it() {
    // The intermediate's issuer is nowhere, so its data cannot be checked and
    // the walk cannot go on: `unchecked`, reason `issuer`, and the chain says
    // so rather than reading as complete.
    let pki = pki();
    let chain = ask(
        &pki.signer,
        &[&pki.intermediate.certificate],
        &[
            ocsp(&pki.signer, &pki.intermediate, Says::Good),
            ocsp(&pki.intermediate.certificate, &pki.root, Says::Good),
        ],
        &[],
    );
    assert_eq!(chain.end, End::NoIssuer);
    assert_eq!(
        (chain.standing, chain.decided_by),
        (Status::Unchecked, Some(1)),
        "{chain:?}"
    );
    assert_eq!(chain.certificates[1].revocation.why, Some(Gap::Issuer));
}

#[test]
fn a_cross_certificate_of_a_root_in_the_document_ends_the_chain() {
    // DigiCert's and Sectigo's shape: the chain reaches a certificate with a
    // root's name and key, issued by an older root. With the self-issued
    // twin among the candidates, the chain ends there and the
    // cross-certificate is not judged --- the older root is not even present.
    let pki = pki();
    let older = TestCa::new("tpdf test older root", 0x44);
    let cross = older.cross(&pki.root, 3);
    let responses = [
        ocsp(&pki.signer, &pki.intermediate, Says::Good),
        ocsp(&pki.intermediate.certificate, &pki.root, Says::Good),
    ];
    let chain = ask(
        &pki.signer,
        &[&pki.intermediate.certificate, &cross, &pki.root.certificate],
        &responses,
        &[],
    );
    assert_eq!(
        (chain.standing, chain.end, chain.certificates.len()),
        (Status::Good, End::Root, 2),
        "{chain:?}"
    );
    // The walk itself, with the cross-certificate met first: it is the
    // intermediate's issuer by name and key, and ends the chain as a root.
    let (cross_parsed, root_parsed) = (parsed(&cross), parsed(&pki.root.certificate));
    let walked = walk(
        &parsed(&pki.intermediate.certificate),
        &[&cross_parsed, &root_parsed],
    );
    assert_eq!(walked.links.len(), 1);
    assert_eq!(
        walked.anchor.as_ref().and_then(|a| a.to_der().ok()),
        Some(pki.root.certificate.clone()),
        "the anchor is the self-issued twin, not the cross-certificate"
    );
    // The control: without the twin, the cross-certificate is a link like any
    // other, asked about, and its issuer the older root.
    let chain = ask(
        &pki.signer,
        &[&pki.intermediate.certificate, &cross, &older.certificate],
        &responses,
        &[],
    );
    assert_eq!(chain.certificates.len(), 3, "{chain:?}");
    assert_eq!(
        (chain.standing, chain.decided_by),
        (Status::None, Some(2)),
        "{chain:?}"
    );
}

#[test]
fn a_chain_longer_than_the_bound_is_counted_and_cannot_read_good() {
    // Ten intermediates between the root and the signer: eleven certificates
    // below the root, eight judged, three counted.
    let root = TestCa::new("tpdf test deep root", 0x45);
    let mut authorities = vec![root];
    for depth in 0..10u8 {
        let next = authorities.last().expect("one").intermediate(
            &format!("tpdf test depth {depth}"),
            0x46 + depth,
            depth + 1,
        );
        authorities.push(next);
    }
    let signer = authorities
        .last()
        .expect("one")
        .issue("tpdf test deep signer", 0x60, 99, None)
        .certificate;
    let mut responses = vec![ocsp(&signer, authorities.last().expect("one"), Says::Good)];
    for pair in authorities.windows(2) {
        responses.push(ocsp(&pair[1].certificate, &pair[0], Says::Good));
    }
    let certificates: Vec<&[u8]> = authorities.iter().map(|a| &a.certificate[..]).collect();
    let chain = ask(&signer, &certificates, &responses, &[]);
    assert_eq!(chain.certificates.len(), MAX_CHAIN, "{chain:?}");
    assert_eq!(chain.dropped, 3);
    assert!(
        chain
            .certificates
            .iter()
            .all(|c| c.revocation.standing == Status::Good),
        "every certificate judged is good: the bound, not a certificate, decides"
    );
    assert_eq!(
        (chain.standing, chain.decided_by),
        (Status::Unchecked, None),
        "{chain:?}"
    );
    // A revocation among those judged still stands: the second authority
    // above the signer, whose response is the ninth minted.
    responses[9] = ocsp(
        &authorities[9].certificate,
        &authorities[8],
        revoked(T - DAY),
    );
    let chain = ask(&signer, &certificates, &responses, &[]);
    assert_eq!(
        (chain.standing, chain.decided_by),
        (Status::Revoked, Some(2)),
        "{chain:?}"
    );
}

#[test]
fn two_authorities_certifying_each_other_end_the_walk_rather_than_loop_it() {
    let a = TestCa::new("tpdf test authority A", 0x70);
    let b = TestCa::new("tpdf test authority B", 0x71);
    // A's name and key issued by B, and B's issued by A; neither root present.
    let a_by_b = b.cross(&a, 1);
    let b_by_a = a.cross(&b, 2);
    let signer = a
        .issue("tpdf test looped signer", 0x72, 3, None)
        .certificate;
    let chain = ask(&signer, &[&a_by_b, &b_by_a], &[], &[]);
    assert_eq!(chain.end, End::Loop, "{chain:?}");
    assert_eq!(chain.certificates.len(), 3);
    assert_eq!(chain.dropped, 0);
}

#[test]
fn a_certificate_that_needs_no_check_is_walked_through_and_not_judged() {
    let root = TestCa::new("tpdf test root above a nocheck", 0x73);
    let middle = root.intermediate_no_check("tpdf test nocheck authority", 0x74, 4);
    let signer = middle
        .issue("tpdf test signer below", 0x75, 5, None)
        .certificate;
    let chain = ask(
        &signer,
        &[&middle.certificate, &root.certificate],
        &[ocsp(&signer, &middle, Says::Good)],
        &[],
    );
    assert_eq!(
        (chain.standing, chain.end, chain.certificates.len()),
        (Status::Good, End::Root, 1),
        "{chain:?}"
    );
}

#[test]
fn a_leaf_that_is_a_root_is_the_whole_chain() {
    let root = TestCa::new("tpdf test self-issued signer", 0x76);
    let chain = ask(&root.certificate, &[], &[], &[]);
    assert_eq!(chain.certificates.len(), 1);
    assert_eq!((chain.standing, chain.decided_by), (Status::None, Some(0)));
}

// --------------------------------------------------------------- the rule

fn answer(standing: Status, after_moment: bool) -> Judged {
    Judged {
        revocation: Revocation {
            standing,
            after_moment,
            ..Revocation::default()
        },
        ..Judged::default()
    }
}

/// The answers on a chain, leaf first, and what the chain reads and who decides.
type Case<'a> = (&'a [(Status, bool)], Status, Option<usize>);

#[test]
fn the_most_telling_answer_decides_and_the_leaf_breaks_a_tie() {
    use Status::{Good, None, Revoked, Unchecked, Unknown};
    let cases: &[Case] = &[
        (&[(Good, false), (Good, false)], Good, Option::None),
        (&[(Good, false), (None, false)], None, Some(1)),
        (&[(None, false), (Unchecked, false)], Unchecked, Some(1)),
        (&[(Unchecked, false), (Unknown, false)], Unknown, Some(1)),
        (&[(Unknown, false), (Revoked, false)], Revoked, Some(1)),
        (&[(Revoked, true), (None, false)], None, Some(1)),
        (&[(Good, false), (Revoked, true)], Revoked, Some(1)),
        (&[(None, false), (None, false)], None, Some(0)),
        (&[(Revoked, false), (Revoked, false)], Revoked, Some(0)),
        (
            &[(Good, false), (Revoked, true), (Revoked, false)],
            Revoked,
            Some(2),
        ),
    ];
    for (answers, standing, decided_by) in cases {
        let chain = combine(
            answers.iter().map(|(s, a)| answer(*s, *a)).collect(),
            0,
            End::Root,
        );
        assert_eq!(
            (chain.standing, chain.decided_by),
            (*standing, *decided_by),
            "{answers:?}"
        );
    }
}

#[test]
fn past_the_bound_nothing_reassuring_stands() {
    use Status::{Good, None, Revoked, Unchecked, Unknown};
    for (answers, standing, decided_by, after) in [
        (vec![(Good, false)], Unchecked, Option::None, false),
        (vec![(None, false)], Unchecked, Option::None, false),
        (vec![(Revoked, true)], Unchecked, Option::None, false),
        (vec![(Unknown, false)], Unknown, Some(0), false),
        (vec![(Unchecked, false)], Unchecked, Some(0), false),
        (vec![(Revoked, false)], Revoked, Some(0), false),
    ] {
        let chain = combine(
            answers.iter().map(|(s, a)| answer(*s, *a)).collect(),
            1,
            End::Root,
        );
        assert_eq!(
            (chain.standing, chain.decided_by, chain.after_moment),
            (standing, decided_by, after),
            "{answers:?}"
        );
    }
}
