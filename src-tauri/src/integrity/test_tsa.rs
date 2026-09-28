//! A software timestamp authority that mints RFC 3161 tokens, for tests only.
//!
//! ## Why it exists, and who calls it
//!
//! Every token in `testdata/` is pyHanko's, made by a generator with a TSA
//! certificate that states no purpose --- so one fixture cannot say whether the
//! checks in [`super::token`] refuse what they should. This mints tokens whose
//! every property a test chooses: the imprint and its hash, the nonce, the
//! time, the authority's purpose, the ESS binding, and five ways of being
//! wrong ([`Faults`]). The unit tests call it, and so will the fake
//! timestamp authority a later increment's tests run as a local HTTP server
//! --- which is why [`mint`] takes exactly what a `TimeStampReq` carries
//! (RFC 3161 §2.4.1: an imprint, its hash, an optional nonce) and
//! [`granted`] wraps a token the way a `TimeStampResp` does.
//!
//! ## How both kinds of test reach it, and why it never ships
//!
//! It is one file, **included by path in two crates**:
//!
//! - in the library, as `integrity::test_tsa`, under `#[cfg(test)]`, so it is
//!   compiled into `cargo test`'s unit-test binary and into nothing else;
//! - in `tests/cli.rs`, as `#[path = "../src/integrity/test_tsa.rs"] mod
//!   test_tsa;`, the way that file already includes `tests/cli/forms.rs`.
//!
//! An integration test links the library as a release consumer would, so a
//! `#[cfg(test)]` item is invisible to it; `sign_cms::testkeys` has that
//! limit, and `tests/cli.rs` answered it by writing its own key. Including
//! the source instead gives both the same code without a Cargo feature (which
//! a release build could switch on) and without a `pub` module in the
//! shipped library. The price is that this file may name only external
//! crates --- never `crate::`, which means a different crate in each place ---
//! so it is written against `cms`, `der`, `x509-cert`, `p256` and the hash
//! crates alone, all already dependencies.
//!
//! ## The authority
//!
//! A P-256 key from a fixed seed, a self-signed root, and an authority
//! certificate the root issues --- by default with the extended key usage RFC
//! 3161 §2.3 requires, `id-kp-timeStamping`, critical and alone, which is also
//! what `openssl ts -verify` insists on. The root is what a test hands
//! `trust::Anchors::Only`, so no keychain or certificate store is touched.
//! Both certificates are valid from 2020 to 2040, so a test run at the real
//! present, as `tests/cli.rs` runs, is inside their dates for years.

#![allow(dead_code)] // each including crate calls a different subset

use std::str::FromStr as _;
use std::time::Duration;

use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::content_info::{CmsVersion, ContentInfo};
use cms::signed_data::{
    CertificateSet, EncapsulatedContentInfo, SignedData, SignerIdentifier, SignerInfo, SignerInfos,
};
use der::asn1::{BitString, GeneralizedTime, ObjectIdentifier, OctetString, SetOfVec};
use der::{Decode as _, Encode as _};
use ecdsa::signature::hazmat::PrehashSigner as _;
use sha2_10::Digest as _;
use x509_cert::attr::Attribute;
use x509_cert::certificate::{TbsCertificate, Version};
use x509_cert::ext::Extension;
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};
use x509_cert::time::{Time, Validity};
use x509_cert::Certificate;

/// id-kp-timeStamping, the purpose RFC 3161 §2.3 requires of an authority.
pub const TIMESTAMPING: &str = "1.3.6.1.5.5.7.3.8";

/// id-kp-emailProtection: a purpose that is not timestamping, for the refusal.
pub const EMAIL_PROTECTION: &str = "1.3.6.1.5.5.7.3.4";

/// The first moments of 2020 and of 2040, the certificates' dates.
pub const FROM: u64 = 1_577_836_800;
pub const UNTIL: u64 = 2_208_988_800;

/// The name the authority's certificate carries.
pub const AUTHORITY: &str = "tpdf test timestamp authority - not a real authority";

/// The root's name.
pub const ROOT: &str = "tpdf test timestamp root";

const TST_INFO: &str = "1.2.840.113549.1.9.16.1.4";
const SIGNED_DATA: &str = "1.2.840.113549.1.7.2";
const CONTENT_TYPE: &str = "1.2.840.113549.1.9.3";
const MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
const SIGNING_CERTIFICATE: &str = "1.2.840.113549.1.9.16.2.12";
const SIGNING_CERTIFICATE_V2: &str = "1.2.840.113549.1.9.16.2.47";
/// A policy OID under the documentation arc, which no real authority uses.
const POLICY: &str = "1.3.6.1.4.1.0.1";

/// The hash an imprint is made with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Imprint {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Imprint {
    /// Its OID, dotted.
    #[must_use]
    pub fn oid(self) -> &'static str {
        match self {
            Imprint::Sha1 => "1.3.14.3.2.26",
            Imprint::Sha256 => "2.16.840.1.101.3.4.2.1",
            Imprint::Sha384 => "2.16.840.1.101.3.4.2.2",
            Imprint::Sha512 => "2.16.840.1.101.3.4.2.3",
        }
    }

    /// The digest of `data` under it --- what a client puts in a request.
    #[must_use]
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            Imprint::Sha1 => sha1::Sha1::digest(data).to_vec(),
            Imprint::Sha256 => sha2_10::Sha256::digest(data).to_vec(),
            Imprint::Sha384 => sha2_10::Sha384::digest(data).to_vec(),
            Imprint::Sha512 => sha2_10::Sha512::digest(data).to_vec(),
        }
    }
}

/// Which ESS binding the token's signed attributes carry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Binding {
    /// `signingCertificateV2` over SHA-256, with issuer and serial. RFC 5816.
    #[default]
    V2,
    /// `signingCertificate`, SHA-1 --- what pyHanko's dummy authority writes.
    V1,
    /// Both.
    Both,
    /// Neither, which RFC 3161 §2.4.1 forbids.
    Neither,
    /// `signingCertificateV2` naming the root rather than the authority.
    Other,
}

/// How a minted token is wrong. The default is a token with nothing wrong.
#[derive(Clone, Copy, Debug, Default)]
pub struct Faults {
    /// The `TSTInfo` states an imprint one bit off the one asked for, and is
    /// signed that way: a sound token of different data.
    pub wrong_imprint: bool,
    /// The token's own signature is ECDSA over SHA-1, its digest algorithm
    /// SHA-1 --- as against an SHA-1 *imprint*, which [`mint`]'s argument sets.
    pub sha1_signature: bool,
    /// Which binding to write.
    pub binding: Binding,
    /// One bit of the signature value flipped after signing.
    pub corrupt_signature: bool,
    /// The `TSTInfo` changed after signing --- `genTime` one second later ---
    /// with the signed `messageDigest` left as it was.
    pub altered_content: bool,
}

/// A software timestamp authority: its key, its certificate, and the root.
pub struct TestTsa {
    key: p256::ecdsa::SigningKey,
    /// The authority's certificate, DER, issued by [`TestTsa::root`].
    pub certificate: Vec<u8>,
    /// The self-signed root that issued it, DER: the anchor a test hands
    /// `trust::Anchors::Only`.
    pub root: Vec<u8>,
    /// Certificates the token carries beside the authority's own: none, as
    /// pyHanko's dummy authority carries none; the root, for
    /// [`TestTsa::publishing`], as the public authorities carry their chain.
    pub carried: Vec<Vec<u8>>,
}

impl Default for TestTsa {
    fn default() -> Self {
        TestTsa::new()
    }
}

impl TestTsa {
    /// An authority whose certificate names timestamping, critical and alone.
    #[must_use]
    pub fn new() -> Self {
        TestTsa::with_purposes(Some(&[TIMESTAMPING]))
    }

    /// An authority whose certificate states `purposes` as its extended key
    /// usage, or none at all for `None`.
    #[must_use]
    pub fn with_purposes(purposes: Option<&[&str]>) -> Self {
        let root_key = key(0x51);
        let key = key(0x52);
        let root = certificate(&Spec {
            subject: ROOT,
            issuer: ROOT,
            key: &root_key,
            signer: &root_key,
            serial: 1,
            purposes: None,
            authority: true,
        });
        let certificate = certificate(&Spec {
            subject: AUTHORITY,
            issuer: ROOT,
            key: &key,
            signer: &root_key,
            serial: 2,
            purposes,
            authority: false,
        });
        TestTsa {
            key,
            certificate,
            root,
            carried: Vec::new(),
        }
    }

    /// The authority of [`TestTsa::new`] --- the same keys and root --- whose
    /// certificate says where its revocation data is published, and whose
    /// tokens carry the root, as a public authority's carry its chain: what a
    /// B-LT signing needs to find the issuer it asks about.
    #[must_use]
    pub fn publishing(published: &Published) -> Self {
        let root_key = key(0x51);
        let key = key(0x52);
        let root = certificate(&Spec {
            subject: ROOT,
            issuer: ROOT,
            key: &root_key,
            signer: &root_key,
            serial: 1,
            purposes: None,
            authority: true,
        });
        let certificate = certificate_with(
            &Spec {
                subject: AUTHORITY,
                issuer: ROOT,
                key: &key,
                signer: &root_key,
                serial: 2,
                purposes: Some(&[TIMESTAMPING]),
                authority: false,
            },
            Dates {
                from: FROM,
                until: UNTIL,
            },
            &published.extensions(),
        );
        TestTsa {
            key,
            certificate,
            carried: vec![root.clone()],
            root,
        }
    }
}

/// A token for `imprint`, made with `imprint_hash`, at `gen_time` (seconds
/// since the epoch), carrying `nonce` when given --- a sound one.
///
/// # Panics
///
/// Never on inputs a test chooses; every encoding here is of values built here.
#[must_use]
pub fn mint(
    imprint_hash: Imprint,
    imprint: &[u8],
    nonce: Option<&[u8]>,
    gen_time: u64,
    tsa: &TestTsa,
) -> Vec<u8> {
    mint_with(
        imprint_hash,
        imprint,
        nonce,
        gen_time,
        tsa,
        &Faults::default(),
    )
}

/// [`mint`], wrong in the ways `faults` says.
///
/// # Panics
///
/// As [`mint`].
#[must_use]
pub fn mint_with(
    imprint_hash: Imprint,
    imprint: &[u8],
    nonce: Option<&[u8]>,
    gen_time: u64,
    tsa: &TestTsa,
    faults: &Faults,
) -> Vec<u8> {
    let mut stated = imprint.to_vec();
    if faults.wrong_imprint {
        if let Some(first) = stated.first_mut() {
            *first ^= 0x01;
        }
    }
    let statement = tst_info(imprint_hash, &stated, nonce, gen_time);
    let (digest_oid, signature_oid) = if faults.sha1_signature {
        (Imprint::Sha1.oid(), "1.2.840.10045.4.1")
    } else {
        (Imprint::Sha256.oid(), "1.2.840.10045.4.3.2")
    };
    let hash = if faults.sha1_signature {
        Imprint::Sha1
    } else {
        Imprint::Sha256
    };

    let authority = Certificate::from_der(&tsa.certificate).expect("the authority's certificate");
    let mut attributes = vec![
        attribute(
            CONTENT_TYPE,
            der::Any::encode_from(&oid(TST_INFO)).expect("type"),
        ),
        attribute(
            MESSAGE_DIGEST,
            der::Any::encode_from(&OctetString::new(hash.digest(&statement)).expect("octets"))
                .expect("digest"),
        ),
    ];
    let v1 = || {
        // SigningCertificate { certs SEQUENCE OF ESSCertID { certHash } }
        let id = sequence(&[&octets(&Imprint::Sha1.digest(&tsa.certificate))[..]]);
        attribute(
            SIGNING_CERTIFICATE,
            any(&sequence(&[&sequence(&[&id[..]])[..]])),
        )
    };
    let v2 = |named: &[u8]| {
        let parsed = Certificate::from_der(named).expect("a certificate");
        // IssuerSerial { issuer GeneralNames { [4] directoryName }, serial }
        let issuer = parsed.tbs_certificate.issuer.to_der().expect("issuer");
        let general_name = tlv(0xa4, &issuer);
        let serial = parsed
            .tbs_certificate
            .serial_number
            .to_der()
            .expect("serial");
        let issuer_serial = sequence(&[&sequence(&[&general_name[..]])[..], &serial[..]]);
        // ESSCertIDv2 { hashAlgorithm DEFAULT sha256 (omitted), certHash, issuerSerial }
        let hashed = octets(&Imprint::Sha256.digest(named));
        let id = sequence(&[&hashed[..], &issuer_serial[..]]);
        attribute(
            SIGNING_CERTIFICATE_V2,
            any(&sequence(&[&sequence(&[&id[..]])[..]])),
        )
    };
    match faults.binding {
        Binding::V2 => attributes.push(v2(&tsa.certificate)),
        Binding::V1 => attributes.push(v1()),
        Binding::Both => {
            attributes.push(v1());
            attributes.push(v2(&tsa.certificate));
        }
        Binding::Neither => {}
        Binding::Other => attributes.push(v2(&tsa.root)),
    }
    let signed_attrs = SetOfVec::try_from(attributes).expect("a set of attributes");
    let to_sign = signed_attrs.to_der().expect("signed attributes");
    let digest = hash.digest(&to_sign);
    let value: p256::ecdsa::Signature = tsa.key.sign_prehash(&digest).expect("signed");
    let mut value = value.to_der().as_bytes().to_vec();
    if faults.corrupt_signature {
        let last = value.len() - 1;
        value[last] ^= 0x01;
    }

    let content = if faults.altered_content {
        tst_info(imprint_hash, &stated, nonce, gen_time + 1)
    } else {
        statement
    };

    let digest_algorithm = AlgorithmIdentifierOwned {
        oid: oid(digest_oid),
        parameters: None,
    };
    let signer = SignerInfo {
        version: CmsVersion::V1,
        sid: SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
            issuer: authority.tbs_certificate.issuer.clone(),
            serial_number: authority.tbs_certificate.serial_number.clone(),
        }),
        digest_alg: digest_algorithm.clone(),
        signed_attrs: Some(signed_attrs),
        signature_algorithm: AlgorithmIdentifierOwned {
            oid: oid(signature_oid),
            parameters: None,
        },
        signature: OctetString::new(value).expect("octets"),
        unsigned_attrs: None,
    };
    let signed = SignedData {
        version: CmsVersion::V3,
        digest_algorithms: SetOfVec::try_from(vec![digest_algorithm]).expect("a set"),
        encap_content_info: EncapsulatedContentInfo {
            econtent_type: oid(TST_INFO),
            econtent: Some(
                der::Any::encode_from(&OctetString::new(content).expect("octets"))
                    .expect("content"),
            ),
        },
        certificates: Some({
            let mut set = vec![CertificateChoices::Certificate(authority)];
            for carried in &tsa.carried {
                set.push(CertificateChoices::Certificate(
                    Certificate::from_der(carried).expect("a carried certificate"),
                ));
            }
            CertificateSet::try_from(set).expect("certificates")
        }),
        crls: None,
        signer_infos: SignerInfos::try_from(vec![signer]).expect("signer infos"),
    };
    ContentInfo {
        content_type: oid(SIGNED_DATA),
        content: der::Any::encode_from(&signed).expect("signed data"),
    }
    .to_der()
    .expect("a token")
}

/// A `TimeStampResp` granting `token`: status `granted` (0), then the token.
///
/// RFC 3161 §2.4.2. What an authority answers over HTTP, for a fake one.
#[must_use]
pub fn granted(token: &[u8]) -> Vec<u8> {
    let status = sequence(&[&0u8.to_der().expect("status")[..]]);
    sequence(&[&status[..], token])
}

/// The `TSTInfo`, DER. Separate from the minting so the altered-content fault
/// can make a second one that differs in `genTime` alone.
fn tst_info(hash: Imprint, imprint: &[u8], nonce: Option<&[u8]>, gen_time: u64) -> Vec<u8> {
    let algorithm = AlgorithmIdentifierOwned {
        oid: oid(hash.oid()),
        parameters: None,
    }
    .to_der()
    .expect("algorithm");
    let message_imprint = sequence(&[&algorithm[..], &octets(imprint)[..]]);
    let time = GeneralizedTime::from_unix_duration(Duration::from_secs(gen_time))
        .expect("a time")
        .to_der()
        .expect("time");
    let mut fields: Vec<Vec<u8>> = vec![
        1u8.to_der().expect("version"),
        oid(POLICY).to_der().expect("policy"),
        message_imprint,
        // serialNumber: the time is unique enough for a test authority.
        gen_time.to_der().expect("serial"),
        time,
    ];
    if let Some(nonce) = nonce {
        fields.push(
            der::asn1::Uint::new(nonce)
                .expect("a nonce")
                .to_der()
                .expect("nonce"),
        );
    }
    let parts: Vec<&[u8]> = fields.iter().map(Vec::as_slice).collect();
    sequence(&parts)
}

/// A P-256 key whose scalar is `seed` repeated.
fn key(seed: u8) -> p256::ecdsa::SigningKey {
    p256::ecdsa::SigningKey::from_bytes(&[seed; 32].into()).expect("a scalar")
}

struct Spec<'a> {
    subject: &'a str,
    issuer: &'a str,
    key: &'a p256::ecdsa::SigningKey,
    signer: &'a p256::ecdsa::SigningKey,
    serial: u8,
    purposes: Option<&'a [&'a str]>,
    authority: bool,
}

/// id-pkix-ocsp-nocheck (RFC 6960 §4.2.2.2.1), which a delegated responder's
/// certificate carries so that nobody asks about its own revocation: the
/// GlobalSign responder measured on 2026-09-28 carries it.
const OCSP_NO_CHECK: &str = "1.3.6.1.5.5.7.48.1.5";

/// [`Spec`]'s dates, when a certificate needs other ones than 2020 to 2040.
#[derive(Clone, Copy)]
struct Dates {
    from: u64,
    until: u64,
}

/// A certificate for `spec.key`, signed by `spec.signer` with ECDSA SHA-256,
/// valid 2020 to 2040.
fn certificate(spec: &Spec<'_>) -> Vec<u8> {
    certificate_dated(
        spec,
        Dates {
            from: FROM,
            until: UNTIL,
        },
    )
}

/// [`certificate`], valid between `dates`.
fn certificate_dated(spec: &Spec<'_>, dates: Dates) -> Vec<u8> {
    certificate_with(spec, dates, &[])
}

/// [`certificate_dated`], with `more` extensions after the ones `spec` says.
fn certificate_with(spec: &Spec<'_>, dates: Dates, more: &[Extension]) -> Vec<u8> {
    let point = spec.key.verifying_key().to_encoded_point(false);
    let spki = SubjectPublicKeyInfoOwned {
        algorithm: AlgorithmIdentifierOwned {
            oid: oid("1.2.840.10045.2.1"),
            parameters: Some(der::Any::encode_from(&oid("1.2.840.10045.3.1.7")).expect("curve")),
        },
        subject_public_key: BitString::from_bytes(point.as_bytes()).expect("bits"),
    };
    let ecdsa_sha256 = AlgorithmIdentifierOwned {
        oid: oid("1.2.840.10045.4.3.2"),
        parameters: None,
    };
    let mut extensions = vec![Extension {
        extn_id: oid("2.5.29.19"),
        critical: true,
        extn_value: OctetString::new(
            x509_cert::ext::pkix::BasicConstraints {
                ca: spec.authority,
                path_len_constraint: None,
            }
            .to_der()
            .expect("constraints"),
        )
        .expect("octets"),
    }];
    if let Some(purposes) = spec.purposes {
        let usage =
            x509_cert::ext::pkix::ExtendedKeyUsage(purposes.iter().map(|p| oid(p)).collect());
        extensions.push(Extension {
            extn_id: oid("2.5.29.37"),
            critical: true,
            extn_value: OctetString::new(usage.to_der().expect("purposes")).expect("octets"),
        });
    }
    extensions.extend_from_slice(more);
    let time = |seconds: u64| {
        Time::GeneralTime(
            GeneralizedTime::from_unix_duration(Duration::from_secs(seconds)).expect("a time"),
        )
    };
    let tbs = TbsCertificate {
        version: Version::V3,
        serial_number: SerialNumber::new(&[spec.serial]).expect("serial"),
        signature: ecdsa_sha256.clone(),
        issuer: Name::from_str(&format!("CN={}", spec.issuer)).expect("issuer"),
        validity: Validity {
            not_before: time(dates.from),
            not_after: time(dates.until),
        },
        subject: Name::from_str(&format!("CN={}", spec.subject)).expect("subject"),
        subject_public_key_info: spki,
        issuer_unique_id: None,
        subject_unique_id: None,
        extensions: Some(extensions),
    };
    let digest = sha2_10::Sha256::digest(tbs.to_der().expect("tbs"));
    let value: p256::ecdsa::Signature = spec.signer.sign_prehash(&digest).expect("signed");
    Certificate {
        tbs_certificate: tbs,
        signature_algorithm: ecdsa_sha256,
        signature: BitString::from_bytes(value.to_der().as_bytes()).expect("bits"),
    }
    .to_der()
    .expect("a certificate")
}

fn oid(text: &str) -> ObjectIdentifier {
    ObjectIdentifier::new_unwrap(text)
}

/// One attribute of one value.
fn attribute(type_oid: &str, value: der::Any) -> Attribute {
    Attribute {
        oid: oid(type_oid),
        values: SetOfVec::try_from(vec![value]).expect("a set"),
    }
}

/// Already-encoded DER as a value an attribute can carry.
fn any(encoded: &[u8]) -> der::Any {
    der::Any::from_der(encoded).expect("encoded DER")
}

fn octets(bytes: &[u8]) -> Vec<u8> {
    OctetString::new(bytes)
        .expect("octets")
        .to_der()
        .expect("octets")
}

fn sequence(parts: &[&[u8]]) -> Vec<u8> {
    tlv(0x30, &parts.concat())
}

/// One tag-length-value, definite length.
fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
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
}

// ------------------------------------------------------ revocation data
//
// A certificate authority that issues end-entity certificates, OCSP responses
// (RFC 6960) and revocation lists (RFC 5280 §5), for the revocation tests and
// for the fake OCSP and CRL servers a later increment's tests run. Same rule
// as the authority above: external crates only, never `crate::`.

/// id-kp-OCSPSigning, the purpose a delegated responder's certificate names.
pub const OCSP_SIGNING: &str = "1.3.6.1.5.5.7.3.9";

/// A certificate authority: a P-256 key and its self-signed certificate.
pub struct TestCa {
    key: p256::ecdsa::SigningKey,
    name: String,
    /// The authority's own certificate, DER: the anchor a test hands
    /// `trust::Anchors::Only`.
    pub certificate: Vec<u8>,
}

/// A certificate a [`TestCa`] issued, with its key.
pub struct Issued {
    key: p256::ecdsa::SigningKey,
    /// The seed the key was made from, so `sign_cms::testkeys::Soft::p256`
    /// can make the same key to sign a document with.
    pub seed: u8,
    /// The certificate, DER.
    pub certificate: Vec<u8>,
}

impl TestCa {
    /// A root named `name`, its key made from `seed`, valid 2020 to 2040.
    #[must_use]
    pub fn new(name: &str, seed: u8) -> Self {
        let key = key(seed);
        let certificate = certificate(&Spec {
            subject: name,
            issuer: name,
            key: &key,
            signer: &key,
            serial: 1,
            purposes: None,
            authority: true,
        });
        TestCa {
            key,
            name: name.to_string(),
            certificate,
        }
    }

    /// A root whose key usage permits signing certificates and **not**
    /// revocation lists (`keyCertSign` without `cRLSign`): a list it signs is
    /// one RFC 5280 §6.3.3 (f) refuses.
    #[must_use]
    pub fn without_list_signing(name: &str, seed: u8) -> Self {
        let key = key(seed);
        let spec = Spec {
            subject: name,
            issuer: name,
            key: &key,
            signer: &key,
            serial: 1,
            purposes: None,
            authority: true,
        };
        // KeyUsage: a BIT STRING with keyCertSign (bit 5) alone.
        let usage = der::asn1::BitString::new(2, vec![0x04]).expect("bits");
        let certificate = certificate_with(
            &spec,
            Dates {
                from: FROM,
                until: UNTIL,
            },
            &[Extension {
                extn_id: oid("2.5.29.15"),
                critical: true,
                extn_value: OctetString::new(usage.to_der().expect("usage")).expect("octets"),
            }],
        );
        TestCa {
            key,
            name: name.to_string(),
            certificate,
        }
    }

    /// The root that issued [`TestTsa`]'s certificate --- the same key and the
    /// same bytes --- so data it signs is about the test authority.
    #[must_use]
    pub fn of_tsa() -> Self {
        TestCa::new(ROOT, 0x51)
    }

    /// An end-entity certificate for a key made from `seed`, with `serial`
    /// and the stated `purposes` (none for `None`), valid 2020 to 2040.
    #[must_use]
    pub fn issue(&self, subject: &str, seed: u8, serial: u8, purposes: Option<&[&str]>) -> Issued {
        let key = key(seed);
        let certificate = certificate(&Spec {
            subject,
            issuer: &self.name,
            key: &key,
            signer: &self.key,
            serial,
            purposes,
            authority: false,
        });
        Issued {
            key,
            seed,
            certificate,
        }
    }

    /// [`TestCa::issue`], valid from `from` until `until` rather than 2020 to
    /// 2040: for a signer whose certificate was not in force at some moment.
    #[must_use]
    pub fn issue_dated(
        &self,
        subject: &str,
        seed: u8,
        serial: u8,
        from: u64,
        until: u64,
    ) -> Issued {
        let key = key(seed);
        let certificate = certificate_dated(
            &Spec {
                subject,
                issuer: &self.name,
                key: &key,
                signer: &self.key,
                serial,
                purposes: None,
                authority: false,
            },
            Dates { from, until },
        );
        Issued {
            key,
            seed,
            certificate,
        }
    }

    /// An intermediate authority this one issues: a certificate that may
    /// issue others, for a chain the signature does not carry in full.
    #[must_use]
    pub fn intermediate(&self, name: &str, seed: u8, serial: u8) -> TestCa {
        let key = key(seed);
        let certificate = certificate(&Spec {
            subject: name,
            issuer: &self.name,
            key: &key,
            signer: &self.key,
            serial,
            purposes: None,
            authority: true,
        });
        TestCa {
            key,
            name: name.to_string(),
            certificate,
        }
    }

    /// [`TestCa::intermediate`] carrying `id-pkix-ocsp-nocheck`: an authority
    /// no real issuer makes, for the one rule a chain walk applies to it ---
    /// walked through, never asked about.
    #[must_use]
    pub fn intermediate_no_check(&self, name: &str, seed: u8, serial: u8) -> TestCa {
        let key = key(seed);
        let certificate = certificate_with(
            &Spec {
                subject: name,
                issuer: &self.name,
                key: &key,
                signer: &self.key,
                serial,
                purposes: None,
                authority: true,
            },
            Dates {
                from: FROM,
                until: UNTIL,
            },
            &[Extension {
                extn_id: oid(OCSP_NO_CHECK),
                critical: false,
                extn_value: OctetString::new(der::asn1::Null.to_der().expect("null"))
                    .expect("octets"),
            }],
        );
        TestCa {
            key,
            name: name.to_string(),
            certificate,
        }
    }

    /// A delegated OCSP responder: a certificate this authority issues with
    /// `id-kp-OCSPSigning`, as RFC 6960 §4.2.2.2 requires, and
    /// `id-pkix-ocsp-nocheck`, as real responders' certificates carry it.
    #[must_use]
    pub fn responder(&self, seed: u8) -> Issued {
        self.responder_dated(seed, FROM, UNTIL)
    }

    /// [`TestCa::responder`], in force only from `from` until `until`: a
    /// responder whose certificate was not in force when it answered.
    #[must_use]
    pub fn responder_dated(&self, seed: u8, from: u64, until: u64) -> Issued {
        let key = key(seed);
        let spec = Spec {
            subject: "tpdf test OCSP responder",
            issuer: &self.name,
            key: &key,
            signer: &self.key,
            serial: 0x70,
            purposes: Some(&[OCSP_SIGNING]),
            authority: false,
        };
        let certificate = certificate_with(
            &spec,
            Dates { from, until },
            &[Extension {
                extn_id: oid(OCSP_NO_CHECK),
                critical: false,
                extn_value: OctetString::new(der::asn1::Null.to_der().expect("null"))
                    .expect("octets"),
            }],
        );
        Issued {
            key,
            seed,
            certificate,
        }
    }
}

/// What an OCSP response says about a certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Good,
    /// Revoked at `at` (seconds since the epoch), with an RFC 5280 reason
    /// code when `reason` is given.
    Revoked {
        at: u64,
        reason: Option<u8>,
    },
    Unknown,
}

/// Who signs an OCSP response.
#[derive(Clone, Copy)]
pub enum Responder<'a> {
    /// The certificate's issuer itself; the `ResponderID` names it by key.
    Issuer,
    /// A responder certificate, carried in the response and named by name.
    /// Whether it is authorised is the test's choice: one from
    /// [`TestCa::responder`] is; one issued without `id-kp-OCSPSigning`, or
    /// by another authority, is not.
    Delegated(&'a Issued),
}

/// How a minted OCSP response is wrong. The default is nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct OcspFaults {
    /// The `CertID`'s serial is one more than the certificate's: an answer
    /// about another certificate.
    pub wrong_cert_id: bool,
    /// One bit of the response's signature flipped after signing.
    pub corrupt_signature: bool,
    /// An extension tpdf does not know, marked critical, on the response.
    pub critical_extension: bool,
    /// `archiveCutoff` at this time: the responder keeps expired certificates.
    pub archive_cutoff: Option<u64>,
}

/// An `OCSPResponse`, DER --- status successful, type basic --- saying
/// `status` about `certificate`, which `issuer` issued, with `this_update`
/// and `next_update` (seconds since the epoch; `producedAt` is
/// `this_update`), signed by `responder`, wrong in the ways `faults` says.
///
/// `CertID` hashes with SHA-1, as OpenSSL and the public responders do.
///
/// # Panics
///
/// Never on inputs a test chooses.
#[must_use]
pub fn mint_ocsp(
    certificate: &[u8],
    issuer: &TestCa,
    status: Status,
    this_update: u64,
    next_update: Option<u64>,
    responder: Responder<'_>,
    faults: &OcspFaults,
) -> Vec<u8> {
    use x509_ocsp::{
        BasicOcspResponse, CertId, CertStatus, OcspGeneralizedTime, OcspResponse,
        OcspResponseStatus, ResponderId, ResponseBytes, ResponseData, RevokedInfo, SingleResponse,
    };

    let subject = Certificate::from_der(certificate).expect("the certificate");
    let authority = Certificate::from_der(&issuer.certificate).expect("the issuer");
    let key_bits = |c: &Certificate| {
        c.tbs_certificate
            .subject_public_key_info
            .subject_public_key
            .raw_bytes()
            .to_vec()
    };
    let mut serial = subject.tbs_certificate.serial_number.as_bytes().to_vec();
    if faults.wrong_cert_id {
        if let Some(last) = serial.last_mut() {
            *last = last.wrapping_add(1);
        }
    }
    let cert_id = CertId {
        hash_algorithm: AlgorithmIdentifierOwned {
            oid: oid(Imprint::Sha1.oid()),
            parameters: Some(der::asn1::Null.into()),
        },
        issuer_name_hash: OctetString::new(
            Imprint::Sha1.digest(&subject.tbs_certificate.issuer.to_der().expect("issuer")),
        )
        .expect("octets"),
        issuer_key_hash: OctetString::new(Imprint::Sha1.digest(&key_bits(&authority)))
            .expect("octets"),
        serial_number: SerialNumber::new(&serial).expect("serial"),
    };
    let when = |seconds: u64| {
        OcspGeneralizedTime(
            GeneralizedTime::from_unix_duration(Duration::from_secs(seconds)).expect("a time"),
        )
    };
    let cert_status = match status {
        Status::Good => CertStatus::good(),
        Status::Unknown => CertStatus::unknown(),
        Status::Revoked { at, reason } => CertStatus::Revoked(RevokedInfo {
            revocation_time: when(at),
            revocation_reason: reason.map(crl_reason),
        }),
    };
    let (responder_id, signer, certs) = match responder {
        Responder::Issuer => (
            ResponderId::ByKey(
                OctetString::new(Imprint::Sha1.digest(&key_bits(&authority))).expect("octets"),
            ),
            &issuer.key,
            None,
        ),
        Responder::Delegated(delegate) => {
            let certificate =
                Certificate::from_der(&delegate.certificate).expect("the responder's certificate");
            (
                ResponderId::ByName(certificate.tbs_certificate.subject.clone()),
                &delegate.key,
                Some(vec![certificate]),
            )
        }
    };
    let mut extensions = Vec::new();
    if faults.critical_extension {
        extensions.push(Extension {
            extn_id: oid("1.3.6.1.4.1.0.9"),
            critical: true,
            extn_value: OctetString::new(der::asn1::Null.to_der().expect("null")).expect("octets"),
        });
    }
    if let Some(cutoff) = faults.archive_cutoff {
        extensions.push(Extension {
            extn_id: oid("1.3.6.1.5.5.7.48.1.6"),
            critical: false,
            extn_value: OctetString::new(
                GeneralizedTime::from_unix_duration(Duration::from_secs(cutoff))
                    .expect("a time")
                    .to_der()
                    .expect("time"),
            )
            .expect("octets"),
        });
    }
    let data = ResponseData {
        version: x509_ocsp::Version::V1,
        responder_id,
        produced_at: when(this_update),
        responses: vec![SingleResponse {
            cert_id,
            cert_status,
            this_update: when(this_update),
            next_update: next_update.map(when),
            single_extensions: None,
        }],
        response_extensions: (!extensions.is_empty()).then_some(extensions),
    };
    let tbs = data.to_der().expect("response data");
    let mut value = sign_p256(signer, &tbs);
    if faults.corrupt_signature {
        let last = value.len() - 1;
        value[last] ^= 0x01;
    }
    let basic = BasicOcspResponse {
        tbs_response_data: data,
        signature_algorithm: ecdsa_sha256(),
        signature: BitString::from_bytes(&value).expect("bits"),
        certs,
    };
    OcspResponse {
        response_status: OcspResponseStatus::Successful,
        response_bytes: Some(ResponseBytes {
            response_type: oid("1.3.6.1.5.5.7.48.1.1"),
            response: OctetString::new(basic.to_der().expect("basic")).expect("octets"),
        }),
    }
    .to_der()
    .expect("a response")
}

/// The `BasicOCSPResponse` inside a minted `OCSPResponse`: what a CMS `crls`
/// set carries in `id-ri-ocsp-response` form.
///
/// # Panics
///
/// On bytes [`mint_ocsp`] did not make.
#[must_use]
pub fn basic_of(response: &[u8]) -> Vec<u8> {
    x509_ocsp::OcspResponse::from_der(response)
        .expect("a response")
        .response_bytes
        .expect("bytes")
        .response
        .as_bytes()
        .to_vec()
}

/// One certificate a list names as revoked.
#[derive(Clone, Copy)]
pub struct Listed<'a> {
    /// The certificate, DER; its serial is what the list carries.
    pub certificate: &'a [u8],
    /// When it was revoked, seconds since the epoch.
    pub at: u64,
    /// An RFC 5280 reason code, when the entry states one.
    pub reason: Option<u8>,
}

/// How a minted list is wrong. The default is nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct CrlFaults {
    /// One bit of the list's signature flipped after signing.
    pub corrupt_signature: bool,
    /// The list names another issuer than the one that signs it.
    pub wrong_issuer: bool,
    /// A delta-CRL indicator, critical: a form tpdf does not interpret.
    pub delta: bool,
    /// `expiredCertsOnCRL` at this time: the list keeps expired certificates.
    pub expired_certs_on_crl: Option<u64>,
    /// An `issuingDistributionPoint` saying the list covers only authorities'
    /// certificates (`onlyContainsCACerts`): its silence about an end
    /// entity's says nothing.
    pub only_authorities: bool,
    /// An `issuingDistributionPoint` naming a distribution point the test
    /// certificates do not name: a partition that is not theirs.
    pub partition: bool,
}

/// A `CertificateList`, DER, by `issuer`, listing `listed`, with
/// `this_update` and `next_update`, wrong in the ways `faults` says.
///
/// # Panics
///
/// Never on inputs a test chooses.
#[must_use]
pub fn mint_crl(
    issuer: &TestCa,
    listed: &[Listed<'_>],
    this_update: u64,
    next_update: Option<u64>,
    faults: &CrlFaults,
) -> Vec<u8> {
    use x509_cert::crl::{CertificateList, RevokedCert, TbsCertList};

    let utc = |seconds: u64| {
        Time::UtcTime(
            der::asn1::UtcTime::from_unix_duration(Duration::from_secs(seconds)).expect("a time"),
        )
    };
    let revoked: Vec<RevokedCert> = listed
        .iter()
        .map(|entry| {
            let certificate = Certificate::from_der(entry.certificate).expect("a certificate");
            RevokedCert {
                serial_number: certificate.tbs_certificate.serial_number.clone(),
                revocation_date: utc(entry.at),
                crl_entry_extensions: entry.reason.map(|reason| {
                    vec![Extension {
                        extn_id: oid("2.5.29.21"),
                        critical: false,
                        extn_value: OctetString::new(crl_reason(reason).to_der().expect("reason"))
                            .expect("octets"),
                    }]
                }),
            }
        })
        .collect();
    let mut extensions = vec![Extension {
        // cRLNumber, which RFC 5280 §5.2.3 requires of a conforming issuer.
        extn_id: oid("2.5.29.20"),
        critical: false,
        extn_value: OctetString::new(this_update.to_der().expect("number")).expect("octets"),
    }];
    if faults.delta {
        extensions.push(Extension {
            extn_id: oid("2.5.29.27"),
            critical: true,
            extn_value: OctetString::new(1u8.to_der().expect("number")).expect("octets"),
        });
    }
    if faults.only_authorities || faults.partition {
        // IssuingDistributionPoint ::= SEQUENCE {
        //   distributionPoint [0] DistributionPointName OPTIONAL,
        //   onlyContainsUserCerts [1] BOOLEAN DEFAULT FALSE,
        //   onlyContainsCACerts [2] BOOLEAN DEFAULT FALSE, ... }
        let mut point = Vec::new();
        if faults.partition {
            let uri = b"http://crl.example/partition-2.crl";
            // [0] { fullName [0] { uniformResourceIdentifier [6] uri } }
            point.extend(tlv(0xa0, &tlv(0xa0, &tlv(0x86, uri))));
        }
        if faults.only_authorities {
            point.extend(tlv(0x82, &[0xff]));
        }
        extensions.push(Extension {
            extn_id: oid("2.5.29.28"),
            critical: true,
            extn_value: OctetString::new(sequence(&[&point[..]])).expect("octets"),
        });
    }
    if let Some(since) = faults.expired_certs_on_crl {
        extensions.push(Extension {
            extn_id: oid("2.5.29.60"),
            critical: false,
            extn_value: OctetString::new(
                GeneralizedTime::from_unix_duration(Duration::from_secs(since))
                    .expect("a time")
                    .to_der()
                    .expect("time"),
            )
            .expect("octets"),
        });
    }
    let name = if faults.wrong_issuer {
        "tpdf test - some other authority".to_string()
    } else {
        issuer.name.clone()
    };
    let tbs = TbsCertList {
        version: x509_cert::Version::V2,
        signature: ecdsa_sha256(),
        issuer: Name::from_str(&format!("CN={name}")).expect("issuer"),
        this_update: utc(this_update),
        next_update: next_update.map(utc),
        revoked_certificates: (!revoked.is_empty()).then_some(revoked),
        crl_extensions: Some(extensions),
    };
    let mut value = sign_p256(&issuer.key, &tbs.to_der().expect("tbs"));
    if faults.corrupt_signature {
        let last = value.len() - 1;
        value[last] ^= 0x01;
    }
    CertificateList {
        tbs_cert_list: tbs,
        signature_algorithm: ecdsa_sha256(),
        signature: BitString::from_bytes(&value).expect("bits"),
    }
    .to_der()
    .expect("a list")
}

/// `bytes` with one incremental revision appended that gives the catalog a
/// `/DSS` of `certificates`, `responses` (full `OCSPResponse`s) and `lists`,
/// each an uncompressed stream --- what a PAdES B-LT writer appends after
/// signing. Every earlier byte is kept, so the signatures still cover what
/// they covered.
///
/// # Panics
///
/// On a document `lopdf` cannot load.
#[must_use]
pub fn with_dss(
    bytes: &[u8],
    certificates: &[Vec<u8>],
    responses: &[Vec<u8>],
    lists: &[Vec<u8>],
) -> Vec<u8> {
    use lopdf::{Dictionary, Document, IncrementalDocument, Object, Stream};

    let prev = Document::load_mem(bytes).expect("a document");
    let root = prev
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("a catalog reference");
    let mut catalog = prev
        .get_object(root)
        .and_then(Object::as_dict)
        .expect("a catalog")
        .clone();
    let mut incremental = IncrementalDocument::create_from(bytes.to_vec(), prev);
    let doc = &mut incremental.new_document;
    let mut streams = |items: &[Vec<u8>]| -> Object {
        Object::Array(
            items
                .iter()
                .map(|item| {
                    Object::Reference(doc.add_object(Stream::new(Dictionary::new(), item.clone())))
                })
                .collect(),
        )
    };
    let mut dss = Dictionary::new();
    dss.set("Certs", streams(certificates));
    dss.set("OCSPs", streams(responses));
    dss.set("CRLs", streams(lists));
    let dss = doc.add_object(dss);
    catalog.set("DSS", dss);
    doc.set_object(root, catalog);
    let mut out = Vec::new();
    incremental.save_to(&mut out).expect("saved");
    out
}

fn ecdsa_sha256() -> AlgorithmIdentifierOwned {
    AlgorithmIdentifierOwned {
        oid: oid("1.2.840.10045.4.3.2"),
        parameters: None,
    }
}

/// ECDSA P-256 over SHA-256 of `data`, DER.
fn sign_p256(key: &p256::ecdsa::SigningKey, data: &[u8]) -> Vec<u8> {
    let digest = sha2_10::Sha256::digest(data);
    let value: p256::ecdsa::Signature = key.sign_prehash(&digest).expect("signed");
    value.to_der().as_bytes().to_vec()
}

/// An RFC 5280 reason code as the type both formats carry it in.
fn crl_reason(code: u8) -> x509_cert::ext::pkix::CrlReason {
    use x509_cert::ext::pkix::CrlReason as R;
    match code {
        1 => R::KeyCompromise,
        2 => R::CaCompromise,
        3 => R::AffiliationChanged,
        4 => R::Superseded,
        5 => R::CessationOfOperation,
        6 => R::CertificateHold,
        8 => R::RemoveFromCRL,
        9 => R::PrivilegeWithdrawn,
        10 => R::AaCompromise,
        _ => R::Unspecified,
    }
}

// ----------------------------------------------- a fake PKI on 127.0.0.1
//
// For the long-term validation data a signing gathers (increment C2): test
// certificates that say where their revocation data is published --- an OCSP
// responder in `authorityInfoAccess`, a list in `cRLDistributionPoints` ---
// and a server on 127.0.0.1 answering at those addresses with what the
// minters above make, one fault at a time. Same rule as everything here:
// external crates and `std` only, so `tests/cli.rs` runs the same server.

/// id-ad-ocsp, RFC 5280 §4.2.2.1: an `authorityInfoAccess` entry naming an
/// OCSP responder.
pub const ID_AD_OCSP: &str = "1.3.6.1.5.5.7.48.1";

/// Where a certificate says its revocation data is published: OCSP
/// responders and revocation lists, each a URI, in the order written.
#[derive(Clone, Debug, Default)]
pub struct Published {
    /// `authorityInfoAccess` entries of the `id-ad-ocsp` method.
    pub ocsp: Vec<String>,
    /// `cRLDistributionPoints`, each a `fullName` of one URI.
    pub crl: Vec<String>,
    /// `id-pkix-ocsp-nocheck` as well: nobody need ask about this one.
    pub no_check: bool,
}

impl Published {
    /// The two extensions, non-critical as RFC 5280 requires; neither when
    /// there is nothing in it.
    fn extensions(&self) -> Vec<Extension> {
        use x509_cert::ext::pkix::crl::dp::DistributionPoint;
        use x509_cert::ext::pkix::name::{DistributionPointName, GeneralName};
        use x509_cert::ext::pkix::{AccessDescription, AuthorityInfoAccessSyntax};
        let uri = |text: &str| {
            GeneralName::UniformResourceIdentifier(der::asn1::Ia5String::new(text).expect("a URI"))
        };
        let mut out = Vec::new();
        if !self.ocsp.is_empty() {
            let access = AuthorityInfoAccessSyntax(
                self.ocsp
                    .iter()
                    .map(|url| AccessDescription {
                        access_method: oid(ID_AD_OCSP),
                        access_location: uri(url),
                    })
                    .collect(),
            );
            out.push(Extension {
                extn_id: oid("1.3.6.1.5.5.7.1.1"),
                critical: false,
                extn_value: OctetString::new(access.to_der().expect("access")).expect("octets"),
            });
        }
        if !self.crl.is_empty() {
            let points = x509_cert::ext::pkix::CrlDistributionPoints(
                self.crl
                    .iter()
                    .map(|url| DistributionPoint {
                        distribution_point: Some(DistributionPointName::FullName(vec![uri(url)])),
                        reasons: None,
                        crl_issuer: None,
                    })
                    .collect(),
            );
            out.push(Extension {
                extn_id: oid("2.5.29.31"),
                critical: false,
                extn_value: OctetString::new(points.to_der().expect("points")).expect("octets"),
            });
        }
        if self.no_check {
            out.push(Extension {
                extn_id: oid(OCSP_NO_CHECK),
                critical: false,
                extn_value: OctetString::new(der::asn1::Null.to_der().expect("null"))
                    .expect("octets"),
            });
        }
        out
    }
}

impl TestCa {
    /// [`TestCa::issue`] with no stated purpose, saying where its revocation
    /// data is published.
    #[must_use]
    pub fn issue_publishing(
        &self,
        subject: &str,
        seed: u8,
        serial: u8,
        published: &Published,
    ) -> Issued {
        let key = key(seed);
        let certificate = certificate_with(
            &Spec {
                subject,
                issuer: &self.name,
                key: &key,
                signer: &self.key,
                serial,
                purposes: None,
                authority: false,
            },
            Dates {
                from: FROM,
                until: UNTIL,
            },
            &published.extensions(),
        );
        Issued {
            key,
            seed,
            certificate,
        }
    }

    /// A cross-certificate: `other`'s name and key, issued by this authority
    /// --- the form DigiCert's and Sectigo's tokens carry their roots in.
    #[must_use]
    pub fn cross(&self, other: &TestCa, serial: u8) -> Vec<u8> {
        certificate(&Spec {
            subject: &other.name,
            issuer: &self.name,
            key: &other.key,
            signer: &self.key,
            serial,
            purposes: None,
            authority: true,
        })
    }

    /// [`TestCa::intermediate`], saying where its own revocation data is.
    #[must_use]
    pub fn intermediate_publishing(
        &self,
        name: &str,
        seed: u8,
        serial: u8,
        published: &Published,
    ) -> TestCa {
        let key = key(seed);
        let certificate = certificate_with(
            &Spec {
                subject: name,
                issuer: &self.name,
                key: &key,
                signer: &self.key,
                serial,
                purposes: None,
                authority: true,
            },
            Dates {
                from: FROM,
                until: UNTIL,
            },
            &published.extensions(),
        );
        TestCa {
            key,
            name: name.to_string(),
            certificate,
        }
    }
}

/// How the fake PKI answers at one address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Serve {
    /// A sound response or list saying the certificate is not revoked.
    Good,
    /// A sound response or list saying it was revoked a day ago, for key
    /// compromise.
    Revoked,
    /// A sound OCSP response saying the responder does not know it. A list
    /// has no such answer, and serves [`Serve::Good`].
    Unknown,
    /// A good answer with one bit of its signature flipped.
    CorruptSignature,
    /// A good answer whose `nextUpdate` passed three days ago.
    Stale,
    /// OCSP's `tryLater` status; for a list, HTTP 503.
    TryLater,
    /// This HTTP status, and no body.
    Status(u16),
    /// A body longer than any bound: 70 KiB at an OCSP address, 9 MiB at a
    /// list's.
    Long,
    /// Reads the request and says nothing for ten seconds.
    Silent,
    /// `200 OK` with this many bytes of `0x30`.
    Bytes(usize),
    /// `200 OK` with bytes that are not DER.
    Garbage,
}

/// What the fake PKI publishes and serves: `None` is an address the
/// certificate does not name at all.
#[derive(Clone, Copy, Debug, Default)]
pub struct Plan {
    /// The signer's OCSP responder.
    pub signer_ocsp: Option<Serve>,
    /// The list the signer's certificate names.
    pub signer_crl: Option<Serve>,
    /// Whether an intermediate authority stands between the signer and the
    /// root.
    pub intermediate: bool,
    /// The intermediate's own responder, when there is one.
    pub intermediate_ocsp: Option<Serve>,
    /// The intermediate's own list, when there is one.
    pub intermediate_crl: Option<Serve>,
    /// The timestamp authority's responder.
    pub authority_ocsp: Option<Serve>,
    /// The timestamp authority's list.
    pub authority_crl: Option<Serve>,
    /// Every certificate names an `ldap:` list first, before any `http:` one.
    pub ldap_first: bool,
    /// The signer's certificate carries `id-pkix-ocsp-nocheck`.
    pub signer_no_check: bool,
}

/// Every request the fake PKI has read, as `(path, body)`, shared with its
/// threads.
pub type Asked = std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

/// A certificate authority, a signer it issued, and the publishing test
/// timestamp authority, all saying where their revocation data is --- and a
/// server on 127.0.0.1 answering there.
pub struct Pki {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    /// The signer's root.
    pub root: TestCa,
    /// The authority between them, when [`Plan::intermediate`].
    pub intermediate: Option<TestCa>,
    /// The signer, whose key is `Soft::p256(signer.seed)`.
    pub signer: Issued,
    /// Above the signer, as an OS chain API returns it: the intermediate,
    /// when there is one, then the root.
    pub chain: Vec<Vec<u8>>,
    /// The timestamp authority, publishing ([`TestTsa::publishing`]).
    pub tsa: TestTsa,
    /// Every request the server has read, as `(path, body)`, in order.
    pub asked: Asked,
}

/// The world `plan` describes at `base`, built the same way twice --- once
/// for the test and once inside the server's thread --- since keys are not
/// shared across it.
fn pki_world(base: &str, plan: &Plan) -> (TestCa, Option<TestCa>, Issued, TestTsa) {
    let names = |ocsp: Option<Serve>, crl: Option<Serve>, path: &str| {
        let mut published = Published::default();
        if ocsp.is_some() {
            published.ocsp.push(format!("{base}/ocsp/{path}"));
        }
        if plan.ldap_first {
            published
                .crl
                .push("ldap://127.0.0.1/cn=tpdf%20test,o=nobody?certificateRevocationList".into());
        }
        if crl.is_some() {
            published.crl.push(format!("{base}/crl/{path}.crl"));
        }
        published
    };
    let root = TestCa::new("tpdf test PKI root", 0x71);
    let intermediate = plan.intermediate.then(|| {
        root.intermediate_publishing(
            "tpdf test PKI intermediate",
            0x72,
            2,
            &names(
                plan.intermediate_ocsp,
                plan.intermediate_crl,
                "intermediate",
            ),
        )
    });
    let issuer = intermediate.as_ref().unwrap_or(&root);
    let signer = issuer.issue_publishing(
        "tpdf test PKI signer - not a real identity",
        0x73,
        5,
        &Published {
            no_check: plan.signer_no_check,
            ..names(plan.signer_ocsp, plan.signer_crl, "signer")
        },
    );
    let tsa = TestTsa::publishing(&names(plan.authority_ocsp, plan.authority_crl, "authority"));
    (root, intermediate, signer, tsa)
}

impl Pki {
    /// Starts the server, and returns the world it answers for.
    ///
    /// # Panics
    ///
    /// No port on 127.0.0.1.
    #[must_use]
    pub fn start(plan: Plan) -> Pki {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = listener.local_addr().expect("an address").port();
        let base = format!("http://127.0.0.1:{port}");
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (root, intermediate, signer, tsa) = pki_world(&base, &plan);
        let chain = intermediate
            .iter()
            .map(|ca| ca.certificate.clone())
            .chain(std::iter::once(root.certificate.clone()))
            .collect();
        let world = std::sync::Arc::new(pki_world(&base, &plan));
        let seen = asked.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    return;
                };
                let world = world.clone();
                let seen = seen.clone();
                std::thread::spawn(move || answer(stream, &world, &plan, &seen));
            }
        });
        Pki {
            base,
            root,
            intermediate,
            signer,
            chain,
            tsa,
            asked,
        }
    }

    /// The paths asked so far, in order.
    ///
    /// # Panics
    ///
    /// A server thread panicked while holding the record.
    #[must_use]
    pub fn paths(&self) -> Vec<String> {
        self.asked
            .lock()
            .expect("the record")
            .iter()
            .map(|(path, _)| path.clone())
            .collect()
    }
}

/// Reads one request and answers it as `plan` says for its path.
fn answer(
    mut stream: std::net::TcpStream,
    world: &(TestCa, Option<TestCa>, Issued, TestTsa),
    plan: &Plan,
    seen: &std::sync::Mutex<Vec<(String, Vec<u8>)>>,
) {
    use std::io::{BufRead as _, Read as _, Write as _};
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let mut reader = std::io::BufReader::new(clone);
    let mut first = String::new();
    if reader.read_line(&mut first).unwrap_or(0) == 0 {
        return;
    }
    let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    if let Ok(mut record) = seen.lock() {
        record.push((path.clone(), body));
    }

    let (root, intermediate, signer, tsa) = world;
    let tsa_root = TestCa::of_tsa();
    let signer_issuer = intermediate.as_ref().unwrap_or(root);
    let intermediate_certificate = intermediate
        .as_ref()
        .map(|ca| ca.certificate.clone())
        .unwrap_or_default();
    let (serve, ocsp, subject, issuer): (Option<Serve>, bool, &[u8], &TestCa) = match path.as_str()
    {
        "/ocsp/signer" => (plan.signer_ocsp, true, &signer.certificate, signer_issuer),
        "/crl/signer.crl" => (plan.signer_crl, false, &signer.certificate, signer_issuer),
        "/ocsp/intermediate" => (
            plan.intermediate_ocsp,
            true,
            &intermediate_certificate,
            root,
        ),
        "/crl/intermediate.crl" => (
            plan.intermediate_crl,
            false,
            &intermediate_certificate,
            root,
        ),
        "/ocsp/authority" => (plan.authority_ocsp, true, &tsa.certificate, &tsa_root),
        "/crl/authority.crl" => (plan.authority_crl, false, &tsa.certificate, &tsa_root),
        _ => (Some(Serve::Status(404)), true, &[], root),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    const DAY: u64 = 86_400;
    let fresh = (now - 3_600, Some(now + 7 * DAY));
    let mint = |status: Status, (this, next): (u64, Option<u64>), corrupt: bool| {
        if ocsp {
            mint_ocsp(
                subject,
                issuer,
                status,
                this,
                next,
                Responder::Issuer,
                &OcspFaults {
                    corrupt_signature: corrupt,
                    ..OcspFaults::default()
                },
            )
        } else {
            let listed = [Listed {
                certificate: subject,
                at: now - DAY,
                reason: Some(1),
            }];
            let revoked = matches!(status, Status::Revoked { .. });
            mint_crl(
                issuer,
                if revoked { &listed } else { &[] },
                this,
                next,
                &CrlFaults {
                    corrupt_signature: corrupt,
                    ..CrlFaults::default()
                },
            )
        }
    };
    let revoked = Status::Revoked {
        at: now - DAY,
        reason: Some(1),
    };
    let reply: Result<Vec<u8>, u16> = match serve.unwrap_or(Serve::Status(404)) {
        Serve::Good => Ok(mint(Status::Good, fresh, false)),
        Serve::Revoked => Ok(mint(revoked, fresh, false)),
        Serve::Unknown => Ok(mint(Status::Unknown, fresh, false)),
        Serve::CorruptSignature => Ok(mint(Status::Good, fresh, true)),
        Serve::Stale => Ok(mint(
            Status::Good,
            (now - 10 * DAY, Some(now - 3 * DAY)),
            false,
        )),
        // OCSPResponse { responseStatus tryLater (3) }.
        Serve::TryLater if ocsp => Ok(vec![0x30, 0x03, 0x0a, 0x01, 0x03]),
        Serve::TryLater => Err(503),
        Serve::Status(code) => Err(code),
        Serve::Long => Ok(vec![0x30; if ocsp { 70 * 1024 } else { 9 << 20 }]),
        Serve::Silent => {
            std::thread::sleep(Duration::from_secs(10));
            return;
        }
        Serve::Garbage => Ok(b"this is not DER".to_vec()),
        Serve::Bytes(n) => Ok(vec![0x30; n]),
    };
    let kind = if ocsp {
        "application/ocsp-response"
    } else {
        "application/pkix-crl"
    };
    let out = match reply {
        Ok(body) => {
            let mut out = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            out.extend(body);
            out
        }
        Err(code) => {
            format!("HTTP/1.1 {code} Nope\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .into_bytes()
        }
    };
    let _ = stream.write_all(&out);
}
