//! Whether an RFC 3161 timestamp token is sound, and whether it covers what it
//! is attached to.
//!
//! ## What a token is, and the two places tpdf meets one
//!
//! A timestamp token is a timestamp authority's signed statement that a given
//! digest existed at a given time: a CMS `SignedData` whose encapsulated
//! content is a `TSTInfo` holding `genTime` and a `messageImprint`. tpdf meets
//! it in two places, and the only difference between them is what the
//! imprint is a digest **of**:
//!
//! - **on a signature**, as the unsigned attribute 1.2.840.113549.1.9.16.2.14
//!   on its `SignerInfo`. RFC 3161 Appendix A: the imprint is over **the value
//!   octets of that `SignerInfo`'s `signature`** --- not its DER, not the
//!   document. So the token says the signature existed then.
//! - **as a document timestamp**, a signature field whose `/SubFilter` is
//!   `ETSI.RFC3161` and whose `/Contents` *is* the token. The imprint is over
//!   the `/ByteRange` bytes, exactly as a detached signature's `messageDigest`
//!   is, and the range is vouched for by the same rule
//!   ([`super::covered`]) before anything is hashed.
//!
//! ## The verdict, in the integrity vocabulary
//!
//! [`check`] answers in [`Integrity`], so a token reads with the same five words
//! a signature does, and each means the analogous thing:
//!
//! - `intact` --- the token's own signature checks out under the key in the
//!   certificate it names, its `TSTInfo` is the one that key signed, it binds
//!   that certificate (ESS), and its imprint equals the digest of what it is
//!   attached to. Then, and only then, `genTime` is an attested time.
//! - `weak` --- all of that, with SHA-1 in the imprint or in the token's own
//!   signature. A collision there lets a token for one thing pass as a token
//!   for another, so the time is not shown to belong to *this* signature.
//! - `altered` --- the token itself is sound, and its imprint is of something
//!   else. It is a genuine timestamp **of different data**, so it attests
//!   nothing about this signature.
//! - `broken` --- the token's signature fails, **or** its `TSTInfo` does not
//!   hash to the digest its signer signed. Both mean the statement in hand is
//!   not the one the authority made, so nothing it states stands --- the time
//!   least of all. (For a signature the second case is `altered`, because the
//!   covered bytes are the document; for a token they are the statement.)
//! - `unchecked` --- with the reason: an unreadable token, an algorithm this
//!   does not carry, a certificate it does not name, or [`Why::Binding`].
//!
//! **The order is the rule `integrity.rs` states**: the token's signature is
//! tested before its `messageDigest` is believed, its `messageDigest` before
//! its `TSTInfo`, and its `TSTInfo` --- imprint and time --- only after both.
//! A token whose signature fails is `broken` whatever its imprint says.
//!
//! ## Why a missing binding is `unchecked`, and why it is tested last
//!
//! RFC 3161 §2.4.1 (and RFC 5816 for the SHA-2 form) requires the token to
//! carry an ESS `signingCertificate` or `signingCertificateV2` signed
//! attribute naming the authority's certificate by hash. Without it the
//! signature binds a **key**, not a certificate: anybody holding a second
//! certificate over the same key --- issued for another purpose, by another
//! issuer, under another name --- could present the token as that
//! certificate's. The key's arithmetic still holds, so the token is not
//! `broken`; but which certificate the trust question should be asked about is
//! exactly what is missing, so it is `unchecked`, binding. `openssl ts -verify`
//! refuses such a token outright, which is the same judgement.
//!
//! An imprint mismatch is still reported as `altered` even when the binding
//! is missing: "this token is of something else" needs only the key, and
//! hiding it behind a reason about certificates would withhold a conclusive
//! negative the arithmetic already reached.
//!
//! ESS v1 hashes the certificate with SHA-1 and is **not** read as `weak`. The
//! hash is compared against a certificate already in hand whose key has just
//! verified the signature; forging a match means a second certificate over
//! the same key built to collide, which is the substitution the binding
//! guards against, not the statement the time rests on. RFC 5816 permits v1,
//! and both oracles accept it (`docs/PLAN.md` §9 records them).
//!
//! ## What this does not decide
//!
//! Whose key the authority's is. That is [`crate::trust`]'s question, asked with
//! the timestamping purpose ([`crate::trust::Purpose::Timestamping`]) and only
//! beside an `intact` or `weak` verdict --- the same rule the signer's trust
//! follows. Revocation is [`crate::revocation`]'s, and whether `genTime` may
//! stand for when the signer is judged is decided in `docinfo`.

use der::Decode;
use x509_cert::attr::Attributes;
use x509_cert::spki::AlgorithmIdentifierOwned;

use super::{Hash, Integrity, Shape, Signer, Verdict, Why};

/// 1.2.840.113549.1.9.16.1.4, id-ct-TSTInfo: the content type of a token.
pub const TST_INFO: &str = "1.2.840.113549.1.9.16.1.4";

/// 1.2.840.113549.1.9.16.2.12, ESS signingCertificate (RFC 2634, SHA-1).
const SIGNING_CERTIFICATE: &str = "1.2.840.113549.1.9.16.2.12";

/// 1.2.840.113549.1.9.16.2.47, ESS signingCertificateV2 (RFC 5035).
const SIGNING_CERTIFICATE_V2: &str = "1.2.840.113549.1.9.16.2.47";

/// What a token's imprint must be the digest of.
#[derive(Clone, Copy, Debug)]
pub enum Target<'a> {
    /// The value octets of the `SignerInfo.signature` the token is attached to.
    Signature(&'a [u8]),
    /// The covered pieces of the file, already vouched for by
    /// [`super::covered`] --- a document timestamp's.
    Range(&'a [&'a [u8]]),
}

/// `MessageImprint`, RFC 3161 §2.4.1.
///
/// Crate-visible because a request carries one too: `tsa.rs` writes the same
/// structure into a `TimeStampReq` and reads the answer's back with
/// [`imprint_of`], so what is asked and what is checked are one type.
#[derive(Clone, Debug, der::Sequence)]
pub(crate) struct MessageImprint {
    pub(crate) hash_algorithm: AlgorithmIdentifierOwned,
    pub(crate) hashed_message: der::asn1::OctetString,
}

/// `SigningCertificate`, RFC 2634 §5.4.
#[derive(Clone, Debug, der::Sequence)]
struct SigningCertificate {
    certs: Vec<EssCertId>,
    #[asn1(optional = "true")]
    policies: Option<der::Any>,
}

/// `ESSCertID`: a SHA-1 of the certificate, and optionally its issuer and
/// serial --- which is read as a value and not compared, because a hash that
/// matches is already a statement about the whole certificate, issuer and
/// serial included, so a comparison after it could never fail.
#[derive(Clone, Debug, der::Sequence)]
struct EssCertId {
    cert_hash: der::asn1::OctetString,
    #[asn1(optional = "true")]
    issuer_serial: Option<der::Any>,
}

/// `SigningCertificateV2`, RFC 5035 §3.
#[derive(Clone, Debug, der::Sequence)]
struct SigningCertificateV2 {
    certs: Vec<EssCertIdV2>,
    #[asn1(optional = "true")]
    policies: Option<der::Any>,
}

/// `ESSCertIDv2`: the hash algorithm defaults to SHA-256, and DER omits a
/// default, so an absent one is SHA-256 and a present one is honoured. The
/// issuer and serial are not compared, for [`EssCertId`]'s reason.
#[derive(Clone, Debug, der::Sequence)]
struct EssCertIdV2 {
    #[asn1(optional = "true")]
    hash_algorithm: Option<AlgorithmIdentifierOwned>,
    cert_hash: der::asn1::OctetString,
    #[asn1(optional = "true")]
    issuer_serial: Option<der::Any>,
}

/// Checks one token against what it is attached to.
///
/// `token` is the `ContentInfo` in definite-length DER; `budget` is the
/// document's remaining [`super::MAX_HASHED`], charged before hashing, both
/// for the `TSTInfo` and for the imprint's target --- which for a document
/// timestamp is nearly the whole file.
pub fn check(token: &[u8], target: Target<'_>, budget: &mut u64) -> Integrity {
    if content_type(token).as_deref() != Some(TST_INFO) {
        // A CMS carrying something else is not a timestamp, and reading its
        // content as a `TSTInfo` would find an imprint in whatever sat there.
        return Integrity::unchecked(Why::Unreadable);
    }
    let Some(signer) = Signer::read(token, Shape::Token) else {
        return Integrity::unchecked(Why::Unreadable);
    };
    let Some(content) = signer.content.clone() else {
        return Integrity::unchecked(Why::Unreadable);
    };
    let attributes = signer.attributes.clone();
    let certificate = signer.certificate.clone();

    // The token's own CMS, by the signature's own arithmetic: its signature
    // over the signed attributes first, then its `messageDigest` against the
    // `TSTInfo`. Nothing below reads the `TSTInfo` until both hold.
    let own = signer.judge(&[content.as_slice()], budget);
    match own.verdict {
        Verdict::Unchecked => return own,
        // A `TSTInfo` that no longer hashes to what the authority signed is
        // not the authority's statement: for a token that is broken, not a
        // changed document.
        Verdict::Broken | Verdict::Altered => {
            return Integrity {
                verdict: Verdict::Broken,
                ..own
            };
        }
        Verdict::Weak | Verdict::Intact => {}
    }
    let sha1_signed = own.verdict == Verdict::Weak;

    let Some(imprint) = imprint_of(&content) else {
        return Integrity::unchecked(Why::Unreadable);
    };
    let Some(hash) = Hash::from_oid(&imprint.hash_algorithm.oid.to_string()) else {
        return Integrity::unchecked(Why::Algorithm);
    };

    let pieces: Vec<&[u8]> = match target {
        Target::Signature(value) => vec![value],
        Target::Range(pieces) => pieces.to_vec(),
    };
    let cost = pieces.iter().map(|piece| piece.len() as u64).sum::<u64>();
    if cost > *budget {
        return Integrity::unchecked(Why::Budget);
    }
    *budget -= cost;

    let weak = sha1_signed || hash == Hash::Sha1;
    let named = |verdict: Verdict| Integrity {
        verdict,
        why: None,
        // The imprint's hash is what ties the token to what it is attached
        // to, so it is the one named --- unless the token's own signature
        // rests on SHA-1, which is then the weaker link and the one to name.
        digest: if sha1_signed {
            own.digest.clone()
        } else {
            hash.name().into()
        },
        method: own.method.clone(),
    };

    if hash.digest(&pieces) != imprint.hashed_message.as_bytes() {
        return named(Verdict::Altered);
    }
    if let Err(why) = binds(attributes.as_ref(), certificate.as_deref()) {
        return Integrity::unchecked(why);
    }
    if weak {
        return named(Verdict::Weak);
    }
    named(Verdict::Intact)
}

/// The token's encapsulated content type, dotted.
fn content_type(token: &[u8]) -> Option<String> {
    let info = cms::content_info::ContentInfo::from_der(token).ok()?;
    let signed: cms::signed_data::SignedData = info.content.decode_as().ok()?;
    Some(signed.encap_content_info.econtent_type.to_string())
}

/// `messageImprint` out of a `TSTInfo`: its third field.
///
/// The two ahead of it --- `version` and `policy` --- are read as the types
/// they must be rather than skipped as opaque values, so a `TSTInfo` shifted
/// by a missing or extra field reads as unreadable rather than yielding some
/// other `SEQUENCE` as an imprint. Trailing bytes after the `TSTInfo` refuse
/// it for the same reason.
pub(crate) fn imprint_of(tst_info: &[u8]) -> Option<MessageImprint> {
    use der::{Reader as _, Tagged as _};

    let mut outer = der::SliceReader::new(tst_info).ok()?;
    let sequence = der::asn1::AnyRef::decode(&mut outer).ok()?;
    if sequence.tag() != der::Tag::Sequence || !outer.is_finished() {
        return None;
    }
    let mut inner = der::SliceReader::new(sequence.value()).ok()?;
    let _version = der::asn1::Int::decode(&mut inner).ok()?;
    let _policy = der::asn1::ObjectIdentifier::decode(&mut inner).ok()?;
    MessageImprint::decode(&mut inner).ok()
}

/// Whether the token's signed attributes bind the certificate its `sid` names.
///
/// At least one of ESS v1 and v2, each at most once and with one value; every
/// one present must name the certificate by hash. RFC 5035 §3: the first
/// `ESSCertID` is the signer's.
fn binds(attributes: Option<&Attributes>, certificate: Option<&[u8]>) -> Result<(), Why> {
    // Both are set whenever `judge` reached a verdict --- it needs signed
    // attributes to have a digest, and the named certificate to have a key ---
    // so these two refusals are for a caller that skipped it.
    let attributes = attributes.ok_or(Why::Binding)?;
    let der_bytes = certificate.ok_or(Why::Certificate)?;

    let one = |oid: &str| -> Result<Option<&der::Any>, Why> {
        let mut found = attributes.iter().filter(|a| a.oid.to_string() == oid);
        let Some(attribute) = found.next() else {
            return Ok(None);
        };
        if found.next().is_some() {
            return Err(Why::Attributes);
        }
        match attribute.values.as_slice() {
            [value] => Ok(Some(value)),
            _ => Err(Why::Attributes),
        }
    };
    let v1 = one(SIGNING_CERTIFICATE)?;
    let v2 = one(SIGNING_CERTIFICATE_V2)?;
    if v1.is_none() && v2.is_none() {
        return Err(Why::Binding);
    }
    if let Some(value) = v1 {
        let stated: SigningCertificate = value.decode_as().map_err(|_| Why::Attributes)?;
        let first = stated.certs.first().ok_or(Why::Binding)?;
        names(Hash::Sha1, first.cert_hash.as_bytes(), der_bytes)?;
    }
    if let Some(value) = v2 {
        let stated: SigningCertificateV2 = value.decode_as().map_err(|_| Why::Attributes)?;
        let first = stated.certs.first().ok_or(Why::Binding)?;
        let hash = match &first.hash_algorithm {
            None => Hash::Sha256,
            Some(algorithm) => Hash::from_oid(&algorithm.oid.to_string()).ok_or(Why::Algorithm)?,
        };
        names(hash, first.cert_hash.as_bytes(), der_bytes)?;
    }
    Ok(())
}

/// [`binds`] for a document signature, which need not carry the attribute:
/// nothing to hold it to when it carries neither form, and exactly what
/// [`binds`] answers when it carries one.
pub(super) fn binds_when_stated(
    attributes: Option<&Attributes>,
    certificate: Option<&[u8]>,
) -> Result<(), Why> {
    let stated = attributes.is_some_and(|attributes| {
        attributes.iter().any(|attribute| {
            let oid = attribute.oid.to_string();
            oid == SIGNING_CERTIFICATE || oid == SIGNING_CERTIFICATE_V2
        })
    });
    if !stated {
        return Ok(());
    }
    binds(attributes, certificate)
}

/// Whether one `ESSCertID`'s hash is the hash of `certificate`'s DER.
///
/// The DER is the certificate as `x509-cert` re-encodes it, which is the
/// authority's own bytes: `der` refuses any encoding that is not canonical, so
/// a certificate that decoded at all re-encodes to what was there.
fn names(hash: Hash, stated: &[u8], certificate: &[u8]) -> Result<(), Why> {
    if hash.digest(&[certificate]) == stated {
        Ok(())
    } else {
        Err(Why::Binding)
    }
}

#[cfg(test)]
mod tests;
