use super::*;
use crate::sign_cms::testkeys::{certificate, Soft, Spec, NOW};

const DAY: u64 = 86_400;

/// A certificate authority: its key, its certificate, and its name.
struct Authority {
    key: Soft,
    der: Vec<u8>,
    name: &'static str,
}

fn root(seed: u8, name: &'static str) -> Authority {
    let key = Soft::p256(seed);
    let mut spec = Spec::new(name);
    spec.authority = Some(true);
    let der = certificate(&key, &spec);
    Authority { key, der, name }
}

fn issued_by(issuer: &Authority, seed: u8, name: &'static str, authority: bool) -> Authority {
    let key = Soft::p256(seed);
    let mut spec = Spec::new(name);
    spec.issuer = Some((&issuer.key, issuer.name));
    spec.authority = Some(authority);
    spec.serial = seed;
    let der = certificate(&key, &spec);
    Authority { key, der, name }
}

/// The standing of `leaf`, with `others` offered as issuers and `roots` the
/// only roots --- the test seam, so no test reads or writes the reader's store.
fn standing(leaf: &[u8], others: &[Vec<u8>], roots: &[Vec<u8>], now: u64) -> Trust {
    let parsed = Certificate::from_der(leaf).expect("a certificate");
    judge(&parsed, now, |at| {
        platform::evaluate(leaf, others, Anchors::Only(roots), at)
    })
}

fn trusted() -> Trust {
    Trust {
        standing: Standing::Trusted,
        why: None,
        store: platform::STORE,
        attested_at: String::new(),
    }
}

fn untrusted(why: Doubt) -> Trust {
    Trust {
        standing: Standing::Untrusted,
        why: Some(why),
        store: platform::STORE,
        attested_at: String::new(),
    }
}

// ------------------------------------------------------------- the platform
//
// Every test below this line asks the operating system. The first `SecTrust`
// evaluation in a process has been measured at seconds (`keystore/tests.rs`),
// so these share that cost rather than each paying it.

#[cfg(any(target_os = "macos", windows))]
mod platform_tests {
    use super::*;

    /// Read each Windows ROOT view without opening it for writes. At least one
    /// presently valid root in each view must pass the production system engine.
    /// No personal certificate names or certificate bytes enter the test log.
    #[cfg(windows)]
    #[test]
    fn a_windows_system_root_is_trusted() {
        for line in windows_system_root_outcomes() {
            println!("{line}");
        }
    }

    #[cfg(windows)]
    fn windows_system_root_outcomes() -> Vec<String> {
        use sha2::Digest;
        let mut outcomes = Vec::new();
        use windows_sys::Win32::Security::Cryptography::{
            CertCloseStore, CertEnumCertificatesInStore, CertOpenStore,
            CERT_STORE_OPEN_EXISTING_FLAG, CERT_STORE_PROV_SYSTEM_W, CERT_STORE_READONLY_FLAG,
            CERT_SYSTEM_STORE_CURRENT_USER, CERT_SYSTEM_STORE_LOCAL_MACHINE,
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        let name: Vec<u16> = "ROOT\0".encode_utf16().collect();
        for (label, location) in [
            ("current user", CERT_SYSTEM_STORE_CURRENT_USER),
            ("local machine", CERT_SYSTEM_STORE_LOCAL_MACHINE),
        ] {
            let store = unsafe {
                CertOpenStore(
                    CERT_STORE_PROV_SYSTEM_W,
                    0,
                    0,
                    location | CERT_STORE_READONLY_FLAG | CERT_STORE_OPEN_EXISTING_FLAG,
                    name.as_ptr().cast(),
                )
            };
            assert!(!store.is_null(), "cannot read {label} ROOT store");
            let mut context = std::ptr::null();
            let mut candidates = Vec::new();
            // Enumeration releases its preceding context, including on exhaustion.
            loop {
                context = unsafe { CertEnumCertificatesInStore(store, context) };
                if context.is_null() {
                    break;
                }
                candidates.push(platform::encoded(context));
            }
            let enumeration_error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            unsafe { CertCloseStore(store, 0) };
            assert_eq!(
                enumeration_error, 0x8009_2004,
                "{label} ROOT enumeration did not finish"
            );
            assert!(!candidates.is_empty(), "{label} ROOT store is empty");
            candidates.sort();
            let mut digest = sha2::Sha256::new();
            let mut accepted = 0;
            for der in &candidates {
                let found = Certificate::from_der(der)
                    .map(|certificate| {
                        judge(&certificate, now, |at| {
                            platform::evaluate(der, &[], Anchors::System, at)
                        })
                    })
                    .unwrap_or_else(|_| Trust::unchecked(Doubt::Certificate));
                accepted += usize::from(found == trusted());
                digest.update(der.len().to_le_bytes());
                digest.update(der);
                digest.update(serde_json::to_vec(&found).unwrap());
            }
            outcomes.push(format!(
                "{label} ROOT: {} certificates, {accepted} trusted for documents; outcomes {}",
                candidates.len(),
                crate::docinfo::hex_of(&digest.finalize())
            ));
            assert!(accepted > 0, "no usable trusted root in {label} ROOT store");
        }
        outcomes
    }

    /// This test is called only by the parent below: running it as an ordinary
    /// test would measure the test runner's token instead of a worker's token.
    #[cfg(windows)]
    #[test]
    #[ignore = "spawned by windows_worker_trust_matches_uncontained_controls"]
    fn contained_windows_trust_child() {
        // Observe the token and the job, then an actual denied write. Equality
        // of trust verdicts alone would also pass with containment removed.
        assert_eq!(crate::sandbox_win::integrity_level().unwrap(), 0x1000);
        assert!(crate::sandbox_win::in_any_job().unwrap());
        let path = std::env::current_dir()
            .unwrap()
            .join(format!("tpdf-trust-denied-{}.tmp", std::process::id()));
        let write = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path);
        if write.is_ok() {
            drop(write);
            let _ = std::fs::remove_file(&path);
            panic!("low-integrity child could write its medium-integrity working directory");
        }
        assert_eq!(
            write.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        windows_trust_controls();
        println!("[PASS] contained Windows trust controls");
    }

    #[cfg(windows)]
    fn windows_trust_controls() {
        a_windows_system_root_is_trusted();
        a_chain_to_an_anchor_is_trusted();
        a_self_signed_certificate_nobody_anchored_ends_at_an_untrusted_root();
        a_missing_intermediate_is_a_missing_link_not_an_untrusted_root();
        a_certificate_that_has_expired_since_reads_as_expired();
        an_expired_certificate_nobody_vouched_for_is_untrusted_not_expired();
        a_certificate_not_yet_in_force_reads_as_not_yet_valid();
        a_certificate_issued_only_for_web_servers_is_not_trusted_for_signing();
        a_blob_is_read_for_its_signer_and_the_rest_of_its_set();
        super::authority_tests::a_minted_authority_chains_to_its_root_for_timestamping();
        super::authority_tests::an_authority_not_issued_for_timestamping_is_not_trusted_for_it();
    }

    /// Run this test's contained child before its uncontained controls, so the
    /// comparison does not itself warm the store before measuring the boundary.
    /// Other trust tests may run concurrently; this is not a cold-cache claim.
    #[cfg(windows)]
    #[test]
    fn windows_worker_trust_matches_uncontained_controls() {
        use crate::sandbox_win::{spawn_contained, Containment, Stdio};
        use std::io::Read;
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        let writable = std::env::current_dir()
            .unwrap()
            .join(format!("tpdf-trust-control-{}.tmp", std::process::id()));
        let control = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&writable)
            .expect("the parent must be able to write the probe directory");
        drop(control);
        std::fs::remove_file(writable).unwrap();
        let (read, write) = crate::sandbox_win::pipe().unwrap();
        let mut output = unsafe { std::fs::File::from_raw_handle(read) };
        let output_write = unsafe { std::fs::File::from_raw_handle(write) };
        let output_error = output_write.try_clone().unwrap();
        let input = std::fs::File::open("NUL").unwrap();
        let stdio = Stdio {
            stdin: input.as_raw_handle(),
            stdout: output_write.as_raw_handle(),
            stderr: output_error.as_raw_handle(),
        };
        let executable = std::env::current_exe().unwrap();
        let command = format!(
            "\"{}\" --exact trust::tests::platform_tests::contained_windows_trust_child --ignored --nocapture",
            executable.display()
        );
        let child = spawn_contained(&command, &[], &Containment::default(), Some(&stdio)).unwrap();
        drop(output_write);
        drop(output_error);
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            output.read_to_string(&mut text).unwrap();
            text
        });
        child.resume().unwrap();
        let code = child.wait_timeout(60_000).unwrap();
        if code.is_none() {
            child.kill().unwrap();
        }
        let text = reader.join().unwrap();
        println!("{text}");
        assert_eq!(code, Some(0), "contained trust test failed: {text}");
        assert!(text.contains("[PASS] contained Windows trust controls"));
        for expected in windows_system_root_outcomes() {
            assert!(
                text.lines().any(|line| line == expected),
                "system store outcomes differ from child: {expected}\n{text}"
            );
        }
        windows_trust_controls();
    }

    #[test]
    fn a_chain_to_an_anchor_is_trusted() {
        // The control. A checker that answered "not trusted" for everything
        // would pass every refusal below; this is what fails it.
        let ca = root(61, "tpdf test root");
        let leaf = issued_by(&ca, 62, "Signer", false);
        assert_eq!(
            standing(&leaf.der, &[], std::slice::from_ref(&ca.der), NOW),
            trusted()
        );
    }

    #[test]
    fn a_self_signed_certificate_nobody_anchored_ends_at_an_untrusted_root() {
        let ca = root(61, "tpdf test root");
        let alone = root(63, "Self-made signer");
        assert_eq!(
            standing(&alone.der, &[], std::slice::from_ref(&ca.der), NOW),
            untrusted(Doubt::Root)
        );
    }

    #[test]
    fn a_missing_intermediate_is_a_missing_link_not_an_untrusted_root() {
        let ca = root(61, "tpdf test root");
        let middle = issued_by(&ca, 64, "tpdf test intermediate", true);
        let leaf = issued_by(&middle, 65, "Signer", false);
        assert_eq!(
            standing(&leaf.der, &[], std::slice::from_ref(&ca.der), NOW),
            untrusted(Doubt::Incomplete)
        );
        // And the control: the same chain with the intermediate carried, as a
        // signature's certificate set carries it.
        assert_eq!(
            standing(
                &leaf.der,
                std::slice::from_ref(&middle.der),
                std::slice::from_ref(&ca.der),
                NOW
            ),
            trusted()
        );
    }

    #[test]
    fn a_certificate_that_has_expired_since_reads_as_expired() {
        // Trusted at its own last moment, out of date now. Neither "trusted"
        // nor "not trusted" is true of it.
        let ca = root(61, "tpdf test root");
        let key = Soft::p256(66);
        let mut spec = Spec::new("Expired signer");
        spec.issuer = Some((&ca.key, ca.name));
        spec.not_before = NOW - 400 * DAY;
        spec.not_after = NOW - 30 * DAY;
        spec.serial = 66;
        let leaf = certificate(&key, &spec);
        let found = standing(&leaf, &[], std::slice::from_ref(&ca.der), NOW);
        assert_eq!(found.standing, Standing::Expired, "{found:?}");
        assert_eq!(found.why, None);
    }

    #[test]
    fn an_expired_certificate_nobody_vouched_for_is_untrusted_not_expired() {
        // Its dates are not the news when it was never vouched for.
        let ca = root(61, "tpdf test root");
        let key = Soft::p256(67);
        let mut spec = Spec::new("Expired and self-made");
        spec.not_before = NOW - 400 * DAY;
        spec.not_after = NOW - 30 * DAY;
        let leaf = certificate(&key, &spec);
        assert_eq!(
            standing(&leaf, &[], std::slice::from_ref(&ca.der), NOW),
            untrusted(Doubt::Root)
        );
    }

    #[test]
    fn a_certificate_not_yet_in_force_reads_as_not_yet_valid() {
        let ca = root(61, "tpdf test root");
        let key = Soft::p256(68);
        let mut spec = Spec::new("Future signer");
        spec.issuer = Some((&ca.key, ca.name));
        spec.not_before = NOW + 30 * DAY;
        spec.not_after = NOW + 400 * DAY;
        spec.serial = 68;
        let leaf = certificate(&key, &spec);
        let found = standing(&leaf, &[], std::slice::from_ref(&ca.der), NOW);
        assert_eq!(found.standing, Standing::NotYetValid, "{found:?}");
    }

    #[test]
    fn a_certificate_issued_only_for_web_servers_is_not_trusted_for_signing() {
        let ca = root(61, "tpdf test root");
        let signer = |purposes: Vec<&'static str>, serial: u8| {
            let key = Soft::p256(serial);
            let mut spec = Spec::new("Purposeful signer");
            spec.issuer = Some((&ca.key, ca.name));
            spec.purposes = Some(purposes);
            spec.serial = serial;
            certificate(&key, &spec)
        };
        let server = signer(vec!["1.3.6.1.5.5.7.3.1"], 69);
        assert_eq!(
            standing(&server, &[], std::slice::from_ref(&ca.der), NOW),
            untrusted(Doubt::Purpose)
        );
        // The control: e-mail protection, a purpose a document signature serves.
        let mail = signer(vec!["1.3.6.1.5.5.7.3.4"], 70);
        assert_eq!(
            standing(&mail, &[], std::slice::from_ref(&ca.der), NOW),
            trusted()
        );
    }

    /// The control for the system store itself: a root this Mac trusts is
    /// trusted when it is asked about, through `Anchors::System`.
    ///
    /// Every other system-store assertion here is a refusal, and a store that
    /// refused everything --- anchors-only left on with nothing anchored, say
    /// --- would pass them all. The root comes from the store's own list,
    /// read-only (`SecTrustCopyAnchorCertificates`); nothing is added to it.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_root_this_mac_trusts_is_trusted_through_the_system_store() {
        use security_framework::trust::SecTrust;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        let anchors = SecTrust::copy_anchor_certificates().expect("the system's anchors");
        let chosen = anchors
            .iter()
            .map(|anchor| anchor.to_der())
            .find(|der| {
                Certificate::from_der(der).is_ok_and(|c| {
                    let v = &c.tbs_certificate.validity;
                    v.not_before.to_unix_duration().as_secs() < now
                        && now < v.not_after.to_unix_duration().as_secs()
                        && serves(&c, Purpose::Documents) == Some(true)
                })
            })
            .expect("a system root in its dates");
        let parsed = Certificate::from_der(&chosen).expect("a certificate");
        let found = judge(&parsed, now, |at| {
            platform::evaluate(&chosen, &[], Anchors::System, at)
        });
        assert_eq!(found, trusted(), "{} anchors", anchors.len());
    }

    /// A `trustd` that never answered is tpdf's failure, not the document's.
    ///
    /// Measured 2026-09-27 by running `signature-probe --mode trust` under
    /// `sandbox-exec` with the Mach lookup to `trustd` denied:
    /// `SecTrustEvaluateWithError` returns `errSecInternalError` (-26276), and
    /// the first version of this module read that as `untrusted`, *rejected*.
    /// A unit test cannot deny the lookup to itself, so this holds the reading
    /// of the code, beside the control that a real refusal is still one.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_trust_service_that_did_not_answer_is_not_a_refusal() {
        let alone = root(63, "Self-made signer");
        let chain = [alone.der.clone()];
        assert!(platform::failure_of(-26_276, &chain).is_err());
        assert_eq!(platform::failure_of(-67_843, &chain), Ok(Doubt::Root));
        let signer = Certificate::from_der(&alone.der).expect("a certificate");
        let found = judge(&signer, NOW, |_| {
            platform::failure_of(-26_276, &chain).map(|failure| Evaluation {
                passed: false,
                failure,
                chain: chain.to_vec(),
            })
        });
        assert_eq!(found, Trust::unchecked(Doubt::Unavailable));
    }

    #[test]
    fn a_blob_is_read_for_its_signer_and_the_rest_of_its_set() {
        // `of_blob` is what the worker calls: the signer named by `sid`, the
        // other certificates offered as issuers. The intermediate travels in
        // the set, so the chain completes only if the set is read.
        use crate::sign_cms::testkeys::{plain_pdf, signed};
        let ca = root(61, "tpdf test root");
        let middle = issued_by(&ca, 64, "tpdf test intermediate", true);
        let leaf = issued_by(&middle, 65, "Signer", false);
        let bytes = signed(
            &plain_pdf(),
            &leaf.der,
            std::slice::from_ref(&middle.der),
            &leaf.key,
        )
        .expect("signed");
        let blob = blob_of_any(&bytes);
        assert_eq!(
            of_blob(&blob, NOW, Anchors::Only(std::slice::from_ref(&ca.der))),
            trusted()
        );
        // The same signature against the system's roots. The test root is in
        // no store and not in the set, so the chain stops at the intermediate
        // with its issuer nowhere to be found.
        assert_eq!(
            of_blob(&blob, NOW, Anchors::System),
            untrusted(Doubt::Incomplete)
        );
        // Carrying the root as well completes the chain, at a root this
        // machine does not trust --- the shape a self-made CA produces, and
        // the one a root only Adobe's list carries produces too.
        let whole = signed(
            &plain_pdf(),
            &leaf.der,
            &[middle.der.clone(), ca.der.clone()],
            &leaf.key,
        )
        .expect("signed");
        assert_eq!(
            of_blob(&blob_of_any(&whole), NOW, Anchors::System),
            untrusted(Doubt::Root)
        );
    }
}

// ---------------------------------------------- timestamp tokens' authorities

#[cfg(any(target_os = "macos", windows))]
mod authority_tests {
    use super::*;
    use crate::integrity::test_tsa::{mint, Imprint, TestTsa, EMAIL_PROTECTION};

    fn token(tsa: &TestTsa) -> Vec<u8> {
        mint(
            Imprint::Sha256,
            &Imprint::Sha256.digest(b"x"),
            None,
            NOW,
            tsa,
        )
    }

    #[test]
    pub(super) fn a_minted_authority_chains_to_its_root_for_timestamping() {
        // The control: the minter's authority, anchored at its own root, is
        // what increment B's fake timestamp authority will present.
        let tsa = TestTsa::new();
        let roots = std::slice::from_ref(&tsa.root);
        assert_eq!(
            of_blob_for(
                &token(&tsa),
                Purpose::Timestamping,
                NOW,
                Anchors::Only(roots)
            ),
            trusted()
        );
        // And with no anchor the chain stops short: the token carries the
        // authority's certificate and not the root, so the root's absence is
        // a missing link rather than an untrusted root (`docs/TRAPS.md`, *A
        // signature that does not carry its root reads as a missing link*).
        assert_eq!(
            of_blob_for(&token(&tsa), Purpose::Timestamping, NOW, Anchors::Only(&[])),
            untrusted(Doubt::Incomplete)
        );
    }

    #[test]
    pub(super) fn an_authority_not_issued_for_timestamping_is_not_trusted_for_it() {
        for purposes in [None, Some(&[EMAIL_PROTECTION][..])] {
            let tsa = TestTsa::with_purposes(purposes);
            let roots = std::slice::from_ref(&tsa.root);
            assert_eq!(
                of_blob_for(
                    &token(&tsa),
                    Purpose::Timestamping,
                    NOW,
                    Anchors::Only(roots)
                ),
                untrusted(Doubt::Timestamping),
                "{purposes:?}"
            );
        }
    }

    /// The chain handed back is the one vouched for, and there is none
    /// without a `trusted`: not for a chain the store refused, and not for
    /// one it accepted under a certificate that does not serve the purpose,
    /// where the store did assemble a chain.
    #[test]
    fn a_chain_is_handed_back_only_with_a_trusted_answer() {
        let tsa = TestTsa::new();
        let roots = std::slice::from_ref(&tsa.root);
        let (trust, chain) = of_blob_with_chain(
            &token(&tsa),
            Purpose::Timestamping,
            NOW,
            Anchors::Only(roots),
        );
        assert_eq!(trust, trusted());
        assert_eq!(chain, [tsa.certificate.clone(), tsa.root.clone()]);

        let (trust, chain) =
            of_blob_with_chain(&token(&tsa), Purpose::Timestamping, NOW, Anchors::Only(&[]));
        assert_eq!(trust, untrusted(Doubt::Incomplete));
        assert!(chain.is_empty(), "{chain:?}");

        let other = TestTsa::with_purposes(Some(&[EMAIL_PROTECTION]));
        let (trust, chain) = of_blob_with_chain(
            &token(&other),
            Purpose::Timestamping,
            NOW,
            Anchors::Only(roots),
        );
        assert_eq!(trust, untrusted(Doubt::Timestamping));
        assert!(chain.is_empty(), "{chain:?}");
        // The same token asked about as a document signer's passes, and has
        // its chain: the purpose and not the fixture is what emptied it.
        let (trust, chain) = of_blob_with_chain(
            &token(&other),
            Purpose::Documents,
            NOW,
            Anchors::Only(roots),
        );
        assert_eq!(trust, trusted());
        assert_eq!(chain.len(), 2);
    }
}

// ------------------------------------------------- the order of the questions
//
// `judge` with scripted evaluations: no operating system, so these hold the
// reading of an answer rather than any platform's error codes.

fn scripted(
    passes_at: impl Fn(u64) -> bool,
    failure: Doubt,
) -> impl FnMut(u64) -> Result<Evaluation, String> {
    move |at| {
        Ok(Evaluation {
            passed: passes_at(at),
            failure,
            chain: Vec::new(),
        })
    }
}

fn leaf(not_before: u64, not_after: u64, purposes: Option<Vec<&'static str>>) -> Certificate {
    let key = Soft::p256(71);
    let mut spec = Spec::new("Scripted");
    spec.not_before = not_before;
    spec.not_after = not_after;
    spec.purposes = purposes;
    Certificate::from_der(&certificate(&key, &spec)).expect("a certificate")
}

#[test]
fn an_expired_certificate_is_asked_about_at_its_own_last_moment() {
    let until = NOW - 30 * DAY;
    let signer = leaf(NOW - 400 * DAY, until, None);
    let mut asked = Vec::new();
    let found = judge(&signer, NOW, |at| {
        asked.push(at);
        Ok(Evaluation {
            passed: at == until,
            failure: Doubt::Dates,
            chain: Vec::new(),
        })
    });
    assert_eq!(asked, [NOW, until]);
    assert_eq!(found.standing, Standing::Expired);
}

#[test]
fn a_certificate_in_its_dates_is_asked_about_once() {
    // A second evaluation at another moment would let a chain that fails now
    // pass then, and read as something other than its present answer.
    let signer = leaf(NOW - DAY, NOW + DAY, None);
    let mut asked = 0;
    let found = judge(&signer, NOW, |_| {
        asked += 1;
        Ok(Evaluation {
            passed: false,
            failure: Doubt::Root,
            chain: Vec::new(),
        })
    });
    assert_eq!(asked, 1);
    assert_eq!(found, untrusted(Doubt::Root));
}

#[test]
fn an_evaluation_that_could_not_run_is_unchecked_never_untrusted() {
    let signer = leaf(NOW - DAY, NOW + DAY, None);
    let found = judge(&signer, NOW, |_| Err("trustd is not answering".into()));
    assert_eq!(found.standing, Standing::Unchecked);
    assert_eq!(found.why, Some(Doubt::Unavailable));
    assert_eq!(found.store, None, "no store answered");
}

#[test]
fn the_purpose_is_asked_of_a_trusted_chain_and_an_expired_one() {
    let server = Some(vec!["1.3.6.1.5.5.7.3.1"]);
    let current = leaf(NOW - DAY, NOW + DAY, server.clone());
    assert_eq!(
        judge(&current, NOW, scripted(|_| true, Doubt::Rejected)),
        untrusted(Doubt::Purpose)
    );
    let old = leaf(NOW - 400 * DAY, NOW - DAY, server);
    assert_eq!(
        judge(&old, NOW, scripted(|at| at < NOW, Doubt::Dates)),
        untrusted(Doubt::Purpose)
    );
    // `anyExtendedKeyUsage` and no extension at all both admit signing.
    for purposes in [Some(vec!["2.5.29.37.0"]), None] {
        let any = leaf(NOW - DAY, NOW + DAY, purposes);
        assert_eq!(
            judge(&any, NOW, scripted(|_| true, Doubt::Rejected)),
            trusted()
        );
    }
}

#[test]
fn a_timestamp_authority_must_name_timestamping_alone_and_critical() {
    // RFC 3161 §2.3 makes the purpose a requirement, not a restriction: the
    // certificate that restricts nothing is trusted for documents and not for
    // timestamping, which is the one case where the two purposes part.
    let ask = |purposes: Option<Vec<&'static str>>, purpose: Purpose| {
        judge_for(
            &leaf(NOW - DAY, NOW + DAY, purposes),
            purpose,
            NOW,
            scripted(|_| true, Doubt::Rejected),
        )
    };
    let stamping = "1.3.6.1.5.5.7.3.8";
    assert_eq!(ask(Some(vec![stamping]), Purpose::Timestamping), trusted());
    // Among others is not enough: RFC 3161 §2.3 asks for it alone, and
    // OpenSSL enforces that.
    assert_eq!(
        ask(
            Some(vec!["1.3.6.1.5.5.7.3.4", stamping]),
            Purpose::Timestamping
        ),
        untrusted(Doubt::Timestamping)
    );
    // Alone but not critical is not enough either, for the same section.
    let mut relaxed = leaf(NOW - DAY, NOW + DAY, Some(vec![stamping]));
    for extension in relaxed.tbs_certificate.extensions.iter_mut().flatten() {
        if extension.extn_id.to_string() == "2.5.29.37" {
            extension.critical = false;
        }
    }
    assert_eq!(
        judge_for(
            &relaxed,
            Purpose::Timestamping,
            NOW,
            scripted(|_| true, Doubt::Rejected),
        ),
        untrusted(Doubt::Timestamping)
    );
    assert_eq!(
        ask(None, Purpose::Timestamping),
        untrusted(Doubt::Timestamping)
    );
    assert_eq!(
        ask(Some(vec!["1.3.6.1.5.5.7.3.4"]), Purpose::Timestamping),
        untrusted(Doubt::Timestamping)
    );
    // `anyExtendedKeyUsage` names no purpose in particular, and a timestamp
    // authority is required to name this one.
    assert_eq!(
        ask(Some(vec!["2.5.29.37.0"]), Purpose::Timestamping),
        untrusted(Doubt::Timestamping)
    );
    // The same certificates, for documents: no stated purpose restricts
    // nothing, and a timestamping-only certificate is not one to sign with.
    assert_eq!(ask(None, Purpose::Documents), trusted());
    assert_eq!(
        ask(Some(vec![stamping]), Purpose::Documents),
        untrusted(Doubt::Purpose)
    );
}

#[test]
fn a_standing_that_is_not_trusted_says_why() {
    // Over every standing `judge` produces from the scripted answers: `why` is
    // set exactly for `Untrusted` and `Unchecked`.
    let current = leaf(NOW - DAY, NOW + DAY, None);
    let old = leaf(NOW - 400 * DAY, NOW - DAY, None);
    let future = leaf(NOW + DAY, NOW + 400 * DAY, None);
    let found = [
        judge(&current, NOW, scripted(|_| true, Doubt::Rejected)),
        judge(&current, NOW, scripted(|_| false, Doubt::Root)),
        judge(&old, NOW, scripted(|at| at < NOW, Doubt::Dates)),
        judge(&future, NOW, scripted(|at| at > NOW, Doubt::Dates)),
        judge(&current, NOW, |_| Err(String::new())),
    ];
    let standings: Vec<Standing> = found.iter().map(|t| t.standing).collect();
    assert_eq!(
        standings,
        [
            Standing::Trusted,
            Standing::Untrusted,
            Standing::Expired,
            Standing::NotYetValid,
            Standing::Unchecked
        ]
    );
    for trust in &found {
        assert_eq!(
            matches!(trust.standing, Standing::Untrusted | Standing::Unchecked),
            trust.why.is_some(),
            "{trust:?}"
        );
    }
    assert_eq!(Trust::default().standing, Standing::Unchecked);
}

#[test]
fn a_set_larger_than_the_bound_is_not_handed_to_the_os() {
    use crate::sign_cms::testkeys::{plain_pdf, signed};
    let key = Soft::p256(72);
    let der = certificate(&key, &Spec::new("Signer"));
    let padding: Vec<Vec<u8>> = (0..MAX_CERTIFICATES)
        .map(|n| {
            let mut spec = Spec::new("Filler");
            spec.serial = u8::try_from(n + 100).expect("small");
            certificate(&Soft::p256(73), &spec)
        })
        .collect();
    let within =
        signed(&plain_pdf(), &der, &padding[..MAX_CERTIFICATES - 1], &key).expect("signed");
    let over = signed(&plain_pdf(), &der, &padding, &key).expect("signed");
    let prepared = |bytes: &[u8]| {
        let blob = blob_of_any(bytes);
        certificates(&blob).map(|(_, others)| others.len())
    };
    assert_eq!(prepared(&within), Some(MAX_CERTIFICATES - 1));
    assert_eq!(prepared(&over), None);
    assert_eq!(
        of_blob(&blob_of_any(&over), NOW, Anchors::System),
        Trust::unchecked(Doubt::Certificate)
    );
}

/// The one signature's CMS blob in `bytes`, definite length: the hole its
/// `/ByteRange` leaves, decoded.
fn blob_of_any(bytes: &[u8]) -> Vec<u8> {
    let at = bytes
        .windows(10)
        .position(|w| w == b"/ByteRange")
        .expect("a /ByteRange");
    let open = at + bytes[at..].iter().position(|b| *b == b'[').expect("[");
    let close = open + bytes[open..].iter().position(|b| *b == b']').expect("]");
    let numbers: Vec<usize> = std::str::from_utf8(&bytes[open + 1..close])
        .expect("ascii")
        .split_whitespace()
        .map(|n| n.parse().expect("an integer"))
        .collect();
    let hex = std::str::from_utf8(&bytes[numbers[1] + 1..numbers[2] - 1]).expect("ascii");
    let raw: Vec<u8> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex"))
        .collect();
    crate::ber::to_definite_length(&raw).expect("a blob")
}

// -------------------------------------------- judged at an attested moment
//
// Scripted, so the order of the questions is held whatever the platform: the
// evaluation is a closure that records the moment it was asked about.

fn passing(chain: &[u8]) -> Result<Evaluation, String> {
    Ok(Evaluation {
        passed: true,
        failure: Doubt::Rejected,
        chain: vec![chain.to_vec()],
    })
}

#[test]
fn an_attested_moment_is_the_one_moment_asked_about() {
    let ca = root(61, "tpdf test root");
    let key = Soft::p256(67);
    let mut spec = Spec::new("Signer since expired");
    spec.issuer = Some((&ca.key, ca.name));
    spec.not_before = NOW - 400 * DAY;
    spec.not_after = NOW - 30 * DAY;
    spec.serial = 67;
    let leaf = certificate(&key, &spec);
    let parsed = Certificate::from_der(&leaf).expect("a certificate");
    let at = NOW - 60 * DAY;
    let mut asked = Vec::new();
    let found = judge_at(&parsed, Purpose::Documents, at, |moment| {
        asked.push(moment);
        passing(&leaf)
    });
    // Out of date now, and in force then: trusted at the timestamp, and the
    // present never asked about.
    assert_eq!(asked, vec![at]);
    assert_eq!(found.standing, Standing::TrustedAtTimestamp, "{found:?}");
    assert_eq!(found.attested_at, crate::revocation::format_time(at));
    // The same certificate judged now is expired: the attested moment is what
    // changed the answer.
    let now = judge(&parsed, NOW, |_| {
        Ok(Evaluation {
            passed: false,
            failure: Doubt::Dates,
            chain: vec![leaf.clone()],
        })
    });
    assert_ne!(now.standing, Standing::TrustedAtTimestamp);
}

#[test]
fn a_certificate_not_in_force_at_the_attested_moment_is_refused_before_asking() {
    let ca = root(61, "tpdf test root");
    let leaf = issued_by(&ca, 68, "Signer", false);
    let parsed = Certificate::from_der(&leaf.der).expect("a certificate");
    let validity = &parsed.tbs_certificate.validity;
    let from = validity.not_before.to_unix_duration().as_secs();
    let until = validity.not_after.to_unix_duration().as_secs();
    for at in [from - 1, until + 1] {
        let mut asked = 0;
        let found = judge_at(&parsed, Purpose::Documents, at, |_| {
            asked += 1;
            passing(&leaf.der)
        });
        assert_eq!(
            (found.standing, found.why),
            (Standing::Untrusted, Some(Doubt::NotInForce)),
            "{at}"
        );
        assert_eq!(asked, 0, "the dates are the answer, not the chain");
    }
    // The boundaries themselves are inside.
    for at in [from, until] {
        let found = judge_at(&parsed, Purpose::Documents, at, |_| passing(&leaf.der));
        assert_eq!(found.standing, Standing::TrustedAtTimestamp, "{at}");
    }
}

#[test]
fn a_chain_refused_at_the_attested_moment_is_untrusted_with_that_reason_and_moment() {
    let ca = root(61, "tpdf test root");
    let leaf = issued_by(&ca, 69, "Signer", false);
    let parsed = Certificate::from_der(&leaf.der).expect("a certificate");
    let found = judge_at(&parsed, Purpose::Documents, NOW, |_| {
        Ok(Evaluation {
            passed: false,
            failure: Doubt::Root,
            chain: vec![leaf.der.clone()],
        })
    });
    assert_eq!(
        (found.standing, found.why),
        (Standing::Untrusted, Some(Doubt::Root))
    );
    assert!(!found.attested_at.is_empty());
    // A trust service that did not answer is tpdf's failure, not a refusal.
    let found = judge_at(&parsed, Purpose::Documents, NOW, |_| Err("down".into()));
    assert_eq!(
        (found.standing, found.why),
        (Standing::Unchecked, Some(Doubt::Unavailable))
    );
    // And the purpose is still asked.
    let web = {
        let key = Soft::p256(70);
        let mut spec = Spec::new("A web server");
        spec.issuer = Some((&ca.key, ca.name));
        spec.purposes = Some(vec!["1.3.6.1.5.5.7.3.1"]);
        spec.serial = 70;
        certificate(&key, &spec)
    };
    let parsed = Certificate::from_der(&web).expect("a certificate");
    let found = judge_at(&parsed, Purpose::Documents, NOW, |_| passing(&web));
    assert_eq!(
        (found.standing, found.why),
        (Standing::Untrusted, Some(Doubt::Purpose))
    );
}

#[test]
fn dss_certificates_are_offered_within_their_own_bound() {
    // Up to MAX_CERTIFICATES extras, each under the size bound, duplicates
    // once; an oversized one is left out rather than refusing the lot.
    let Ok(bytes) = std::fs::read("../testdata/incr-signed.pdf") else {
        println!("[SKIP] incr-signed.pdf: not generated");
        return;
    };
    let raw = blob_of_any(&bytes);
    let (_, own) = certificates_with(&raw, &[]).expect("the signature's set");
    let extras: Vec<Vec<u8>> = (0..MAX_CERTIFICATES as u8 + 3)
        .map(|n| root(0x10 + n, "tpdf test extra").der)
        .collect();
    let mut offered = extras.clone();
    offered.push(extras[0].clone());
    offered.push(vec![0x30; MAX_CERTIFICATE_BYTES + 1]);
    let (_, with) = certificates_with(&raw, &offered).expect("the set and extras");
    assert_eq!(with.len(), own.len() + MAX_CERTIFICATES);
}
