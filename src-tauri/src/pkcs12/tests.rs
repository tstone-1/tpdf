//! `pkcs12.rs` against files the system's `openssl` writes: modern (PBES2 with
//! AES, a SHA-256 MAC) and legacy (RC2 certificates, a triple-DES key, a SHA-1
//! MAC), P-256 and P-384, a chain, RSA, a wrong password, no key. Each case is
//! a `[SKIP]` where `openssl` cannot write it.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

/// A folder of this test's own, under the temporary folder.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tpdf-p12-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs `openssl` in `dir`; `false` when it is absent or refuses.
fn openssl(dir: &Path, args: &[&str]) -> bool {
    Command::new("openssl")
        .args(args)
        .current_dir(dir)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// A self-signed certificate and key in `dir`, as `cert.pem` and `key.pem`.
fn self_signed(dir: &Path, key_options: &[&str]) -> bool {
    let mut args = vec![
        "req",
        "-x509",
        "-nodes",
        "-days",
        "2",
        "-subj",
        "/CN=tpdf p12 test",
    ];
    args.extend_from_slice(key_options);
    args.extend_from_slice(&["-keyout", "key.pem", "-out", "cert.pem"]);
    openssl(dir, &args)
}

/// Exports `key.pem` and `cert.pem` in `dir` to `out` with `extra` options.
fn export(dir: &Path, out: &str, extra: &[&str]) -> Option<Vec<u8>> {
    let mut args = vec![
        "pkcs12",
        "-export",
        "-inkey",
        "key.pem",
        "-in",
        "cert.pem",
        "-passout",
        "pass:geheim",
        "-out",
        out,
    ];
    args.extend_from_slice(extra);
    openssl(dir, &args).then(|| std::fs::read(dir.join(out)).unwrap())
}

/// Whether `opened` signs a digest its own certificate's key verifies.
fn signs(opened: &Opened) -> bool {
    use ecdsa::signature::hazmat::PrehashVerifier as _;
    let digest = [0x42; 32];
    let signature = opened.sign(&digest).unwrap();
    let public = public_key_of(&opened.certificate).unwrap();
    match opened.kind() {
        KeyKind::P256 => {
            let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(&public).unwrap();
            let sig = p256::ecdsa::Signature::from_der(&signature).unwrap();
            key.verify_prehash(&digest, &sig).is_ok()
        }
        KeyKind::P384 => {
            let key = p384::ecdsa::VerifyingKey::from_sec1_bytes(&public).unwrap();
            let sig = p384::ecdsa::Signature::from_der(&signature).unwrap();
            key.verify_prehash(&digest, &sig).is_ok()
        }
        KeyKind::Rsa(_) => false,
    }
}

#[test]
fn ecdsa_files_open_and_sign_and_the_rest_are_refused_by_name() {
    let dir = scratch("cases");
    if !self_signed(
        &dir,
        &["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256"],
    ) {
        println!("[SKIP] openssl cannot write a P-256 certificate here");
        return;
    }

    // Modern: OpenSSL 3's defaults.
    let modern = export(&dir, "modern.p12", &[]).expect("a default export");
    let opened = open(&modern, "geheim").expect("modern P-256 opens");
    assert_eq!(opened.kind(), KeyKind::P256);
    assert!(
        opened.chain.is_empty(),
        "a file of one certificate has no chain"
    );
    assert!(
        signs(&opened),
        "the modern file's key signs for its certificate"
    );
    assert_eq!(open(&modern, "falsch").err(), Some(Refusal::WrongPassword));

    // The right password and a damaged MAC: only the MAC check can refuse
    // this, since everything it covers still decrypts.
    let mut pfx = pkcs12::pfx::Pfx::from_der(&modern).unwrap();
    let mac = pfx.mac_data.as_mut().expect("OpenSSL writes a MAC");
    let mut digest = mac.mac.digest.as_bytes().to_vec();
    digest[0] ^= 0x01;
    mac.mac.digest = der::asn1::OctetString::new(digest).unwrap();
    let tampered = pfx.to_der().unwrap();
    assert_eq!(
        open(&tampered, "geheim").err(),
        Some(Refusal::WrongPassword)
    );

    // Legacy: RC2-40 certificates, a triple-DES key, a SHA-1 MAC.
    match export(&dir, "legacy.p12", &["-legacy"]) {
        Some(legacy) => {
            let opened = open(&legacy, "geheim").expect("legacy P-256 opens");
            assert!(signs(&opened), "the legacy file's key signs");
            assert_eq!(open(&legacy, "falsch").err(), Some(Refusal::WrongPassword));
        }
        None => println!("[SKIP] openssl has no legacy provider for RC2"),
    }

    // Certificates in the clear, the key under triple DES, a SHA-256 MAC.
    let mixed = export(
        &dir,
        "mixed.p12",
        &[
            "-certpbe",
            "NONE",
            "-keypbe",
            "PBE-SHA1-3DES",
            "-macalg",
            "sha256",
        ],
    )
    .expect("a mixed export");
    assert!(signs(&open(&mixed, "geheim").expect("mixed opens")));

    // A key file with no certificate in it.
    let keyless_args = [
        "pkcs12",
        "-export",
        "-nocerts",
        "-inkey",
        "key.pem",
        "-passout",
        "pass:geheim",
        "-out",
        "keyless.p12",
    ];
    assert!(openssl(&dir, &keyless_args), "a key-only export");
    let keyless = std::fs::read(dir.join("keyless.p12")).unwrap();
    assert_eq!(open(&keyless, "geheim").err(), Some(Refusal::NoKey));

    assert!(matches!(
        open(b"not a certificate file", "geheim"),
        Err(Refusal::Malformed(_))
    ));
}

#[test]
fn a_p384_key_signs_a_sha256_digest() {
    let dir = scratch("p384");
    if !self_signed(
        &dir,
        &["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-384"],
    ) {
        println!("[SKIP] openssl cannot write a P-384 certificate here");
        return;
    }
    let file = export(&dir, "p384.p12", &[]).expect("a P-384 export");
    let opened = open(&file, "geheim").expect("P-384 opens");
    assert_eq!(opened.kind(), KeyKind::P384);
    assert!(signs(&opened));
}

#[test]
fn an_rsa_key_is_refused_and_says_what_can_sign_instead() {
    let dir = scratch("rsa");
    if !self_signed(&dir, &["-newkey", "rsa:2048"]) {
        println!("[SKIP] openssl cannot write an RSA certificate here");
        return;
    }
    let file = export(&dir, "rsa.p12", &[]).expect("an RSA export");
    let refused = open(&file, "geheim").err();
    assert_eq!(refused, Some(Refusal::Rsa));
    assert!(Refusal::Rsa.sentence().contains("smart card"));
}

#[test]
fn the_signer_is_the_certificate_that_matches_the_key_and_the_rest_is_the_chain() {
    let dir = scratch("chain");
    let made = openssl(
        &dir,
        &[
            "req",
            "-x509",
            "-nodes",
            "-days",
            "2",
            "-subj",
            "/CN=tpdf p12 root",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-keyout",
            "ca.key",
            "-out",
            "ca.pem",
        ],
    ) && openssl(
        &dir,
        &[
            "req",
            "-new",
            "-nodes",
            "-subj",
            "/CN=tpdf p12 leaf",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-keyout",
            "key.pem",
            "-out",
            "leaf.csr",
        ],
    ) && openssl(
        &dir,
        &[
            "x509", "-req", "-in", "leaf.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-days", "2",
            "-out", "cert.pem",
        ],
    );
    if !made {
        println!("[SKIP] openssl cannot write a chain here");
        return;
    }
    // OpenSSL always writes the certificate that matches the key first, so a
    // reader that took the first one would pass on its files by accident.
    // Certificates in the clear and no MAC, so the bags can be put in the
    // other order here: the root first.
    let file = export(
        &dir,
        "chain.p12",
        &["-certfile", "ca.pem", "-certpbe", "NONE", "-nomac"],
    )
    .expect("an export with a chain");
    let file = root_first(&file);
    let opened = open(&file, "geheim").expect("opens");
    assert_eq!(opened.chain.len(), 1, "the root is the chain");
    let leaf = x509_cert::Certificate::from_der(&opened.certificate).unwrap();
    assert!(
        leaf.tbs_certificate.subject.to_string().contains("leaf"),
        "{}",
        leaf.tbs_certificate.subject
    );
    assert!(signs(&opened));
}

#[test]
fn an_added_file_is_listed_reopened_and_refused_once_replaced() {
    let dir = scratch("remembered");
    if !self_signed(
        &dir,
        &["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256"],
    ) {
        println!("[SKIP] openssl cannot write a P-256 certificate here");
        return;
    }
    export(&dir, "mine.p12", &[]).expect("an export");
    let file = dir.join("mine.p12");
    let store = dir.join("config").join("certificate-files.json");

    assert!(
        crate::certfiles::add(&store, &file, "falsch").is_err(),
        "a wrong password adds nothing"
    );
    assert!(crate::certfiles::read(&store).is_empty());

    let added = crate::certfiles::add(&store, &file, "geheim").expect("added");
    let listed = crate::certfiles::read(&store);
    assert_eq!(
        listed,
        std::slice::from_ref(&added),
        "the list holds the one file"
    );
    let again = crate::certfiles::add(&store, &file, "geheim").expect("added twice");
    assert_eq!(
        crate::certfiles::read(&store),
        [again],
        "adding again replaces, not duplicates"
    );
    let opened = crate::certfiles::reopen(&added, "geheim").expect("reopens");
    assert!(signs(&opened));
    let stored = std::fs::read_to_string(&store).unwrap();
    assert!(!stored.contains("geheim"), "the password is not written");

    // Another certificate and key under the same name.
    assert!(self_signed(
        &dir,
        &["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256"]
    ));
    export(&dir, "mine.p12", &[]).expect("a second export");
    let refused = crate::certfiles::reopen(&added, "geheim")
        .err()
        .unwrap_or_default();
    assert!(refused.contains("different certificate"), "{refused}");
}

/// `file` with the certificate bags of its unencrypted contents reversed.
fn root_first(file: &[u8]) -> Vec<u8> {
    use der::asn1::OctetString;
    use der::Any;
    let mut pfx = pkcs12::pfx::Pfx::from_der(file).unwrap();
    let mut safes = Vec::<ContentInfo>::from_der(&data_of(&pfx.auth_safe).unwrap()).unwrap();
    let mut reversed = 0;
    for safe in &mut safes {
        if safe.content_type.to_string() != ID_DATA {
            continue;
        }
        let mut bags = pkcs12::safe_bag::SafeContents::from_der(&data_of(safe).unwrap()).unwrap();
        if bags
            .iter()
            .filter(|b| b.bag_id == pkcs12::PKCS_12_CERT_BAG_OID)
            .count()
            < 2
        {
            continue;
        }
        // `pkcs12` decodes a bag value with its [0] tag on and encodes it as
        // if it had none (`bag_value` in `pkcs12.rs`), so take the tag off.
        for bag in &mut bags {
            bag.bag_value = bag_value(bag).unwrap().to_vec();
        }
        bags.reverse();
        let inner = OctetString::new(bags.to_der().unwrap()).unwrap();
        safe.content = Any::encode_from(&inner).unwrap();
        reversed += 1;
    }
    assert_eq!(
        reversed, 1,
        "one unencrypted list of certificates to reverse"
    );
    let outer = OctetString::new(safes.to_der().unwrap()).unwrap();
    pfx.auth_safe.content = Any::encode_from(&outer).unwrap();
    pfx.to_der().unwrap()
}
