use super::*;

#[test]
fn an_identity_is_named_by_the_sha256_of_its_certificate() {
    // What the chooser sends back and `find` matches on. Pinned against a
    // known digest so a change of hash or of case is visible.
    assert_eq!(
        id_of(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// The macOS signing primitive, with a key that lives in no keychain.
///
/// **Not the temporary-keychain test the plan asked for, and why.** Creating
/// a keychain file --- `security create-keychain` or `SecKeychainCreate` ---
/// adds it to the user's keychain search list (the Security framework's
/// `StorageManager::created` does so unconditionally), which alters user-level
/// keychain state; the plan's rule was to stop there rather than do that and
/// undo it. What this covers instead needs no keychain at all:
/// `SecKeyCreateWithData` makes a key object in memory from the test's own
/// software key, and `platform::sign_with` --- the function the shipped path
/// calls after `SecIdentityCopyPrivateKey` --- signs with it through
/// `SecKeyCreateSignature` and the same two algorithm constants. The one step
/// left uncovered is the identity search itself (`SecItemCopyMatching`), which
/// only a keychain holding an identity can answer.
///
/// `SecKeyCreateWithData` rather than generating a key: an in-memory
/// `SecKeyCreateRandomKey` took 25 seconds on its first call here, measured,
/// which is not a cost to put in every gate run.
#[cfg(target_os = "macos")]
#[test]
fn the_os_signs_a_revision_that_the_verifier_calls_intact() {
    use crate::integrity::Verdict;
    use crate::sign_cms::testkeys::{self, certificate, signed, verdicts, Soft, Spec};
    use core_foundation::base::TCFType as _;
    use core_foundation::data::CFData;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use security_framework::key::SecKey;
    use security_framework_sys::item::{
        kSecAttrKeyClass, kSecAttrKeyClassPrivate, kSecAttrKeyType,
        kSecAttrKeyTypeECSECPrimeRandom, kSecAttrKeyTypeRSA,
    };

    /// The OS's own key object over the bytes of `soft`'s private key.
    fn in_memory(soft: &Soft) -> SecKey {
        let (data, kind) = match soft {
            Soft::Rsa(key) => {
                use rsa::pkcs1::EncodeRsaPrivateKey as _;
                let der = key.to_pkcs1_der().expect("PKCS#1");
                (der.as_bytes().to_vec(), unsafe { kSecAttrKeyTypeRSA })
            }
            Soft::P256(key) => {
                // X9.63: the uncompressed public point, then the scalar.
                let mut data = key
                    .verifying_key()
                    .to_encoded_point(false)
                    .as_bytes()
                    .to_vec();
                data.extend_from_slice(&key.to_bytes());
                (data, unsafe { kSecAttrKeyTypeECSECPrimeRandom })
            }
            Soft::P384(_) => unreachable!("not used here"),
        };
        let attributes = CFDictionary::from_CFType_pairs(&[
            (
                unsafe { CFString::wrap_under_get_rule(kSecAttrKeyType) },
                unsafe { CFString::wrap_under_get_rule(kind) },
            ),
            (
                unsafe { CFString::wrap_under_get_rule(kSecAttrKeyClass) },
                unsafe { CFString::wrap_under_get_rule(kSecAttrKeyClassPrivate) },
            ),
        ]);
        let data = CFData::from_buffer(&data);
        let mut error = std::ptr::null_mut();
        // Both arguments live across the call; a null result is an error the
        // assertion reports.
        let key = unsafe {
            security_framework_sys::key::SecKeyCreateWithData(
                data.as_concrete_TypeRef(),
                attributes.as_concrete_TypeRef(),
                &mut error,
            )
        };
        assert!(!key.is_null(), "SecKeyCreateWithData refused the key");
        unsafe { SecKey::wrap_under_create_rule(key) }
    }

    struct Os(SecKey);
    impl Key for Os {
        fn sign_digest(&self, kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
            platform::sign_with(&self.0, kind, digest)
        }
    }

    for soft in [Soft::rsa(), Soft::p256(51)] {
        let kind = soft.kind();
        let cert = certificate(&soft, &Spec::new("OS signer"));
        let os = Os(in_memory(&soft));

        let bytes = signed(&testkeys::plain_pdf(), &cert, &[], &os).expect("signed by the OS");
        let found = verdicts(&bytes);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].1.verdict,
            Verdict::Intact,
            "{kind:?}: {:?}",
            found[0].1
        );
    }
}

/// The chain API on a self-issued certificate: nothing above it, and with the
/// network disallowed the call returns rather than hanging.
///
/// Ignored in the gates for its cost, which is the finding: **the first
/// `SecTrust` evaluation in a process took 5.4 s and 19.2 s** in two runs here
/// (the second, in the same process, 0.5 ms). The chooser pays that once, the
/// first time it lists identities. Run it with
/// `cargo test --manifest-path src-tauri/Cargo.toml --lib keystore::tests::the_chain_api -- --ignored`.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "the first SecTrust evaluation in a process costs seconds"]
fn the_chain_api_returns_nothing_above_a_self_issued_certificate() {
    use crate::sign_cms::testkeys::{certificate, Soft, Spec};
    let cert = certificate(&Soft::p256(52), &Spec::new("Alone"));
    let sec = security_framework::certificate::SecCertificate::from_der(&cert).expect("cert");
    assert!(platform::chain_of(&sec).is_empty());
}

/// The Windows store, end to end: a self-signed certificate created in
/// `CurrentUser\My` with a CNG key, found by `identities`, used to sign, and
/// removed with its key afterwards --- including when an assertion fails.
///
/// **Not run on macOS; first run on Windows 2026-09-27** (see the cleanup). Run it on an unlocked
/// Windows desktop with
/// `cargo test --manifest-path src-tauri/Cargo.toml --lib keystore::tests::the_windows_store -- --ignored --nocapture`.
#[cfg(windows)]
#[test]
#[ignore = "creates and removes a certificate in CurrentUser\\My; run explicitly on Windows"]
fn the_windows_store_signs_a_revision_that_the_verifier_calls_intact() {
    use crate::integrity::Verdict;
    use crate::sign_cms::testkeys::{self, signed, verdicts};

    let subject = format!(
        "tpdf-keystore-probe-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    let powershell = |script: &str| -> String {
        let out = std::process::Command::new("pwsh")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .expect("pwsh runs");
        assert!(
            out.status.success(),
            "{script}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let mut made = Vec::new();
    for algorithm in ["RSA", "ECDSA_nistP256"] {
        let thumbprint = powershell(&format!(
            "(New-SelfSignedCertificate -Subject 'CN={subject}-{algorithm}' \
             -CertStoreLocation Cert:\\CurrentUser\\My -KeyAlgorithm {algorithm} \
             -KeyUsage DigitalSignature -Provider 'Microsoft Software Key Storage Provider' \
             -NotAfter (Get-Date).AddDays(1)).Thumbprint"
        ));
        made.push(thumbprint);
    }
    let result = std::panic::catch_unwind(|| {
        let found = identities().expect("the store");
        for thumbprint in &made {
            use sha1::Digest as _;
            let identity = found
                .iter()
                .find(|i| {
                    sha1::Sha1::digest(&i.certificate)
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<String>()
                        == *thumbprint
                })
                .expect("the probe certificate is listed");
            let bytes = signed(
                &testkeys::plain_pdf(),
                &identity.certificate,
                &identity.chain,
                identity,
            )
            .expect("signed by Windows");
            assert_eq!(
                verdicts(&bytes)[0].1.verdict,
                Verdict::Intact,
                "{thumbprint}"
            );
        }
    });
    // **The path before `-DeleteKey`**: the switch is a dynamic parameter of the
    // certificate provider, and PowerShell 7 does not know it exists until the
    // path is bound. Written the other way round, the first run on Windows
    // (2026-09-27) failed with "A parameter cannot be found that matches
    // parameter name 'DeleteKey'" and left both probe certificates, keys
    // included, in the store. And every removal is attempted before any is
    // reported, because a panic on the first stranded the second.
    let mut stuck = Vec::new();
    for thumbprint in &made {
        let out = std::process::Command::new("pwsh")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!("Remove-Item -Path Cert:\\CurrentUser\\My\\{thumbprint} -DeleteKey"),
            ])
            .output();
        match out {
            Ok(out) if out.status.success() => {}
            Ok(out) => stuck.push(format!(
                "{thumbprint}: {}",
                String::from_utf8_lossy(&out.stderr)
            )),
            Err(e) => stuck.push(format!("{thumbprint}: {e}")),
        }
    }
    assert!(
        stuck.is_empty(),
        "probe certificates left in CurrentUser\\My: {stuck:?}"
    );
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
