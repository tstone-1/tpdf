//! Gathering long-term validation data against a fake PKI on 127.0.0.1.
//!
//! Every test is offline: the certificate authority, its OCSP responders and
//! its revocation lists are `integrity/test_tsa.rs`'s [`Pki`], a
//! `TcpListener` minting with increment C1's minters at the addresses the
//! test certificates publish. The signature is made here with the test
//! signer's key and timestamped with the publishing test authority, and the
//! `/DSS` is appended by `save::Here` --- the same `sign_dss::extend` a worker
//! runs --- so each test covers the whole second half of a B-LT signing.

use std::time::Duration;

use super::*;
use crate::integrity::test_tsa::{
    mint, mint_ocsp, with_dss, Imprint, OcspFaults, Pki, Plan, Responder, Serve, Status as Says,
};
use crate::sign_cms::testkeys::{plain_pdf, Soft};

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A signature made, timestamped and sealed with `pki`'s signer and
/// authority: the B-T document, its CMS and its field.
struct Sealed {
    bytes: Vec<u8>,
    cms: Vec<u8>,
    field: String,
}

fn sealed(pki: &Pki) -> Sealed {
    let at = now();
    let original = plain_pdf();
    let key = Soft::p256(pki.signer.seed);
    let unsigned = crate::sign_prepare::prepare(original.clone(), at, None).expect("prepared");
    let made = crate::sign_cms::sign(
        original,
        unsigned,
        at,
        &pki.signer.certificate,
        &pki.chain,
        &key,
    )
    .expect("made");
    let value = made.value().expect("a value");
    let token = mint(
        Imprint::Sha256,
        &Imprint::Sha256.digest(&value),
        Some(&[5; 8]),
        at,
        &pki.tsa,
    );
    let cms = made.stamped(&token).expect("stamped");
    let field = made.field.clone();
    let bytes = made.seal(Some(cms.clone())).expect("sealed");
    Sealed { bytes, cms, field }
}

/// The real client, with a short total so a silent server costs a second.
fn quick(
    url: &url::Url,
    body: Option<(&str, Vec<u8>)>,
    limits: &tsa::Limits,
) -> Result<Vec<u8>, tsa::Refusal> {
    let limits = tsa::Limits {
        total: limits.total.min(Duration::from_secs(1)),
        ..*limits
    };
    fetch_blocking(url, body, &limits)
}

fn no_os_chain(_: &[u8], _: &[Vec<u8>]) -> Vec<Vec<u8>> {
    Vec::new()
}

/// [`Vouch`] as a reader whose store trusts the test authority's root has it:
/// the reader's own rule, that root the only anchor --- no keychain or
/// certificate store is touched.
fn anchored(token: &[u8], now: u64) -> crate::trust::Trust {
    let root = crate::integrity::test_tsa::TestTsa::new().root;
    crate::trust::of_blob_for(
        token,
        crate::trust::Purpose::Timestamping,
        now,
        crate::trust::Anchors::Only(std::slice::from_ref(&root)),
    )
}

/// [`Vouch`] for a reader who trusts no root at all: an authority substituted
/// by somebody on the path, as far as this computer can tell.
fn stranger(token: &[u8], now: u64) -> crate::trust::Trust {
    crate::trust::of_blob_for(
        token,
        crate::trust::Purpose::Timestamping,
        now,
        crate::trust::Anchors::Only(&[]),
    )
}

/// The archive timestamp, from the test authority: a token over the pieces,
/// now, as `tsa::ask_over_range` would return one.
fn archive(pieces: &[&[u8]]) -> Result<Vec<u8>, String> {
    let covered: Vec<u8> = pieces.concat();
    Ok(mint(
        Imprint::Sha256,
        &Imprint::Sha256.digest(&covered),
        None,
        now(),
        &crate::integrity::test_tsa::TestTsa::new(),
    ))
}

/// The whole second half: planned, gathered, appended and checked.
fn extended(signed: &Sealed) -> Result<Vec<u8>, Refusal> {
    extend(
        &signed.bytes,
        &signed.cms,
        &signed.field,
        now(),
        &crate::save::Here,
        &no_os_chain,
        &anchored,
        &mut quick,
        &mut archive,
    )
}

fn good() -> Plan {
    Plan {
        signer_ocsp: Some(Serve::Good),
        authority_ocsp: Some(Serve::Good),
        ..Plan::default()
    }
}

/// The new signature, as the properties dialog reads the file.
fn read(bytes: &[u8]) -> crate::docinfo::Signature {
    crate::docinfo::scan(bytes, 1, None)
        .expect("scanned")
        .signatures
        .into_iter()
        .find(|s| s.signed)
        .expect("a signature")
}

/// The `/DSS` streams of `bytes`, by array.
/// Streams' contents, as DER.
type Streams = Vec<Vec<u8>>;

fn dss(bytes: &[u8]) -> (Streams, Streams, Streams) {
    let document = lopdf::Document::load_mem(bytes).expect("parses");
    let catalog = document.catalog().expect("a catalog");
    let dss = crate::encoding::resolve(&document, catalog.get(b"DSS").expect("a /DSS"))
        .as_dict()
        .expect("a dictionary")
        .clone();
    let streams = |key: &[u8]| -> Vec<Vec<u8>> {
        dss.get(key)
            .ok()
            .and_then(|a| {
                crate::encoding::resolve(&document, a)
                    .as_array()
                    .ok()
                    .cloned()
            })
            .unwrap_or_default()
            .iter()
            .map(|item| {
                crate::encoding::resolve(&document, item)
                    .as_stream()
                    .expect("a stream")
                    .content
                    .clone()
            })
            .collect()
    };
    (streams(b"Certs"), streams(b"OCSPs"), streams(b"CRLs"))
}

// ------------------------------------------------------------ the controls

#[test]
fn a_b_lt_signature_is_written_and_reads_back_good_for_signer_and_authority() {
    let pki = Pki::start(good());
    let signed = sealed(&pki);
    let bytes = extended(&signed).expect("extended");
    assert_eq!(
        &bytes[..signed.bytes.len()],
        &signed.bytes[..],
        "a revision appended"
    );
    let signature = read(&bytes);
    assert_eq!(
        signature.integrity.as_ref().map(|i| i.verdict),
        Some(crate::integrity::Verdict::Intact)
    );
    let signer = signature.revocation.expect("the signer's answer");
    assert_eq!(
        (signer.standing, signer.source),
        (Status::Good, Some(Source::Ocsp)),
        "{signer:?}"
    );
    let stamp = signature.timestamp.expect("a timestamp");
    let authority = stamp.revocation.expect("the authority's answer");
    assert_eq!(authority.standing, Status::Good, "{authority:?}");
    assert_eq!(pki.paths(), ["/ocsp/signer", "/ocsp/authority"]);
    let (certs, ocsps, crls) = dss(&bytes);
    assert_eq!((ocsps.len(), crls.len()), (2, 0));
    // C1's reader takes issuers only from the document, so every one is there.
    for issuer in [&pki.root.certificate, &pki.tsa.root] {
        assert!(
            certs.contains(issuer),
            "an issuer is missing from /DSS /Certs"
        );
    }
    assert!(certs.contains(&pki.signer.certificate));
    assert!(certs.contains(&pki.tsa.certificate));
}

#[test]
fn the_ocsp_request_names_the_certificate_by_a_sha1_cert_id() {
    use sha1::Digest as _;
    let pki = Pki::start(good());
    extended(&sealed(&pki)).expect("extended");
    let asked = pki.asked.lock().expect("the record").clone();
    let (_, body) = asked.first().expect("a request");
    let request = x509_ocsp::OcspRequest::from_der(body).expect("an OCSPRequest");
    let [one] = request.tbs_request.request_list.as_slice() else {
        panic!("one CertID");
    };
    let signer = Certificate::from_der(&pki.signer.certificate).expect("the signer");
    let root = Certificate::from_der(&pki.root.certificate).expect("the root");
    let id = &one.req_cert;
    assert_eq!(id.hash_algorithm.oid.to_string(), ID_SHA1);
    assert_eq!(id.serial_number, signer.tbs_certificate.serial_number);
    assert_eq!(
        id.issuer_name_hash.as_bytes(),
        sha1::Sha1::digest(signer.tbs_certificate.issuer.to_der().expect("name")).as_slice()
    );
    assert_eq!(
        id.issuer_key_hash.as_bytes(),
        sha1::Sha1::digest(
            root.tbs_certificate
                .subject_public_key_info
                .subject_public_key
                .raw_bytes()
        )
        .as_slice()
    );
}

// ------------------------------------------------------------ the fallback

#[test]
fn a_list_is_fetched_when_the_responder_does_not_answer() {
    for down in [Serve::Status(500), Serve::TryLater, Serve::Garbage] {
        let pki = Pki::start(Plan {
            signer_ocsp: Some(down),
            signer_crl: Some(Serve::Good),
            ..good()
        });
        let bytes = extended(&sealed(&pki)).unwrap_or_else(|why| panic!("{down:?}: {why:?}"));
        let signer = read(&bytes).revocation.expect("an answer");
        assert_eq!(
            (signer.standing, signer.source),
            (Status::Good, Some(Source::Crl)),
            "{down:?}"
        );
        assert_eq!(
            pki.paths(),
            ["/ocsp/signer", "/crl/signer.crl", "/ocsp/authority"],
            "{down:?}"
        );
    }
}

#[test]
fn a_certificate_naming_only_a_list_gets_the_list() {
    let pki = Pki::start(Plan {
        signer_ocsp: None,
        signer_crl: Some(Serve::Good),
        authority_ocsp: None,
        authority_crl: Some(Serve::Good),
        ..Plan::default()
    });
    let bytes = extended(&sealed(&pki)).expect("extended");
    let (_, ocsps, crls) = dss(&bytes);
    assert_eq!((ocsps.len(), crls.len()), (0, 2));
    assert_eq!(
        read(&bytes).revocation.map(|r| r.standing),
        Some(Status::Good)
    );
}

#[test]
fn an_ldap_point_is_skipped_and_the_http_one_asked() {
    let pki = Pki::start(Plan {
        signer_ocsp: None,
        signer_crl: Some(Serve::Good),
        ldap_first: true,
        ..good()
    });
    extended(&sealed(&pki)).expect("extended");
    assert!(pki.paths().contains(&"/crl/signer.crl".to_string()));
}

#[test]
fn a_b_lt_signature_under_an_intermediate_reads_back_good_for_the_whole_chain() {
    // The round trip that proves the writer and the reader walk one chain:
    // what the gathering asked about is what the reading judges.
    let pki = Pki::start(Plan {
        intermediate: true,
        intermediate_ocsp: Some(Serve::Good),
        ..good()
    });
    let bytes = extended(&sealed(&pki)).expect("extended");
    let signature = read(&bytes);
    let chain = signature.revocation_chain.expect("a chain");
    assert_eq!(
        (chain.standing, chain.certificates.len(), chain.end),
        (
            crate::revocation::Status::Good,
            2,
            crate::revocation::chain::End::Root
        ),
        "{chain:?}"
    );
}

#[test]
fn the_archive_timestamp_covers_the_signature_and_its_validation_data() {
    // PAdES B-LTA: the last revision is a document timestamp over the whole,
    // the /DSS included, and it reads back intact.
    let pki = Pki::start(good());
    let signed = sealed(&pki);
    let bytes = extended(&signed).expect("extended");
    let found: Vec<crate::docinfo::Signature> = crate::docinfo::scan(&bytes, 1, None)
        .expect("scanned")
        .signatures
        .into_iter()
        .filter(|s| s.signed)
        .collect();
    let [ours, archive] = found.as_slice() else {
        panic!("a signature and an archive timestamp: {found:?}");
    };
    assert_eq!(ours.field, signed.field);
    assert_eq!(archive.kind, "ETSI.RFC3161");
    // The whole of what `--long-term` is for, by name: the signature has every
    // part of B-LTA, and the timestamp that seals it has no level of its own.
    assert_eq!(ours.pades, Some(crate::pades::Level::Lta), "{ours:?}");
    assert_eq!(archive.pades, None);
    assert!(archive.covers_whole_file, "{archive:?}");
    assert_eq!(
        archive.integrity.as_ref().map(|i| i.verdict),
        Some(crate::integrity::Verdict::Intact)
    );
    // It covers the /DSS: the signature's own range ends before it, and the
    // archive's reaches the end of the file.
    let (certificates, _, _) = dss(&bytes);
    assert!(!certificates.is_empty(), "the /DSS is there");
    assert!(ours.appended_bytes > 0 && !ours.covers_whole_file);
    assert!(archive.covered_bytes > ours.covered_bytes);
    // What was appended after the signature rewrote one page, and only to
    // list the timestamp's field: the readout says so rather than "1 page
    // was rewritten", which reads as a change to the page.
    let appendix = ours.appendix.as_ref().expect("an appendix");
    assert_eq!(
        (appendix.pages_touched, appendix.pages_listing.as_slice()),
        (
            1,
            [crate::docinfo::PageListing {
                page: 1,
                timestamp: true
            }]
            .as_slice()
        ),
        "{appendix:?}"
    );
    // `verify` and `info` head it as a document timestamp, count it apart,
    // and word its certificate as the authority's: nobody signed twice, and
    // the certificate names no person.
    let reports: Vec<_> = found
        .iter()
        .map(crate::cli::verify::signature_report)
        .collect();
    assert_eq!(
        crate::cli::verify::counted(&reports),
        "1 signature and 1 document timestamp"
    );
    let (said, stamped) = (
        crate::cli::verify::signature_text(&reports[0]),
        crate::cli::verify::signature_text(&reports[1]),
    );
    assert!(
        said.starts_with(&format!("  {} --- ", signed.field)),
        "{said}"
    );
    assert!(said.contains("\n    Trust: "), "{said}");
    assert!(
        stamped.starts_with(&format!("  Document timestamp {} --- ", archive.field)),
        "{stamped}"
    );
    assert_eq!(
        stamped.matches("    Timestamp authority: ").count(),
        1,
        "{stamped}"
    );
    assert!(
        !stamped.contains("    Trust: ")
            && !stamped.contains("signer")
            && !stamped.contains("the person the certificate names"),
        "{stamped}"
    );
    assert!(stamped.contains("\n    Timestamped: "), "{stamped}");
    // And the report after signing names it as the archive, not as an
    // earlier signature.
    let report = crate::sign_cms::report(String::new(), signed.field.clone(), found.clone());
    let flags: Vec<(bool, bool)> = report
        .signatures
        .iter()
        .map(|c| (c.ours, c.archive))
        .collect();
    assert_eq!(flags, [(true, false), (false, true)]);
}

#[test]
fn an_archive_timestamp_that_does_not_come_writes_nothing() {
    let pki = Pki::start(good());
    let signed = sealed(&pki);
    for (what, answer) in [
        (
            "the authority declined",
            Err::<Vec<u8>, String>("the authority at 127.0.0.1 declined".into()),
        ),
        (
            "a token over other bytes",
            Ok(mint(
                Imprint::Sha256,
                &Imprint::Sha256.digest(b"other bytes"),
                None,
                now(),
                &crate::integrity::test_tsa::TestTsa::new(),
            )),
        ),
    ] {
        let why = extend(
            &signed.bytes,
            &signed.cms,
            &signed.field,
            now(),
            &crate::save::Here,
            &no_os_chain,
            &anchored,
            &mut quick,
            &mut |_: &[&[u8]]| answer.clone(),
        )
        .expect_err(what);
        assert!(matches!(why, Refusal::Archive(_)), "{what}: {why:?}");
        assert!(!why.revoked(), "{what}");
        assert!(
            why.sentence().contains("archive timestamp"),
            "{what}: {why:?}"
        );
    }
}

#[test]
fn an_intermediate_is_asked_about_and_carried() {
    let pki = Pki::start(Plan {
        intermediate: true,
        intermediate_ocsp: Some(Serve::Good),
        ..good()
    });
    let bytes = extended(&sealed(&pki)).expect("extended");
    assert_eq!(
        pki.paths(),
        ["/ocsp/signer", "/ocsp/intermediate", "/ocsp/authority"]
    );
    let (certs, ocsps, _) = dss(&bytes);
    assert_eq!(ocsps.len(), 3);
    let intermediate = &pki.intermediate.as_ref().expect("one").certificate;
    assert!(certs.contains(intermediate) && certs.contains(&pki.root.certificate));
}

// ------------------------------------------------------------ the refusals

#[test]
fn a_revoked_signer_is_refused_and_no_list_is_shopped_for() {
    let pki = Pki::start(Plan {
        signer_ocsp: Some(Serve::Revoked),
        signer_crl: Some(Serve::Good),
        ..good()
    });
    let why = extended(&sealed(&pki)).expect_err("refused");
    assert!(why.revoked(), "{why:?}");
    let sentence = why.sentence();
    assert!(
        sentence.contains("the signer's certificate (tpdf test PKI signer")
            && sentence.contains("revoked")
            && sentence.contains("will not write"),
        "{sentence}"
    );
    assert_eq!(pki.paths(), ["/ocsp/signer"]);
}

#[test]
fn a_revoked_authority_found_by_its_list_is_refused() {
    let pki = Pki::start(Plan {
        authority_ocsp: None,
        authority_crl: Some(Serve::Revoked),
        ..good()
    });
    let why = extended(&sealed(&pki)).expect_err("refused");
    assert!(why.revoked(), "{why:?}");
    assert!(why.sentence().contains("timestamp authority's certificate"));
}

/// Above the signer, the gathering's own judgement is the only guard: the
/// worker's reading judges the signer and the authority, and nobody else.
#[test]
fn a_revoked_intermediate_is_refused_before_anything_is_written() {
    let pki = Pki::start(Plan {
        intermediate: true,
        intermediate_crl: Some(Serve::Revoked),
        ..good()
    });
    let why = extended(&sealed(&pki)).expect_err("refused");
    // Refused by the gathering, naming where the list came from --- not
    // later, by the reading of a file already built: that one would say "the
    // revocation list in the document", and the list would have been written.
    assert!(
        why.revoked()
            && why
                .sentence()
                .contains("the certificate above the signer's")
            && why
                .sentence()
                .contains("the revocation list from 127.0.0.1"),
        "{why:?}"
    );
}

#[test]
fn an_unknown_answer_is_refused() {
    let pki = Pki::start(Plan {
        signer_ocsp: Some(Serve::Unknown),
        signer_crl: Some(Serve::Good),
        ..good()
    });
    let why = extended(&sealed(&pki)).expect_err("refused");
    assert!(matches!(why, Refusal::Unknown { .. }), "{why:?}");
    assert!(!why.revoked());
    assert_eq!(pki.paths(), ["/ocsp/signer"]);
}

#[test]
fn an_answer_that_does_not_check_out_is_refused() {
    for (serve, words) in [
        (Serve::CorruptSignature, "signature does not check out"),
        (Serve::Stale, "out of date"),
    ] {
        let pki = Pki::start(Plan {
            signer_ocsp: Some(serve),
            signer_crl: Some(Serve::Good),
            ..good()
        });
        let why = extended(&sealed(&pki)).expect_err("refused");
        assert!(
            matches!(why, Refusal::DoesNotCheck { .. }) && why.sentence().contains(words),
            "{serve:?}: {why:?}"
        );
        // Not answered by shopping for another answer.
        assert_eq!(pki.paths(), ["/ocsp/signer"], "{serve:?}");
    }
    // The list's own signature, the same.
    let pki = Pki::start(Plan {
        signer_ocsp: None,
        signer_crl: Some(Serve::CorruptSignature),
        ..good()
    });
    let why = extended(&sealed(&pki)).expect_err("refused");
    assert!(why.sentence().contains("revocation list"), "{why:?}");
}

#[test]
fn a_certificate_that_publishes_nothing_is_refused_with_what_would_work() {
    for plan in [
        Plan {
            signer_ocsp: None,
            ..good()
        },
        // An `ldap:` point is not something tpdf fetches.
        Plan {
            signer_ocsp: None,
            ldap_first: true,
            ..good()
        },
    ] {
        let pki = Pki::start(plan);
        let why = extended(&sealed(&pki)).expect_err("refused");
        let sentence = why.sentence();
        assert!(
            matches!(why, Refusal::NotPublished(_))
                && sentence.contains("certificate authority that publishes revocation data")
                && sentence.contains("A timestamp alone works"),
            "{sentence}"
        );
        assert!(pki.paths().is_empty(), "{:?}", pki.paths());
    }
}

#[test]
fn a_responder_that_is_down_silent_or_too_long_leaves_nothing_answered() {
    for (serve, words) in [
        (Serve::Silent, "did not answer"),
        (Serve::Long, "more than 64 KiB"),
        (Serve::Status(404), "HTTP status 404"),
    ] {
        let pki = Pki::start(Plan {
            signer_ocsp: Some(serve),
            ..good()
        });
        let why = extended(&sealed(&pki)).expect_err("refused");
        assert!(
            matches!(why, Refusal::Unanswered { .. }) && why.sentence().contains(words),
            "{serve:?}: {}",
            why.sentence()
        );
    }
    // A list past the bound too.
    let pki = Pki::start(Plan {
        signer_ocsp: None,
        signer_crl: Some(Serve::Long),
        ..good()
    });
    let why = extended(&sealed(&pki)).expect_err("refused");
    assert!(why.sentence().contains("more than 4096 KiB"), "{why:?}");

    // Nobody listening at all.
    let pki = Pki::start(good());
    let signed = sealed(&pki);
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let dead = closed.local_addr().expect("an address").port();
    drop(closed);
    let why = extend(
        &signed.bytes,
        &signed.cms,
        &signed.field,
        now(),
        &crate::save::Here,
        &no_os_chain,
        &anchored,
        &mut |url: &url::Url, body: Option<(&str, Vec<u8>)>, limits: &tsa::Limits| {
            let mut elsewhere = url.clone();
            let _ = elsewhere.set_port(Some(dead));
            // The shipped limits, not `quick`'s one second: Windows reports a
            // closed local port only after about two seconds of retries, so
            // the shorter total read it as a silent server (first Windows CI
            // run, 2026-09-28; `tsa::tests::a_refused_connection_is_unreachable`).
            fetch_blocking(&elsewhere, body, limits)
        },
        &mut archive,
    )
    .expect_err("refused");
    assert!(why.sentence().contains("could not be reached"), "{why:?}");
}

#[test]
fn the_whole_gathering_is_bounded_in_time() {
    let pki = Pki::start(good());
    let signed = sealed(&pki);
    let (subjects, certificates) = plan(&signed.cms, &no_os_chain).expect("planned");
    let why =
        gather(&subjects, certificates, now(), Duration::ZERO, &mut quick).expect_err("refused");
    assert!(why.sentence().contains("took longer than"), "{why:?}");
    assert!(pki.paths().is_empty());
}

#[test]
fn long_term_data_needs_a_timestamp() {
    let pki = Pki::start(good());
    let at = now();
    let original = plain_pdf();
    let unsigned = crate::sign_prepare::prepare(original.clone(), at, None).expect("prepared");
    let made = crate::sign_cms::sign(
        original,
        unsigned,
        at,
        &pki.signer.certificate,
        &pki.chain,
        &Soft::p256(pki.signer.seed),
    )
    .expect("made");
    let why = plan(&made.unstamped_blob(), &no_os_chain).expect_err("refused");
    assert_eq!(why, Refusal::NoTimestamp);
}

#[test]
fn an_issuer_nowhere_to_be_found_is_refused() {
    let pki = Pki::start(good());
    let at = now();
    let original = plain_pdf();
    let unsigned = crate::sign_prepare::prepare(original.clone(), at, None).expect("prepared");
    // The chain left out of the signature, and no OS to assemble it.
    let made = crate::sign_cms::sign(
        original,
        unsigned,
        at,
        &pki.signer.certificate,
        &[],
        &Soft::p256(pki.signer.seed),
    )
    .expect("made");
    let value = made.value().expect("a value");
    let token = mint(
        Imprint::Sha256,
        &Imprint::Sha256.digest(&value),
        None,
        at,
        &pki.tsa,
    );
    let cms = made.stamped(&token).expect("stamped");
    let why = plan(&cms, &no_os_chain).expect_err("refused");
    assert!(matches!(why, Refusal::NoIssuer(_)), "{why:?}");
    // The OS's chain is a source of issuers.
    let root = pki.root.certificate.clone();
    plan(&cms, &move |_: &[u8], _: &[Vec<u8>]| vec![root.clone()]).expect("planned");
}

// ------------------------------------------------------------ the last check

#[test]
fn the_check_before_writing_wants_good_for_both_and_names_a_revocation() {
    let pki = Pki::start(good());
    let signed = sealed(&pki);
    let at = now();
    let ocsp = |certificate: &[u8], issuer: &crate::integrity::test_tsa::TestCa, says| {
        mint_ocsp(
            certificate,
            issuer,
            says,
            at - 3_600,
            Some(at + 86_400),
            Responder::Issuer,
            &OcspFaults::default(),
        )
    };
    let tsa_root = crate::integrity::test_tsa::TestCa::of_tsa();
    let certs = [pki.root.certificate.clone(), pki.tsa.root.clone()];
    let found = |signer: Says, authority: Says| {
        let bytes = with_dss(
            &signed.bytes,
            &certs,
            &[
                ocsp(&pki.signer.certificate, &pki.root, signer),
                ocsp(&pki.tsa.certificate, &tsa_root, authority),
            ],
            &[],
        );
        crate::docinfo::scan(&bytes, 1, None)
            .expect("scanned")
            .signatures
    };
    // The control: both good.
    check(&found(Says::Good, Says::Good), &signed.field).expect("good");
    // No data at all: B-T, and not what was asked for.
    let plain = crate::docinfo::scan(&signed.bytes, 1, None)
        .expect("scanned")
        .signatures;
    assert!(matches!(
        check(&plain, &signed.field),
        Err(Refusal::Written(_))
    ));
    // The authority's answer alone missing.
    let revoked = Says::Revoked {
        at: at - 86_400,
        reason: Some(1),
    };
    assert!(matches!(
        check(&found(Says::Good, Says::Unknown), &signed.field),
        Err(Refusal::Written(_))
    ));
    let why = check(&found(revoked, Says::Good), &signed.field).expect_err("revoked");
    assert!(
        why.revoked() && why.sentence().contains("signer's certificate"),
        "{why:?}"
    );
    let why = check(&found(Says::Good, revoked), &signed.field).expect_err("revoked");
    assert!(
        why.revoked() && why.sentence().contains("authority"),
        "{why:?}"
    );
    // Another field is not ours.
    assert!(check(&found(Says::Good, Says::Good), "Signature9").is_err());
    // A certificate above either, read back revoked or not good: the chain
    // the reader judges is refused as the leaf would be.
    for authority in [false, true] {
        for (standing, is_revoked) in [
            (crate::revocation::Status::Revoked, true),
            (crate::revocation::Status::None, false),
        ] {
            let mut above = found(Says::Good, Says::Good);
            let ours = above
                .iter_mut()
                .find(|s| s.field == signed.field)
                .expect("ours");
            let chain = if authority {
                ours.timestamp
                    .as_mut()
                    .and_then(|t| t.revocation_chain.as_mut())
            } else {
                ours.revocation_chain.as_mut()
            }
            .expect("a chain");
            chain.certificates.push(crate::revocation::chain::Judged {
                subject_cn: "tpdf test issuing authority".into(),
                revocation: crate::revocation::Revocation {
                    standing,
                    revoked: "2026-09-01 00:00:00 UTC".into(),
                    ..Default::default()
                },
                ..Default::default()
            });
            chain.standing = standing;
            chain.decided_by = Some(1);
            let why = check(&above, &signed.field).expect_err("refused");
            let whose = if authority {
                "the certificate above the timestamp authority's (tpdf test issuing authority)"
            } else {
                "the certificate above the signer's (tpdf test issuing authority)"
            };
            assert_eq!(why.revoked(), is_revoked, "{why:?}");
            assert!(why.sentence().contains(whose), "{why:?}");
        }
        // A chain past what the reader judges is not a chain read good.
        let mut long = found(Says::Good, Says::Good);
        let ours = long
            .iter_mut()
            .find(|s| s.field == signed.field)
            .expect("ours");
        let chain = if authority {
            ours.timestamp
                .as_mut()
                .and_then(|t| t.revocation_chain.as_mut())
        } else {
            ours.revocation_chain.as_mut()
        }
        .expect("a chain");
        chain.dropped = 1;
        assert!(matches!(
            check(&long, &signed.field),
            Err(Refusal::Written(_))
        ));
    }
    // A signature, or its timestamp, that does not read as intact.
    let mut broken = found(Says::Good, Says::Good);
    for s in &mut broken {
        if let Some(integrity) = &mut s.integrity {
            integrity.verdict = crate::integrity::Verdict::Altered;
        }
    }
    assert!(matches!(
        check(&broken, &signed.field),
        Err(Refusal::Written(_))
    ));
    let mut unstamped = found(Says::Good, Says::Good);
    for s in &mut unstamped {
        if let Some(stamp) = &mut s.timestamp {
            if let Some(integrity) = &mut stamp.integrity {
                integrity.verdict = crate::integrity::Verdict::Broken;
            }
        }
    }
    assert!(matches!(
        check(&unstamped, &signed.field),
        Err(Refusal::Written(_))
    ));
}

// ------------------------------------------------------------ the bounds

#[test]
fn a_certificate_that_needs_no_check_is_not_asked_about() {
    let pki = Pki::start(Plan {
        signer_ocsp: None,
        signer_no_check: true,
        ..good()
    });
    let (subjects, _) = plan(&sealed(&pki).cms, &no_os_chain).expect("planned");
    let whose: Vec<Whose> = subjects.iter().map(|s| s.whose).collect();
    assert_eq!(whose, [Whose::Authority]);
}

#[test]
fn a_self_issued_signer_publishes_nothing_and_says_so() {
    use crate::sign_cms::testkeys::{certificate, Spec};
    let pki = Pki::start(good());
    let at = now();
    let key = Soft::p256(0x33);
    let own = certificate(&key, &Spec::new("A self-made signer"));
    let original = plain_pdf();
    let unsigned = crate::sign_prepare::prepare(original.clone(), at, None).expect("prepared");
    let made = crate::sign_cms::sign(original, unsigned, at, &own, &[], &key).expect("made");
    let value = made.value().expect("a value");
    let token = mint(
        Imprint::Sha256,
        &Imprint::Sha256.digest(&value),
        None,
        at,
        &pki.tsa,
    );
    let why = plan(&made.stamped(&token).expect("stamped"), &no_os_chain).expect_err("refused");
    assert!(
        matches!(&why, Refusal::NotPublished(name) if name.contains("A self-made signer")),
        "{why:?}"
    );
}

#[test]
fn the_data_is_bounded_in_size_before_it_is_read() {
    // A list the body limit admits, which with the certificates is more
    // than tpdf adds.
    let pki = Pki::start(Plan {
        signer_ocsp: None,
        signer_crl: Some(Serve::Bytes(MAX_GATHERED - 16)),
        ..good()
    });
    let why = extended(&sealed(&pki)).expect_err("refused");
    assert!(
        matches!(why, Refusal::Bound(_)) && why.sentence().contains("MiB"),
        "{why:?}"
    );
}

#[test]
fn the_requests_are_bounded_whatever_a_certificate_lists() {
    use crate::integrity::test_tsa::{Published, TestCa};
    let root = TestCa::new("tpdf test many-responder root", 0x21);
    let many = root.issue_publishing(
        "tpdf test many-responder signer",
        0x22,
        3,
        &Published {
            ocsp: (0..40)
                .map(|n| format!("http://127.0.0.1:9/ocsp/{n}"))
                .collect(),
            ..Published::default()
        },
    );
    let subject = Subject {
        certificate: Certificate::from_der(&many.certificate).expect("a certificate"),
        issuer: Certificate::from_der(&root.certificate).expect("the root"),
        whose: Whose::Signer,
    };
    let mut asked = 0usize;
    let why = gather(
        &[subject],
        vec![root.certificate.clone()],
        now(),
        TOTAL,
        &mut |_: &url::Url, _: Option<(&str, Vec<u8>)>, _: &tsa::Limits| {
            asked += 1;
            Err(tsa::Refusal::Unreachable("down".into()))
        },
    )
    .expect_err("refused");
    assert!(why.sentence().contains("requests"), "{why:?}");
    assert_eq!(asked, MAX_REQUESTS);
}

#[test]
fn a_chain_deeper_than_any_real_one_is_refused() {
    use crate::integrity::test_tsa::{Published, TestCa};
    let pki = Pki::start(good());
    let root = TestCa::new("tpdf test deep root", 0x31);
    let mut chain = vec![root.certificate.clone()];
    let mut issuer = root;
    for n in 0..MAX_SUBJECTS as u8 {
        let next = issuer.intermediate(&format!("tpdf test deep {n}"), 0x90 + n, 2 + n);
        chain.insert(0, next.certificate.clone());
        issuer = next;
    }
    let signer = issuer.issue_publishing("tpdf test deep signer", 0x35, 4, &Published::default());
    let at = now();
    let original = plain_pdf();
    let unsigned = crate::sign_prepare::prepare(original.clone(), at, None).expect("prepared");
    let key = Soft::p256(signer.seed);
    let made = crate::sign_cms::sign(original, unsigned, at, &signer.certificate, &chain, &key)
        .expect("made");
    let value = made.value().expect("a value");
    let token = mint(
        Imprint::Sha256,
        &Imprint::Sha256.digest(&value),
        None,
        at,
        &pki.tsa,
    );
    let why = plan(&made.stamped(&token).expect("stamped"), &no_os_chain).expect_err("refused");
    assert!(matches!(why, Refusal::Bound(_)), "{why:?}");
}

/// A worker that answers a revision built against other bytes.
struct Elsewhere;

impl crate::save::Verifier for Elsewhere {
    fn scan(
        &self,
        _: &mut std::fs::File,
        _: usize,
        _: &[String],
        _: Option<&str>,
    ) -> Result<crate::verify::Report, String> {
        Err("not asked".into())
    }

    fn signatures(
        &self,
        _: &mut std::fs::File,
        _: usize,
    ) -> Result<Vec<crate::docinfo::Signature>, String> {
        Err("not asked".into())
    }

    fn validation(
        &self,
        signed: &[u8],
        gathered: &Gathered,
    ) -> Result<crate::sign_dss::Extended, String> {
        let mut extended = crate::save::Here.validation(signed, gathered)?;
        extended.built_against += 1;
        Ok(extended)
    }

    fn document_timestamp(&self, signed: &[u8]) -> Result<crate::sign_prepare::Unsigned, String> {
        crate::save::Here.document_timestamp(signed)
    }
}

#[test]
fn a_revision_built_against_other_bytes_is_refused() {
    let pki = Pki::start(good());
    let signed = sealed(&pki);
    let why = extend(
        &signed.bytes,
        &signed.cms,
        &signed.field,
        now(),
        &Elsewhere,
        &no_os_chain,
        &anchored,
        &mut quick,
        &mut archive,
    )
    .expect_err("refused");
    assert!(
        why.sentence().contains("built its revision against"),
        "{why:?}"
    );
}

// ------------------------------------------------------------ an instrument

/// Writes B-LT documents made here, with the roots and what each should read
/// as, for pyHanko, `qpdf --check` and `openssl ocsp -respin` (`BUILD.md`).
#[test]
#[ignore = "an instrument: writes files for other readers"]
fn write_b_lt_documents_for_other_readers() {
    let dir = std::path::PathBuf::from(
        std::env::var("TPDF_LONG_TERM_OUT").unwrap_or_else(|_| "/tmp/tpdf-long-term".into()),
    );
    std::fs::create_dir_all(&dir).expect("a directory");
    for (name, plan) in [
        ("ocsp", good()),
        (
            "crl",
            Plan {
                signer_ocsp: None,
                signer_crl: Some(Serve::Good),
                authority_ocsp: None,
                authority_crl: Some(Serve::Good),
                ..Plan::default()
            },
        ),
        (
            "intermediate",
            Plan {
                intermediate: true,
                intermediate_ocsp: Some(Serve::Good),
                ..good()
            },
        ),
    ] {
        let pki = Pki::start(plan);
        let bytes = extended(&sealed(&pki)).expect("extended");
        std::fs::write(dir.join(format!("b-lt-{name}.pdf")), &bytes).expect("written");
        let (certs, ocsps, crls) = dss(&bytes);
        for (n, der) in certs.iter().enumerate() {
            std::fs::write(dir.join(format!("{name}-cert-{n}.der")), der).expect("written");
        }
        for (n, der) in ocsps.iter().enumerate() {
            std::fs::write(dir.join(format!("{name}-ocsp-{n}.der")), der).expect("written");
        }
        for (n, der) in crls.iter().enumerate() {
            std::fs::write(dir.join(format!("{name}-crl-{n}.der")), der).expect("written");
        }
        std::fs::write(dir.join(format!("{name}-root.der")), &pki.root.certificate).expect("root");
        std::fs::write(dir.join(format!("{name}-tsa-root.der")), &pki.tsa.root).expect("root");
        std::fs::write(
            dir.join(format!("{name}-signer.der")),
            &pki.signer.certificate,
        )
        .expect("s");
        std::fs::write(dir.join(format!("{name}-tsa.der")), &pki.tsa.certificate).expect("t");
        if let Some(i) = &pki.intermediate {
            std::fs::write(dir.join(format!("{name}-intermediate.der")), &i.certificate)
                .expect("i");
        }
        println!(
            "wrote {name}: {} bytes, {} certs, {} OCSP, {} CRL",
            bytes.len(),
            certs.len(),
            ocsps.len(),
            crls.len()
        );
    }
}

/// DigiCert's and Sectigo's tokens carry their roots as cross-certificates,
/// issued by an older root the OS may not hand back; the chain a verifier
/// builds ends at the self-issued twin the OS holds, and so does the walk.
#[test]
fn a_cross_certificate_of_a_root_the_os_holds_ends_the_chain() {
    use crate::integrity::test_tsa::TestCa;
    let pki = Pki::start(good());
    let older = TestCa::new("tpdf test older root", 0x2a);
    let cross = older.cross(&pki.root, 9);
    let at = now();
    let original = plain_pdf();
    let unsigned = crate::sign_prepare::prepare(original.clone(), at, None).expect("prepared");
    let made = crate::sign_cms::sign(
        original,
        unsigned,
        at,
        &pki.signer.certificate,
        std::slice::from_ref(&cross),
        &Soft::p256(pki.signer.seed),
    )
    .expect("made");
    let value = made.value().expect("a value");
    let token = mint(
        Imprint::Sha256,
        &Imprint::Sha256.digest(&value),
        None,
        at,
        &pki.tsa,
    );
    let cms = made.stamped(&token).expect("stamped");
    let root = pki.root.certificate.clone();
    let (subjects, carried) =
        plan(&cms, &move |_: &[u8], _: &[Vec<u8>]| vec![root.clone()]).expect("planned");
    let whose: Vec<Whose> = subjects.iter().map(|s| s.whose).collect();
    assert_eq!(whose, [Whose::Signer, Whose::Authority]);
    assert!(
        carried.contains(&pki.root.certificate),
        "the anchor is carried"
    );
    assert!(!carried.contains(&cross), "the cross-certificate is not");
}

// ------------------------------------------- the authority must be trusted

/// Over `http://` anybody on the path can answer with a token from an
/// authority of their own, whose certificate names addresses of their
/// choosing --- here the fake PKI's, on this machine. Refused before anything
/// is asked: no request reaches the server, and nothing is written.
#[test]
fn an_authority_this_computer_does_not_trust_is_refused_before_anything_is_fetched() {
    let pki = Pki::start(good());
    let signed = sealed(&pki);
    let mut asked = 0usize;
    let mut archived = 0usize;
    let why = extend(
        &signed.bytes,
        &signed.cms,
        &signed.field,
        now(),
        &crate::save::Here,
        &no_os_chain,
        &stranger,
        &mut |url: &url::Url, body: Option<(&str, Vec<u8>)>, limits: &tsa::Limits| {
            asked += 1;
            quick(url, body, limits)
        },
        &mut |pieces: &[&[u8]]| {
            archived += 1;
            archive(pieces)
        },
    )
    .expect_err("refused");
    assert!(matches!(why, Refusal::Untrusted { .. }), "{why:?}");
    assert!(
        why.sentence().contains(&format!(
            "the timestamp authority {} is not trusted by this computer, so tpdf will not \
             fetch revocation data for it",
            crate::integrity::test_tsa::AUTHORITY
        )),
        "{why:?}"
    );
    assert!(!why.revoked() && !why.tpdf_failed(), "{why:?}");
    assert_eq!((asked, archived), (0, 0), "nothing was asked");
    assert!(pki.paths().is_empty(), "{:?}", pki.paths());

    // The control: the same signature, the authority's root trusted, gathers
    // from the same server.
    let bytes = extended(&signed).expect("extended");
    assert!(bytes.len() > signed.bytes.len());
    assert_eq!(pki.paths(), ["/ocsp/signer", "/ocsp/authority"]);
}

/// A token is attacker-shaped bytes from the network, and its certificates
/// are what the OS parses in this process. More than the reader hands the OS
/// is not a chain anybody vouched for; one fewer is.
#[test]
fn a_token_carrying_more_certificates_than_the_os_is_handed_is_not_vouched_for() {
    use crate::integrity::test_tsa::{Published, TestCa, TestTsa};
    let pki = Pki::start(good());
    let stamped = |carried: usize| {
        let mut tsa = TestTsa::publishing(&Published {
            ocsp: vec![format!("{}/ocsp/authority", pki.base)],
            ..Published::default()
        });
        for n in tsa.carried.len()..carried {
            let n = u8::try_from(n).expect("small");
            tsa.carried
                .push(TestCa::new(&format!("tpdf test bystander {n}"), 0x80 + n).certificate);
        }
        let at = now();
        let original = plain_pdf();
        let unsigned = crate::sign_prepare::prepare(original.clone(), at, None).expect("prepared");
        let made = crate::sign_cms::sign(
            original,
            unsigned,
            at,
            &pki.signer.certificate,
            &pki.chain,
            &Soft::p256(pki.signer.seed),
        )
        .expect("made");
        let value = made.value().expect("a value");
        let token = mint(
            Imprint::Sha256,
            &Imprint::Sha256.digest(&value),
            None,
            at,
            &tsa,
        );
        made.stamped(&token).expect("stamped")
    };
    // One over: the reader's own rule refuses the lot, and nothing is asked.
    let over = crate::trust::MAX_CERTIFICATES;
    let why = vouched(&stamped(over), now(), &anchored).expect_err("refused");
    assert!(
        matches!(&why, Refusal::Untrusted { why, .. } if why.contains("could not be prepared")),
        "{why:?}"
    );
    // At the bound --- the authority's own and fifteen more --- it is vouched
    // for, so the cap and not the fixture is what refused the one above.
    vouched(&stamped(over - 1), now(), &anchored).expect("vouched for");
    assert!(pki.paths().is_empty());
}

/// The chain the OS assembles is asked of it in this process, over the
/// token's certificates among others: at most the reader's count, none over
/// its size, the signature's own first.
#[test]
fn the_os_is_handed_at_most_the_readers_bound_of_certificates() {
    let big = vec![0x30; crate::trust::MAX_CERTIFICATE_BYTES + 1];
    let mut others = vec![big.clone()];
    others.extend((0..20u8).map(|n| vec![0x30, n]));
    let mut handed = Vec::new();
    let chain = os_chain_with(b"leaf", &others, |leaf, given| {
        assert_eq!(leaf, b"leaf");
        handed = given.to_vec();
        Some(vec![b"found".to_vec()])
    });
    assert_eq!(chain, [b"found".to_vec()]);
    let expected: Vec<Vec<u8>> = (0..16u8).map(|n| vec![0x30, n]).collect();
    assert_eq!(handed.len(), crate::trust::MAX_CERTIFICATES);
    assert_eq!(handed, expected, "the first sixteen, the oversized one out");
    // A leaf over the size is not handed over at all.
    let mut asked = false;
    let chain = os_chain_with(&big, &others[1..], |_, _| {
        asked = true;
        Some(Vec::new())
    });
    assert!(chain.is_empty() && !asked);
}

// ----------------------------------------------- whose failure it is

/// A worker that dies, or does not answer, at either of its two revisions.
struct Dead {
    at_validation: bool,
}

impl crate::save::Verifier for Dead {
    fn scan(
        &self,
        _: &mut std::fs::File,
        _: usize,
        _: &[String],
        _: Option<&str>,
    ) -> Result<crate::verify::Report, String> {
        Err("not asked".into())
    }

    fn signatures(
        &self,
        _: &mut std::fs::File,
        _: usize,
    ) -> Result<Vec<crate::docinfo::Signature>, String> {
        Err("not asked".into())
    }

    fn validation(
        &self,
        signed: &[u8],
        gathered: &Gathered,
    ) -> Result<crate::sign_dss::Extended, String> {
        if self.at_validation {
            return Err("the worker did not answer within 30 s".into());
        }
        crate::save::Here.validation(signed, gathered)
    }

    fn document_timestamp(&self, signed: &[u8]) -> Result<crate::sign_prepare::Unsigned, String> {
        if !self.at_validation {
            return Err("the worker exited with signal 9".into());
        }
        crate::save::Here.document_timestamp(signed)
    }
}

/// A worker that died is tpdf's failure, which the command line reports as
/// exit 4 as its README says --- not exit 3, which is for what the document,
/// an authority or a certificate authority refused.
#[test]
fn a_worker_that_dies_while_extending_is_tpdfs_failure() {
    use crate::cli::Exit;
    let pki = Pki::start(good());
    let signed = sealed(&pki);
    for at_validation in [true, false] {
        let why = extend(
            &signed.bytes,
            &signed.cms,
            &signed.field,
            now(),
            &Dead { at_validation },
            &no_os_chain,
            &anchored,
            &mut quick,
            &mut archive,
        )
        .expect_err("refused");
        assert!(why.tpdf_failed(), "{at_validation}: {why:?}");
        assert_eq!(
            crate::cli::sign::long_term_failure(&why).exit,
            Exit::Internal,
            "{at_validation}"
        );
    }
    // The control: a refusal of the certificate authority's is 3.
    let pki = Pki::start(Plan {
        signer_ocsp: Some(Serve::Unknown),
        ..good()
    });
    let why = extended(&sealed(&pki)).expect_err("refused");
    assert!(!why.tpdf_failed(), "{why:?}");
    assert_eq!(
        crate::cli::sign::long_term_failure(&why).exit,
        Exit::Refused
    );
}

// ------------------------------------ the command line's read-back

/// The command line's check of the file it wrote holds the file to what
/// [`check`] held the same bytes to before writing: the authority's answer
/// and the chains above, not only the signer's.
#[test]
fn the_command_lines_read_back_asks_what_the_check_before_writing_asks() {
    use crate::cli::sign::read_back_holds;
    let pki = Pki::start(good());
    let signed = sealed(&pki);
    let at = now();
    let tsa_root = crate::integrity::test_tsa::TestCa::of_tsa();
    let certs = [pki.root.certificate.clone(), pki.tsa.root.clone()];
    let ocsp = |certificate: &[u8], issuer: &crate::integrity::test_tsa::TestCa, says| {
        mint_ocsp(
            certificate,
            issuer,
            says,
            at - 3_600,
            Some(at + 86_400),
            Responder::Issuer,
            &OcspFaults::default(),
        )
    };
    let found = |authority: Says| {
        let bytes = with_dss(
            &signed.bytes,
            &certs,
            &[
                ocsp(&pki.signer.certificate, &pki.root, Says::Good),
                ocsp(&pki.tsa.certificate, &tsa_root, authority),
            ],
            &[],
        );
        crate::docinfo::scan(&bytes, 1, None)
            .expect("scanned")
            .signatures
    };
    let good = found(Says::Good);
    assert!(read_back_holds(&good, &signed.field, true, true));
    // The signer's answer good and the authority's unknown: not what was
    // written --- and without long-term data it would not matter, which is
    // what makes the clause the thing that refuses.
    let unknown = found(Says::Unknown);
    assert!(!read_back_holds(&unknown, &signed.field, true, true));
    assert!(read_back_holds(&unknown, &signed.field, true, false));
    // A certificate above the authority's that is not good.
    let mut above = good.clone();
    let chain = above
        .iter_mut()
        .find(|s| s.field == signed.field)
        .and_then(|s| s.timestamp.as_mut())
        .and_then(|t| t.revocation_chain.as_mut())
        .expect("a chain");
    chain.certificates.push(crate::revocation::chain::Judged {
        subject_cn: "tpdf test issuing authority".into(),
        ..Default::default()
    });
    assert!(!read_back_holds(&above, &signed.field, true, true));
    // Another field is not ours.
    assert!(!read_back_holds(&good, "Signature9", true, true));
}
