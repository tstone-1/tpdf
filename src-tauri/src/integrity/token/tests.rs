//! The token verdict, one property of a minted token at a time.
//!
//! Every token here comes from [`crate::integrity::test_tsa`], so each test
//! changes exactly one thing about an otherwise sound token and asserts the
//! verdict **and** the reason --- a refusal asserted by its verdict alone
//! would pass when a different guard refused it (`docs/TRAPS.md`, *A control
//! refused by a different guard than the one it was written for*).

use super::*;
use crate::integrity::test_tsa::{mint, mint_with, Binding, Faults, Imprint, TestTsa};
use crate::integrity::MAX_HASHED;
use der::Encode as _;

/// A signature value the tokens are attached to. Any bytes will do: the
/// imprint is over the value's octets, whatever they are.
const VALUE: &[u8] = b"the value octets of a SignerInfo.signature, standing in";

/// 2026-08-21 12:00:00 UTC, the instant the fixture generator pins too.
const AT: u64 = 1_787_313_600;

fn tsa() -> TestTsa {
    TestTsa::new()
}

fn token(faults: &Faults) -> Vec<u8> {
    mint_with(
        Imprint::Sha256,
        &Imprint::Sha256.digest(VALUE),
        Some(&[0x0a, 0x0b]),
        AT,
        &tsa(),
        faults,
    )
}

fn verdict_of(token: &[u8]) -> Integrity {
    check(token, Target::Signature(VALUE), &mut MAX_HASHED.clone())
}

fn is(found: &Integrity, verdict: Verdict, why: Option<Why>) {
    assert_eq!((found.verdict, found.why), (verdict, why), "{found:?}");
}

#[test]
fn a_sound_token_over_the_signature_is_intact() {
    // The control. A checker that refused everything would pass every test
    // below; this is what fails it.
    let found = verdict_of(&token(&Faults::default()));
    is(&found, Verdict::Intact, None);
    assert_eq!(found.digest, "SHA-256");
    assert_eq!(found.method, "ECDSA P-256");
}

#[test]
fn every_imprint_hash_tpdf_carries_is_checked_by_its_own_digest() {
    for hash in [Imprint::Sha256, Imprint::Sha384, Imprint::Sha512] {
        let minted = mint(hash, &hash.digest(VALUE), None, AT, &tsa());
        is(&verdict_of(&minted), Verdict::Intact, None);
        // And the same token against a value it is not of: without this, a
        // digest that ignored its input would pass the line above.
        let other = check(
            &minted,
            Target::Signature(b"another"),
            &mut MAX_HASHED.clone(),
        );
        is(&other, Verdict::Altered, None);
    }
}

#[test]
fn a_token_of_different_data_is_altered() {
    // Sound in itself, and an imprint of something else: a genuine timestamp
    // of different data.
    let found = verdict_of(&token(&Faults {
        wrong_imprint: true,
        ..Faults::default()
    }));
    is(&found, Verdict::Altered, None);
}

#[test]
fn a_corrupted_token_signature_is_broken() {
    let found = verdict_of(&token(&Faults {
        corrupt_signature: true,
        ..Faults::default()
    }));
    is(&found, Verdict::Broken, None);
}

#[test]
fn a_tst_info_changed_after_signing_is_broken_not_altered() {
    // The signature over the signed attributes holds; the `TSTInfo` in hand is
    // not the one whose digest they carry. For a signature that would be
    // "the document changed"; for a token the content *is* the statement.
    let found = verdict_of(&token(&Faults {
        altered_content: true,
        ..Faults::default()
    }));
    is(&found, Verdict::Broken, None);
}

#[test]
fn a_broken_token_is_broken_even_when_its_imprint_is_also_wrong() {
    // The order: nothing the token states is read until its signature holds.
    let found = verdict_of(&token(&Faults {
        corrupt_signature: true,
        wrong_imprint: true,
        ..Faults::default()
    }));
    is(&found, Verdict::Broken, None);
}

#[test]
fn an_sha1_imprint_is_weak_and_never_intact() {
    let minted = mint(
        Imprint::Sha1,
        &Imprint::Sha1.digest(VALUE),
        None,
        AT,
        &tsa(),
    );
    let found = verdict_of(&minted);
    is(&found, Verdict::Weak, None);
    assert_eq!(found.digest, "SHA-1");
}

#[test]
fn an_sha1_token_signature_is_weak_and_names_sha1() {
    // The imprint is SHA-256 here, so only the token's own signature can
    // make this weak --- and the digest named is the weaker of the two.
    let found = verdict_of(&token(&Faults {
        sha1_signature: true,
        ..Faults::default()
    }));
    is(&found, Verdict::Weak, None);
    assert_eq!(found.digest, "SHA-1");
}

#[test]
fn an_sha1_mismatch_is_still_altered() {
    // As for a signature: SHA-1 weakens only the good answer.
    let found = verdict_of(&mint_with(
        Imprint::Sha1,
        &Imprint::Sha1.digest(VALUE),
        None,
        AT,
        &tsa(),
        &Faults {
            wrong_imprint: true,
            ..Faults::default()
        },
    ));
    is(&found, Verdict::Altered, None);
}

#[test]
fn a_token_with_no_ess_binding_is_not_checked() {
    let found = verdict_of(&token(&Faults {
        binding: Binding::Neither,
        ..Faults::default()
    }));
    is(&found, Verdict::Unchecked, Some(Why::Binding));
}

#[test]
fn a_binding_naming_another_certificate_is_not_checked() {
    let found = verdict_of(&token(&Faults {
        binding: Binding::Other,
        ..Faults::default()
    }));
    is(&found, Verdict::Unchecked, Some(Why::Binding));
}

#[test]
fn each_form_of_the_binding_is_read() {
    // The controls for the two refusals above: v1 alone is what pyHanko's
    // dummy authority writes, and both at once must each be checked.
    for binding in [Binding::V1, Binding::V2, Binding::Both] {
        let found = verdict_of(&token(&Faults {
            binding,
            ..Faults::default()
        }));
        is(&found, Verdict::Intact, None);
    }
}

#[test]
fn an_imprint_of_something_else_is_altered_even_with_no_binding() {
    // "This token is of different data" needs only the key, so a missing
    // binding does not hide it.
    let found = verdict_of(&token(&Faults {
        binding: Binding::Neither,
        wrong_imprint: true,
        ..Faults::default()
    }));
    is(&found, Verdict::Altered, None);
}

#[test]
fn a_cms_that_is_not_a_token_is_not_read_as_one() {
    use cms::content_info::ContentInfo;
    use cms::signed_data::SignedData;

    let minted = token(&Faults::default());
    let info = ContentInfo::from_der(&minted).expect("a token");
    let mut signed: SignedData = info.content.decode_as().expect("signed data");
    // id-data: the same bytes, no longer claiming to be a `TSTInfo`.
    signed.encap_content_info.econtent_type =
        der::asn1::ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
    let relabelled = ContentInfo {
        content_type: info.content_type,
        content: der::Any::encode_from(&signed).expect("re-encoded"),
    }
    .to_der()
    .expect("re-encoded");
    is(
        &verdict_of(&relabelled),
        Verdict::Unchecked,
        Some(Why::Unreadable),
    );
    is(
        &verdict_of(b"not DER at all"),
        Verdict::Unchecked,
        Some(Why::Unreadable),
    );
}

#[test]
fn a_document_timestamp_imprint_is_over_the_covered_pieces() {
    let (head, tail) = (
        &b"%PDF-1.7 the bytes before"[..],
        &b"and after the hole"[..],
    );
    let whole = [head, tail].concat();
    let minted = mint(
        Imprint::Sha256,
        &Imprint::Sha256.digest(&whole),
        None,
        AT,
        &tsa(),
    );
    let found = check(
        &minted,
        Target::Range(&[head, tail]),
        &mut MAX_HASHED.clone(),
    );
    is(&found, Verdict::Intact, None);
    // One byte of the covered range changed.
    let changed = [&b"%PDF-1.7 the bytes befoRe"[..], tail];
    let found = check(&minted, Target::Range(&changed), &mut MAX_HASHED.clone());
    is(&found, Verdict::Altered, None);
}

#[test]
fn a_budget_smaller_than_the_target_refuses_before_hashing() {
    let minted = token(&Faults::default());
    // What the token's own `TSTInfo` costs, measured rather than assumed.
    let mut all = MAX_HASHED;
    is(
        &check(&minted, Target::Signature(VALUE), &mut all),
        Verdict::Intact,
        None,
    );
    let spent = MAX_HASHED - all;
    assert!(
        spent > VALUE.len() as u64,
        "the TSTInfo and the value are both charged"
    );

    let mut short = spent - 1;
    let found = check(&minted, Target::Signature(VALUE), &mut short);
    is(&found, Verdict::Unchecked, Some(Why::Budget));
    let mut exact = spent;
    is(
        &check(&minted, Target::Signature(VALUE), &mut exact),
        Verdict::Intact,
        None,
    );
    assert_eq!(exact, 0, "what was hashed is charged");
}

/// Writes the minted tokens a person checks with `openssl ts -verify`, to
/// `$TPDF_TOKEN_DIR`: each token, the data it is over, and the root.
///
/// Ignored, because it is an instrument rather than a check: `BUILD.md`'s
/// *Timestamp tokens against OpenSSL* runs it and then OpenSSL over its output.
#[test]
#[ignore = "writes files for a manual OpenSSL run; BUILD.md says how"]
fn write_tokens_for_openssl() {
    let Ok(dir) = std::env::var("TPDF_TOKEN_DIR") else {
        panic!("set TPDF_TOKEN_DIR to the directory to write into");
    };
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).expect("the directory");
    let authority = tsa();
    std::fs::write(dir.join("root.der"), &authority.root).expect("root");
    std::fs::write(dir.join("data.bin"), VALUE).expect("data");
    let cases: [(&str, Imprint, Faults); 8] = [
        ("intact", Imprint::Sha256, Faults::default()),
        ("sha1-imprint", Imprint::Sha1, Faults::default()),
        (
            "sha1-signature",
            Imprint::Sha256,
            Faults {
                sha1_signature: true,
                ..Faults::default()
            },
        ),
        (
            "wrong-imprint",
            Imprint::Sha256,
            Faults {
                wrong_imprint: true,
                ..Faults::default()
            },
        ),
        (
            "corrupt-signature",
            Imprint::Sha256,
            Faults {
                corrupt_signature: true,
                ..Faults::default()
            },
        ),
        (
            "altered-content",
            Imprint::Sha256,
            Faults {
                altered_content: true,
                ..Faults::default()
            },
        ),
        (
            "no-binding",
            Imprint::Sha256,
            Faults {
                binding: Binding::Neither,
                ..Faults::default()
            },
        ),
        (
            "other-binding",
            Imprint::Sha256,
            Faults {
                binding: Binding::Other,
                ..Faults::default()
            },
        ),
    ];
    for (name, hash, faults) in cases {
        let minted = mint_with(hash, &hash.digest(VALUE), None, AT, &authority, &faults);
        let found = verdict_of(&minted);
        std::fs::write(dir.join(format!("{name}.tst")), &minted).expect("token");
        println!("{name}: {:?} {:?}", found.verdict, found.why);
    }
    let no_purpose = TestTsa::with_purposes(None);
    let minted = mint(
        Imprint::Sha256,
        &Imprint::Sha256.digest(VALUE),
        None,
        AT,
        &no_purpose,
    );
    std::fs::write(dir.join("no-purpose.tst"), &minted).expect("token");
    std::fs::write(dir.join("no-purpose-root.der"), &no_purpose.root).expect("root");
}
