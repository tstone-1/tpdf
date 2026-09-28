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
        certificates: Some(
            CertificateSet::try_from(vec![CertificateChoices::Certificate(authority)])
                .expect("certificates"),
        ),
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

/// A certificate for `spec.key`, signed by `spec.signer` with ECDSA SHA-256.
fn certificate(spec: &Spec<'_>) -> Vec<u8> {
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
            not_before: time(FROM),
            not_after: time(UNTIL),
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
