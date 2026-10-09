use super::testkeys::{
    self, certificate, signed, verdicts, Misdirected, Refusing, Soft, Spec, NOW,
};
use super::*;
use crate::integrity::Verdict;
use x509_cert::ext::pkix::KeyUsages;

/// A self-issued certificate for `key` with the defaults.
fn own(key: &Soft, name: &str) -> Vec<u8> {
    certificate(key, &Spec::new(name))
}

// ------------------------------------------------------ the control, first

#[test]
fn a_signature_made_here_is_intact_under_the_verifier_the_dialog_uses() {
    // The control for every refusal below: a pipeline that refused everything
    // would pass them all, and this is what fails it. One per key kind, because
    // each signs by different mathematics and `integrity.rs` checks each.
    let original = testkeys::plain_pdf();
    for (key, method) in [
        (Soft::rsa(), "RSA"),
        (Soft::p256(7), "ECDSA P-256"),
        (Soft::p384(9), "ECDSA P-384"),
    ] {
        let cert = own(&key, "Signer");
        let bytes = signed(&original, &cert, &[], &key).expect("signed");
        // An incremental revision: the original is the file's first bytes.
        assert_eq!(&bytes[..original.len()], &original[..], "{method}");
        let found = verdicts(&bytes);
        assert_eq!(found.len(), 1, "{method}");
        let (field, integrity) = &found[0];
        assert_eq!(field, "Signature1");
        assert_eq!(
            integrity.verdict,
            Verdict::Intact,
            "{method}: {integrity:?}"
        );
        assert_eq!(integrity.digest, "SHA-256", "{method}");
        assert_eq!(integrity.method, method);
    }
}

// ---------------------------------------- the worker's numbers are checked

#[test]
fn the_workers_digest_is_recomputed_rather_than_believed() {
    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = own(&key, "Signer");
    let mut unsigned = crate::sign_prepare::prepare(original.clone(), NOW, None).expect("prepared");
    unsigned.digest[5] ^= 0x40;
    let why = finish(original, unsigned, NOW, &cert, &[], &key).expect_err("refused");
    assert!(why.contains("digest"), "{why}");
}

#[test]
fn an_update_built_against_other_bytes_is_refused() {
    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = own(&key, "Signer");
    let unsigned = crate::sign_prepare::prepare(original.clone(), NOW, None).expect("prepared");

    // A different length: the offsets cannot be this file's.
    let mut longer = original.clone();
    longer.push(b'\n');
    let why = finish(longer, unsigned.clone(), NOW, &cert, &[], &key).expect_err("refused");
    assert!(why.contains("was built against"), "{why}");

    // The same length and one byte changed: only the digest can see it.
    let mut changed = original.clone();
    changed[20] ^= 0x01;
    let why = finish(changed, unsigned, NOW, &cert, &[], &key).expect_err("refused");
    assert!(why.contains("digest"), "{why}");
}

#[test]
fn a_range_that_does_not_frame_its_hole_is_refused() {
    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = own(&key, "Signer");
    let prepared = crate::sign_prepare::prepare(original.clone(), NOW, None).expect("prepared");
    for (index, delta) in [(1usize, 2i64), (2, -2), (3, 1), (0, 1)] {
        let mut unsigned = prepared.clone();
        unsigned.range[index] = (unsigned.range[index] as i64 + delta) as u64;
        let why = finish(original.clone(), unsigned, NOW, &cert, &[], &key).expect_err("refused");
        assert!(
            why.contains("does not frame"),
            "range[{index}]{delta:+}: {why}"
        );
    }
}

#[test]
fn a_hole_that_is_not_empty_is_refused() {
    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = own(&key, "Signer");
    let mut unsigned = crate::sign_prepare::prepare(original.clone(), NOW, None).expect("prepared");
    let at = unsigned.range[1] as usize - original.len() + 5;
    unsigned.update[at] = b'1';
    let why = finish(original, unsigned, NOW, &cert, &[], &key).expect_err("refused");
    assert!(why.contains("empty value"), "{why}");
}

// ------------------------------------------------ the signature and the key

#[test]
fn a_signature_over_the_wrong_digest_is_broken_and_not_written() {
    // A real key with a real certificate; only the value is over something
    // else. The self-check is what refuses it, and the verdict it names is the
    // one the dialog would have shown.
    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = own(&key, "Signer");
    let why = signed(&original, &cert, &[], &Misdirected(Soft::p256(3))).expect_err("refused");
    assert!(why.contains("Broken"), "{why}");
}

#[test]
fn a_key_that_is_not_the_certificates_is_refused() {
    let original = testkeys::plain_pdf();
    let cert = own(&Soft::p256(3), "Signer");
    let why = signed(&original, &cert, &[], &Soft::p256(4)).expect_err("refused");
    assert!(why.contains("Broken"), "{why}");
}

#[test]
fn the_oss_refusal_is_what_the_reader_is_told() {
    // The builder's own error carries no message; the key's must survive it.
    let original = testkeys::plain_pdf();
    let cert = own(&Soft::p256(3), "Signer");
    let why = signed(&original, &cert, &[], &Refusing).expect_err("refused");
    assert_eq!(why, "the reader cancelled the PIN prompt");
}

#[test]
fn a_value_spliced_at_the_wrong_offset_is_not_intact() {
    // The control for the splice's arithmetic: the same blob two digits late
    // leaves the hole holding something other than the value --- which the
    // verifier refuses to call intact.
    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = own(&key, "Signer");
    let unsigned = crate::sign_prepare::prepare(original.clone(), NOW, None).expect("prepared");
    let digest = check(&original, &unsigned).expect("checked");
    let blob = build(&digest, &cert, &[], &key).expect("built");

    let mut right = unsigned.update.clone();
    splice(&mut right, unsigned.built_against, unsigned.range, &blob).expect("spliced");
    let mut late = unsigned.update.clone();
    let mut shifted = unsigned.range;
    shifted[1] += 2;
    splice(&mut late, unsigned.built_against, shifted, &blob).expect("spliced");

    let file = |update: &[u8]| [original.as_slice(), update].concat();
    assert_eq!(verdicts(&file(&right))[0].1.verdict, Verdict::Intact);
    let wrong = &verdicts(&file(&late))[0].1;
    assert_ne!(wrong.verdict, Verdict::Intact, "{wrong:?}");
}

#[test]
fn the_value_is_spliced_as_uppercase_hex_at_the_start_of_the_hole() {
    let mut update = vec![b'0'; 20];
    update[4] = b'<';
    update[19] = b'>';
    splice(&mut update, 100, [0, 104, 120, 0], &[0xab, 0x01]).expect("spliced");
    assert_eq!(&update[4..10], b"<AB010");
}

#[test]
fn a_blob_past_the_step_two_limit_is_refused() {
    let mut update = vec![b'0'; RESERVED * 2 + 2];
    let range = [0, 0, (RESERVED * 2 + 2) as u64, 0];
    assert!(splice(&mut update, 0, range, &vec![1; STEP_TWO_LIMIT]).is_ok());
    let why = splice(&mut update, 0, range, &vec![1; STEP_TWO_LIMIT + 1]).expect_err("refused");
    assert!(why.contains("timestamp"), "{why}");
}

// ------------------------------------------------------ what the CMS holds

/// The one `SignerInfo` of a finished blob, and the whole `SignedData`.
fn decoded(blob: &[u8]) -> cms::signed_data::SignedData {
    let info = cms::content_info::ContentInfo::from_der(blob).expect("content info");
    info.content.decode_as().expect("signed data")
}

#[test]
fn the_signed_attributes_are_exactly_the_three_pades_b_b_names() {
    let original = testkeys::plain_pdf();
    let key = Soft::rsa();
    let cert = own(&key, "Signer");
    let unsigned = crate::sign_prepare::prepare(original.clone(), NOW, None).expect("prepared");
    let digest = check(&original, &unsigned).expect("checked");
    let data = decoded(&build(&digest, &cert, &[], &key).expect("built"));
    let info = data.signer_infos.0.as_slice()[0].clone();
    let mut names: Vec<String> = info
        .signed_attrs
        .as_ref()
        .expect("signed attributes")
        .iter()
        .map(|a| a.oid.to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "1.2.840.113549.1.9.16.2.47", // signingCertificateV2
            "1.2.840.113549.1.9.3",       // contentType
            "1.2.840.113549.1.9.4",       // messageDigest
        ],
        "no signingTime: B-B keeps the time in /M"
    );
    assert!(data.encap_content_info.econtent.is_none(), "detached");
    assert_eq!(info.digest_alg.oid.to_string(), "2.16.840.1.101.3.4.2.1");
    assert_eq!(
        info.signature_algorithm.oid.to_string(),
        "1.2.840.113549.1.1.11"
    );
}

#[test]
fn the_signing_certificate_attribute_names_this_certificate_by_hash_and_serial() {
    let original = testkeys::plain_pdf();
    let key = Soft::p256(5);
    let cert = own(&key, "Signer");
    let unsigned = crate::sign_prepare::prepare(original.clone(), NOW, None).expect("prepared");
    let digest = check(&original, &unsigned).expect("checked");
    let data = decoded(&build(&digest, &cert, &[], &key).expect("built"));
    let attributes = data.signer_infos.0.as_slice()[0]
        .signed_attrs
        .clone()
        .expect("attributes");
    let attribute = attributes
        .iter()
        .find(|a| a.oid == SIGNING_CERTIFICATE_V2)
        .expect("signingCertificateV2");
    let value: SigningCertificateV2 = attribute.values.as_slice()[0].decode_as().expect("decodes");
    let parsed = Certificate::from_der(&cert).expect("certificate");
    let [id] = value.certs.as_slice() else {
        panic!("one certificate named");
    };
    assert_eq!(
        id.cert_hash.as_bytes(),
        sha2_10::Sha256::digest(&cert).as_slice()
    );
    assert_eq!(
        id.issuer_serial.serial_number,
        parsed.tbs_certificate.serial_number
    );
    assert_eq!(
        id.issuer_serial.issuer,
        vec![GeneralName::DirectoryName(parsed.tbs_certificate.issuer)]
    );
}

#[test]
fn the_certificates_set_carries_the_chain_and_the_signer_once() {
    let original = testkeys::plain_pdf();
    let ca = Soft::p384(11);
    let ca_cert = certificate(
        &ca,
        &Spec {
            serial: 9,
            ..Spec::new("Test CA")
        },
    );
    let key = Soft::p256(12);
    let cert = certificate(
        &key,
        &Spec {
            issuer: Some((&ca, "Test CA")),
            ..Spec::new("Signer")
        },
    );
    let unsigned = crate::sign_prepare::prepare(original.clone(), NOW, None).expect("prepared");
    let digest = check(&original, &unsigned).expect("checked");
    // The OS may return the signer among the chain; it must appear once.
    let blob = build(&digest, &cert, &[ca_cert.clone(), cert.clone()], &key).expect("built");
    let set = decoded(&blob).certificates.expect("certificates");
    let mut held: Vec<Vec<u8>> = set
        .0
        .iter()
        .map(|choice| match choice {
            CertificateChoices::Certificate(c) => c.to_der().expect("der"),
            CertificateChoices::Other(_) => panic!("only certificates"),
        })
        .collect();
    held.sort();
    let mut want = vec![ca_cert, cert.clone()];
    want.sort();
    assert_eq!(held, want);
    // And the result is still intact with an issuer that is not the signer.
    let bytes = signed(&original, &cert, &[], &key).expect("signed");
    assert_eq!(verdicts(&bytes)[0].1.verdict, Verdict::Intact);
}

#[test]
fn a_raw_ecdsa_value_becomes_the_der_the_curve_crate_writes() {
    // Windows' CNG answers r || s. The oracle is `p256`'s own DER encoder, over
    // enough signatures that a high bit and a leading zero both turn up.
    use ecdsa::signature::hazmat::PrehashSigner as _;
    let key = p256::ecdsa::SigningKey::from_bytes(&[3u8; 32].into()).expect("key");
    let (mut high, mut zero) = (false, false);
    for n in 0u8..=255 {
        let signature: p256::ecdsa::Signature = key.sign_prehash(&[n; 32]).expect("signed");
        let raw = signature.to_bytes();
        high |= raw[0] & 0x80 != 0 || raw[32] & 0x80 != 0;
        zero |= raw[0] == 0 || raw[32] == 0;
        assert_eq!(
            ecdsa_der(&raw).expect("converted"),
            signature.to_der().as_bytes(),
            "digest byte {n}"
        );
    }
    assert!(
        high && zero,
        "both edge cases reached: high {high}, zero {zero}"
    );
    // And the case the loop reaches only by luck, pinned by hand: a half whose
    // leading zeros are followed by a byte *without* its top bit, which must
    // lose every one of them. A half led by a zero and then a high byte comes
    // out the same with the zero stripped or kept, so it cannot tell --- and a
    // mutation keeping the zeros survived the loop alone.
    assert_eq!(
        ecdsa_der(&[0, 0, 1, 5, 0x80, 0, 0, 1]).expect("converted"),
        [0x30, 0x0b, 0x02, 0x02, 1, 5, 0x02, 0x05, 0, 0x80, 0, 0, 1]
    );
    // An all-zero half is the integer zero, one byte long.
    assert_eq!(
        ecdsa_der(&[0, 0, 0, 7]).expect("converted"),
        [0x30, 0x06, 0x02, 0x01, 0, 0x02, 0x01, 7]
    );
    assert!(ecdsa_der(&[1, 2, 3]).is_err());
    assert!(ecdsa_der(&[]).is_err());
}

// --------------------------------------------------- which certificates

/// **A code-signing certificate is not offered for signing a document.** The
/// shape is the owner's own Developer ID Application certificate, measured
/// 2026-09-26: key usage `digitalSignature`, extended key usage code signing
/// alone, both critical. The key usage passes; only the extended key usage can
/// say the certificate was issued to sign code. The controls are the purposes
/// that are a document signature's, and a certificate that states none.
#[test]
fn signing_refuses_a_certificate_the_listing_would_not_offer() {
    // The rule is applied where the key is used, not only where certificates
    // are listed: the window's command signs whatever identity the webview
    // names. The control is the same key under a certificate for documents,
    // which must sign, or a `finish` that refused everything would pass.
    let original = testkeys::plain_pdf();
    let key = Soft::p256(23);
    let sign = |spec: Spec<'_>| {
        let unsigned = crate::sign_prepare::prepare(original.clone(), NOW, None).expect("prepared");
        finish(
            original.clone(),
            unsigned,
            NOW,
            &certificate(&key, &spec),
            &[],
            &key,
        )
    };

    sign(Spec::new("Signer")).expect("a certificate for documents signs");
    let code = sign(Spec {
        usage: Some(vec![KeyUsages::DigitalSignature]),
        purposes: Some(vec!["1.3.6.1.5.5.7.3.3"]),
        ..Spec::new("Developer")
    })
    .expect_err("code signing");
    assert_eq!(
        code,
        "tpdf will not sign with this certificate: it is issued for code signing, not for signing documents"
    );
    let expired = sign(Spec {
        not_before: NOW - 86_400 * 400,
        not_after: NOW - 86_400,
        ..Spec::new("Lapsed")
    })
    .expect_err("expired");
    assert!(
        expired.starts_with("tpdf will not sign with this certificate: "),
        "{expired}"
    );
}

#[test]
fn a_certificate_issued_for_code_signing_is_not_offered() {
    let key = Soft::p256(22);
    let offer = |purposes: Option<Vec<&str>>| {
        usable(
            &certificate(
                &key,
                &Spec {
                    usage: Some(vec![KeyUsages::DigitalSignature]),
                    purposes,
                    ..Spec::new("Developer")
                },
            ),
            NOW,
        )
    };

    assert_eq!(
        offer(Some(vec!["1.3.6.1.5.5.7.3.3"])).expect_err("code signing"),
        "it is issued for code signing, not for signing documents"
    );
    assert_eq!(
        offer(Some(vec!["1.3.6.1.5.5.7.3.1", "1.3.6.1.5.5.7.3.2"])).expect_err("TLS"),
        "it is issued for web servers and logging in, not for signing documents"
    );
    for purpose in super::DOCUMENT_PURPOSES {
        assert!(
            offer(Some(vec!["1.3.6.1.5.5.7.3.2", purpose])).is_ok(),
            "{purpose}"
        );
    }
    assert!(offer(None).is_ok(), "no extended key usage restricts none");
}

#[test]
fn a_certificate_is_offered_only_while_valid_for_signing() {
    let key = Soft::p256(21);
    let offer = |spec: Spec<'_>| usable(&certificate(&key, &spec), NOW);

    let ok = offer(Spec::new("Signer")).expect("offered");
    assert_eq!(ok.subject, "Signer");
    assert_eq!(ok.issuer, "Signer");
    assert_eq!(ok.method, "ECDSA P-256");
    assert!(ok.expires.ends_with(" UTC"), "{}", ok.expires);

    let expired = offer(Spec {
        not_after: NOW - 1,
        ..Spec::new("Old")
    });
    assert_eq!(expired.expect_err("expired"), "it has expired");
    let early = offer(Spec {
        not_before: NOW + 1,
        ..Spec::new("New")
    });
    assert_eq!(early.expect_err("early"), "it is not valid yet");
    // The edges are inside.
    assert!(offer(Spec {
        not_after: NOW,
        ..Spec::new("Edge")
    })
    .is_ok());
    assert!(offer(Spec {
        not_before: NOW,
        ..Spec::new("Edge")
    })
    .is_ok());

    let encipher = offer(Spec {
        usage: Some(vec![KeyUsages::KeyEncipherment]),
        ..Spec::new("Encipher")
    });
    assert_eq!(
        encipher.expect_err("not for signing"),
        "it is not issued for signing"
    );
    for usage in [KeyUsages::DigitalSignature, KeyUsages::NonRepudiation] {
        assert!(
            offer(Spec {
                usage: Some(vec![usage, KeyUsages::KeyEncipherment]),
                ..Spec::new("Signing")
            })
            .is_ok(),
            "{usage:?}"
        );
    }
}

#[test]
fn only_the_keys_tpdf_signs_with_are_offered() {
    let rsa = usable(&own(&Soft::rsa(), "R"), NOW).expect("RSA 2048");
    assert_eq!(rsa.method, "RSA 2048");
    assert_eq!(
        usable(&own(&Soft::p384(2), "P"), NOW)
            .expect("P-384")
            .method,
        "ECDSA P-384"
    );

    let small = usable(&own(&Soft::rsa_of(1024), "Small"), NOW).expect_err("1024 bits");
    assert!(small.contains("1024 bits"), "{small}");

    // A key of a kind nothing here signs with: the SPKI relabelled Ed25519.
    let mut cert = Certificate::from_der(&own(&Soft::p256(2), "Ed")).expect("cert");
    cert.tbs_certificate.subject_public_key_info.algorithm = AlgorithmIdentifierOwned {
        oid: ObjectIdentifier::new_unwrap("1.3.101.112"),
        parameters: None,
    };
    let why = usable(&cert.to_der().expect("der"), NOW).expect_err("Ed25519");
    assert!(why.contains("only RSA and ECDSA"), "{why}");

    // A curve that is not one of the two.
    let mut cert = Certificate::from_der(&own(&Soft::p256(2), "K")).expect("cert");
    cert.tbs_certificate
        .subject_public_key_info
        .algorithm
        .parameters = Some(
        der::Any::encode_from(&ObjectIdentifier::new_unwrap("1.3.132.0.10")).expect("secp256k1"),
    );
    let why = usable(&cert.to_der().expect("der"), NOW).expect_err("secp256k1");
    assert!(why.contains("curve"), "{why}");

    assert!(usable(b"not a certificate", NOW).is_err());
}

#[test]
fn the_chooser_lists_both_what_may_sign_and_what_may_not() {
    let found = vec![
        ("a".to_string(), own(&Soft::p256(31), "Usable")),
        (
            "b".to_string(),
            certificate(
                &Soft::p256(32),
                &Spec {
                    not_after: NOW - 1,
                    ..Spec::new("Expired")
                },
            ),
        ),
        ("c".to_string(), b"garbage".to_vec()),
    ];
    let choices = choices(&found, NOW);
    assert_eq!(choices.usable.len(), 1);
    assert_eq!(choices.usable[0].id, "a");
    assert_eq!(choices.usable[0].subject, "Usable");
    assert_eq!(
        choices.skipped,
        vec![
            Skipped {
                subject: "Expired".into(),
                why: "it has expired".into()
            },
            Skipped {
                subject: "(a certificate with no readable name)".into(),
                why: "it could not be read".into()
            },
        ]
    );
}

#[test]
fn the_report_marks_the_new_signature_and_leaves_out_unsigned_fields() {
    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = own(&key, "Signer");
    let bytes = signed(&original, &cert, &[], &key).expect("signed");
    let first = crate::docinfo::scan(&bytes, 1, None)
        .expect("parses")
        .signatures;
    let mut unsigned_field = first[0].clone();
    unsigned_field.signed = false;
    unsigned_field.field = "Empty".into();
    let mut found = first.clone();
    found.push(unsigned_field);
    let at = ours(&found, "Signature1", 0);
    assert_eq!(at, Some(0));
    let report = report("/x.pdf".into(), "Signature1".into(), found, at);
    assert_eq!(report.signatures.len(), 1);
    assert!(report.signatures[0].ours);
    assert_eq!(report.signatures[0].field, "Signature1");
    let at = ours(&first, "Signature9", 0);
    let other = super::report("/x.pdf".into(), "Signature9".into(), first, at);
    assert!(!other.signatures[0].ours);
}

/// Which signature is the new one is decided by where its range ends. A name
/// can be an older field's too, and until 2026-10-09 the name alone decided.
#[test]
fn the_new_signature_is_the_one_whose_range_ends_where_its_revision_does() {
    use crate::docinfo::Signature;
    use crate::integrity::{Integrity, Verdict, Why};
    let field = |name: &str, appended, verdict, why| Signature {
        field: name.into(),
        signed: true,
        appended_bytes: appended,
        integrity: Some(Integrity {
            verdict,
            why,
            ..Integrity::default()
        }),
        ..Signature::default()
    };
    let older = field("Signature1", 4_321, Verdict::Intact, None);
    let new = field("Signature1", 0, Verdict::Intact, None);
    let other = field("Signature2", 0, Verdict::Intact, None);

    // An older signature of the same name, listed first: not ours.
    let found = [older.clone(), new.clone()];
    assert_eq!(ours(&found, "Signature1", 0), Some(1));
    // And the report marks one signature as the new one, not both.
    let marked: Vec<bool> = report(String::new(), "Signature1".into(), found.to_vec(), Some(1))
        .signatures
        .iter()
        .map(|s| s.ours)
        .collect();
    assert_eq!(marked, [false, true]);
    // The new one missing: the older one does not stand in for it.
    assert_eq!(ours(std::slice::from_ref(&older), "Signature1", 0), None);
    // After a long-term signing the new signature is followed by exactly what
    // was appended to it, and the older one by more.
    let extended = [
        field("Signature1", 9_000, Verdict::Intact, None),
        field("Signature1", 4_679, Verdict::Intact, None),
    ];
    assert_eq!(ours(&extended, "Signature1", 4_679), Some(1));
    // The name still counts, and so does being signed.
    assert_eq!(ours(std::slice::from_ref(&other), "Signature1", 0), None);
    let mut empty = new.clone();
    empty.signed = false;
    assert_eq!(ours(&[empty], "Signature1", 0), None);
    // A field whose range nobody could read is given no appended bytes
    // either; where it shares the name, the intact one is ours, whichever is
    // listed first --- and with no intact one, the first that fits.
    let unread = field("Signature1", 0, Verdict::Unchecked, Some(Why::Range));
    assert_eq!(
        ours(&[unread.clone(), new.clone()], "Signature1", 0),
        Some(1)
    );
    assert_eq!(ours(&[new, unread.clone()], "Signature1", 0), Some(0));
    assert_eq!(ours(&[older, unread], "Signature1", 0), Some(1));
}

#[test]
fn a_document_past_the_workers_bound_is_refused_and_one_at_it_is_not() {
    let bound = crate::save::APPEND_MAX_BYTES;
    assert!(refuse_too_large(bound).is_ok());
    let why = refuse_too_large(bound + 1).expect_err("refused");
    assert!(why.contains("tpdf signs documents of up to"), "{why}");
}

#[test]
fn unsaved_edits_refuse_before_anything_is_signed() {
    assert!(refuse_unsaved(false).is_ok());
    let why = refuse_unsaved(true).expect_err("refused");
    assert!(why.starts_with("Save your changes first"), "{why}");
}

#[test]
fn a_blob_measured_for_the_reserve_fits_with_room_for_a_timestamp() {
    // The number `sign_prepare::RESERVED`'s note quotes, measured rather than
    // asserted: an RSA-2048 signer with an RSA-2048 issuer is a few kilobytes
    // and far inside the half step 2 may use.
    let original = testkeys::plain_pdf();
    let ca = Soft::rsa();
    let ca_cert = certificate(
        &ca,
        &Spec {
            serial: 2,
            ..Spec::new("Test CA")
        },
    );
    let key = Soft::rsa();
    let cert = certificate(
        &key,
        &Spec {
            issuer: Some((&ca, "Test CA")),
            ..Spec::new("Signer")
        },
    );
    let unsigned = crate::sign_prepare::prepare(original.clone(), NOW, None).expect("prepared");
    let digest = check(&original, &unsigned).expect("checked");
    let blob = build(&digest, &cert, &[ca_cert], &key).expect("built");
    println!(
        "RSA-2048 signer + RSA-2048 issuer: {} bytes of CMS",
        blob.len()
    );
    assert!(blob.len() < 4096, "{}", blob.len());
}

#[test]
fn no_private_key_type_is_named_outside_the_tests() {
    // The control `.cargo/audit.toml` cites for accepting RUSTSEC-2023-0071, a
    // timing leak in `rsa`'s private-key operations: tpdf performs them only
    // here, with keys the tests make. A private-key type named in any shipped
    // source file is the day that stops being true, and this goes red on it.
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .as_path(),
        &mut files,
    );
    let test_only = |path: &std::path::Path| {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // `test_tsa.rs` is the software timestamp authority: `#[cfg(test)]` in
        // the library and included by path into `tests/cli.rs`, never shipped.
        name == "tests.rs"
            || name == "testkeys.rs"
            || name == "test_tsa.rs"
            || name.ends_with("_tests.rs")
    };
    let naming = |path: &std::path::Path| {
        let text = std::fs::read_to_string(path).expect("source");
        ["RsaPrivateKey", "SigningKey"]
            .iter()
            .any(|needle| text.contains(needle))
    };
    // The emptiness control: the scan reaches the files that do name them.
    let seen: Vec<_> = files.iter().filter(|p| test_only(p) && naming(p)).collect();
    assert!(
        seen.len() >= 2,
        "the scan found no test file naming a key: {seen:?}"
    );
    let shipped: Vec<_> = files
        .iter()
        .filter(|p| !test_only(p) && naming(p))
        .collect();
    assert!(files.len() > 100, "only {} source files", files.len());
    assert_eq!(shipped, Vec::<&std::path::PathBuf>::new());
}

// ------------------------------------------------ a timestamp on the signature

/// A signature made over the plain test document, not yet written.
fn made() -> Made {
    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = own(&key, "Signer");
    let unsigned = crate::sign_prepare::prepare(original.clone(), NOW, None).expect("prepared");
    sign(original, unsigned, NOW, &cert, &[], &key).expect("made")
}

/// A token from the test authority over `made`'s signature value, as an
/// authority answers a request for it.
fn token_for(made: &Made, faults: &crate::integrity::test_tsa::Faults) -> Vec<u8> {
    use crate::integrity::test_tsa::{mint_with, Imprint, TestTsa};
    let value = made.value().expect("a value");
    mint_with(
        Imprint::Sha256,
        &Imprint::Sha256.digest(&value),
        Some(&[7; 8]),
        NOW,
        &TestTsa::new(),
        faults,
    )
}

#[test]
fn a_timestamped_signature_is_written_intact_with_its_time_attested() {
    let signature = made();
    let token = token_for(&signature, &Default::default());
    let stamped = signature.stamped(&token).expect("stamped");
    let bytes = signature.seal(Some(stamped)).expect("sealed");
    // Read as the properties dialog reads it: the signature intact, and its
    // timestamp a verdict of its own, intact and attested.
    let found = crate::docinfo::scan(&bytes, 1, None).expect("scanned");
    let ours = found.signatures.iter().find(|s| s.signed).expect("one");
    assert_eq!(
        ours.integrity.as_ref().map(|i| i.verdict),
        Some(Verdict::Intact)
    );
    let stamp = ours.timestamp.as_ref().expect("a timestamp");
    assert_eq!(
        stamp.integrity.as_ref().map(|i| i.verdict),
        Some(Verdict::Intact),
        "{stamp:?}"
    );
    assert!(stamp.attested);
}

#[test]
fn stamping_changes_nothing_the_key_signed() {
    // The token is an *unsigned* attribute: decoded and encoded again, every
    // other byte of the CMS --- the signed attributes, the value, the
    // certificates --- is the one the OS signed.
    let signature = made();
    let token = token_for(&signature, &Default::default());
    let stamped = signature.stamped(&token).expect("stamped");
    assert_ne!(stamped, signature.blob, "the token is not in it");
    assert_eq!(blob_without_token(&stamped), signature.blob);
    let (_, decoded) = super::decoded(&stamped).expect("decodes");
    let signer = &decoded.signer_infos.0.as_slice()[0];
    let unsigned = signer.unsigned_attrs.as_ref().expect("unsigned attributes");
    assert_eq!(unsigned.len(), 1);
    assert_eq!(
        unsigned.iter().next().map(|a| a.oid),
        Some(TIME_STAMP_TOKEN)
    );
}

#[test]
fn a_timestamp_that_does_not_check_out_in_the_written_bytes_is_not_written() {
    // `tsa::accept` refuses these before they get here; this is the last check,
    // over the finished file, and it must refuse them on its own.
    use crate::integrity::test_tsa::Faults;
    for faults in [
        Faults {
            corrupt_signature: true,
            ..Faults::default()
        },
        Faults {
            wrong_imprint: true,
            ..Faults::default()
        },
        Faults {
            sha1_signature: true,
            ..Faults::default()
        },
    ] {
        let signature = made();
        let token = token_for(&signature, &faults);
        let stamped = signature.stamped(&token).expect("stamped");
        let why = signature.seal(Some(stamped)).expect_err("refused");
        assert!(
            why.contains("timestamp") && why.contains("nothing was written"),
            "{faults:?}: {why}"
        );
    }
}

#[test]
fn a_timestamp_past_the_reserved_span_is_refused() {
    // Any one DER value serves: an OCTET STRING the size of the whole span.
    let signature = made();
    let mut huge = vec![0x04, 0x82, 0x80, 0x00];
    huge.extend(vec![0u8; 0x8000]);
    let why = signature.stamped(&huge).expect_err("refused");
    assert!(why.contains("set aside"), "{why}");
    // And the span holds what a real authority's answer needs, measured at
    // 6 to 8 KiB with its certificates: 16 KiB of anything still fits.
    let mut fits = vec![0x04, 0x82, 0x40, 0x00];
    fits.extend(vec![0u8; 0x4000]);
    signature.stamped(&fits).expect("fits");
}

// ------------------------------------------------------ document timestamps

#[test]
fn a_document_timestamp_is_sealed_only_when_it_reads_back_intact() {
    use crate::integrity::test_tsa::{mint, Imprint, TestTsa};
    let original = testkeys::plain_pdf();
    let tsa = TestTsa::new();
    let unsigned =
        crate::sign_prepare::prepare_document_timestamp(original.clone(), None).expect("prepared");
    // The control: a token over the range is written, and reads as a document
    // timestamp whose verdict is intact.
    let token = mint(Imprint::Sha256, &unsigned.digest, None, NOW, &tsa);
    let bytes =
        seal_document_timestamp(original.clone(), unsigned.clone(), &token).expect("sealed");
    let found = crate::docinfo::scan(&bytes, 1, None).expect("scanned");
    let stamp = found
        .signatures
        .iter()
        .find(|s| s.signed)
        .expect("the field");
    assert_eq!(stamp.kind, "ETSI.RFC3161");
    assert_eq!(
        stamp.integrity.as_ref().map(|i| i.verdict),
        Some(Verdict::Intact)
    );
    // A token over anything else is refused, and so is one against other bytes.
    let other = mint(
        Imprint::Sha256,
        &Imprint::Sha256.digest(b"else"),
        None,
        NOW,
        &tsa,
    );
    assert!(seal_document_timestamp(original.clone(), unsigned.clone(), &other).is_err());
    let mut longer = original;
    longer.push(b'\n');
    let why = seal_document_timestamp(longer, unsigned, &token).expect_err("refused");
    assert!(why.contains("built against"), "{why}");
}

// ------------------------------- the certificate a signature says it was made with

/// `original` signed with the CMS `blob_of` makes for the range's digest, and
/// **not** held to tpdf's own check before it is returned --- which is what
/// lets a test write a signature the writer would refuse, and ask the reader.
fn written_with(original: &[u8], blob_of: impl FnOnce(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let unsigned = crate::sign_prepare::prepare(original.to_vec(), NOW, None).expect("prepared");
    let digest = check(original, &unsigned).expect("the worker's numbers");
    let blob = blob_of(&digest);
    let Unsigned {
        mut update,
        built_against,
        range,
        ..
    } = unsigned;
    splice(&mut update, built_against, range, &blob).expect("spliced");
    [original, &update[..]].concat()
}

/// Whether a CMS signer's signed attributes carry `signingCertificateV2`.
fn states_its_certificate(blob: &[u8]) -> bool {
    decoded(blob).signer_infos.0.as_slice()[0]
        .signed_attrs
        .as_ref()
        .is_some_and(|attributes| {
            attributes
                .iter()
                .any(|attribute| attribute.oid == SIGNING_CERTIFICATE_V2)
        })
}

/// A second certificate over the same key, with the first one's issuer and
/// serial --- so a signature's `sid` names it as well as the first --- and
/// another subject: what somebody holding one key presents a signature under
/// when they would rather it were somebody else's.
fn twin_of(key: &Soft, issuer: &str, subject: &str) -> Vec<u8> {
    certificate(
        key,
        &Spec {
            issuer: Some((key, issuer)),
            ..Spec::new(subject)
        },
    )
}

/// The control: tpdf's own signature states the certificate it was made with,
/// and reads as intact.
#[test]
fn a_signature_naming_the_certificate_it_was_made_with_is_intact() {
    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = own(&key, "Signer");
    let mut stated = false;
    let bytes = written_with(&original, |digest| {
        let blob = build(digest, &cert, &[], &key).expect("built");
        stated = states_its_certificate(&blob);
        blob
    });
    assert!(stated, "the writer adds signingCertificateV2");
    let found = verdicts(&bytes);
    assert_eq!(found[0].1.verdict, Verdict::Intact, "{found:?}");
}

/// The signature tpdf made, with its certificate swapped for another over the
/// same key that the signer's identifier names just as well. The key's
/// arithmetic still holds and the digest still matches, so nothing but the
/// stated certificate says the signature was made under a different one ---
/// and that is what the attribute is for. Not intact: not checked, with the
/// reason.
#[test]
fn a_signature_naming_another_certificate_than_the_one_it_carries_is_not_intact() {
    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = own(&key, "Signer");
    let other = twin_of(&key, "Signer", "Somebody Else");
    let bytes = written_with(&original, |digest| {
        let blob = build(digest, &cert, &[], &key).expect("built");
        let (content_type, mut signed) = super::decoded(&blob).expect("a CMS");
        signed.certificates = Some(
            cms::signed_data::CertificateSet::try_from(vec![CertificateChoices::Certificate(
                Certificate::from_der(&other).expect("the twin"),
            )])
            .expect("a set"),
        );
        cms::content_info::ContentInfo {
            content_type,
            content: der::Any::encode_from(&signed).expect("encoded"),
        }
        .to_der()
        .expect("a blob")
    });
    let found = verdicts(&bytes);
    assert_eq!(
        (found[0].1.verdict, found[0].1.why),
        (Verdict::Unchecked, Some(crate::integrity::Why::Binding)),
        "{found:?}"
    );
    // The fixture is the substitution and nothing else: the reader names the
    // twin as the signer, and the twin is a certificate a signature made
    // under it reads intact with.
    let read = crate::docinfo::scan(&bytes, 1, None).expect("scanned");
    let named = read.signatures[0].certificate.as_ref().expect("a signer");
    assert_eq!(named.subject_cn, "Somebody Else");
    assert!(named.matched_signer);
    let under_the_twin = written_with(&original, |digest| {
        build(digest, &other, &[], &key).expect("built")
    });
    assert_eq!(verdicts(&under_the_twin)[0].1.verdict, Verdict::Intact);
}

/// A signature that states no certificate reads as it always did: most
/// signatures made before PAdES carry no such attribute, and it is not this
/// reader's to require one of them.
#[test]
fn a_signature_that_does_not_name_its_certificate_reads_as_before() {
    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = own(&key, "Signer");
    let mut stated = true;
    let bytes = written_with(&original, |digest| {
        // `build`, less the one attribute.
        let signer = Certificate::from_der(&cert).expect("the certificate");
        let kind = key_kind(&signer).expect("a key this signs with");
        let encapsulated = EncapsulatedContentInfo {
            econtent_type: ID_DATA,
            econtent: None,
        };
        let sha256 = AlgorithmIdentifierOwned {
            oid: ID_SHA256,
            parameters: None,
        };
        let sid = SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
            issuer: signer.tbs_certificate.issuer.clone(),
            serial_number: signer.tbs_certificate.serial_number.clone(),
        });
        let signing = Signing {
            key: &key,
            kind,
            refused: RefCell::new(None),
        };
        let info =
            SignerInfoBuilder::new(&signing, sid, sha256.clone(), &encapsulated, Some(digest))
                .expect("a signer");
        let mut data = SignedDataBuilder::new(&encapsulated);
        data.add_digest_algorithm(sha256).expect("a digest");
        data.add_certificate(CertificateChoices::Certificate(signer.clone()))
            .expect("a certificate");
        data.add_signer_info::<Signing<'_>, Value>(info)
            .expect("signed");
        let blob = data.build().expect("built").to_der().expect("a blob");
        stated = states_its_certificate(&blob);
        blob
    });
    assert!(!stated, "this one carries no signingCertificateV2");
    let found = verdicts(&bytes);
    assert_eq!(found[0].1.verdict, Verdict::Intact, "{found:?}");
}

/// A worker's numbers are a worker's: a range that does not frame its own
/// value in the revision is refused before anything is indexed by it, as the
/// signing path refuses one (`check`), rather than taking this process down.
#[test]
fn a_document_timestamp_whose_range_does_not_frame_its_value_is_refused() {
    use crate::integrity::test_tsa::{mint, Imprint, TestTsa};
    let original = testkeys::plain_pdf();
    let unsigned =
        crate::sign_prepare::prepare_document_timestamp(original.clone(), None).expect("prepared");
    let token = mint(
        Imprint::Sha256,
        &unsigned.digest,
        None,
        NOW,
        &TestTsa::new(),
    );
    let [_, first, second, last] = unsigned.range;
    let was = original.len() as u64;
    for (what, range) in [
        // The first is the one that took the process down: the offset into
        // the revision is the hole's start less the document's length.
        ("a hole at the very start of the file", [0, 0, second, last]),
        (
            "a hole that begins before the revision",
            [0, was - 1, second, last],
        ),
        (
            "a hole that begins past the end",
            [0, u64::MAX, second, last],
        ),
        ("a hole of another size", [0, first, second + 2, last - 2]),
        (
            "a range that stops short of the end",
            [0, first, second, last - 1],
        ),
        (
            "a range that does not begin the file",
            [1, first, second, last],
        ),
    ] {
        let mut wrong = unsigned.clone();
        wrong.range = range;
        let why = seal_document_timestamp(original.clone(), wrong, &token).expect_err(what);
        assert!(
            why.contains("covers a range that does not frame its own value"),
            "{what}: {why}"
        );
    }
    // The control: the worker's own numbers, with the same token.
    seal_document_timestamp(original, unsigned, &token).expect("sealed");
}
