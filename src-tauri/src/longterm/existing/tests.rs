//! Long-term validation data for documents that are already signed, against
//! the fake PKI on 127.0.0.1.
//!
//! Every test is offline, as `longterm/tests.rs`'s are, and shares its
//! helpers: the documents are signed here with the test signer's key, with
//! or without the publishing test authority's timestamp, and then handed to
//! [`add`] as bytes somebody else made. The worker is `save::Here` --- the
//! same `sign_dss::survey`, `sign_dss::extend` and document timestamp a
//! worker runs --- so each test covers the whole path but the process
//! boundary, which `tests/cli.rs` covers.

use super::super::tests::{dss, good, now, quick, sealed};
use super::*;
use crate::integrity::test_tsa::{mint, Imprint, Pki, Plan, Serve, TestTsa};
use crate::revocation::Status;
use crate::sign_cms::testkeys::{plain_pdf, Soft, Spec};
use crate::sign_dss::{Extended, Gathered, Survey};

/// The roots a reader's store would hold for `pki`: its signer's, and the
/// test timestamp authority's.
fn roots(pki: &Pki) -> Vec<Vec<u8>> {
    vec![pki.root.certificate.clone(), pki.tsa.root.clone()]
}

/// `bytes` with one more signature after what it holds, made with `key`
/// under `certificate`, and no timestamp.
fn signed_again(bytes: Vec<u8>, certificate: &[u8], chain: &[Vec<u8>], key: &Soft) -> Vec<u8> {
    let at = now();
    let unsigned = crate::sign_prepare::prepare(bytes.clone(), at, None).expect("prepared");
    crate::sign_cms::sign(bytes, unsigned, at, certificate, chain, key)
        .expect("made")
        .seal(None)
        .expect("sealed")
}

/// The plain document signed by `pki`'s signer, with no timestamp.
fn untimestamped(pki: &Pki) -> Vec<u8> {
    signed_again(
        plain_pdf(),
        &pki.signer.certificate,
        &pki.chain,
        &Soft::p256(pki.signer.seed),
    )
}

/// [`add`] over `original`, with `roots` the only roots trusted, the fake
/// PKI's server for revocation data, and `tsa` for the archive timestamp.
fn added_by(
    original: &[u8],
    roots: &[Vec<u8>],
    tsa: &TestTsa,
    worker: &dyn crate::save::Verifier,
) -> Result<Added, Refusal> {
    added_at(original, roots, tsa, worker, now())
}

/// [`added_by`], at the moment `at`: the present every certificate is judged
/// at, which a test moves to after a certificate's last day.
fn added_at(
    original: &[u8],
    roots: &[Vec<u8>],
    tsa: &TestTsa,
    worker: &dyn crate::save::Verifier,
    at: u64,
) -> Result<Added, Refusal> {
    let vouch = |blob: &[u8], purpose: Purpose, now: u64| {
        super::super::vouched_for(blob, purpose, now, crate::trust::Anchors::Only(roots))
    };
    add(original, at, worker, &vouch, &mut quick, &mut |pieces| {
        Ok(mint(
            Imprint::Sha256,
            &Imprint::Sha256.digest(&pieces.concat()),
            None,
            now(),
            tsa,
        ))
    })
}

/// [`added_by`], as a reader whose store trusts both of `pki`'s roots has
/// it, the archive timestamp from `pki`'s own authority.
fn added(original: &[u8], pki: &Pki) -> Result<Added, Refusal> {
    added_by(original, &roots(pki), &pki.tsa, &crate::save::Here)
}

/// Every signed field of `bytes`, as the properties dialog reads the file.
fn read(bytes: &[u8]) -> Vec<Signature> {
    crate::docinfo::scan(bytes, 1, None)
        .expect("scanned")
        .signatures
        .into_iter()
        .filter(|s| s.signed)
        .collect()
}

fn intact(signature: &Signature) -> bool {
    signature.integrity.as_ref().map(|i| i.verdict) == Some(Verdict::Intact)
}

fn standing(revocation: Option<&crate::revocation::Revocation>) -> Option<Status> {
    revocation.map(|r| r.standing)
}

// ------------------------------------------------------------ the controls

#[test]
fn a_signed_document_gets_validation_data_and_an_archive_timestamp() {
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    let was = read(&original);
    assert_eq!(standing(was[0].revocation.as_ref()), Some(Status::None));

    let done = added(&original, &pki).expect("added");
    assert_eq!(
        &done.bytes[..original.len()],
        &original[..],
        "no earlier byte was written"
    );
    let found = read(&done.bytes);
    let [signature, archive] = found.as_slice() else {
        panic!("a signature and an archive timestamp: {found:?}");
    };
    assert!(intact(signature));
    assert_eq!(
        standing(signature.revocation.as_ref()),
        Some(Status::Good),
        "{:?}",
        signature.revocation
    );
    let stamp = signature.timestamp.as_ref().expect("its timestamp");
    assert_eq!(standing(stamp.revocation.as_ref()), Some(Status::Good));
    assert_eq!(archive.kind, "ETSI.RFC3161");
    assert!(archive.covers_whole_file && intact(archive));
    assert_eq!(pki.paths(), ["/ocsp/signer", "/ocsp/authority"]);
    let (certs, ocsps, _) = dss(&done.bytes);
    assert_eq!(ocsps.len(), 2);
    for issuer in [&pki.root.certificate, &pki.tsa.root] {
        assert!(certs.contains(issuer), "an issuer is missing from /Certs");
    }
    // What the written file is held to, over the same bytes.
    let appended = (done.bytes.len() - original.len()) as u64;
    assert_eq!(
        read_back(&done.before, &found, appended),
        Ok(()),
        "the read-back's rule"
    );
    assert_eq!(
        crate::cli::verify::after_last_signature(&found),
        crate::cli::verify::After::Unchanged,
        "nothing verify --strict would count as a change"
    );
}

#[test]
fn a_signature_with_no_timestamp_of_its_own_is_covered_too() {
    let pki = Pki::start(good());
    let original = untimestamped(&pki);
    assert!(read(&original)[0].timestamp.is_none(), "the fixture");
    let done = added(&original, &pki).expect("added");
    let found = read(&done.bytes);
    assert_eq!(found.len(), 2);
    assert_eq!(standing(found[0].revocation.as_ref()), Some(Status::Good));
    assert!(found[1].covers_whole_file && intact(&found[1]));
    assert_eq!(pki.paths(), ["/ocsp/signer"], "nobody else to ask about");
}

/// Run on its own result: the first archive timestamp's authority is asked
/// about, a second archive timestamp follows, and everything the first run
/// put in the `/DSS` is still there.
#[test]
fn a_document_already_archived_gets_a_further_archive_and_keeps_its_dss() {
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    let first = added(&original, &pki).expect("added").bytes;
    let (certs, ocsps, lists) = dss(&first);

    let second = added(&first, &pki).expect("added again");
    assert_eq!(&second.bytes[..first.len()], &first[..]);
    let found = read(&second.bytes);
    assert_eq!(found.len(), 3, "the signature and two archive timestamps");
    assert!(found.iter().all(intact));
    assert_eq!(
        found
            .iter()
            .filter(|s| s.kind == "ETSI.RFC3161")
            .map(|s| s.covers_whole_file)
            .collect::<Vec<_>>(),
        [false, true],
        "the later archive covers the earlier"
    );
    // The first archive's authority read back good: what the second run adds
    // that the first could not.
    assert_eq!(standing(found[1].revocation.as_ref()), Some(Status::Good));
    assert_eq!(
        second.before.iter().filter(|s| s.signed).count(),
        2,
        "the first archive is one of what was covered"
    );
    let (certs_now, ocsps_now, lists_now) = dss(&second.bytes);
    for (were, are) in [
        (&certs, &certs_now),
        (&ocsps, &ocsps_now),
        (&lists, &lists_now),
    ] {
        assert!(
            were.iter().all(|entry| are.contains(entry)),
            "an entry the document carried was dropped"
        );
    }
    // (The second run's answers are added unless they are the first run's
    // byte for byte, which they are when both were minted in one second.)
    assert!(ocsps_now.len() >= ocsps.len());
    // The signature's authority and the archive's are one certificate: asked
    // once in each run.
    assert_eq!(
        pki.paths(),
        [
            "/ocsp/signer",
            "/ocsp/authority",
            "/ocsp/signer",
            "/ocsp/authority"
        ]
    );
}

#[test]
fn every_signature_of_a_document_is_covered_and_each_certificate_asked_once() {
    let pki = Pki::start(good());
    let key = Soft::p256(pki.signer.seed);
    let original = signed_again(
        sealed(&pki).bytes,
        &pki.signer.certificate,
        &pki.chain,
        &key,
    );
    let done = added(&original, &pki).expect("added");
    let found = read(&done.bytes);
    assert_eq!(found.len(), 3);
    for signature in &found[..2] {
        assert!(intact(signature), "{}", signature.field);
        assert_eq!(standing(signature.revocation.as_ref()), Some(Status::Good));
    }
    assert_eq!(pki.paths(), ["/ocsp/signer", "/ocsp/authority"]);
}

// ------------------------------------------------------------- the refusals

#[test]
fn a_document_with_no_signature_is_refused() {
    let pki = Pki::start(good());
    let why = added(&plain_pdf(), &pki).expect_err("refused");
    assert_eq!(why, Refusal::Unsigned);
    assert!(
        why.sentence().contains("has no signature"),
        "{}",
        why.sentence()
    );
    assert!(!why.tpdf_failed());
    assert!(pki.paths().is_empty());
}

/// The plain document, encrypted: opened by the empty password, or only by
/// `swordfish`.
fn encrypted(user_password: &str) -> Vec<u8> {
    use lopdf::Object;
    let mut document = lopdf::Document::load_mem(&plain_pdf()).expect("parses");
    document.trailer.set(
        "ID",
        vec![
            Object::string_literal("synthetic-id"),
            Object::string_literal("synthetic-id"),
        ],
    );
    let encryption = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V2 {
        document: &document,
        owner_password: "synthetic-owner",
        user_password,
        key_length: 128,
        permissions: lopdf::Permissions::default(),
    })
    .expect("a state");
    document.encrypt(&encryption).expect("encrypted");
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("saved");
    bytes
}

#[test]
fn an_encrypted_document_is_refused_whether_or_not_it_opens() {
    let pki = Pki::start(good());
    for password in ["", "swordfish"] {
        let why = added(&encrypted(password), &pki).expect_err("refused");
        assert!(
            matches!(&why, Refusal::Document(said) if said.contains("encrypted")),
            "{password:?}: {why:?}"
        );
        assert!(!why.tpdf_failed(), "the document's refusal, not tpdf's");
    }
    assert!(pki.paths().is_empty());
    // The control: the same document, not encrypted, is refused for having
    // no signature --- so the refusal above is about the encryption.
    assert_eq!(added(&plain_pdf(), &pki), Err(Refusal::Unsigned));
}

#[test]
fn a_signature_that_does_not_verify_is_refused_before_anything_is_fetched() {
    let pki = Pki::start(good());
    let mut original = sealed(&pki).bytes;
    // One byte of the first page's revision, which the signature covers.
    let at = original
        .windows(8)
        .position(|w| w == b"MediaBox")
        .expect("the page");
    original[at] = b'm';
    let why = added(&original, &pki).expect_err("refused");
    assert_eq!(
        why,
        Refusal::NotIntact {
            field: "Signature1".into(),
            timestamp: false,
            reads: "altered".into(),
        }
    );
    assert_eq!(
        why.sentence(),
        "the signature Signature1 does not verify (it reads altered), and long-term validation \
         data cannot make it verify"
    );
    assert!(pki.paths().is_empty(), "{:?}", pki.paths());
}

#[test]
fn a_signer_this_computer_does_not_trust_is_refused_and_nothing_is_fetched() {
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    // The authority's root alone: the signer chains to nothing trusted.
    let why = added_by(
        &original,
        std::slice::from_ref(&pki.tsa.root),
        &pki.tsa,
        &crate::save::Here,
    )
    .expect_err("refused");
    let Refusal::Untrusted { field, name, .. } = &why else {
        panic!("{why:?}");
    };
    assert_eq!(field, "Signature1");
    assert!(name.contains("tpdf test PKI signer"), "{name}");
    assert!(
        why.sentence()
            .contains("is not trusted by this computer, so tpdf will not fetch"),
        "{}",
        why.sentence()
    );
    assert!(pki.paths().is_empty(), "{:?}", pki.paths());
    // The control: with the signer's root as well, the same bytes are covered.
    assert!(added(&original, &pki).is_ok());
}

#[test]
fn a_timestamp_authority_this_computer_does_not_trust_is_refused_and_nothing_is_fetched() {
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    let why = added_by(
        &original,
        std::slice::from_ref(&pki.root.certificate),
        &pki.tsa,
        &crate::save::Here,
    )
    .expect_err("refused");
    assert!(
        matches!(
            &why,
            Refusal::About {
                field: Some(field),
                why: super::super::Refusal::Untrusted { .. },
            } if field == "Signature1"
        ),
        "{why:?}"
    );
    assert!(
        why.sentence()
            .starts_with("for the signature Signature1: the timestamp authority "),
        "{}",
        why.sentence()
    );
    assert!(pki.paths().is_empty(), "{:?}", pki.paths());
}

/// A self-signed second signature can never be covered, so nothing is: the
/// first signature's data is not fetched either.
#[test]
fn one_signature_that_cannot_be_covered_means_nothing_is_written() {
    let pki = Pki::start(good());
    let key = Soft::p256(0x19);
    let own = crate::sign_cms::testkeys::certificate(&key, &Spec::new("tpdf test self-signed"));
    let original = signed_again(sealed(&pki).bytes, &own, &[], &key);
    assert_eq!(read(&original).len(), 2, "the fixture");

    // Not trusted: refused by name.
    let why = added(&original, &pki).expect_err("refused");
    assert!(
        matches!(&why, Refusal::Untrusted { field, .. } if field == "Signature2"),
        "{why:?}"
    );
    assert!(pki.paths().is_empty(), "{:?}", pki.paths());

    // Trusted as a root of its own: it still publishes nothing, and is
    // refused for that, by name.
    let mut trusting = roots(&pki);
    trusting.push(own);
    let why = added_by(&original, &trusting, &pki.tsa, &crate::save::Here).expect_err("refused");
    assert!(
        matches!(
            &why,
            Refusal::About {
                field: Some(field),
                why: super::super::Refusal::NotPublished(_),
            } if field == "Signature2"
        ),
        "{why:?}"
    );
    assert!(
        why.sentence().ends_with("so there is none to add for it"),
        "{}",
        why.sentence()
    );
    assert!(pki.paths().is_empty(), "{:?}", pki.paths());
}

#[test]
fn revocation_data_that_cannot_be_had_writes_nothing() {
    // A certificate that names no address, a responder that fails, one that
    // does not know the certificate, and one that says it is revoked.
    for (plan, said) in [
        (
            Plan {
                signer_ocsp: None,
                authority_ocsp: Some(Serve::Good),
                ..Plan::default()
            },
            "so there is none to add for it",
        ),
        (
            Plan {
                signer_ocsp: Some(Serve::Status(500)),
                authority_ocsp: Some(Serve::Good),
                ..Plan::default()
            },
            "tpdf could not get revocation data for the signer's certificate",
        ),
        (
            Plan {
                signer_ocsp: Some(Serve::Unknown),
                authority_ocsp: Some(Serve::Good),
                ..Plan::default()
            },
            "says it does not know the signer's certificate",
        ),
        (
            Plan {
                signer_ocsp: Some(Serve::Revoked),
                authority_ocsp: Some(Serve::Good),
                ..Plan::default()
            },
            "adds long-term validation data only for certificates that are not revoked",
        ),
    ] {
        let pki = Pki::start(plan);
        let why = added(&sealed(&pki).bytes, &pki).expect_err("refused");
        assert!(why.sentence().contains(said), "{}", why.sentence());
        assert!(
            !why.sentence().contains("will not write a signature"),
            "a signing's words about a document nobody is signing: {}",
            why.sentence()
        );
        assert!(!why.tpdf_failed(), "{why:?}");
    }
}

/// The archive timestamp must come from an authority this computer trusts
/// for timestamping, or it would be sealed intact and attest nothing.
#[test]
fn an_archive_timestamp_from_an_authority_nobody_trusts_is_refused() {
    let pki = Pki::start(good());
    let original = untimestamped(&pki);
    let signer_root = std::slice::from_ref(&pki.root.certificate);
    let why = added_by(&original, signer_root, &pki.tsa, &crate::save::Here).expect_err("refused");
    assert!(
        matches!(
            &why,
            Refusal::About {
                field: None,
                why: super::super::Refusal::Archive(said),
            } if said.contains("is not trusted by this computer")
        ),
        "{why:?}"
    );
    // The control: the same signature, the authority's root trusted too.
    assert!(added(&original, &pki).is_ok());
}

/// One run asks about a bounded number of certificates, however many
/// signatures name them.
#[test]
fn more_certificates_than_one_run_asks_about_is_a_refusal() {
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    let survey = crate::sign_dss::survey(&original, 1);
    let held = held(&survey).expect("held");
    let trusted = roots(&pki);
    let vouch = |blob: &[u8], purpose: Purpose, now: u64| {
        super::super::vouched_for(blob, purpose, now, crate::trust::Anchors::Only(&trusted))
    };
    // The signer's certificate and the authority's: two to ask about.
    let (subjects, _) = plan_within(&held, &survey.store, now(), &vouch, 2).expect("planned");
    assert_eq!(subjects.len(), 2);
    assert_eq!(
        plan_within(&held, &survey.store, now(), &vouch, 1).map(|_| ()),
        Err(Refusal::TooMany)
    );
    assert!(Refusal::TooMany.sentence().contains("in one run"));
    assert!(pki.paths().is_empty(), "planning asks nobody");
}

// ------------------------------------- the worker's answers, held to account

/// How a test changes what a worker read in the result.
type Alter = fn(Extended) -> Extended;

/// A worker that answers as `save::Here` does, and then changes its answer.
#[derive(Default)]
struct Altering {
    survey: Option<fn(Survey) -> Result<Survey, String>>,
    validation: Option<Alter>,
}

impl crate::save::Verifier for Altering {
    fn scan(
        &self,
        _: &mut std::fs::File,
        _: usize,
        _: &[String],
        _: Option<&str>,
    ) -> Result<crate::verify::Report, String> {
        Err("not asked".into())
    }

    fn signatures(&self, _: &mut std::fs::File, _: usize) -> Result<Vec<Signature>, String> {
        Err("not asked".into())
    }

    fn validation(&self, signed: &[u8], gathered: &Gathered) -> Result<Extended, String> {
        let extended = crate::save::Here.validation(signed, gathered)?;
        Ok(self
            .validation
            .map_or(extended.clone(), |alter| alter(extended)))
    }

    fn document_timestamp(&self, signed: &[u8]) -> Result<crate::sign_prepare::Unsigned, String> {
        crate::save::Here.document_timestamp(signed)
    }

    fn survey(&self, signed: &[u8]) -> Result<Survey, String> {
        let survey = crate::save::Here.survey(signed)?;
        match self.survey {
            Some(alter) => alter(survey),
            None => Ok(survey),
        }
    }
}

#[test]
fn a_worker_that_died_is_tpdfs_failure_and_a_document_it_refuses_is_not() {
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    let dead = Altering {
        survey: Some(|_| Err("the worker did not answer within 30 s".into())),
        ..Altering::default()
    };
    let why = added_by(&original, &roots(&pki), &pki.tsa, &dead).expect_err("refused");
    assert!(
        matches!(why, Refusal::Failed(_)) && why.tpdf_failed(),
        "{why:?}"
    );

    let refusing = Altering {
        survey: Some(|mut survey| {
            survey.refused = Some("this document could not be parsed: no".into());
            Ok(survey)
        }),
        ..Altering::default()
    };
    let why = added_by(&original, &roots(&pki), &pki.tsa, &refusing).expect_err("refused");
    assert!(
        matches!(why, Refusal::Document(_)) && !why.tpdf_failed(),
        "{why:?}"
    );
    assert!(pki.paths().is_empty());
}

#[test]
fn a_survey_that_is_not_complete_is_refused() {
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    for alter in [
        (|mut survey| {
            survey.complete = false;
            Ok(survey)
        }) as fn(Survey) -> Result<Survey, String>,
        // A value that could not be read, and a value under another name.
        |mut survey| {
            survey.values[0].1.clear();
            Ok(survey)
        },
        |mut survey| {
            survey.values[0].0 = "Signature9".into();
            Ok(survey)
        },
        |mut survey| {
            survey.values.clear();
            Ok(survey)
        },
    ] {
        let worker = Altering {
            survey: Some(alter),
            ..Altering::default()
        };
        assert_eq!(
            added_by(&original, &roots(&pki), &pki.tsa, &worker),
            Err(Refusal::Incomplete)
        );
    }
    assert!(pki.paths().is_empty());
}

#[test]
fn a_certification_with_no_changes_permitted_is_refused_and_the_others_are_not() {
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    let certified = |level: u8| Altering {
        survey: Some(match level {
            1 => |mut survey: Survey| {
                survey.signatures[0].certification = 1;
                Ok(survey)
            },
            2 => |mut survey: Survey| {
                survey.signatures[0].certification = 2;
                Ok(survey)
            },
            _ => |mut survey: Survey| {
                survey.signatures[0].certification = 3;
                Ok(survey)
            },
        }),
        ..Altering::default()
    };
    let why = added_by(&original, &roots(&pki), &pki.tsa, &certified(1)).expect_err("refused");
    assert_eq!(why, Refusal::Certified(Some("Signature1".into())));
    assert!(
        why.sentence().contains("no changes permitted"),
        "{}",
        why.sentence()
    );
    assert!(pki.paths().is_empty(), "refused before anything is fetched");
    for level in [2, 3] {
        assert!(
            added_by(&original, &roots(&pki), &pki.tsa, &certified(level)).is_ok(),
            "level {level}"
        );
    }
}

#[test]
fn a_timestamp_that_does_not_verify_is_refused_by_name() {
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    let worker = Altering {
        survey: Some(|mut survey| {
            let stamp = survey.signatures[0].timestamp.as_mut().expect("a token");
            stamp.integrity.as_mut().expect("a verdict").verdict = Verdict::Broken;
            Ok(survey)
        }),
        ..Altering::default()
    };
    let why = added_by(&original, &roots(&pki), &pki.tsa, &worker).expect_err("refused");
    assert_eq!(
        why.sentence(),
        "the timestamp of the signature Signature1 does not verify (it reads broken), and \
         long-term validation data cannot make it verify"
    );
    assert!(pki.paths().is_empty());
}

#[test]
fn signatures_past_the_hashing_budget_are_refused_before_anything_is_fetched() {
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    // The existing signatures already unchecked for want of budget.
    let starved = Altering {
        survey: Some(|mut survey| {
            let verdict = survey.signatures[0].integrity.as_mut().expect("a verdict");
            verdict.verdict = Verdict::Unchecked;
            verdict.why = Some(Why::Budget);
            Ok(survey)
        }),
        ..Altering::default()
    };
    assert_eq!(
        added_by(&original, &roots(&pki), &pki.tsa, &starved),
        Err(Refusal::Budget)
    );
    // Checked, and covering so much that the archive timestamp over the
    // whole would be the one left unchecked.
    let large = Altering {
        survey: Some(|mut survey| {
            survey.signatures[0].covered_bytes = crate::integrity::MAX_HASHED;
            Ok(survey)
        }),
        ..Altering::default()
    };
    assert_eq!(
        added_by(&original, &roots(&pki), &pki.tsa, &large),
        Err(Refusal::Budget)
    );
    assert!(pki.paths().is_empty(), "{:?}", pki.paths());
    // Inside the budget as the document is, and past it by the two
    // revisions: found once they are built, and still before anything is
    // written.
    static ORIGINAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    ORIGINAL.store(original.len() as u64, std::sync::atomic::Ordering::Relaxed);
    let grown = Altering {
        survey: Some(|mut survey| {
            survey.signatures[0].covered_bytes =
                crate::integrity::MAX_HASHED - ORIGINAL.load(std::sync::atomic::Ordering::Relaxed);
            Ok(survey)
        }),
        ..Altering::default()
    };
    assert_eq!(
        added_by(&original, &roots(&pki), &pki.tsa, &grown),
        Err(Refusal::Budget)
    );
    assert!(
        !pki.paths().is_empty(),
        "the control: this one was gathered for"
    );
    // The control: well under, the same document is covered.
    let fits = Altering {
        survey: Some(|mut survey| {
            survey.signatures[0].covered_bytes = crate::integrity::MAX_HASHED - 1_000_000;
            Ok(survey)
        }),
        ..Altering::default()
    };
    assert!(added_by(&original, &roots(&pki), &pki.tsa, &fits).is_ok());
}

/// The worker's reading of the result is what decides, before anything is
/// written: a signature that no longer reads as it did, for any reason, is
/// a refusal.
#[test]
fn a_result_in_which_an_earlier_signature_reads_differently_is_not_written() {
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    let cases: [(Alter, &str); 4] = [
        (
            |mut extended| {
                extended.signatures[0]
                    .integrity
                    .as_mut()
                    .expect("a verdict")
                    .verdict = Verdict::Altered;
                extended
            },
            "does not read as intact",
        ),
        (
            |mut extended| {
                extended.signatures[0].appended_bytes += 1;
                extended
            },
            "is not in it where it was",
        ),
        (
            |mut extended| {
                extended.signatures[0]
                    .appendix
                    .as_mut()
                    .expect("an appendix")
                    .pages_touched = 1;
                extended
            },
            "reads as followed by a change to a page",
        ),
        (
            |mut extended| {
                extended.signatures[0].timestamp = None;
                extended
            },
            "no longer carries its timestamp",
        ),
    ];
    for (alter, said) in cases {
        let worker = Altering {
            validation: Some(alter),
            ..Altering::default()
        };
        let why = added_by(&original, &roots(&pki), &pki.tsa, &worker).expect_err("refused");
        assert!(
            matches!(&why, Refusal::Changed { field, what } if field == "Signature1" && what == said),
            "{said}: {why:?}"
        );
        assert!(why.tpdf_failed());
    }
    // A revision built against other bytes.
    let elsewhere = Altering {
        validation: Some(|mut extended| {
            extended.built_against += 1;
            extended
        }),
        ..Altering::default()
    };
    let why = added_by(&original, &roots(&pki), &pki.tsa, &elsewhere).expect_err("refused");
    assert!(
        matches!(
            &why,
            Refusal::About {
                why: super::super::Refusal::Written(_),
                ..
            }
        ),
        "{why:?}"
    );
}

// ------------------------------------------------------------ the two rules

/// A signature as a worker reads one this added data for.
fn covered_signature(field: &str, appended: u64) -> Signature {
    let good = || {
        Some(crate::revocation::Revocation {
            standing: Status::Good,
            ..crate::revocation::Revocation::default()
        })
    };
    let chain = || {
        Some(crate::revocation::chain::Chain {
            standing: Status::Good,
            ..crate::revocation::chain::Chain::default()
        })
    };
    let verdict = || {
        Some(crate::integrity::Integrity {
            verdict: Verdict::Intact,
            ..crate::integrity::Integrity::default()
        })
    };
    Signature {
        field: field.into(),
        signed: true,
        kind: "ETSI.CAdES.detached".into(),
        appended_bytes: appended,
        appendix: (appended > 0).then(crate::docinfo::Appendix::default),
        integrity: verdict(),
        revocation: good(),
        revocation_chain: chain(),
        timestamp: Some(crate::docinfo::Timestamp {
            integrity: verdict(),
            revocation: good(),
            revocation_chain: chain(),
            ..crate::docinfo::Timestamp::default()
        }),
        ..Signature::default()
    }
}

#[test]
fn every_earlier_signature_must_read_good_with_the_data_added() {
    let before = [covered_signature("Signature1", 0)];
    let after = || vec![covered_signature("Signature1", 700)];
    assert_eq!(covered(&before, &after(), 700), Ok(()), "the control");

    let about = |why: Result<(), Refusal>| match why {
        Err(Refusal::About {
            field: Some(field),
            why,
        }) => (field, why),
        other => panic!("{other:?}"),
    };
    // The signer's own answer.
    let mut found = after();
    found[0].revocation.as_mut().expect("an answer").standing = Status::None;
    let (field, why) = about(covered(&before, &found, 700));
    assert_eq!(field, "Signature1");
    assert!(matches!(why, super::super::Refusal::Written(_)), "{why:?}");
    let mut found = after();
    found[0].revocation = None;
    assert!(covered(&before, &found, 700).is_err());
    // Its timestamp authority's.
    let mut found = after();
    let stamp = found[0].timestamp.as_mut().expect("a token");
    stamp.revocation.as_mut().expect("an answer").standing = Status::Revoked;
    let (_, why) = about(covered(&before, &found, 700));
    assert!(
        matches!(why, super::super::Refusal::Revoked { .. }),
        "{why:?}"
    );
    // A chain past the reader's bound, above either.
    for authority in [false, true] {
        let mut found = after();
        let chain = if authority {
            found[0]
                .timestamp
                .as_mut()
                .expect("a token")
                .revocation_chain
                .as_mut()
        } else {
            found[0].revocation_chain.as_mut()
        };
        chain.expect("a chain").dropped = 1;
        let (_, why) = about(covered(&before, &found, 700));
        assert!(matches!(why, super::super::Refusal::Written(_)), "{why:?}");
    }
    // Its timestamp's verdict.
    let mut found = after();
    let stamp = found[0].timestamp.as_mut().expect("a token");
    stamp.integrity.as_mut().expect("a verdict").verdict = Verdict::Broken;
    assert!(matches!(
        covered(&before, &found, 700),
        Err(Refusal::Changed { .. })
    ));
    // An appendix tpdf could not read, and an object removed.
    let mut found = after();
    found[0].appendix.as_mut().expect("an appendix").unread = true;
    assert!(matches!(
        covered(&before, &found, 700),
        Err(Refusal::Changed { .. })
    ));
    let mut found = after();
    found[0].appendix.as_mut().expect("an appendix").removed = 1;
    assert!(matches!(
        covered(&before, &found, 700),
        Err(Refusal::Changed { .. })
    ));
    // A page that only lists a new field is not a page changed.
    let mut found = after();
    let appendix = found[0].appendix.as_mut().expect("an appendix");
    appendix.pages_touched = 1;
    appendix.pages_listing = vec![crate::docinfo::PageListing {
        page: 1,
        timestamp: true,
    }];
    assert_eq!(covered(&before, &found, 700), Ok(()));
    // And a page that was already rewritten before this ran is not this
    // run's change.
    let mut was = covered_signature("Signature1", 50);
    was.appendix.as_mut().expect("an appendix").pages_touched = 1;
    let mut found = vec![covered_signature("Signature1", 750)];
    found[0]
        .appendix
        .as_mut()
        .expect("an appendix")
        .pages_touched = 1;
    assert_eq!(covered(&[was], &found, 700), Ok(()));
    // No hashing budget left for it.
    let mut found = after();
    let verdict = found[0].integrity.as_mut().expect("a verdict");
    verdict.verdict = Verdict::Unchecked;
    verdict.why = Some(Why::Budget);
    assert_eq!(covered(&before, &found, 700), Err(Refusal::Budget));
    // A document timestamp's certificate is its authority's, and is named so.
    let stamp = |appended: u64| Signature {
        kind: "ETSI.RFC3161".into(),
        ..covered_signature("Signature2", appended)
    };
    assert_eq!(covered(&[stamp(0)], &[stamp(700)], 700), Ok(()));
    let mut found = vec![stamp(700)];
    found[0].revocation.as_mut().expect("an answer").standing = Status::None;
    let (field, why) = about(covered(&[stamp(0)], &found, 700));
    assert_eq!(field, "Signature2");
    assert!(
        why.sentence()
            .contains("the timestamp authority's certificate reads none"),
        "{}",
        why.sentence()
    );
    // And a signature's is its signer's.
    let mut found = after();
    found[0].revocation.as_mut().expect("an answer").standing = Status::None;
    let (_, why) = about(covered(&before, &found, 700));
    assert!(
        why.sentence()
            .contains("the signer's certificate reads none"),
        "{}",
        why.sentence()
    );
    // An unsigned field of the original asks nothing.
    let empty = Signature {
        field: "Empty".into(),
        ..Signature::default()
    };
    assert_eq!(covered(&[empty], &[], 700), Ok(()));
}

#[test]
fn the_written_file_must_hold_the_archive_over_the_whole_file() {
    let before = [covered_signature("Signature1", 0)];
    let archive = || Signature {
        kind: "ETSI.RFC3161".into(),
        covers_whole_file: true,
        timestamp: None,
        ..covered_signature("Signature2", 0)
    };
    let after = |archive: Signature| vec![covered_signature("Signature1", 900), archive];
    assert_eq!(read_back(&before, &after(archive()), 900), Ok(()));

    let refused = |found: &[Signature]| match read_back(&before, found, 900) {
        Err(Refusal::Archive(why)) => why,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        refused(&[covered_signature("Signature1", 900)]),
        "the archive timestamp is not in it"
    );
    // A signature that ends at the last byte is not a timestamp.
    let mut not_one = archive();
    not_one.kind = "ETSI.CAdES.detached".into();
    assert_eq!(
        refused(&after(not_one)),
        "the archive timestamp is not in it"
    );
    let mut partial = archive();
    partial.covers_whole_file = false;
    assert_eq!(
        refused(&after(partial)),
        "the archive timestamp does not cover the whole file"
    );
    let mut broken = archive();
    broken.integrity.as_mut().expect("a verdict").verdict = Verdict::Broken;
    assert_eq!(
        refused(&after(broken)),
        "the archive timestamp does not read as intact"
    );
    // An earlier timestamp, with bytes after it, is not the archive this
    // added.
    let mut earlier = archive();
    earlier.appended_bytes = 500;
    earlier.appendix = Some(crate::docinfo::Appendix::default());
    assert_eq!(
        refused(&after(earlier)),
        "the archive timestamp is not in it"
    );
    // And the earlier signatures are held to `covered` first.
    let mut found = after(archive());
    found[0].integrity.as_mut().expect("a verdict").verdict = Verdict::Altered;
    assert!(matches!(
        read_back(&before, &found, 900),
        Err(Refusal::Changed { .. })
    ));
}

// ------------------------------------------- what the 2026-10-10 review found

/// How a test puts a survey past one of a worker's bounds.
type Past = fn(&mut Survey);

/// The worker's word is not taken for its own bounds: a survey holding more
/// than the scan and `sign_dss::survey` allow is refused before any value in
/// it is parsed or handed to the OS.
#[test]
fn a_survey_past_a_workers_bounds_is_refused_before_any_of_it_is_parsed() {
    use crate::docinfo::{MAX_SIGNATURES, MAX_SIG_BLOB};
    let most = || Survey {
        complete: true,
        signatures: vec![Signature::default(); MAX_SIGNATURES],
        values: (0..MAX_SIGNATURES)
            .map(|n| (format!("S{n}"), vec![0x30; 16]))
            .collect(),
        store: vec![vec![0x30; 16]; crate::revocation::MAX_DSS_CERTIFICATES],
        ..Survey::default()
    };
    assert_eq!(bounded(&most()), Ok(()), "the control: at every bound");
    let cases: [(&str, Past); 6] = [
        ("one signature more", |s| {
            s.signatures.push(Signature::default());
        }),
        ("one value more", |s| s.values.push(("S".into(), vec![1]))),
        ("a value past its size", |s| {
            s.values[0].1 = vec![0; MAX_SIG_BLOB + 1];
        }),
        ("one certificate more", |s| s.store.push(vec![1])),
        ("a certificate past its size", |s| {
            s.store[0] = vec![0; crate::trust::MAX_CERTIFICATE_BYTES + 1];
        }),
        // Each inside its own bound, and more together than an answer carries.
        ("more together", |s| {
            for value in s.values.iter_mut().take(4) {
                value.1 = vec![0; MAX_SIG_BLOB];
            }
        }),
    ];
    for (what, alter) in cases {
        let mut survey = most();
        alter(&mut survey);
        let why = bounded(&survey).expect_err(what);
        assert!(
            matches!(why, Refusal::Failed(_)) && why.tpdf_failed(),
            "{what}: {why:?}"
        );
    }

    // And through the whole path: refused, with nobody asked.
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    let worker = Altering {
        survey: Some(|mut survey| {
            survey.store = vec![vec![0x30; 16]; crate::revocation::MAX_DSS_CERTIFICATES + 1];
            Ok(survey)
        }),
        ..Altering::default()
    };
    let why = added_by(&original, &roots(&pki), &pki.tsa, &worker).expect_err("refused");
    assert!(matches!(why, Refusal::Failed(_)), "{why:?}");
    assert!(pki.paths().is_empty(), "{:?}", pki.paths());
}

/// A later run works only while the certificates are still valid. Once the
/// signer's has expired nothing vouches for it now, and the run is refused
/// with nobody asked --- so a further archive timestamp has to be added
/// before that day, not after it.
#[test]
fn a_later_run_is_refused_once_a_certificate_has_expired() {
    let pki = Pki::start(good());
    let first = added(&sealed(&pki).bytes, &pki).expect("added").bytes;
    let asked = pki.paths().len();
    // Thirty years on: past every test certificate's last day.
    let later = now() + 30 * 365 * 86_400;
    let why =
        added_at(&first, &roots(&pki), &pki.tsa, &crate::save::Here, later).expect_err("refused");
    assert_eq!(
        why.sentence(),
        "the signer of Signature1 (tpdf test PKI signer - not a real identity) is not trusted \
         by this computer, so tpdf will not fetch revocation data for it: its certificate is \
         not in force now"
    );
    assert!(!why.tpdf_failed());
    assert_eq!(
        pki.paths().len(),
        asked,
        "nobody was asked for the later run"
    );
    // The control: the same bytes, today.
    assert!(added(&first, &pki).is_ok());
}

/// A second run adds what is new and writes nothing the `/DSS` already holds:
/// the certificates are the same ones, and tpdf's reader takes a bounded
/// number of streams of each kind.
#[test]
fn a_second_run_writes_no_certificate_the_dss_already_holds() {
    let pki = Pki::start(good());
    let first = added(&sealed(&pki).bytes, &pki).expect("added").bytes;
    let (certs, ocsps, _) = dss(&first);
    let second = added(&first, &pki).expect("added again").bytes;
    let (certs_now, ocsps_now, _) = dss(&second);
    assert_eq!(certs_now, certs, "the same certificates, each once");
    let mut distinct = ocsps_now.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), ocsps_now.len(), "no response twice");
    // Not "more than before": the test responder signs deterministically,
    // so a second answer minted in the same second is the first one byte for
    // byte, and is rightly not written again. What was there is still there.
    assert!(ocsps.iter().all(|response| ocsps_now.contains(response)));
    // And what the survey says the `/DSS` holds is what its arrays list.
    let survey = crate::sign_dss::survey(&second, 1);
    assert_eq!(survey.held, [certs_now.len(), ocsps_now.len(), 0]);
    assert!(!survey.store_cut);
}

/// A `/DSS` that already holds as much as tpdf's reader takes is refused
/// before anything is fetched: what would be added could not be read back,
/// and finding that out after the authorities were asked would be tpdf's own
/// failure for a property of the document.
#[test]
fn a_dss_with_no_room_left_is_refused_before_anything_is_fetched() {
    use crate::revocation::{MAX_DSS_CERTIFICATES, MAX_LISTS, MAX_RESPONSES};
    let pki = Pki::start(good());
    let original = sealed(&pki).bytes;
    // This document asks about two certificates and carries four.
    const NEW: usize = 4;
    const ASKED: usize = 2;
    let full: [fn(Survey) -> Result<Survey, String>; 4] = [
        |mut survey| {
            survey.held = [MAX_DSS_CERTIFICATES - NEW + 1, 0, 0];
            Ok(survey)
        },
        |mut survey| {
            survey.held = [0, MAX_RESPONSES - ASKED + 1, 0];
            Ok(survey)
        },
        |mut survey| {
            survey.held = [0, 0, MAX_LISTS - ASKED + 1];
            Ok(survey)
        },
        // The reader already leaves some of it out.
        |mut survey| {
            survey.store_cut = true;
            Ok(survey)
        },
    ];
    for alter in full {
        let worker = Altering {
            survey: Some(alter),
            ..Altering::default()
        };
        let why = added_by(&original, &roots(&pki), &pki.tsa, &worker).expect_err("refused");
        assert_eq!(why, Refusal::Full);
        assert!(!why.tpdf_failed(), "the document's, exit 3");
    }
    assert!(Refusal::Full
        .sentence()
        .starts_with("this document already carries as much validation data as tpdf reads"),);
    assert!(pki.paths().is_empty(), "{:?}", pki.paths());
    // The control: exactly room enough for all three, and it is done.
    let fits = Altering {
        survey: Some(|mut survey| {
            survey.held = [
                MAX_DSS_CERTIFICATES - NEW,
                MAX_RESPONSES - ASKED,
                MAX_LISTS - ASKED,
            ];
            Ok(survey)
        }),
        ..Altering::default()
    };
    assert!(added_by(&original, &roots(&pki), &pki.tsa, &fits).is_ok());
    // A certificate the `/DSS` already holds is not one more: a document
    // whose store is at the bound with exactly these certificates is done.
    let again = added(&original, &pki).expect("added").bytes;
    let holding = Altering {
        survey: Some(|mut survey| {
            survey.held = [MAX_DSS_CERTIFICATES, survey.held[1], 0];
            Ok(survey)
        }),
        ..Altering::default()
    };
    assert!(added_by(&again, &roots(&pki), &pki.tsa, &holding).is_ok());
}

/// A certificate with an issuer's name and key, under a root of its maker's
/// and naming a responder of its maker's, offered wherever a document can
/// offer one: in the signature's own set, in its token's, and in the `/DSS`.
/// It is never asked about and never carried.
#[test]
fn a_look_alike_issuer_is_never_asked_about_wherever_the_document_offers_it() {
    use crate::integrity::test_tsa::{Published, TestCa};
    static TWIN: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let plan = Plan {
        intermediate: true,
        intermediate_ocsp: Some(Serve::Good),
        ..good()
    };
    let asked_about_the_twin = |pki: &Pki| pki.paths().iter().any(|path| path.contains("twin"));
    let twin_of = |pki: &Pki| {
        let theirs = TestCa::new("tpdf test injected root", 0x5a);
        let twin = theirs.cross_publishing(
            pki.intermediate.as_ref().expect("an intermediate"),
            3,
            &Published {
                ocsp: vec![format!("{}/ocsp/twin", pki.base)],
                ..Published::default()
            },
        );
        (twin, theirs.certificate)
    };
    let key = |pki: &Pki| Soft::p256(pki.signer.seed);

    // In the signature's own set, before the genuine issuer: the walk meets
    // it first, it is on no chain the store vouched for, and the run is
    // refused with nobody asked.
    let pki = Pki::start(plan);
    let (twin, theirs) = twin_of(&pki);
    let mut chain = vec![twin.clone(), theirs.clone()];
    chain.extend(pki.chain.iter().cloned());
    let original = signed_again(plain_pdf(), &pki.signer.certificate, &chain, &key(&pki));
    let value = crate::sign_dss::survey(&original, 1).values[0].1.clone();
    assert!(
        value.windows(twin.len()).any(|w| w == twin),
        "the fixture: the twin is in the signature"
    );
    let why = added(&original, &pki).expect_err("refused");
    assert!(
        matches!(
            &why,
            Refusal::About {
                field: Some(field),
                why: super::super::Refusal::NoIssuer(_),
            } if field == "Signature1"
        ),
        "{why:?}"
    );
    assert!(pki.paths().is_empty(), "{:?}", pki.paths());

    // In the token's set, and in the `/DSS`: the genuine issuer is in the
    // signature and is found first, so the run is done --- and the twin is
    // neither asked about nor written.
    let pki = Pki::start(plan);
    let (twin, theirs) = twin_of(&pki);
    let mut tampered = pki.tsa.clone();
    tampered.carried.push(twin.clone());
    tampered.carried.push(theirs.clone());
    let original = super::super::tests::sealed_by(&pki, &tampered).bytes;
    let _ = TWIN.set(twin.clone());
    let offering = Altering {
        survey: Some(|mut survey| {
            // First in the store, where a walk over the store would meet it.
            survey
                .store
                .insert(0, TWIN.get().expect("the twin").clone());
            Ok(survey)
        }),
        ..Altering::default()
    };
    let done = added_by(&original, &roots(&pki), &pki.tsa, &offering).expect("added");
    assert!(!asked_about_the_twin(&pki), "{:?}", pki.paths());
    assert_eq!(
        pki.paths(),
        ["/ocsp/signer", "/ocsp/intermediate", "/ocsp/authority"]
    );
    let (certs, _, _) = dss(&done.bytes);
    assert!(!certs.contains(&twin) && !certs.contains(&theirs));
}

/// `signed`, with a revision after it that certifies the document at `level`
/// through the catalog's `/Perms /DocMDP` --- a real reference, as
/// `sign_prepare`'s refusal reads one, and no field of the form.
fn certified_after(signed: &[u8], level: lopdf::Object) -> Vec<u8> {
    use lopdf::{dictionary, Document, IncrementalDocument, Object};
    let prev = Document::load_mem(signed).expect("parses");
    let root = prev
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("a catalog");
    let mut catalog = prev.get_dictionary(root).expect("the catalog").clone();
    let mut incremental = IncrementalDocument::create_from(signed.to_vec(), prev);
    let certification = incremental.new_document.add_object(dictionary! {
        "Type" => "Sig",
        "Reference" => vec![Object::Dictionary(dictionary! {
            "Type" => "SigRef",
            "TransformMethod" => "DocMDP",
            "TransformParams" => dictionary! { "Type" => "TransformParams", "P" => level },
        })],
    });
    catalog.set("Perms", dictionary! { "DocMDP" => certification });
    incremental.new_document.set_object(root, catalog);
    let mut bytes = Vec::new();
    incremental.save_to(&mut bytes).expect("saved");
    bytes
}

/// The catalog's certification decides, read from the document and not set
/// by hand: no changes permitted is refused before anybody is asked ---
/// written as `1` or as `1.0` --- and the two levels that permit a signature
/// are done.
#[test]
fn a_document_certified_in_its_catalog_is_judged_by_the_level_it_states() {
    use lopdf::Object;
    let pki = Pki::start(good());
    let signed = sealed(&pki).bytes;
    // `1.0` as the file spells it: `lopdf` writes the real number one as
    // `1`, which reads back as an integer and would test nothing, so the
    // level is written as another real and respelt in the bytes.
    let mut real = certified_after(&signed, Object::Real(1.5));
    let at = real
        .windows(6)
        .position(|w| w == b"/P 1.5")
        .expect("the level as written");
    real[at..at + 6].copy_from_slice(b"/P 1.0");
    for (level, original) in [
        ("1", certified_after(&signed, Object::Integer(1))),
        ("1.0", real),
    ] {
        assert_eq!(
            crate::sign_dss::survey(&original, 1).certified,
            1,
            "{level:?}"
        );
        let why = added(&original, &pki).expect_err("refused");
        assert_eq!(why, Refusal::Certified(None), "{level:?}");
        assert!(
            why.sentence()
                .starts_with("this document is certified with no changes permitted"),
            "{}",
            why.sentence()
        );
    }
    assert!(pki.paths().is_empty(), "{:?}", pki.paths());
    for level in [2, 3] {
        let original = certified_after(&signed, Object::Integer(level));
        assert_eq!(
            crate::sign_dss::survey(&original, 1).certified,
            u8::try_from(level).expect("a level")
        );
        let done = added(&original, &pki).unwrap_or_else(|why| panic!("level {level}: {why:?}"));
        assert_eq!(read(&done.bytes).len(), 2, "level {level}");
    }
    // A level that is no whole number states nothing the specification
    // defines, and is not read as one that is.
    let odd = certified_after(&signed, Object::Real(1.5));
    assert_eq!(crate::sign_dss::survey(&odd, 1).certified, 0);
}

/// Writes documents for other readers to judge: a signed and timestamped
/// document, the same with what [`add`] adds, and the roots beside them.
///
/// `TPDF_LONG_TERM_OUT=<dir> cargo test --lib
/// write_documents_with_data_added_for_other_readers -- --ignored`.
#[test]
#[ignore = "an instrument: writes files for pyHanko, qpdf and OpenSSL to read"]
fn write_documents_with_data_added_for_other_readers() {
    let Ok(dir) = std::env::var("TPDF_LONG_TERM_OUT") else {
        panic!("set TPDF_LONG_TERM_OUT to a directory");
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("the directory");
    let pki = Pki::start(good());
    std::fs::write(dir.join("root.der"), &pki.root.certificate).expect("written");
    std::fs::write(dir.join("tsa-root.der"), &pki.tsa.root).expect("written");
    for (name, original) in [
        ("stamped", sealed(&pki).bytes),
        ("unstamped", untimestamped(&pki)),
    ] {
        std::fs::write(dir.join(format!("{name}.pdf")), &original).expect("written");
        let once = added(&original, &pki).expect("added").bytes;
        std::fs::write(dir.join(format!("{name}-added.pdf")), &once).expect("written");
        let twice = added(&once, &pki).expect("added again").bytes;
        std::fs::write(dir.join(format!("{name}-added-twice.pdf")), &twice).expect("written");
    }
}
