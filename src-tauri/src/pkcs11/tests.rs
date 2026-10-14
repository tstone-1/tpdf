//! `pkcs11.rs` against a SoftHSM token. A file of its own because it names a
//! private-key type, which `sign_cms::tests::no_private_key_type_is_named_outside_the_tests`
//! allows only in test files.
use std::sync::Mutex;

use cryptoki::object::KeyType;
use cryptoki::session::UserType;
use cryptoki::types::AuthPin;

use super::*;
use crate::integrity::test_tsa::TestCa;

/// Where Debian, Ubuntu and Fedora install SoftHSM's module.
const SOFTHSM: [&str; 3] = [
    "/usr/lib/x86_64-linux-gnu/softhsm/libsofthsm2.so",
    "/usr/lib/softhsm/libsofthsm2.so",
    "/usr/lib64/pkcs11/libsofthsm2.so",
];
/// The DER of P-256's object identifier, `CKA_EC_PARAMS` for that curve.
const P256_PARAMS: [u8; 10] = [0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
const USER_PIN: &str = "4711";

/// What the test's PIN asker answers next; `None` is the reader cancelling.
static ANSWER: Mutex<Option<&'static str>> = Mutex::new(None);
static ASKED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A certificate's subject name, DER.
fn subject_of(certificate: &[u8]) -> Vec<u8> {
    use der::{Decode as _, Encode as _};
    x509_cert::Certificate::from_der(certificate)
        .unwrap()
        .tbs_certificate
        .subject
        .to_der()
        .unwrap()
}

fn pin(text: &str) -> AuthPin {
    AuthPin::new(text.to_string().into())
}

/// A SoftHSM token in a folder of its own, with a signer's certificate
/// and key under one ID and a certificate with no key under another.
/// `None`, and a `[SKIP]`, where SoftHSM is not installed.
fn token() -> Option<(Pkcs11, p256::ecdsa::VerifyingKey, Session)> {
    let Some(module) = SOFTHSM
        .iter()
        .find(|path| std::path::Path::new(path).exists())
    else {
        println!("[SKIP] SoftHSM is not installed (the package softhsm2)");
        return None;
    };
    let dir = std::env::temp_dir().join(format!("tpdf-softhsm-{}", std::process::id()));
    let tokens = dir.join("tokens");
    std::fs::create_dir_all(&tokens).unwrap();
    let conf = dir.join("softhsm2.conf");
    std::fs::write(
        &conf,
        format!(
            "directories.tokendir = {}\nobjectstore.backend = file\n",
            tokens.display()
        ),
    )
    .unwrap();
    // Read by SoftHSM in `C_Initialize`, below; nothing else here reads it.
    std::env::set_var("SOFTHSM2_CONF", &conf);
    let context = open(module).unwrap();
    let slot = context.get_all_slots().unwrap()[0];
    context
        .init_token(slot, &pin("so-pin"), "tpdf test")
        .unwrap();
    let session = context.open_rw_session(slot).unwrap();
    session.login(UserType::So, Some(&pin("so-pin"))).unwrap();
    session.init_pin(&pin(USER_PIN)).unwrap();
    session.logout().unwrap();
    session.login(UserType::User, Some(&pin(USER_PIN))).unwrap();

    let ca = TestCa::new("tpdf token root", 0x31);
    let signer = ca.issue("tpdf token signer", 0x32, 7, None);
    let secret = p256::ecdsa::SigningKey::from_bytes(&[signer.seed; 32].into()).unwrap();
    let orphan = ca.issue("tpdf orphan", 0x33, 8, None);
    for (certificate, id) in [(&signer.certificate, b"k1"), (&orphan.certificate, b"k2")] {
        session
            .create_object(&[
                Attribute::Class(ObjectClass::CERTIFICATE),
                Attribute::CertificateType(CertificateType::X_509),
                Attribute::Value(certificate.clone()),
                // Mandatory for a certificate object; SoftHSM refuses one without.
                Attribute::Subject(subject_of(certificate)),
                Attribute::Id(id.to_vec()),
                Attribute::Token(true),
            ])
            .unwrap();
    }
    session
        .create_object(&[
            Attribute::Class(ObjectClass::PRIVATE_KEY),
            Attribute::KeyType(KeyType::EC),
            Attribute::EcParams(P256_PARAMS.to_vec()),
            Attribute::Value(secret.to_bytes().to_vec()),
            Attribute::Id(b"k1".to_vec()),
            Attribute::Token(true),
            Attribute::Private(true),
            Attribute::Sign(true),
        ])
        .unwrap();
    session.logout().unwrap();
    set_pin_asker(Box::new(|_| {
        ASKED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(ANSWER.lock().unwrap().map(pin))
    }));
    Some((context, *secret.verifying_key(), session))
}

/// The whole path on one token, in one test: `C_Initialize` runs once per
/// process, and PKCS#11 login state belongs to the token rather than the
/// session, so a second test logging in would change what this one sees.
/// Listing; signing with a wrong PIN, a cancelled prompt and the right one;
/// a certificate with no key; and RSA through [`sign_with`], whose
/// `DigestInfo` an ECDSA signature cannot show.
#[test]
fn a_token_lists_its_certificates_and_signs_after_its_pin() {
    let Some((context, verifying, setup)) = token() else {
        return;
    };
    let found = certificates(&context).unwrap();
    let ids: Vec<&[u8]> = found.iter().map(|f| f.id.as_slice()).collect();
    assert_eq!(
        ids,
        [b"k1".as_slice(), b"k2".as_slice()],
        "both certificates, by ID"
    );
    assert!(
        found.iter().all(|f| f.token == "tpdf test"),
        "named by the token's label"
    );
    let (signer, orphan) = (&found[0], &found[1]);
    let digest = [0x5a; 32];
    let asked = || ASKED.load(std::sync::atomic::Ordering::SeqCst);
    let answer = |with: Option<&'static str>| *ANSWER.lock().unwrap() = with;

    answer(Some("0000"));
    let before = asked();
    let wrong = sign(&context, signer, KeyKind::P256, &digest);
    assert!(
        matches!(&wrong, Err(why) if why.contains("the PIN for the token \"tpdf test\" is not correct")),
        "{wrong:?}"
    );
    assert_eq!(asked(), before + 1, "asked once");

    answer(None);
    let cancelled = sign(&context, signer, KeyKind::P256, &digest);
    assert_eq!(cancelled, Err("signing was cancelled".into()));

    answer(Some(USER_PIN));
    let signature = sign(&context, signer, KeyKind::P256, &digest).unwrap();
    let parsed = p256::ecdsa::Signature::from_der(&signature).expect("DER, as CMS wants");
    use p256::ecdsa::signature::hazmat::PrehashVerifier as _;
    verifying
        .verify_prehash(&digest, &parsed)
        .expect("the token's key signed this digest");

    // The set-up session is still open, so the token stays logged in:
    // the next signature asks for nothing.
    let before = asked();
    let again = sign(&context, signer, KeyKind::P256, &digest);
    assert!(again.is_ok(), "{again:?}");
    assert_eq!(asked(), before, "no PIN asked while logged in");

    let none = sign(&context, orphan, KeyKind::P256, &digest);
    assert!(
        matches!(&none, Err(why) if why.contains("no key")),
        "{none:?}"
    );

    // RSA, in the set-up session, which the signing above logged in.
    let (public, private) = setup
        .generate_key_pair(
            &Mechanism::RsaPkcsKeyPairGen,
            &[
                Attribute::ModulusBits(2048.into()),
                Attribute::PublicExponent(vec![1, 0, 1]),
                Attribute::Token(false),
            ],
            &[Attribute::Token(false), Attribute::Sign(true)],
        )
        .unwrap();
    let rsa_digest = [0xa5; 32];
    let signature = sign_with(&setup, private, KeyKind::Rsa(2048), &rsa_digest).unwrap();
    let attributes = setup
        .get_attributes(
            public,
            &[AttributeType::Modulus, AttributeType::PublicExponent],
        )
        .unwrap();
    let (mut n, mut e) = (Vec::new(), Vec::new());
    for attribute in attributes {
        match attribute {
            Attribute::Modulus(v) => n = v,
            Attribute::PublicExponent(v) => e = v,
            _ => {}
        }
    }
    let key = rsa::RsaPublicKey::new(
        rsa::BigUint::from_bytes_be(&n),
        rsa::BigUint::from_bytes_be(&e),
    )
    .unwrap();
    let verifying = rsa::pkcs1v15::VerifyingKey::<sha2_10::Sha256>::new(key);
    let parsed = rsa::pkcs1v15::Signature::try_from(signature.as_slice()).unwrap();
    verifying
        .verify_prehash(&rsa_digest, &parsed)
        .expect("PKCS#1 v1.5 over a SHA-256 DigestInfo");
}

#[test]
fn a_refused_login_is_said_in_words_a_reader_can_act_on() {
    use cryptoki::context::Function;
    use cryptoki::error::{Error, RvError};
    let said = |rv| super::login_refused("card", &Error::Pkcs11(rv, Function::Login));
    let wrong = said(RvError::PinIncorrect);
    assert!(
        wrong.starts_with("the PIN for the token \"card\" is not correct"),
        "{wrong}"
    );
    assert!(!wrong.contains("More generally"), "{wrong}");
    assert!(said(RvError::PinLocked).contains("is locked after too many wrong PINs"));
    assert!(said(RvError::PinLenRange).contains("wrong number of characters"));
    // Anything else keeps the library's words, which name the failure.
    let other = said(RvError::DeviceRemoved);
    assert!(
        other.starts_with("the token \"card\" did not accept the PIN: "),
        "{other}"
    );
}
