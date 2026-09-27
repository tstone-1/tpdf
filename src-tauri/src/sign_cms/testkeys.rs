//! Software keys and certificates for the signing tests, and nothing else.
//!
//! **Test-only, and that is the point.** The shipped path never holds a
//! private key: `keystore.rs` asks the OS. These let every `cargo test` run
//! the whole of the split --- the worker's revision, the CMS, the splice, the
//! verifier --- without an OS store, which a gate on a hosted runner does not
//! have. They implement the same [`Key`] the OS keys do, so what they exercise
//! is the production path with one piece swapped.
//!
//! Certificates are built here from `x509-cert`'s types and signed by the same
//! keys, so a test can choose exactly the validity, key usage and issuer it
//! needs --- the combinations no fixture on disk has.

use std::str::FromStr as _;
use std::sync::OnceLock;

use der::asn1::{BitString, GeneralizedTime, ObjectIdentifier, OctetString};
use der::Encode;
use ecdsa::signature::hazmat::PrehashSigner as _;
use rand_chacha::rand_core::SeedableRng as _;
use sha2_10::Digest as _;
use x509_cert::certificate::{TbsCertificate, Version};
use x509_cert::ext::pkix::{KeyUsage, KeyUsages};
use x509_cert::ext::Extension;
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};
use x509_cert::time::{Time, Validity};
use x509_cert::Certificate;

use super::{Key, KeyKind};

/// A private key in this process, which only a test may have.
pub enum Soft {
    Rsa(Box<rsa::RsaPrivateKey>),
    P256(p256::ecdsa::SigningKey),
    P384(p384::ecdsa::SigningKey),
}

impl Soft {
    /// The one RSA-2048 key, generated once per test binary from a fixed seed.
    ///
    /// Once, because generating it in a debug build costs seconds and a dozen
    /// tests want one; fixed, so a failure reproduces.
    pub fn rsa() -> Soft {
        static KEY: OnceLock<rsa::RsaPrivateKey> = OnceLock::new();
        let key = KEY.get_or_init(|| {
            let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(0x7064_6673);
            rsa::RsaPrivateKey::new(&mut rng, 2048).expect("an RSA key")
        });
        Soft::Rsa(Box::new(key.clone()))
    }

    /// An RSA key of `bits`, for the test that refuses a small one.
    pub fn rsa_of(bits: usize) -> Soft {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(bits as u64);
        Soft::Rsa(Box::new(
            rsa::RsaPrivateKey::new(&mut rng, bits).expect("an RSA key"),
        ))
    }

    /// A P-256 key whose scalar is `seed` repeated. Deterministic and valid
    /// for any nonzero seed.
    pub fn p256(seed: u8) -> Soft {
        Soft::P256(p256::ecdsa::SigningKey::from_bytes(&[seed; 32].into()).expect("a scalar"))
    }

    /// A P-384 key whose scalar is `seed` repeated.
    pub fn p384(seed: u8) -> Soft {
        Soft::P384(p384::ecdsa::SigningKey::from_bytes(&[seed; 48].into()).expect("a scalar"))
    }

    /// The kind `sign_cms::key_kind` reads off this key's certificate.
    pub fn kind(&self) -> KeyKind {
        match self {
            Soft::Rsa(key) => KeyKind::Rsa(rsa::traits::PublicKeyParts::n(key.as_ref()).bits()),
            Soft::P256(_) => KeyKind::P256,
            Soft::P384(_) => KeyKind::P384,
        }
    }

    /// The public half, as a certificate carries it.
    pub fn spki(&self) -> SubjectPublicKeyInfoOwned {
        match self {
            Soft::Rsa(key) => {
                use rsa::traits::PublicKeyParts as _;
                let n = key.n().to_bytes_be();
                let e = key.e().to_bytes_be();
                let public = rsa::pkcs1::RsaPublicKey {
                    modulus: rsa::pkcs1::UintRef::new(&n).expect("n"),
                    public_exponent: rsa::pkcs1::UintRef::new(&e).expect("e"),
                };
                SubjectPublicKeyInfoOwned {
                    algorithm: AlgorithmIdentifierOwned {
                        oid: ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1"),
                        parameters: Some(der::asn1::Null.into()),
                    },
                    subject_public_key: BitString::from_bytes(&public.to_der().expect("der"))
                        .expect("bits"),
                }
            }
            Soft::P256(key) => ec_spki(
                "1.2.840.10045.3.1.7",
                key.verifying_key().to_encoded_point(false).as_bytes(),
            ),
            Soft::P384(key) => ec_spki(
                "1.3.132.0.34",
                key.verifying_key().to_encoded_point(false).as_bytes(),
            ),
        }
    }
}

/// An RSA `SubjectPublicKeyInfo` around a PKCS#1 `RSAPublicKey`, which is how
/// `SecKeyCopyExternalRepresentation` hands one back.
#[allow(dead_code)] // macOS only
pub fn rsa_spki(pkcs1: &[u8]) -> SubjectPublicKeyInfoOwned {
    SubjectPublicKeyInfoOwned {
        algorithm: AlgorithmIdentifierOwned {
            oid: ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1"),
            parameters: Some(der::asn1::Null.into()),
        },
        subject_public_key: BitString::from_bytes(pkcs1).expect("bits"),
    }
}

pub fn ec_spki(curve: &str, point: &[u8]) -> SubjectPublicKeyInfoOwned {
    SubjectPublicKeyInfoOwned {
        algorithm: AlgorithmIdentifierOwned {
            oid: ObjectIdentifier::new_unwrap("1.2.840.10045.2.1"),
            parameters: Some(
                der::Any::encode_from(&ObjectIdentifier::new_unwrap(curve)).expect("curve"),
            ),
        },
        subject_public_key: BitString::from_bytes(point).expect("bits"),
    }
}

impl Key for Soft {
    fn sign_digest(&self, _kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        match self {
            Soft::Rsa(key) => key
                .sign(rsa::Pkcs1v15Sign::new::<sha2_10::Sha256>(), digest)
                .map_err(|e| e.to_string()),
            Soft::P256(key) => {
                let signature: p256::ecdsa::Signature =
                    key.sign_prehash(digest).map_err(|e| e.to_string())?;
                Ok(signature.to_der().as_bytes().to_vec())
            }
            Soft::P384(key) => {
                let signature: p384::ecdsa::Signature =
                    key.sign_prehash(digest).map_err(|e| e.to_string())?;
                Ok(signature.to_der().as_bytes().to_vec())
            }
        }
    }
}

/// A key that signs something other than the digest it is handed.
///
/// The control for "a signature over the wrong digest is broken": it is a real
/// key with a real certificate, and only the value is wrong.
pub struct Misdirected(pub Soft);

impl Key for Misdirected {
    fn sign_digest(&self, kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        let mut other = *digest;
        other[0] ^= 0x01;
        self.0.sign_digest(kind, &other)
    }
}

/// A key the OS refused: the reader cancelled, the card was pulled.
pub struct Refusing;

impl Key for Refusing {
    fn sign_digest(&self, _kind: KeyKind, _digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        Err("the reader cancelled the PIN prompt".into())
    }
}

/// What a test wants of a certificate. Defaults to a year around now, no key
/// usage stated, self-issued.
pub struct Spec<'a> {
    pub subject: &'a str,
    pub not_before: u64,
    pub not_after: u64,
    pub usage: Option<Vec<KeyUsages>>,
    /// Extended key usages as dotted OIDs; `None` states none.
    pub purposes: Option<Vec<&'a str>>,
    /// The issuer's key and name; `None` issues it to itself.
    pub issuer: Option<(&'a Soft, &'a str)>,
    pub serial: u8,
    /// Basic constraints' `cA`; `None` states no basic constraints.
    pub authority: Option<bool>,
}

/// Seconds since the epoch of a fixed "now" the tests agree on: 2026-09-26.
pub const NOW: u64 = 1_790_380_800;

impl<'a> Spec<'a> {
    pub fn new(subject: &'a str) -> Self {
        Spec {
            subject,
            not_before: NOW - 86_400 * 180,
            not_after: NOW + 86_400 * 180,
            usage: None,
            purposes: None,
            issuer: None,
            serial: 1,
            authority: None,
        }
    }
}

fn time(seconds: u64) -> Time {
    Time::GeneralTime(
        GeneralizedTime::from_unix_duration(std::time::Duration::from_secs(seconds))
            .expect("a time"),
    )
}

/// A certificate for `key` as `spec` describes, DER.
pub fn certificate(key: &Soft, spec: &Spec<'_>) -> Vec<u8> {
    let (signer, issuer) = spec.issuer.unwrap_or((key, spec.subject));
    certificate_of(key.spki(), spec, signer, signer.kind(), issuer)
}

/// A certificate for the public key `spki`, signed by `signer` --- which can be
/// a key in no process at all, such as one the OS holds.
pub fn certificate_of(
    spki: SubjectPublicKeyInfoOwned,
    spec: &Spec<'_>,
    signer: &dyn Key,
    kind: KeyKind,
    issuer: &str,
) -> Vec<u8> {
    let mut stated: Vec<Extension> = Vec::new();
    if let Some(usages) = &spec.usage {
        let flags = usages
            .iter()
            .fold(flagset::FlagSet::<KeyUsages>::default(), |set, usage| {
                set | *usage
            });
        stated.push(Extension {
            extn_id: ObjectIdentifier::new_unwrap("2.5.29.15"),
            critical: true,
            extn_value: OctetString::new(KeyUsage(flags).to_der().expect("usage")).expect("octets"),
        });
    }
    if let Some(purposes) = &spec.purposes {
        let usage = x509_cert::ext::pkix::ExtendedKeyUsage(
            purposes
                .iter()
                .map(|oid| ObjectIdentifier::new_unwrap(oid))
                .collect(),
        );
        stated.push(Extension {
            extn_id: ObjectIdentifier::new_unwrap("2.5.29.37"),
            critical: true,
            extn_value: OctetString::new(usage.to_der().expect("purposes")).expect("octets"),
        });
    }
    if let Some(ca) = spec.authority {
        let constraints = x509_cert::ext::pkix::BasicConstraints {
            ca,
            path_len_constraint: None,
        };
        stated.push(Extension {
            extn_id: ObjectIdentifier::new_unwrap("2.5.29.19"),
            critical: true,
            extn_value: OctetString::new(constraints.to_der().expect("constraints"))
                .expect("octets"),
        });
    }
    let extensions = (!stated.is_empty()).then_some(stated);
    let tbs = TbsCertificate {
        version: Version::V3,
        serial_number: SerialNumber::new(&[spec.serial]).expect("serial"),
        signature: kind.algorithm(),
        issuer: Name::from_str(&format!("CN={issuer}")).expect("issuer"),
        validity: Validity {
            not_before: time(spec.not_before),
            not_after: time(spec.not_after),
        },
        subject: Name::from_str(&format!("CN={}", spec.subject)).expect("subject"),
        subject_public_key_info: spki,
        issuer_unique_id: None,
        subject_unique_id: None,
        extensions,
    };
    let digest: [u8; 32] = sha2_10::Sha256::digest(tbs.to_der().expect("tbs")).into();
    let value = signer.sign_digest(kind, &digest).expect("signed");
    Certificate {
        tbs_certificate: tbs,
        signature_algorithm: kind.algorithm(),
        signature: BitString::from_bytes(&value).expect("bits"),
    }
    .to_der()
    .expect("certificate")
}

/// A one-page document with no form and no annotations, saved by `lopdf`.
pub fn plain_pdf() -> Vec<u8> {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let content = doc.add_object(Stream::new(dictionary! {}, b"0 0 m 10 10 l S".to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page.into()],
            "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("saved");
    bytes
}

/// Prepares `bytes` in-process and finishes it with `key` under `certificate`.
pub fn signed(
    bytes: &[u8],
    certificate: &[u8],
    chain: &[Vec<u8>],
    key: &dyn Key,
) -> Result<Vec<u8>, String> {
    let unsigned = crate::sign_prepare::prepare(bytes.to_vec(), NOW, None)?;
    super::finish(bytes.to_vec(), unsigned, certificate, chain, key)
}

/// Every signed field's verdict in `bytes`, as the properties dialog reads it.
pub fn verdicts(bytes: &[u8]) -> Vec<(String, crate::integrity::Integrity)> {
    crate::docinfo::scan(bytes, 1, None)
        .expect("the signed file parses")
        .signatures
        .into_iter()
        .filter(|s| s.signed)
        .map(|s| (s.field, s.integrity.expect("a verdict")))
        .collect()
}
