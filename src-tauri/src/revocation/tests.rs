//! The revocation answers, one fault at a time, over data the test PKI mints.
//!
//! Every test builds its own authority, certificate and data with
//! `integrity/test_tsa.rs`, and asserts the standing **and** the reason, so a
//! test that only reached `unchecked` for another reason cannot pass.

use super::*;
use crate::integrity::test_tsa::{
    mint_crl, mint_ocsp, CrlFaults, Listed, OcspFaults, Responder, Status as Says, TestCa,
    OCSP_SIGNING,
};

/// 2026-09-01 00:00:00 UTC: the moment judged.
const T: u64 = 1_788_220_800;
/// 2026-09-28: the present these tests run at.
const NOW: u64 = 1_790_553_600;
const DAY: u64 = 86_400;
/// The last moment of the test PKI's certificates, 2040-01-01.
const UNTIL: u64 = crate::integrity::test_tsa::UNTIL;

fn attested() -> Moment {
    Moment {
        basis: Basis::Attested,
        at: T,
    }
}

struct Pki {
    ca: TestCa,
    subject: Vec<u8>,
}

fn pki() -> Pki {
    let ca = TestCa::new("tpdf test revocation authority", 0x61);
    let subject = ca.issue("tpdf test signer", 0x62, 5, None).certificate;
    Pki { ca, subject }
}

fn parsed(der: &[u8]) -> Certificate {
    Certificate::from_der(der).expect("a certificate")
}

/// The answer for `pki.subject` from `material`, with the issuer offered as
/// a candidate the way a signature's own certificate set offers it.
fn ask(pki: &Pki, material: &Material, moment: Moment) -> Revocation {
    let pool = Pool::new(material);
    judge(
        &parsed(&pki.subject),
        &[parsed(&pki.ca.certificate)],
        &[&pool],
        moment,
        NOW,
        &mut crate::integrity::MAX_HASHED.clone(),
    )
}

fn responses(items: &[Vec<u8>]) -> Material {
    let mut material = Material::default();
    for item in items {
        material.response(item);
    }
    material
}

fn lists(items: &[Vec<u8>]) -> Material {
    let mut material = Material::default();
    for item in items {
        material.list(item.clone());
    }
    material
}

fn ocsp(pki: &Pki, says: Says, responder: Responder<'_>, faults: &OcspFaults) -> Vec<u8> {
    mint_ocsp(
        &pki.subject,
        &pki.ca,
        says,
        T - 3_600,
        Some(T + 7 * DAY),
        responder,
        faults,
    )
}

fn good(pki: &Pki) -> Vec<u8> {
    ocsp(pki, Says::Good, Responder::Issuer, &OcspFaults::default())
}

fn standing(answer: &Revocation) -> (Status, Option<Gap>) {
    (answer.standing, answer.why)
}

// ------------------------------------------------------------------ OCSP

#[test]
fn a_document_carrying_nothing_is_none_and_says_nothing_about_the_certificate() {
    let pki = pki();
    let answer = ask(&pki, &Material::default(), attested());
    assert_eq!(standing(&answer), (Status::None, None));
    // None never reads as good: nothing answered, so nothing is named.
    assert_eq!(answer.source, None);
    assert!(answer.issued.is_empty() && answer.revoked.is_empty());
    assert!(!answer.undoes());
}

#[test]
fn a_response_from_the_issuer_saying_good_is_good() {
    // The control for every OCSP refusal below.
    let pki = pki();
    let answer = ask(&pki, &responses(&[good(&pki)]), attested());
    assert_eq!(standing(&answer), (Status::Good, None), "{answer:?}");
    assert_eq!(answer.source, Some(Source::Ocsp));
    assert_eq!(answer.issued, format_time(T - 3_600));
    assert_eq!(answer.next, format_time(T + 7 * DAY));
    assert_eq!(answer.basis, Basis::Attested);
    assert_eq!(answer.moment, format_time(T));
}

#[test]
fn a_response_from_a_delegated_responder_is_good() {
    let pki = pki();
    let responder = pki.ca.responder(0x63);
    let response = ocsp(
        &pki,
        Says::Good,
        Responder::Delegated(&responder),
        &OcspFaults::default(),
    );
    let answer = ask(&pki, &responses(&[response]), attested());
    assert_eq!(standing(&answer), (Status::Good, None), "{answer:?}");
}

#[test]
fn a_responder_without_the_ocsp_signing_purpose_is_not_authorised() {
    // Issued by the right authority, signing correctly, and not delegated:
    // its arithmetic holds and proves nothing about the issuer.
    let pki = pki();
    for purposes in [None, Some(&["1.3.6.1.5.5.7.3.4"][..])] {
        let stranger = pki
            .ca
            .issue("tpdf test not a responder", 0x64, 0x71, purposes);
        let response = ocsp(
            &pki,
            Says::Good,
            Responder::Delegated(&stranger),
            &OcspFaults::default(),
        );
        let answer = ask(&pki, &responses(&[response]), attested());
        assert_eq!(
            standing(&answer),
            (Status::Unchecked, Some(Gap::Unauthorised)),
            "{purposes:?}"
        );
    }
}

#[test]
fn a_responder_another_authority_delegated_is_not_authorised() {
    let pki = pki();
    let other = TestCa::new("tpdf test another authority", 0x65);
    let foreign = other.issue(
        "tpdf test OCSP responder",
        0x66,
        0x72,
        Some(&[OCSP_SIGNING]),
    );
    let response = ocsp(
        &pki,
        Says::Good,
        Responder::Delegated(&foreign),
        &OcspFaults::default(),
    );
    let answer = ask(&pki, &responses(&[response]), attested());
    assert_eq!(
        standing(&answer),
        (Status::Unchecked, Some(Gap::Unauthorised))
    );
}

#[test]
fn a_response_whose_signature_fails_is_not_believed() {
    let pki = pki();
    let response = ocsp(
        &pki,
        Says::Good,
        Responder::Issuer,
        &OcspFaults {
            corrupt_signature: true,
            ..OcspFaults::default()
        },
    );
    let answer = ask(&pki, &responses(&[response]), attested());
    assert_eq!(standing(&answer), (Status::Unchecked, Some(Gap::Signature)));
}

#[test]
fn a_response_about_another_certificate_says_nothing_about_this_one() {
    // Even a *revoked* one: a CertID for another serial is not this one.
    let pki = pki();
    for says in [
        Says::Good,
        Says::Revoked {
            at: T - DAY,
            reason: Some(1),
        },
    ] {
        let response = ocsp(
            &pki,
            says,
            Responder::Issuer,
            &OcspFaults {
                wrong_cert_id: true,
                ..OcspFaults::default()
            },
        );
        let answer = ask(&pki, &responses(&[response]), attested());
        assert_eq!(standing(&answer), (Status::None, None), "{says:?}");
    }
}

#[test]
fn a_revocation_before_the_attested_moment_is_revoked_and_undoes_the_signature() {
    let pki = pki();
    let response = ocsp(
        &pki,
        Says::Revoked {
            at: T - DAY,
            reason: Some(1),
        },
        Responder::Issuer,
        &OcspFaults::default(),
    );
    let answer = ask(&pki, &responses(&[response]), attested());
    assert_eq!(standing(&answer), (Status::Revoked, None));
    assert_eq!(answer.revoked, format_time(T - DAY));
    assert_eq!(answer.reason, Some(Reason::KeyCompromise));
    assert!(!answer.after_moment);
    assert!(answer.undoes());
}

#[test]
fn a_revocation_after_the_attested_moment_does_not_undo_the_signature() {
    let pki = pki();
    let response = mint_ocsp(
        &pki.subject,
        &pki.ca,
        Says::Revoked {
            at: T + DAY,
            reason: Some(4),
        },
        T + 2 * DAY,
        Some(T + 9 * DAY),
        Responder::Issuer,
        &OcspFaults::default(),
    );
    let answer = ask(
        &pki,
        &responses(std::slice::from_ref(&response)),
        attested(),
    );
    assert_eq!(standing(&answer), (Status::Revoked, None));
    assert!(answer.after_moment, "{answer:?}");
    assert!(!answer.undoes());
    // The same revocation at the same moment on the signer's own word is
    // not after anything: only an attested moment puts the signature first.
    for basis in [Basis::Claimed, Basis::Stated, Basis::Now] {
        let answer = ask(
            &pki,
            &responses(std::slice::from_ref(&response)),
            Moment { basis, at: T },
        );
        assert!(!answer.after_moment, "{basis:?}");
        assert!(answer.undoes(), "{basis:?}");
    }
    // And exactly at the moment is not after it.
    let at_the_moment = mint_ocsp(
        &pki.subject,
        &pki.ca,
        Says::Revoked {
            at: T,
            reason: None,
        },
        T + 2 * DAY,
        Some(T + 9 * DAY),
        Responder::Issuer,
        &OcspFaults::default(),
    );
    assert!(!ask(&pki, &responses(&[at_the_moment]), attested()).after_moment);
}

#[test]
fn an_unknown_certificate_is_unknown() {
    let pki = pki();
    let response = ocsp(
        &pki,
        Says::Unknown,
        Responder::Issuer,
        &OcspFaults::default(),
    );
    let answer = ask(&pki, &responses(&[response]), attested());
    assert_eq!(standing(&answer), (Status::Unknown, None));
    assert_eq!(answer.source, Some(Source::Ocsp));
}

#[test]
fn a_good_that_does_not_reach_the_moment_is_stale() {
    let pki = pki();
    // Issued and superseded before the moment; and one that promises no
    // successor, which EN 319 102-1 §5.2.5.4 cannot call fresh.
    for (this, next) in [(T - 9 * DAY, Some(T - 2 * DAY)), (T + DAY, None)] {
        let response = mint_ocsp(
            &pki.subject,
            &pki.ca,
            Says::Good,
            this,
            next,
            Responder::Issuer,
            &OcspFaults::default(),
        );
        let answer = ask(&pki, &responses(&[response]), attested());
        assert_eq!(
            standing(&answer),
            (Status::Unchecked, Some(Gap::Stale)),
            "{next:?}"
        );
    }
    // The boundary: nextUpdate exactly at the moment is not after it.
    let response = mint_ocsp(
        &pki.subject,
        &pki.ca,
        Says::Good,
        T - DAY,
        Some(T),
        Responder::Issuer,
        &OcspFaults::default(),
    );
    assert_eq!(
        standing(&ask(&pki, &responses(&[response]), attested())),
        (Status::Unchecked, Some(Gap::Stale))
    );
}

#[test]
fn a_response_issued_in_the_future_has_no_dates() {
    let pki = pki();
    let response = mint_ocsp(
        &pki.subject,
        &pki.ca,
        Says::Good,
        NOW + DAY,
        Some(NOW + 8 * DAY),
        Responder::Issuer,
        &OcspFaults::default(),
    );
    let answer = ask(&pki, &responses(&[response]), attested());
    assert_eq!(standing(&answer), (Status::Unchecked, Some(Gap::Dates)));
}

#[test]
fn a_critical_extension_tpdf_does_not_know_is_unsupported() {
    let pki = pki();
    let response = ocsp(
        &pki,
        Says::Good,
        Responder::Issuer,
        &OcspFaults {
            critical_extension: true,
            ..OcspFaults::default()
        },
    );
    let answer = ask(&pki, &responses(&[response]), attested());
    assert_eq!(
        standing(&answer),
        (Status::Unchecked, Some(Gap::Unsupported))
    );
}

#[test]
fn a_good_issued_after_the_certificate_expired_counts_only_with_an_archive_cutoff() {
    let pki = pki();
    let late = |cutoff: Option<u64>| {
        mint_ocsp(
            &pki.subject,
            &pki.ca,
            Says::Good,
            UNTIL + DAY,
            Some(UNTIL + 8 * DAY),
            Responder::Issuer,
            &OcspFaults {
                archive_cutoff: cutoff,
                ..OcspFaults::default()
            },
        )
    };
    let judged = |response: Vec<u8>| {
        let pool = Pool::new(&responses(&[response]));
        judge(
            &parsed(&pki.subject),
            &[parsed(&pki.ca.certificate)],
            &[&pool],
            attested(),
            UNTIL + 2 * DAY,
            &mut u64::MAX.clone(),
        )
    };
    assert_eq!(
        standing(&judged(late(None))),
        (Status::Unchecked, Some(Gap::Expired))
    );
    // A cutoff after the certificate's last day does not reach it.
    assert_eq!(
        standing(&judged(late(Some(UNTIL + 1)))),
        (Status::Unchecked, Some(Gap::Expired))
    );
    assert_eq!(standing(&judged(late(Some(UNTIL)))), (Status::Good, None));
}

#[test]
fn data_about_a_certificate_whose_issuer_is_not_in_the_document_is_not_checked() {
    let pki = pki();
    let pool = Pool::new(&responses(&[good(&pki)]));
    let answer = judge(
        &parsed(&pki.subject),
        &[],
        &[&pool],
        attested(),
        NOW,
        &mut u64::MAX.clone(),
    );
    assert_eq!(standing(&answer), (Status::Unchecked, Some(Gap::Issuer)));
}

#[test]
fn an_issuer_is_found_by_its_key_and_not_by_its_name_alone() {
    // A certificate with the issuer's name and another key did not issue the
    // subject, and a response hashed over the real key is not about it.
    let pki = pki();
    let impostor = TestCa::new("tpdf test revocation authority", 0x67);
    let pool = Pool::new(&responses(&[good(&pki)]));
    let answer = judge(
        &parsed(&pki.subject),
        &[parsed(&impostor.certificate)],
        &[&pool],
        attested(),
        NOW,
        &mut u64::MAX.clone(),
    );
    assert_eq!(standing(&answer), (Status::Unchecked, Some(Gap::Issuer)));
}

#[test]
fn the_budget_is_charged_before_a_signature_is_checked() {
    let pki = pki();
    let pool = Pool::new(&responses(&[good(&pki)]));
    let answer = judge(
        &parsed(&pki.subject),
        &[parsed(&pki.ca.certificate)],
        &[&pool],
        attested(),
        NOW,
        &mut 10,
    );
    assert_eq!(standing(&answer), (Status::Unchecked, Some(Gap::Budget)));
}

// ------------------------------------------------------------------ lists

fn crl(pki: &Pki, listed: &[Listed<'_>], faults: &CrlFaults) -> Vec<u8> {
    mint_crl(&pki.ca, listed, T - 3_600, Some(T + 7 * DAY), faults)
}

#[test]
fn a_list_from_the_issuer_that_does_not_name_the_certificate_is_good() {
    let pki = pki();
    let answer = ask(
        &pki,
        &lists(&[crl(&pki, &[], &CrlFaults::default())]),
        attested(),
    );
    assert_eq!(standing(&answer), (Status::Good, None), "{answer:?}");
    assert_eq!(answer.source, Some(Source::Crl));
}

#[test]
fn a_list_naming_the_certificate_is_revoked_with_its_reason() {
    let pki = pki();
    let other = pki
        .ca
        .issue("tpdf test bystander", 0x68, 6, None)
        .certificate;
    let listed = [
        Listed {
            certificate: &other,
            at: T - 5 * DAY,
            reason: None,
        },
        Listed {
            certificate: &pki.subject,
            at: T - 2 * DAY,
            reason: Some(6),
        },
    ];
    let answer = ask(
        &pki,
        &lists(&[crl(&pki, &listed, &CrlFaults::default())]),
        attested(),
    );
    assert_eq!(standing(&answer), (Status::Revoked, None));
    assert_eq!(answer.source, Some(Source::Crl));
    assert_eq!(answer.revoked, format_time(T - 2 * DAY));
    assert_eq!(answer.reason, Some(Reason::CertificateHold));
    // The control: the list naming only somebody else is good for this one.
    let answer = ask(
        &pki,
        &lists(&[crl(&pki, &listed[..1], &CrlFaults::default())]),
        attested(),
    );
    assert_eq!(standing(&answer), (Status::Good, None));
}

#[test]
fn a_list_whose_signature_fails_is_not_believed() {
    let pki = pki();
    let list = crl(
        &pki,
        &[],
        &CrlFaults {
            corrupt_signature: true,
            ..CrlFaults::default()
        },
    );
    let answer = ask(&pki, &lists(&[list]), attested());
    assert_eq!(standing(&answer), (Status::Unchecked, Some(Gap::Signature)));
}

#[test]
fn a_list_naming_another_issuer_says_nothing_about_this_certificate() {
    let pki = pki();
    let renamed = crl(
        &pki,
        &[],
        &CrlFaults {
            wrong_issuer: true,
            ..CrlFaults::default()
        },
    );
    assert_eq!(
        standing(&ask(&pki, &lists(&[renamed]), attested())),
        (Status::None, None)
    );
    // Nor does another authority's own list, soundly signed.
    let other = TestCa::new("tpdf test another authority", 0x65);
    let foreign = mint_crl(
        &other,
        &[],
        T - 3_600,
        Some(T + 7 * DAY),
        &CrlFaults::default(),
    );
    assert_eq!(
        standing(&ask(&pki, &lists(&[foreign]), attested())),
        (Status::None, None)
    );
}

#[test]
fn a_delta_list_is_unsupported() {
    let pki = pki();
    let list = crl(
        &pki,
        &[],
        &CrlFaults {
            delta: true,
            ..CrlFaults::default()
        },
    );
    assert_eq!(
        standing(&ask(&pki, &lists(&[list]), attested())),
        (Status::Unchecked, Some(Gap::Unsupported))
    );
}

#[test]
fn a_stale_list_is_stale_and_a_list_after_expiry_needs_expired_certs_on_crl() {
    let pki = pki();
    let old = mint_crl(
        &pki.ca,
        &[],
        T - 9 * DAY,
        Some(T - 2 * DAY),
        &CrlFaults::default(),
    );
    assert_eq!(
        standing(&ask(&pki, &lists(&[old]), attested())),
        (Status::Unchecked, Some(Gap::Stale))
    );
    let late = |since: Option<u64>| {
        let list = mint_crl(
            &pki.ca,
            &[],
            UNTIL + DAY,
            Some(UNTIL + 8 * DAY),
            &CrlFaults {
                expired_certs_on_crl: since,
                ..CrlFaults::default()
            },
        );
        let pool = Pool::new(&lists(&[list]));
        standing(&judge(
            &parsed(&pki.subject),
            &[parsed(&pki.ca.certificate)],
            &[&pool],
            attested(),
            UNTIL + 2 * DAY,
            &mut u64::MAX.clone(),
        ))
    };
    assert_eq!(late(None), (Status::Unchecked, Some(Gap::Expired)));
    assert_eq!(late(Some(UNTIL - DAY)), (Status::Good, None));
}

// ------------------------------------------------------------- combining

#[test]
fn a_revocation_outweighs_a_good_and_the_earliest_revocation_is_the_one_named() {
    let pki = pki();
    let revoked_later = mint_ocsp(
        &pki.subject,
        &pki.ca,
        Says::Revoked {
            at: T - DAY,
            reason: None,
        },
        T - 3_600,
        Some(T + 7 * DAY),
        Responder::Issuer,
        &OcspFaults::default(),
    );
    let listed = [Listed {
        certificate: &pki.subject,
        at: T - 3 * DAY,
        reason: None,
    }];
    let mut material = responses(&[good(&pki), revoked_later]);
    material.list(crl(&pki, &listed, &CrlFaults::default()));
    let answer = ask(&pki, &material, attested());
    assert_eq!(standing(&answer), (Status::Revoked, None));
    assert_eq!(answer.revoked, format_time(T - 3 * DAY));
    assert_eq!(answer.source, Some(Source::Crl));
}

#[test]
fn the_latest_good_is_the_one_judged_for_freshness() {
    let pki = pki();
    let old = mint_ocsp(
        &pki.subject,
        &pki.ca,
        Says::Good,
        T - 9 * DAY,
        Some(T - 2 * DAY),
        Responder::Issuer,
        &OcspFaults::default(),
    );
    let answer = ask(&pki, &responses(&[old, good(&pki)]), attested());
    assert_eq!(standing(&answer), (Status::Good, None));
    assert_eq!(answer.issued, format_time(T - 3_600));
}

#[test]
fn data_left_unread_at_a_bound_turns_good_and_none_into_not_checked_and_leaves_revoked() {
    let pki = pki();
    let other = pki
        .ca
        .issue("tpdf test bystander", 0x68, 6, None)
        .certificate;
    let filler = |serial_seed: u8| {
        let bystander = pki
            .ca
            .issue("tpdf test bystander", serial_seed, serial_seed, None);
        mint_ocsp(
            &bystander.certificate,
            &pki.ca,
            Says::Good,
            T - 3_600,
            Some(T + 7 * DAY),
            Responder::Issuer,
            &OcspFaults::default(),
        )
    };
    let mut full: Vec<Vec<u8>> = (0..MAX_RESPONSES as u8).map(|n| filler(0x20 + n)).collect();
    // One past the bound: whatever it says, it is not read.
    full.insert(0, good(&pki));
    full.push(good(&pki));
    let mut material = responses(&full);
    assert_eq!(material.responses.len(), MAX_RESPONSES);
    assert_eq!(material.dropped, 1);
    assert_eq!(
        standing(&ask(&pki, &material, attested())),
        (Status::Unchecked, Some(Gap::Bound))
    );
    // A revocation found stands, whatever else went unread.
    material.list(crl(
        &pki,
        &[Listed {
            certificate: &pki.subject,
            at: T - DAY,
            reason: None,
        }],
        &CrlFaults::default(),
    ));
    assert_eq!(
        standing(&ask(&pki, &material, attested())).0,
        Status::Revoked
    );
    let _ = other;
}

#[test]
fn data_that_would_not_read_turns_none_into_not_checked() {
    let pki = pki();
    let mut material = Material::default();
    material.response(b"not a response");
    material.list(b"not a list".to_vec());
    assert_eq!(material.unread, 1, "the response is refused on the way in");
    let answer = ask(&pki, &material, attested());
    assert_eq!(
        standing(&answer),
        (Status::Unchecked, Some(Gap::Unreadable))
    );
    // The control: no data at all is none.
    assert_eq!(
        standing(&ask(&pki, &Material::default(), attested())),
        (Status::None, None)
    );
}

#[test]
fn material_refuses_what_is_over_its_size_bounds() {
    let mut material = Material::default();
    material.response(&vec![0x30; MAX_RESPONSE_BYTES + 1]);
    material.list(vec![0x30; MAX_LIST_BYTES + 1]);
    material.certificate(vec![0x30; crate::trust::MAX_CERTIFICATE_BYTES + 1]);
    assert_eq!(material.unread, 3);
    assert!(material.responses.is_empty() && material.lists.is_empty());
    assert!(material.certificates.is_empty());
    // And the count bounds: one past each is dropped, and counted.
    let mut material = Material::default();
    for n in 0..=MAX_LISTS {
        material.list(vec![0x30, 0x01, n as u8]);
    }
    for n in 0..=MAX_DSS_CERTIFICATES {
        material.certificate(vec![0x30, 0x01, n as u8]);
    }
    assert_eq!(material.lists.len(), MAX_LISTS);
    assert_eq!(material.certificates.len(), MAX_DSS_CERTIFICATES);
    assert_eq!(material.dropped, 2);
}

#[test]
fn the_adobe_archival_attribute_is_read_for_lists_and_responses() {
    // RevocationInfoArchival { [0] { list }, [1] { response } }, by hand.
    let pki = pki();
    let list = crl(&pki, &[], &CrlFaults::default());
    let response = good(&pki);
    let tlv = |tag: u8, content: &[u8]| -> Vec<u8> {
        let mut out = vec![tag];
        let len = content.len();
        if len < 0x80 {
            out.push(len as u8);
        } else {
            let bytes: Vec<u8> = len
                .to_be_bytes()
                .into_iter()
                .skip_while(|b| *b == 0)
                .collect();
            out.push(0x80 | bytes.len() as u8);
            out.extend(bytes);
        }
        out.extend_from_slice(content);
        out
    };
    let archival = tlv(
        0x30,
        &[
            tlv(0xa0, &tlv(0x30, &list)),
            tlv(0xa1, &tlv(0x30, &response)),
        ]
        .concat(),
    );
    let mut material = Material::default();
    archival_into(&archival, &mut material);
    assert_eq!((material.lists.len(), material.responses.len()), (1, 1));
    assert_eq!(material.unread, 0);
    let mut broken = Material::default();
    archival_into(b"\x30\x03\x02\x01\x00", &mut broken);
    assert_eq!(broken.unread, 1);
}

#[test]
fn first_element_is_the_bytes_as_written() {
    let pki = pki();
    let list = crl(&pki, &[], &CrlFaults::default());
    let parsed = CertificateList::from_der(&list).expect("a list");
    assert_eq!(
        first_element(&list),
        Some(parsed.tbs_cert_list.to_der().expect("tbs"))
    );
}

// ------------------------------------------------------------ the oracle

/// Writes minted data for OpenSSL to judge, into `TPDF_REVOCATION_OUT`.
///
/// Not a gate: an instrument, run by hand (`BUILD.md`, *Revocation*), whose
/// output `openssl ocsp -respin` and `openssl crl -verify` read. Each file is
/// named for what tpdf answers about it, so the two can be put side by side.
#[test]
#[ignore = "an instrument: writes files for OpenSSL, run by hand"]
fn write_minted_data_for_openssl() {
    let Ok(out) = std::env::var("TPDF_REVOCATION_OUT") else {
        println!("[SKIP] set TPDF_REVOCATION_OUT to a directory");
        return;
    };
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out).expect("the directory");
    let pki = pki();
    let responder = pki.ca.responder(0x63);
    let stranger = pki.ca.issue("tpdf test not a responder", 0x64, 0x71, None);
    let pem = |der: &[u8]| {
        use std::fmt::Write as _;
        let b64 = base64(der);
        let mut text = String::from("-----BEGIN CERTIFICATE-----\n");
        for line in b64.as_bytes().chunks(64) {
            let _ = writeln!(text, "{}", std::str::from_utf8(line).expect("ascii"));
        }
        text.push_str("-----END CERTIFICATE-----\n");
        text
    };
    let write = |name: &str, bytes: &[u8]| std::fs::write(out.join(name), bytes).expect("written");
    write("ca.pem", pem(&pki.ca.certificate).as_bytes());
    write("subject.pem", pem(&pki.subject).as_bytes());
    write("responder.pem", pem(&responder.certificate).as_bytes());
    write("stranger.pem", pem(&stranger.certificate).as_bytes());
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("ocsp-good", good(&pki)),
        (
            "ocsp-revoked",
            ocsp(
                &pki,
                Says::Revoked {
                    at: T - DAY,
                    reason: Some(1),
                },
                Responder::Issuer,
                &OcspFaults::default(),
            ),
        ),
        (
            "ocsp-unknown",
            ocsp(
                &pki,
                Says::Unknown,
                Responder::Issuer,
                &OcspFaults::default(),
            ),
        ),
        (
            "ocsp-delegated",
            ocsp(
                &pki,
                Says::Good,
                Responder::Delegated(&responder),
                &OcspFaults::default(),
            ),
        ),
        (
            "ocsp-unauthorised",
            ocsp(
                &pki,
                Says::Good,
                Responder::Delegated(&stranger),
                &OcspFaults::default(),
            ),
        ),
        (
            "ocsp-corrupt",
            ocsp(
                &pki,
                Says::Good,
                Responder::Issuer,
                &OcspFaults {
                    corrupt_signature: true,
                    ..OcspFaults::default()
                },
            ),
        ),
        (
            "ocsp-wrong-certid",
            ocsp(
                &pki,
                Says::Good,
                Responder::Issuer,
                &OcspFaults {
                    wrong_cert_id: true,
                    ..OcspFaults::default()
                },
            ),
        ),
    ];
    for (name, der) in &cases {
        let answer = ask(&pki, &responses(std::slice::from_ref(der)), attested());
        write(&format!("{name}.der"), der);
        println!("{name}: tpdf {:?} {:?}", answer.standing, answer.why);
    }
    let listed = [Listed {
        certificate: &pki.subject,
        at: T - DAY,
        reason: Some(1),
    }];
    let lists_made: Vec<(&str, Vec<u8>)> = vec![
        ("crl-empty", crl(&pki, &[], &CrlFaults::default())),
        ("crl-listed", crl(&pki, &listed, &CrlFaults::default())),
        (
            "crl-corrupt",
            crl(
                &pki,
                &[],
                &CrlFaults {
                    corrupt_signature: true,
                    ..CrlFaults::default()
                },
            ),
        ),
        (
            "crl-wrong-issuer",
            crl(
                &pki,
                &[],
                &CrlFaults {
                    wrong_issuer: true,
                    ..CrlFaults::default()
                },
            ),
        ),
    ];
    for (name, der) in &lists_made {
        let answer = ask(&pki, &lists(std::slice::from_ref(der)), attested());
        write(&format!("{name}.der"), der);
        println!("{name}: tpdf {:?} {:?}", answer.standing, answer.why);
    }
}

/// Standard base64, for the PEM the instrument writes.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| n << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Judges real revocation data fetched by hand, from `TPDF_REVOCATION_REAL`:
/// one directory per authority holding `subject.der`, `issuer.der`,
/// `ocsp.der` and `crl.der` (`BUILD.md`, *Revocation*). The response and the
/// list are judged apart, at the present moment, so each can be held to what
/// `openssl ocsp -respin` and `openssl crl -verify` said about it.
#[test]
#[ignore = "an instrument: reads data fetched from real authorities, run by hand"]
fn judge_real_data() {
    let Ok(root) = std::env::var("TPDF_REVOCATION_REAL") else {
        println!("[SKIP] set TPDF_REVOCATION_REAL to a directory");
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_secs();
    let mut entries: Vec<_> = std::fs::read_dir(&root)
        .expect("the directory")
        .filter_map(Result::ok)
        .filter(|e| e.path().join("subject.der").exists())
        .collect();
    entries.sort_by_key(std::fs::DirEntry::path);
    assert!(!entries.is_empty(), "no authority directories in {root}");
    for entry in entries {
        let dir = entry.path();
        let read = |name: &str| std::fs::read(dir.join(name)).expect(name);
        let subject = parsed(&read("subject.der"));
        let issuer = parsed(&read("issuer.der"));
        let moment = Moment {
            basis: Basis::Now,
            at: now,
        };
        for (what, material) in [
            ("ocsp", {
                let mut m = Material::default();
                m.response(&read("ocsp.der"));
                m
            }),
            ("crl", {
                let mut m = Material::default();
                m.list(read("crl.der"));
                m
            }),
        ] {
            let pool = Pool::new(&material);
            let answer = judge(
                &subject,
                std::slice::from_ref(&issuer),
                &[&pool],
                moment,
                now,
                &mut crate::integrity::MAX_HASHED.clone(),
            );
            println!(
                "{} {what}: {:?} {:?} issued {} next {}",
                dir.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                answer.standing,
                answer.why,
                answer.issued,
                answer.next
            );
        }
    }
}

// ------------------------------------------ guards a mutation needed a test for

#[test]
fn a_response_from_a_responder_out_of_its_dates_is_not_authorised() {
    // Delegated, and its certificate ended before the response was produced.
    let pki = pki();
    let lapsed = pki.ca.responder_dated(0x6c, T - 90 * DAY, T - 30 * DAY);
    let response = ocsp(
        &pki,
        Says::Good,
        Responder::Delegated(&lapsed),
        &OcspFaults::default(),
    );
    assert_eq!(
        standing(&ask(&pki, &responses(&[response]), attested())),
        (Status::Unchecked, Some(Gap::Unauthorised))
    );
}

#[test]
fn a_response_under_the_issuers_name_and_another_key_is_not_about_this_certificate() {
    // Same name, same serial, another key: the CertID's key hash is another
    // issuer's, so this is data about some other authority's certificate.
    let pki = pki();
    let impostor = TestCa::new("tpdf test revocation authority", 0x67);
    let response = mint_ocsp(
        &pki.subject,
        &impostor,
        Says::Revoked {
            at: T - DAY,
            reason: Some(1),
        },
        T - 3_600,
        Some(T + 7 * DAY),
        Responder::Issuer,
        &OcspFaults::default(),
    );
    assert_eq!(
        standing(&ask(&pki, &responses(&[response]), attested())),
        (Status::None, None)
    );
}

#[test]
fn another_authoritys_response_about_the_same_serial_is_not_data_about_this_one() {
    // With the issuer not in hand, a response is attributed to this
    // certificate by the issuer's name and the serial; another authority's
    // response about its own serial 5 is neither.
    let pki = pki();
    let other = TestCa::new("tpdf test another authority", 0x65);
    let theirs = other
        .issue("tpdf test their signer", 0x6d, 5, None)
        .certificate;
    let response = mint_ocsp(
        &theirs,
        &other,
        Says::Good,
        T - 3_600,
        Some(T + 7 * DAY),
        Responder::Issuer,
        &OcspFaults::default(),
    );
    let pool = Pool::new(&responses(&[response]));
    let answer = judge(
        &parsed(&pki.subject),
        &[],
        &[&pool],
        attested(),
        NOW,
        &mut u64::MAX.clone(),
    );
    assert_eq!(standing(&answer), (Status::None, None));
}

#[test]
fn a_list_whose_scope_leaves_the_certificate_out_says_nothing_about_it() {
    // A list of authorities' certificates only, and a partition the
    // certificate does not name: each is silent about it, even listing it.
    let pki = pki();
    let listed = [Listed {
        certificate: &pki.subject,
        at: T - DAY,
        reason: None,
    }];
    for faults in [
        CrlFaults {
            only_authorities: true,
            ..CrlFaults::default()
        },
        CrlFaults {
            partition: true,
            ..CrlFaults::default()
        },
    ] {
        for entries in [&listed[..], &[]] {
            let list = crl(&pki, entries, &faults);
            assert_eq!(
                standing(&ask(&pki, &lists(&[list]), attested())),
                (Status::None, None),
                "{faults:?}"
            );
        }
    }
}

#[test]
fn a_list_signed_by_a_key_not_permitted_to_sign_lists_is_not_authorised() {
    let ca = TestCa::without_list_signing("tpdf test certificate-only authority", 0x6e);
    let subject = ca.issue("tpdf test signer", 0x62, 5, None).certificate;
    let pki = Pki { ca, subject };
    let list = crl(&pki, &[], &CrlFaults::default());
    assert_eq!(
        standing(&ask(&pki, &lists(&[list]), attested())),
        (Status::Unchecked, Some(Gap::Unauthorised))
    );
}

#[test]
fn a_list_listing_every_certificate_but_this_one_is_not_a_revocation_of_it() {
    // The control for the serial lookup: a list whose one entry is another
    // certificate of the same issuer.
    let pki = pki();
    let other = pki
        .ca
        .issue("tpdf test bystander", 0x68, 6, None)
        .certificate;
    let list = crl(
        &pki,
        &[Listed {
            certificate: &other,
            at: T - DAY,
            reason: Some(1),
        }],
        &CrlFaults::default(),
    );
    assert_eq!(
        standing(&ask(&pki, &lists(&[list]), attested())),
        (Status::Good, None)
    );
}

#[test]
fn a_basic_response_over_its_bound_is_refused_on_the_way_in_too() {
    // The CMS `crls` set hands `basic` a BasicOCSPResponse directly, past the
    // size check `response` makes; `basic` carries its own. Found by a
    // mutation that survived: every earlier test reached `basic` through
    // `response`, whose check had already refused.
    let mut material = Material::default();
    material.basic(vec![0x30; MAX_RESPONSE_BYTES + 1]);
    assert_eq!((material.unread, material.responses.len()), (1, 0));
    material.basic(vec![0x30; MAX_RESPONSE_BYTES]);
    assert_eq!((material.unread, material.responses.len()), (1, 1));
}

#[test]
fn a_signatures_own_cms_carries_lists_responses_and_the_adobe_attribute() {
    // A token's SignedData, given a `crls` set holding a list and a response
    // in `id-ri-ocsp-response` form, and the Adobe archival attribute among
    // its signer's signed attributes. The signature over them no longer
    // holds, which `of_cms` does not ask: it only gathers.
    use crate::integrity::test_tsa::{basic_of, mint, Imprint, TestTsa};
    use cms::revocation::{OtherRevocationInfoFormat, RevocationInfoChoice, RevocationInfoChoices};
    let pki = pki();
    let list = crl(&pki, &[], &CrlFaults::default());
    let response = good(&pki);
    let token = mint(Imprint::Sha256, &[0; 32], None, T, &TestTsa::new());
    let info = cms::content_info::ContentInfo::from_der(&token).expect("a CMS");
    let mut signed: cms::signed_data::SignedData = info.content.decode_as().expect("signed data");
    let choices = vec![
        RevocationInfoChoice::Crl(CertificateList::from_der(&list).expect("a list")),
        RevocationInfoChoice::Other(OtherRevocationInfoFormat {
            other_format: x509_cert::spki::AlgorithmIdentifierOwned {
                oid: der::asn1::ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.16.2"),
                parameters: None,
            },
            other: der::Any::from_der(&basic_of(&response)).expect("a response"),
        }),
        // A format tpdf does not read is counted, not taken for a response.
        RevocationInfoChoice::Other(OtherRevocationInfoFormat {
            other_format: x509_cert::spki::AlgorithmIdentifierOwned {
                oid: der::asn1::ObjectIdentifier::new_unwrap("1.3.6.1.4.1.0.8"),
                parameters: None,
            },
            other: der::asn1::Null.into(),
        }),
    ];
    signed.crls = Some(RevocationInfoChoices(
        der::asn1::SetOfVec::try_from(choices).expect("a set"),
    ));
    let rebuilt = |signed: &cms::signed_data::SignedData| {
        cms::content_info::ContentInfo {
            content_type: info.content_type,
            content: der::Any::encode_from(signed).expect("signed data"),
        }
        .to_der()
        .expect("a CMS")
    };
    let material = Material::of_cms(&rebuilt(&signed));
    assert_eq!(
        (
            material.lists.len(),
            material.responses.len(),
            material.unread
        ),
        (1, 1, 1)
    );
    assert!(!material.certificates.is_empty(), "its certificates too");

    // The Adobe attribute alone, with a second list inside it.
    let other = mint_crl(&pki.ca, &[], T, Some(T + DAY), &CrlFaults::default());
    let tlv = |tag: u8, content: &[u8]| -> Vec<u8> {
        let mut out = vec![tag];
        let len = content.len();
        if len < 0x80 {
            out.push(len as u8);
        } else {
            let bytes: Vec<u8> = len
                .to_be_bytes()
                .into_iter()
                .skip_while(|b| *b == 0)
                .collect();
            out.push(0x80 | bytes.len() as u8);
            out.extend(bytes);
        }
        out.extend_from_slice(content);
        out
    };
    let archival = tlv(0x30, &tlv(0xa0, &tlv(0x30, &other)));
    signed.crls = None;
    let mut infos: Vec<_> = signed.signer_infos.0.iter().cloned().collect();
    let mut attributes: Vec<_> = infos[0]
        .signed_attrs
        .clone()
        .expect("signed attributes")
        .into_vec();
    attributes.push(x509_cert::attr::Attribute {
        oid: der::asn1::ObjectIdentifier::new_unwrap("1.2.840.113583.1.1.8"),
        values: der::asn1::SetOfVec::try_from(vec![der::Any::from_der(&archival).expect("any")])
            .expect("a set"),
    });
    infos[0].signed_attrs = Some(der::asn1::SetOfVec::try_from(attributes).expect("a set"));
    signed.signer_infos =
        cms::signed_data::SignerInfos(der::asn1::SetOfVec::try_from(infos).expect("a set"));
    let material = Material::of_cms(&rebuilt(&signed));
    assert_eq!((material.lists.len(), material.unread), (1, 0));
}
