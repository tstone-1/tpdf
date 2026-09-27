//! The app process's half of signing: the CMS, the OS's signature, the splice.
//!
//! ## Why this half never sees a PDF object
//!
//! `sign_prepare.rs` builds the revision inside the worker, which is where the
//! document's bytes are parsed, and hands back an update section, a
//! `/ByteRange` and a digest. **Nothing here parses the document.** What this
//! module reads is: the bytes of the file (to hash them, and to check the
//! numbers it was given against them), the reader's own certificates from the
//! OS store, and what the OS signs. The hash is recomputed here, over the bytes
//! that will be written, before the worker's is believed --- so a worker that
//! described one document and built another cannot make the reader sign the
//! wrong one.
//!
//! ## What is signed
//!
//! PAdES baseline B-B (ETSI EN 319 142-1): a detached CMS `SignedData` over the
//! two covered pieces, SHA-256, and three signed attributes --- `contentType`
//! (`data`), `messageDigest`, and ESS `signingCertificateV2` binding the
//! signature to the signer's certificate. No `signingTime`: B-B puts the time in
//! the signature dictionary's `/M`, which `sign_prepare.rs` writes. The
//! `certificates` set carries the signer's certificate and whatever the OS chain
//! API returned above it.
//!
//! The key signs `SHA-256(DER(signed attributes))` and nothing else. RSA keys
//! sign PKCS#1 v1.5; ECDSA keys on P-256 or P-384 sign ECDSA-with-SHA256. Every
//! other key is refused by [`usable`] before a reader is offered it.
//!
//! ## Refusing to write what tpdf itself would call broken
//!
//! [`finish`] ends by running `integrity::check` --- the same function the
//! properties dialog's verdict comes from --- over the whole file it is about
//! to hand back, and refuses unless the answer is `Intact`. A key that does not
//! match its certificate, an OS that signed the wrong thing, a splice at the
//! wrong offset: each produces a file that would open, show a signature, and
//! read as broken, and none of them gets written.

use std::cell::RefCell;

use cms::builder::{SignedDataBuilder, SignerInfoBuilder};
use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::signed_data::{EncapsulatedContentInfo, SignerIdentifier};
use der::asn1::{ObjectIdentifier, OctetString};
use der::{Decode, Encode, Sequence};
use sha2_10::Digest as _;
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::AlgorithmIdentifierOwned;
use x509_cert::Certificate;

use crate::sign_prepare::{Unsigned, RESERVED, STEP_TWO_LIMIT};

/// id-data.
const ID_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
/// id-sha256.
const ID_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");
/// sha256WithRSAEncryption.
const SHA256_WITH_RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.11");
/// ecdsa-with-SHA256.
const ECDSA_WITH_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.2");
/// id-aa-signingCertificateV2, RFC 5035.
const SIGNING_CERTIFICATE_V2: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.2.47");

/// The smallest RSA modulus tpdf signs with, in bits.
///
/// 2048 is what every current policy (ETSI TS 119 312, NIST SP 800-57) asks of
/// a new signature. A smaller key still verifies, and `integrity.rs` checks
/// one; it is only *making* one that is refused.
const MIN_RSA_BITS: usize = 2048;

/// One certificate the chooser may offer, with the name the reader picks it by.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Choice {
    /// `keystore::id_of` its certificate: what comes back when it is picked.
    pub id: String,
    /// The subject's common name, or its whole name when it has none.
    pub subject: String,
    /// The issuer's common name, or its whole name when it has none.
    pub issuer: String,
    /// When it stops being valid, as `YYYY-MM-DD HH:MM:SS UTC`.
    pub expires: String,
    /// `RSA 3072`, `ECDSA P-256`.
    pub method: String,
}

/// A certificate with a key that is not offered, and why.
///
/// Listed rather than dropped: a reader whose card holds only an expired
/// certificate is otherwise told they have none, which sends them looking for
/// the wrong problem.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Skipped {
    /// Who the certificate names, as far as it could be read.
    pub subject: String,
    /// Why it is not offered, as a clause: "it has expired".
    pub why: String,
}

/// What the chooser shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Choices {
    /// The certificates a reader may sign with, in the store's order.
    pub usable: Vec<Choice>,
    /// The ones with a key that may not, each with its reason.
    pub skipped: Vec<Skipped>,
}

/// Sorts certificates into the ones that may sign at `now` and the ones that
/// may not. `found` is `(id, certificate DER)` for each, as the store lists
/// them.
#[must_use]
pub fn choices(found: &[(String, Vec<u8>)], now: u64) -> Choices {
    let mut out = Choices::default();
    for (id, der) in found {
        match usable(der, now) {
            Ok(offer) => out.usable.push(Choice {
                id: id.clone(),
                subject: offer.subject,
                issuer: offer.issuer,
                expires: offer.expires,
                method: offer.method,
            }),
            Err(why) => out.skipped.push(Skipped {
                subject: Certificate::from_der(der)
                    .map(|c| crate::docinfo::common_name(&c.tbs_certificate.subject))
                    .ok()
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "(a certificate with no readable name)".into()),
                why,
            }),
        }
    }
    out
}

/// One signature in the file just written, as the worker's verifier read it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Checked {
    /// The field's name.
    pub field: String,
    /// What `integrity.rs` found. `None` for a field nobody has signed.
    pub integrity: Option<crate::integrity::Integrity>,
    /// Whether this is the signature just made.
    pub ours: bool,
}

/// What signing reports: the new field, and every signature the written file
/// holds, checked after writing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Signed {
    /// Where the signed copy was written.
    pub path: String,
    /// The new signature's field.
    pub field: String,
    /// Every signed field in the written file, in the file's order.
    pub signatures: Vec<Checked>,
}

/// The report for a file written with a new signature in `field`.
///
/// Unsigned fields are left out: an empty signature field is not a signature,
/// and listing one would read as a signature that failed.
#[must_use]
pub fn report(path: String, field: String, found: Vec<crate::docinfo::Signature>) -> Signed {
    let signatures = found
        .into_iter()
        .filter(|signature| signature.signed)
        .map(|signature| Checked {
            ours: signature.field == field,
            field: signature.field,
            integrity: signature.integrity,
        })
        .collect();
    Signed {
        path,
        field,
        signatures,
    }
}

/// Refuses to sign a document with edits nobody has saved.
///
/// A signature is over the **file**, and the reader is looking at the file
/// plus their edits. Signing either one would sign something other than what
/// they see, so the answer is to save first.
///
/// # Errors
///
/// `dirty` is true.
pub fn refuse_unsaved(dirty: bool) -> Result<(), String> {
    if dirty {
        return Err("Save your changes first: a signature covers the file as it is on disk,                     and this document has edits that are not in it yet."
            .into());
    }
    Ok(())
}

/// Refuses a document too large for the worker to prepare a revision of.
///
/// The same bound a marks-only save appends under, `save::APPEND_MAX_BYTES`,
/// and for its reason: the revision is built by re-parsing the document with
/// `lopdf` in the worker, at about three times the file's size in memory, and
/// a Windows worker's commit cap is 1 GiB. Past it the worker would die rather
/// than refuse; this says so first.
///
/// # Errors
///
/// `len` is over the bound.
pub fn refuse_too_large(len: u64) -> Result<(), String> {
    if len > crate::save::APPEND_MAX_BYTES {
        return Err(format!(
            "This document is {} MB, and tpdf signs documents of up to {} MB.",
            len / 1_000_000,
            crate::save::APPEND_MAX_BYTES / 1_000_000
        ));
    }
    Ok(())
}

/// What kind of key a certificate holds, which decides how it signs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// RSA, PKCS#1 v1.5, of this many bits.
    Rsa(usize),
    /// ECDSA on NIST P-256.
    P256,
    /// ECDSA on NIST P-384.
    P384,
}

impl KeyKind {
    /// As a reader would name it: `RSA 3072`, `ECDSA P-256`.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            KeyKind::Rsa(bits) => format!("RSA {bits}"),
            KeyKind::P256 => "ECDSA P-256".into(),
            KeyKind::P384 => "ECDSA P-384".into(),
        }
    }

    /// The `SignerInfo.signatureAlgorithm` for this key over SHA-256.
    fn algorithm(self) -> AlgorithmIdentifierOwned {
        match self {
            // RFC 4055 §5: the parameters are present and NULL.
            KeyKind::Rsa(_) => AlgorithmIdentifierOwned {
                oid: SHA256_WITH_RSA,
                parameters: Some(der::asn1::Null.into()),
            },
            // RFC 5758 §3.2: the parameters are absent.
            KeyKind::P256 | KeyKind::P384 => AlgorithmIdentifierOwned {
                oid: ECDSA_WITH_SHA256,
                parameters: None,
            },
        }
    }
}

/// A private key somebody else holds, asked to sign one SHA-256 digest.
///
/// **The only thing this process ever asks of a key**, and the shape is chosen
/// so that nothing more can be asked: there is no way to read the key, only to
/// have a digest signed. The OS implementations are `keystore.rs`; the tests'
/// is a software key.
pub trait Key {
    /// Signs `digest`, which is already SHA-256.
    ///
    /// For [`KeyKind::Rsa`] the answer is a PKCS#1 v1.5 signature over the
    /// SHA-256 `DigestInfo`; for the curves it is a DER `ECDSA-Sig-Value`.
    ///
    /// # Errors
    ///
    /// The OS refused, the reader cancelled its prompt, or the key is gone.
    fn sign_digest(&self, kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String>;
}

/// A certificate the reader could sign with, as the chooser shows it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Offer {
    /// The subject's common name, or its whole name when it has none.
    pub subject: String,
    /// The issuer's common name, or its whole name when it has none.
    pub issuer: String,
    /// When it stops being valid, as `YYYY-MM-DD HH:MM:SS UTC`.
    pub expires: String,
    /// `RSA 3072`, `ECDSA P-256`.
    pub method: String,
}

/// What a certificate's key is, from its `SubjectPublicKeyInfo`.
///
/// # Errors
///
/// A key that is not RSA of at least [`MIN_RSA_BITS`], P-256 or P-384.
pub fn key_kind(certificate: &Certificate) -> Result<KeyKind, String> {
    let spki = &certificate.tbs_certificate.subject_public_key_info;
    match spki.algorithm.oid.to_string().as_str() {
        "1.2.840.113549.1.1.1" => {
            let bits = spki
                .subject_public_key
                .as_bytes()
                .and_then(|bits| rsa::pkcs1::RsaPublicKey::from_der(bits).ok())
                .map(|key| {
                    let modulus = key.modulus.as_bytes();
                    let leading = modulus.iter().take_while(|b| **b == 0).count();
                    let significant = &modulus[leading..];
                    significant.first().map_or(0, |top| {
                        (significant.len() - 1) * 8 + (8 - top.leading_zeros() as usize)
                    })
                })
                .ok_or("its RSA key could not be read")?;
            if bits < MIN_RSA_BITS {
                return Err(format!(
                    "its RSA key has {bits} bits, and tpdf signs only with {MIN_RSA_BITS} or more"
                ));
            }
            Ok(KeyKind::Rsa(bits))
        }
        "1.2.840.10045.2.1" => {
            let curve = spki
                .algorithm
                .parameters
                .as_ref()
                .and_then(|p| p.decode_as::<ObjectIdentifier>().ok())
                .map(|oid| oid.to_string());
            match curve.as_deref() {
                Some("1.2.840.10045.3.1.7") => Ok(KeyKind::P256),
                Some("1.3.132.0.34") => Ok(KeyKind::P384),
                _ => Err("its key is on an elliptic curve tpdf does not sign with".into()),
            }
        }
        _ => Err("its key is of a kind tpdf does not sign with (only RSA and ECDSA)".into()),
    }
}

/// Whether a certificate may be offered for signing at `now`, and how it reads.
///
/// Four things are asked, all of the certificate and none of the document:
/// its key is one [`key_kind`] accepts; `now` is inside its validity; its
/// key usage, when it states one, includes `digitalSignature` or
/// `nonRepudiation`; and its extended key usage, when it states one, names a
/// purpose a document signature can serve ([`DOCUMENT_PURPOSES`]). The second
/// key usage is accepted on purpose: qualified signature cards commonly carry
/// `nonRepudiation` alone (it is `contentCommitment` in RFC 5280's later name),
/// and it is a signing usage. A certificate that states no key usage, or no
/// extended key usage, restricts none.
///
/// **The extended key usage is the question the key usage cannot answer.** An
/// Apple *Developer ID Application* certificate states `digitalSignature` and,
/// critically, code signing and nothing else --- measured 2026-09-26 on the
/// only identity in the owner's keychain, which the first version of this
/// function offered for signing documents. The key usage says the key signs;
/// the extended key usage says what it was issued to sign.
///
/// # Errors
///
/// The reason it is not offered, as a clause a reader can be shown.
pub fn usable(certificate_der: &[u8], now: u64) -> Result<Offer, String> {
    let certificate =
        Certificate::from_der(certificate_der).map_err(|_| "it could not be read".to_string())?;
    let kind = key_kind(&certificate)?;
    let validity = &certificate.tbs_certificate.validity;
    let (from, until) = (
        validity.not_before.to_unix_duration().as_secs(),
        validity.not_after.to_unix_duration().as_secs(),
    );
    if now < from {
        return Err("it is not valid yet".into());
    }
    if now > until {
        return Err("it has expired".into());
    }
    if let Some(usage) = key_usage(&certificate)? {
        if !usage.digital_signature() && !usage.non_repudiation() {
            return Err("it is not issued for signing".into());
        }
    }
    if let Some(purposes) = extended_key_usage(&certificate)? {
        if !purposes
            .iter()
            .any(|purpose| DOCUMENT_PURPOSES.contains(&purpose.as_str()))
        {
            return Err(format!(
                "it is issued for {}, not for signing documents",
                purposes_named(&purposes)
            ));
        }
    }
    let name = |name: &x509_cert::name::Name| {
        let common = crate::docinfo::common_name(name);
        if common.is_empty() {
            crate::docinfo::distinguished_name(name)
        } else {
            common
        }
    };
    Ok(Offer {
        subject: name(&certificate.tbs_certificate.subject),
        issuer: name(&certificate.tbs_certificate.issuer),
        expires: crate::docinfo::certificate_date(&validity.not_after),
        method: kind.name(),
    })
}

/// Extended key usages under which a certificate may sign a document.
///
/// `anyExtendedKeyUsage`; e-mail protection, which is what a personal S/MIME
/// certificate --- the common certificate a person holds --- is issued for;
/// RFC 9336's document signing; Microsoft's document signing; and Adobe's
/// Authentic Documents Trust. Server, client-login, code-signing and
/// timestamping certificates are not among them: each was issued to sign
/// something that is not a document.
pub const DOCUMENT_PURPOSES: [&str; 5] = [
    "2.5.29.37.0",
    "1.3.6.1.5.5.7.3.4",
    "1.3.6.1.5.5.7.3.36",
    "1.3.6.1.4.1.311.10.3.12",
    "1.2.840.113583.1.1.5",
];

/// The purposes a refused certificate was issued for, as a reader reads them.
fn purposes_named(purposes: &[String]) -> String {
    let mut named: Vec<&str> = Vec::new();
    for purpose in purposes {
        let name = match purpose.as_str() {
            "1.3.6.1.5.5.7.3.1" => "web servers",
            "1.3.6.1.5.5.7.3.2" => "logging in",
            "1.3.6.1.5.5.7.3.3" => "code signing",
            "1.3.6.1.5.5.7.3.8" => "timestamping",
            "1.3.6.1.5.5.7.3.9" => "revocation responses",
            _ => "other purposes",
        };
        if !named.contains(&name) {
            named.push(name);
        }
    }
    named.join(" and ")
}

/// The extended key usages the certificate states, as dotted OIDs, or `None`
/// when it states none.
fn extended_key_usage(certificate: &Certificate) -> Result<Option<Vec<String>>, String> {
    let Some(extensions) = &certificate.tbs_certificate.extensions else {
        return Ok(None);
    };
    let Some(extension) = extensions
        .iter()
        .find(|e| e.extn_id.to_string() == "2.5.29.37")
    else {
        return Ok(None);
    };
    x509_cert::ext::pkix::ExtendedKeyUsage::from_der(extension.extn_value.as_bytes())
        .map(|usage| Some(usage.0.iter().map(ToString::to_string).collect()))
        .map_err(|_| "its extended key usage could not be read".to_string())
}

/// The key usage the certificate states, or `None` when it states none.
fn key_usage(certificate: &Certificate) -> Result<Option<x509_cert::ext::pkix::KeyUsage>, String> {
    let Some(extensions) = &certificate.tbs_certificate.extensions else {
        return Ok(None);
    };
    let Some(extension) = extensions
        .iter()
        .find(|e| e.extn_id.to_string() == "2.5.29.15")
    else {
        return Ok(None);
    };
    x509_cert::ext::pkix::KeyUsage::from_der(extension.extn_value.as_bytes())
        .map(Some)
        .map_err(|_| "its key usage could not be read".to_string())
}

/// ESS `IssuerSerial`, RFC 5035.
#[derive(Clone, Debug, Eq, PartialEq, Sequence)]
struct IssuerSerial {
    issuer: Vec<GeneralName>,
    serial_number: SerialNumber,
}

/// ESS `ESSCertIDv2`. `hashAlgorithm` is left out, which in DER is how its
/// default --- SHA-256 --- is written.
#[derive(Clone, Debug, Eq, PartialEq, Sequence)]
struct EssCertIdV2 {
    cert_hash: OctetString,
    issuer_serial: IssuerSerial,
}

/// ESS `SigningCertificateV2`, with no policies.
#[derive(Clone, Debug, Eq, PartialEq, Sequence)]
struct SigningCertificateV2 {
    certs: Vec<EssCertIdV2>,
}

/// The `signingCertificateV2` attribute naming `certificate` by hash and by
/// issuer and serial.
///
/// **This is what binds the signature to one certificate**, rather than to any
/// certificate carrying the same key: without it a signature could be
/// re-presented under a second certificate for the same key pair, which PAdES
/// B-B exists to rule out.
fn signing_certificate(
    certificate: &Certificate,
    der: &[u8],
) -> Result<x509_cert::attr::Attribute, String> {
    let value = SigningCertificateV2 {
        certs: vec![EssCertIdV2 {
            cert_hash: OctetString::new(sha2_10::Sha256::digest(der).to_vec())
                .map_err(|e| e.to_string())?,
            issuer_serial: IssuerSerial {
                issuer: vec![GeneralName::DirectoryName(
                    certificate.tbs_certificate.issuer.clone(),
                )],
                serial_number: certificate.tbs_certificate.serial_number.clone(),
            },
        }],
    };
    let any = der::Any::encode_from(&value).map_err(|e| e.to_string())?;
    Ok(x509_cert::attr::Attribute {
        oid: SIGNING_CERTIFICATE_V2,
        values: der::asn1::SetOfVec::try_from(vec![any]).map_err(|e| e.to_string())?,
    })
}

/// What signs the signed attributes: a [`Key`], adapted to the `cms` builder.
///
/// The builder hands its signer the DER of the signed attributes and expects a
/// signature back; this hashes them and asks the key to sign the digest, which
/// is the only operation [`Key`] offers. The builder's own error type carries
/// no message, so the key's refusal --- "the reader cancelled", "the card was
/// removed" --- is kept here and reported instead of the builder's.
struct Signing<'a> {
    key: &'a dyn Key,
    kind: KeyKind,
    refused: RefCell<Option<String>>,
}

/// A signature value, as the key returned it.
struct Value(Vec<u8>);

impl x509_cert::spki::SignatureBitStringEncoding for Value {
    fn to_bitstring(&self) -> der::Result<der::asn1::BitString> {
        der::asn1::BitString::from_bytes(&self.0)
    }
}

impl signature::Keypair for Signing<'_> {
    // Nothing: the builder never asks for it, and there is no public key
    // here to give --- the certificate is where that lives.
    type VerifyingKey = ();
    fn verifying_key(&self) {}
}

impl x509_cert::spki::DynSignatureAlgorithmIdentifier for Signing<'_> {
    fn signature_algorithm_identifier(&self) -> x509_cert::spki::Result<AlgorithmIdentifierOwned> {
        Ok(self.kind.algorithm())
    }
}

impl signature::Signer<Value> for Signing<'_> {
    fn try_sign(&self, message: &[u8]) -> Result<Value, signature::Error> {
        let digest: [u8; 32] = sha2_10::Sha256::digest(message).into();
        self.key
            .sign_digest(self.kind, &digest)
            .map(Value)
            .map_err(|why| {
                *self.refused.borrow_mut() = Some(why);
                signature::Error::new()
            })
    }
}

/// The CMS `ContentInfo` for a detached signature over `digest`.
///
/// `certificate` is the signer's, DER; `chain` is what the OS returned above
/// it, in any order --- the set is sorted by its encoding either way.
///
/// # Errors
///
/// The certificate cannot be read or holds a key [`key_kind`] refuses; the key
/// refused to sign; or the encoding failed.
pub fn build(
    digest: &[u8],
    certificate: &[u8],
    chain: &[Vec<u8>],
    key: &dyn Key,
) -> Result<Vec<u8>, String> {
    let signer = Certificate::from_der(certificate)
        .map_err(|e| format!("the signing certificate could not be read: {e}"))?;
    let kind = key_kind(&signer).map_err(|why| format!("this certificate cannot sign: {why}"))?;
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
        key,
        kind,
        refused: RefCell::new(None),
    };
    let failed = |what: &str, e: &dyn std::fmt::Display| format!("{what}: {e}");

    let mut info =
        SignerInfoBuilder::new(&signing, sid, sha256.clone(), &encapsulated, Some(digest))
            .map_err(|e| failed("the signer could not be described", &e))?;
    info.add_signed_attribute(signing_certificate(&signer, certificate)?)
        .map_err(|e| failed("the signing certificate could not be named", &e))?;

    let mut data = SignedDataBuilder::new(&encapsulated);
    data.add_digest_algorithm(sha256)
        .map_err(|e| failed("the digest could not be named", &e))?;
    data.add_certificate(CertificateChoices::Certificate(signer.clone()))
        .map_err(|e| failed("the certificate could not be included", &e))?;
    for (at, der) in chain.iter().enumerate() {
        let issuer = Certificate::from_der(der)
            .map_err(|e| format!("certificate {} of the chain could not be read: {e}", at + 1))?;
        if issuer != signer {
            data.add_certificate(CertificateChoices::Certificate(issuer))
                .map_err(|e| failed("the chain could not be included", &e))?;
        }
    }
    if let Err(e) = data.add_signer_info::<Signing<'_>, Value>(info) {
        return Err(signing
            .refused
            .borrow_mut()
            .take()
            .unwrap_or_else(|| format!("the signature could not be made: {e}")));
    }
    data.build()
        .map_err(|e| failed("the signature could not be assembled", &e))?
        .to_der()
        .map_err(|e| failed("the signature could not be encoded", &e))
}

/// Converts an `r || s` ECDSA signature to the DER `ECDSA-Sig-Value` CMS wants.
///
/// Windows' `NCryptSignHash` answers in the raw form, two big-endian integers
/// of the curve's size end to end; `SecKeyCreateSignature` already answers in
/// DER. Each half becomes a DER `INTEGER`: leading zeros dropped, and a zero
/// put back in front when the top bit is set, which would otherwise make the
/// integer negative.
///
/// # Errors
///
/// An odd or empty input, which no curve produces.
pub fn ecdsa_der(raw: &[u8]) -> Result<Vec<u8>, String> {
    if raw.is_empty() || raw.len() % 2 != 0 {
        return Err(format!(
            "an ECDSA signature of {} bytes has no halves",
            raw.len()
        ));
    }
    let (r, s) = raw.split_at(raw.len() / 2);
    let integer = |half: &[u8]| -> Vec<u8> {
        let leading = half.iter().take_while(|b| **b == 0).count();
        let mut body = half[leading.min(half.len() - 1)..].to_vec();
        if body[0] & 0x80 != 0 {
            body.insert(0, 0);
        }
        let mut out = vec![0x02];
        out.extend(length(body.len()));
        out.extend(body);
        out
    };
    let (r, s) = (integer(r), integer(s));
    let mut out = vec![0x30];
    out.extend(length(r.len() + s.len()));
    out.extend(r);
    out.extend(s);
    Ok(out)
}

/// A DER length.
fn length(n: usize) -> Vec<u8> {
    if n < 0x80 {
        return vec![n as u8];
    }
    let bytes: Vec<u8> = n
        .to_be_bytes()
        .into_iter()
        .skip_while(|b| *b == 0)
        .collect();
    let mut out = vec![0x80 | bytes.len() as u8];
    out.extend(bytes);
    out
}

/// Checks the worker's numbers against the bytes this process will write.
///
/// `original` is the file as this process read it. The update must have been
/// built against exactly that many bytes; the range must be `[0, a, b, c]` with
/// `a` and `b` inside the update and `b + c` the end of the result; the hole
/// must be `<`, [`RESERVED`] zero bytes as hex, `>`; and the digest the worker
/// sent must be the SHA-256 this process computes over the two covered pieces.
///
/// # Errors
///
/// Each disagreement, named. Every one of them means the worker was describing
/// a different file from the one about to be signed.
pub fn check(original: &[u8], unsigned: &Unsigned) -> Result<[u8; 32], String> {
    let was = original.len();
    let changed = |what: &str| {
        format!(
            "the prepared signature {what}, so it was not built for the file on disk --- \
             reopen the document and sign again"
        )
    };
    if unsigned.built_against != was {
        return Err(changed(&format!(
            "was built against {} bytes and the file has {was}",
            unsigned.built_against
        )));
    }
    let end = was + unsigned.update.len();
    let [start, first, second, last] = unsigned
        .range
        .map(|n| usize::try_from(n).unwrap_or(usize::MAX));
    let shaped = start == 0
        && first >= was
        && second == first.saturating_add(RESERVED * 2 + 2)
        && second <= end
        && second.checked_add(last) == Some(end);
    if !shaped {
        return Err(changed("covers a range that does not frame its own value"));
    }
    let hole = &unsigned.update[first - was..second - was];
    let zeros = hole.first() == Some(&b'<')
        && hole.last() == Some(&b'>')
        && hole[1..hole.len() - 1].iter().all(|b| *b == b'0');
    if !zeros {
        return Err(changed(
            "has something other than an empty value in its hole",
        ));
    }
    let mut hasher = sha2_10::Sha256::new();
    hasher.update(original);
    hasher.update(&unsigned.update[..first - was]);
    hasher.update(&unsigned.update[second - was..]);
    let digest: [u8; 32] = hasher.finalize().into();
    if unsigned.digest != digest {
        return Err(changed(
            "states a digest that is not the one of its own bytes",
        ));
    }
    Ok(digest)
}

/// Writes `blob` into the hole as uppercase hex, leaving the rest zeros.
///
/// # Errors
///
/// The blob does not fit [`STEP_TWO_LIMIT`], which keeps half the hole free for
/// the timestamp token Phase 6 step 3 adds.
pub fn splice(
    update: &mut [u8],
    built_against: usize,
    range: [u64; 4],
    blob: &[u8],
) -> Result<(), String> {
    if blob.len() > STEP_TWO_LIMIT {
        return Err(format!(
            "the signature and its certificates take {} bytes, and tpdf keeps signatures \
             to {STEP_TWO_LIMIT} so a timestamp can be added later",
            blob.len()
        ));
    }
    let at = usize::try_from(range[1]).map_err(|e| e.to_string())? + 1 - built_against;
    for (index, byte) in blob.iter().enumerate() {
        let digits = format!("{byte:02X}");
        update[at + index * 2..at + index * 2 + 2].copy_from_slice(digits.as_bytes());
    }
    Ok(())
}

/// Signs a prepared revision and returns the whole signed file.
///
/// `original` is the file as read, and is extended in place into the result so
/// that a large document is not held twice. The steps, in the order the module
/// note gives: [`check`], [`build`], [`splice`], then `integrity::check` over
/// the finished bytes, which must say `Intact`.
///
/// # Errors
///
/// Everything [`check`], [`build`] and [`splice`] refuse, and tpdf's own
/// verifier not calling the result intact.
pub fn finish(
    original: Vec<u8>,
    unsigned: Unsigned,
    certificate: &[u8],
    chain: &[Vec<u8>],
    key: &dyn Key,
) -> Result<Vec<u8>, String> {
    let digest = check(&original, &unsigned)?;
    let blob = build(&digest, certificate, chain, key)?;
    let Unsigned {
        mut update,
        built_against,
        range,
        ..
    } = unsigned;
    splice(&mut update, built_against, range, &blob)?;

    let mut bytes = original;
    bytes.extend_from_slice(&update);
    let mut contents = blob.clone();
    contents.resize(RESERVED, 0);
    let numbers = range.map(|n| i64::try_from(n).unwrap_or(i64::MAX));
    let verdict = crate::integrity::check(
        &bytes,
        &numbers,
        &contents,
        Some(&blob),
        "ETSI.CAdES.detached",
        &mut crate::integrity::MAX_HASHED.clone(),
    );
    if verdict.verdict != crate::integrity::Verdict::Intact {
        return Err(format!(
            "tpdf's own check of the signature it just made did not find it intact \
             ({:?}{}), so nothing was written",
            verdict.verdict,
            verdict
                .why
                .map(|why| format!(", {why:?}"))
                .unwrap_or_default()
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
pub(crate) mod testkeys;

#[cfg(test)]
mod tests;
