//! Whether a signature still covers the bytes it was made over.
//!
//! ## The one verdict tpdf gives about a signature, and how narrow it is
//!
//! Everything else [`crate::docinfo`] reports about a signature is a claim: the
//! name the signer typed, the date their clock read, what a certificate says
//! about its own subject. This module answers one question with arithmetic
//! instead --- **are the bytes the signature covers the bytes it was made
//! over, and was it made by the key in the certificate the signature names?**
//!
//! That is the question a reader means by "has this been tampered with", and
//! it is the whole of what [`Verdict::Intact`] states. It does **not** state
//! that the key belongs to anybody in particular: no chain is built, no trust
//! store consulted, no revocation looked up, and a certificate made five
//! minutes ago on the signer's own laptop passes exactly as a notary's does.
//! That is Phase 6 step 3's question (`docs/PLAN.md` §9), and the UI says it
//! was not asked every time it shows the answer to this one.
//!
//! ## Why a verdict can be `Unchecked`, and why that is never `Intact`
//!
//! `AGENTS.md`'s rule for redaction applies word for word: tpdf must never
//! claim a thing is clean unless it can prove it. A signature this module
//! cannot fully check --- an encoding it does not implement, an algorithm it
//! does not carry, a range it cannot vouch for, a budget it ran out of --- is
//! [`Verdict::Unchecked`] with the reason, and nothing about it is implied
//! either way. [`Integrity::default`] is `Unchecked`, so a field nobody filled
//! in cannot read as a pass.
//!
//! ## The order the checks run in is a decision
//!
//! The signature is tested **before** the digest is believed. `messageDigest`
//! is a signed attribute, so until the signature over the attributes verifies
//! it is only the blob's own word about what the bytes should hash to. So:
//!
//! - signature fails --- [`Verdict::Broken`], whatever the digest says, because
//!   nothing the blob states can be relied on;
//! - signature holds and the digest differs --- [`Verdict::Altered`]: the
//!   signer did sign that digest, and the covered bytes no longer produce it;
//! - both hold --- [`Verdict::Intact`], or [`Verdict::Weak`] under SHA-1.
//!
//! A signer with **no** signed attributes signs the covered bytes' digest
//! directly, so there is no separately signed digest to compare against: a
//! changed byte and a damaged value both make the signature fail, and both are
//! `Broken`. `Altered` needs the signed digest to tell them apart. Every
//! fixture and all three real signatures to hand carry signed attributes
//! (checked with `openssl cms -cmsout -print`), and CAdES requires them.
//!
//! ## SHA-1 is reported as weak, not as intact
//!
//! A SHA-1 match is not proof that the bytes are the ones signed. Chosen-prefix
//! collisions for SHA-1 are practical (SHAttered, 2017; Shambles, 2020), so
//! somebody who prepared the document for signing can hold a second one that
//! hashes the same. A **mismatch** under SHA-1 is still conclusive --- no
//! collision makes a hash differ --- so SHA-1 changes only the good answer, and
//! [`Verdict::Weak`] is that answer stated honestly. Refusing SHA-1 outright
//! would throw away the altered case, which it still proves.
//!
//! ## The byte range is checked before anything is hashed
//!
//! A `/ByteRange` is the document's arithmetic, not ours (`docs/TRAPS.md` has
//! the entry), and a range that leaves more than its own signature value
//! uncovered is how the published signature-wrapping attacks work: the digest
//! matches because the bytes an attacker changed were never covered. So a
//! range is accepted only when it is exactly two pieces, starts at zero, and
//! the hole between them is a hex string that decodes to **this** signature's
//! `/Contents`. Anything else is [`Why::Range`], never a verdict about bytes the
//! signature does not protect.
//!
//! ## Where this runs
//!
//! In the worker, like every other parser here: [`crate::docinfo::scan_from`]
//! calls it, and that runs in the sandboxed child that already holds the
//! document. The CMS blob, the certificate and the signature value are all
//! attacker-chosen bytes. `docs/THREAT-MODEL.md` §T6.8 states the bounds.

use cms::content_info::ContentInfo;
use cms::signed_data::SignedData;
use der::{Decode, Encode};

/// The most bytes hashed for all the signatures of one document together.
///
/// A signature's range is almost the whole file, so a document may ask for
/// its whole length to be hashed once per signature --- 32 signatures over a
/// 300 MB scan is ten gigabytes, well past the worker's thirty-second
/// deadline. One gigabyte is several seconds at worst and covers any real
/// document's signatures; past it the remaining ones are [`Why::Budget`].
pub const MAX_HASHED: u64 = 1024 * 1024 * 1024;

/// The largest RSA modulus this verifies, in bits.
///
/// `rsa`'s own default ceiling is 4096, which refuses the 8192-bit keys some
/// qualified-signature issuers do hand out. Verification with a small public
/// exponent is cheap at 8192, and `rsa` separately refuses an exponent large
/// enough to make it expensive.
const MAX_RSA_BITS: usize = 8192;

/// What checking a signature found.
///
/// **The only field in [`crate::docinfo::Signature`] that is a verdict**, and
/// deliberately narrow --- see the module note for what it claims and does not.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Integrity {
    /// The answer.
    pub verdict: Verdict,
    /// Why nothing was concluded, when [`Integrity::verdict`] is `Unchecked`.
    ///
    /// `None` for every other verdict. Set for every `Unchecked` one, which
    /// `an_unchecked_verdict_always_says_why` asserts: an unchecked signature
    /// with no reason reads as tpdf having forgotten to look.
    pub why: Option<Why>,
    /// The digest the covered bytes were hashed with, as `SHA-256`. Empty when
    /// the check stopped before choosing one.
    pub digest: String,
    /// How the signature value was made, as `RSA`, `RSA-PSS` or `ECDSA P-256`.
    /// Empty when the check stopped before reading it.
    pub method: String,
}

/// The answer, in increasing order of how much it proves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Nothing was concluded. [`Integrity::why`] says what stopped it.
    #[default]
    Unchecked,
    /// The signature over the signed data does not verify under the signer's
    /// key. Nothing the signature states --- including what the bytes should
    /// hash to --- can be relied on.
    Broken,
    /// The signature verifies, and the covered bytes no longer hash to the
    /// digest it signed: something inside the signed range has changed.
    Altered,
    /// Both hold, under SHA-1 --- which no longer proves the bytes are the ones
    /// signed, because SHA-1 collisions can be manufactured.
    Weak,
    /// The covered bytes hash to the digest the signature signed, and the
    /// signature verifies under the key in the certificate it names. Says
    /// nothing about who holds that key.
    Intact,
}

/// Why a signature was not checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// A `/SubFilter` this does not check: a document timestamp, the raw
    /// `adbe.x509.rsa_sha1`, the SHA-1 wrapped `adbe.pkcs7.sha1`, or a name
    /// nobody here recognises.
    Format,
    /// The `/ByteRange` is not exactly the file minus this signature's own value.
    Range,
    /// The `/Contents` blob is not a CMS signature this can read.
    Unreadable,
    /// The certificate the signature names is not in the blob, or its key
    /// could not be read.
    Certificate,
    /// A digest or signature algorithm this does not implement.
    Algorithm,
    /// The signed attributes are not the shape RFC 5652 requires: no single
    /// `messageDigest`, a content type that disagrees, or several signers.
    Attributes,
    /// The document's signatures together asked for more hashing than
    /// [`MAX_HASHED`].
    Budget,
}

impl Integrity {
    fn unchecked(why: Why) -> Self {
        Integrity {
            verdict: Verdict::Unchecked,
            why: Some(why),
            ..Integrity::default()
        }
    }
}

/// Checks one signature.
///
/// `bytes` is the whole file; `range` the `/ByteRange` numbers as the document
/// states them; `contents` the `/Contents` string exactly as the signature
/// dictionary holds it, padding and all; `blob` that string ended where its
/// structure ends and in definite-length form, or `None` when it would not
/// walk (see `docinfo::signature_contents`); `kind` the `/SubFilter`.
///
/// `budget` is what remains of [`MAX_HASHED`] for this document, and is
/// charged here by the bytes hashed, **before** they are hashed.
pub fn check(
    bytes: &[u8],
    range: &[i64],
    contents: &[u8],
    blob: Option<&[u8]>,
    kind: &str,
    budget: &mut u64,
) -> Integrity {
    // The detached CMS subfilters are the ones where the blob signs the bytes
    // of the range directly. Every other one needs a different computation,
    // and a signature checked by the wrong computation is not checked.
    if !matches!(kind, "adbe.pkcs7.detached" | "ETSI.CAdES.detached") {
        return Integrity::unchecked(Why::Format);
    }
    let Some(pieces) = covered(bytes, range, contents) else {
        return Integrity::unchecked(Why::Range);
    };
    let Some(blob) = blob else {
        return Integrity::unchecked(Why::Unreadable);
    };
    let Some(signer) = Signer::read(blob) else {
        return Integrity::unchecked(Why::Unreadable);
    };
    signer.judge(&pieces, budget)
}

/// The two covered pieces of the file, when the range is one this vouches for.
///
/// Exactly two `(offset, length)` pairs, the first at zero, both inside the
/// file, and between them a hole that is **this** signature's value and
/// nothing else: `<`, hex digits and whitespace, `>`, decoding to `contents`.
/// The comparison with `contents` is what ties the hole to this signature
/// dictionary --- without it, a range could leave some *other* hex string
/// uncovered, such as one a page draws as text, and the digest would still
/// match after it was changed.
pub fn covered<'a>(bytes: &'a [u8], range: &[i64], contents: &[u8]) -> Option<[&'a [u8]; 2]> {
    let [start, first, second, last] = range else {
        return None;
    };
    let size = u64::try_from(bytes.len()).ok()?;
    let start = u64::try_from(*start).ok()?;
    let first = u64::try_from(*first).ok()?;
    let second = u64::try_from(*second).ok()?;
    let last = u64::try_from(*last).ok()?;
    // Two values under 2^63, so their sum fits a `u64` whatever the document
    // chose --- which is why this is a plain addition and not a checked one:
    // a check that cannot fire is not a guard. The bound below is the guard.
    let end = second + last;
    if start != 0 || first == 0 || second <= first || end > size {
        return None;
    }
    let (first, second, end) = (
        usize::try_from(first).ok()?,
        usize::try_from(second).ok()?,
        usize::try_from(end).ok()?,
    );
    let hole = bytes.get(first..second)?;
    let [b'<', inner @ .., b'>'] = hole else {
        return None;
    };
    if decode_hex(inner)? != contents {
        return None;
    }
    Some([&bytes[..first], &bytes[second..end]])
}

/// A PDF hex string's value: whitespace ignored, an odd final digit padded.
///
/// PDF 32000-1 §7.3.4.3. `None` for anything that is not a hex digit or
/// whitespace, because a hole holding anything else holds more than a value.
fn decode_hex(digits: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(digits.len() / 2);
    let mut high: Option<u8> = None;
    for &byte in digits {
        let nibble = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\0' => continue,
            _ => return None,
        };
        match high.take() {
            Some(h) => out.push(h << 4 | nibble),
            None => high = Some(nibble),
        }
    }
    if let Some(h) = high {
        out.push(h << 4);
    }
    Some(out)
}

/// A digest algorithm this can compute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hash {
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    fn from_oid(oid: &str) -> Option<Self> {
        Some(match oid {
            "1.3.14.3.2.26" => Hash::Sha1,
            "2.16.840.1.101.3.4.2.4" => Hash::Sha224,
            "2.16.840.1.101.3.4.2.1" => Hash::Sha256,
            "2.16.840.1.101.3.4.2.2" => Hash::Sha384,
            "2.16.840.1.101.3.4.2.3" => Hash::Sha512,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Hash::Sha1 => "SHA-1",
            Hash::Sha224 => "SHA-224",
            Hash::Sha256 => "SHA-256",
            Hash::Sha384 => "SHA-384",
            Hash::Sha512 => "SHA-512",
        }
    }

    /// The digest of `pieces`, concatenated.
    fn digest(self, pieces: &[&[u8]]) -> Vec<u8> {
        fn run<D: sha2_10::Digest>(pieces: &[&[u8]]) -> Vec<u8> {
            let mut hasher = D::new();
            for piece in pieces {
                hasher.update(piece);
            }
            hasher.finalize().to_vec()
        }
        match self {
            Hash::Sha1 => run::<sha1::Sha1>(pieces),
            Hash::Sha224 => run::<sha2_10::Sha224>(pieces),
            Hash::Sha256 => run::<sha2_10::Sha256>(pieces),
            Hash::Sha384 => run::<sha2_10::Sha384>(pieces),
            Hash::Sha512 => run::<sha2_10::Sha512>(pieces),
        }
    }
}

/// How the signature value was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    /// RSA PKCS#1 v1.5.
    Rsa,
    /// RSASSA-PSS with MGF1 over the same hash, and this salt length.
    Pss { salt: usize },
    /// ECDSA; the curve comes from the key.
    Ecdsa,
}

/// Everything about the signer that the verdict needs, read out of the blob.
struct Signer {
    /// The digest algorithm the covered bytes and the signed attributes use.
    hash: Result<Hash, Why>,
    method: Result<Method, Why>,
    /// The signed attributes, re-tagged as the `SET OF` the signature covers.
    /// `None` when the signer signed the content digest directly.
    signed: Option<Vec<u8>>,
    /// The `messageDigest` attribute's value, when there are signed attributes.
    claimed: Result<Option<Vec<u8>>, Why>,
    signature: Vec<u8>,
    /// The signer's public key, as its `SubjectPublicKeyInfo` holds it.
    key: Result<Key, Why>,
}

/// A public key this can verify with.
enum Key {
    Rsa(Box<rsa::RsaPublicKey>),
    P256(p256::ecdsa::VerifyingKey),
    P384(p384::ecdsa::VerifyingKey),
}

impl Key {
    fn name(&self, method: Method) -> &'static str {
        match (self, method) {
            (Key::Rsa(_), Method::Pss { .. }) => "RSA-PSS",
            (Key::Rsa(_), _) => "RSA",
            (Key::P256(_), _) => "ECDSA P-256",
            (Key::P384(_), _) => "ECDSA P-384",
        }
    }
}

impl Signer {
    /// Reads what the verdict needs, or `None` when the blob is not a
    /// `SignedData` at all.
    ///
    /// Failures *inside* a readable blob are carried as `Err(Why)` in each
    /// field rather than returned, so a blob that is a signature is never
    /// reported as unreadable because one part of it was. [`Signer::judge`]
    /// reports them in a fixed order --- digest, method, attributes, key.
    fn read(blob: &[u8]) -> Option<Self> {
        let info = ContentInfo::from_der(blob).ok()?;
        // 1.2.840.113549.1.7.2, signed data. Anything else is not a signature.
        if info.content_type.to_string() != "1.2.840.113549.1.7.2" {
            return None;
        }
        let signed: SignedData = info.content.decode_as().ok()?;

        let infos = signed.signer_infos.0.as_slice();
        let [info] = infos else {
            // A PDF signature is one signer. Several is a shape nothing here
            // can attribute to "this signature", and none is no signature.
            return Some(Signer::refused(Why::Attributes));
        };

        let hash = Hash::from_oid(&info.digest_alg.oid.to_string()).ok_or(Why::Algorithm);
        let method = method_of(info, hash);

        // A detached signature carries no content of its own. One that does is
        // signing something other than the range, and checking the range
        // against it would be checking the wrong thing.
        let detached = signed.encap_content_info.econtent.is_none();

        let (signed_bytes, claimed) = match &info.signed_attrs {
            None if detached => (None, Ok(None)),
            None => (None, Err(Why::Attributes)),
            Some(attributes) => {
                let claimed = if detached {
                    message_digest(attributes, &signed.encap_content_info.econtent_type)
                } else {
                    Err(Why::Attributes)
                };
                (signed_attributes(blob), claimed)
            }
        };
        // Signed attributes present but not locatable in the blob's bytes.
        let claimed = match (&info.signed_attrs, &signed_bytes, claimed) {
            (Some(_), None, _) => Err(Why::Unreadable),
            (_, _, claimed) => claimed,
        };

        let key = crate::docinfo::signer_certificate(&signed)
            .filter(|(_, matched)| *matched)
            .ok_or(Why::Certificate)
            .and_then(|(certificate, _)| read_key(certificate));

        Some(Signer {
            hash,
            method,
            signed: signed_bytes,
            claimed,
            signature: info.signature.as_bytes().to_vec(),
            key,
        })
    }

    fn refused(why: Why) -> Self {
        Signer {
            hash: Err(why),
            method: Err(why),
            signed: None,
            claimed: Err(why),
            signature: Vec::new(),
            key: Err(why),
        }
    }

    /// The verdict, given the covered pieces of the file.
    fn judge(self, pieces: &[&[u8]; 2], budget: &mut u64) -> Integrity {
        let hash = match self.hash {
            Ok(hash) => hash,
            Err(why) => return Integrity::unchecked(why),
        };
        let method = match self.method {
            Ok(method) => method,
            Err(why) => return Integrity::unchecked(why),
        };
        let claimed = match self.claimed {
            Ok(claimed) => claimed,
            Err(why) => return Integrity::unchecked(why),
        };
        let key = match self.key {
            Ok(key) => key,
            Err(why) => return Integrity::unchecked(why),
        };
        let named = |verdict: Verdict| Integrity {
            verdict,
            why: None,
            digest: hash.name().into(),
            method: key.name(method).into(),
        };

        // A method whose curve and key disagree cannot be checked.
        let compatible = matches!(
            (&key, method),
            (Key::Rsa(_), Method::Rsa | Method::Pss { .. })
                | (Key::P256(_) | Key::P384(_), Method::Ecdsa)
        );
        if !compatible {
            return Integrity::unchecked(Why::Algorithm);
        }

        // Charged before hashing, so a document cannot spend the budget and
        // then be refused only afterwards.
        let cost = pieces.iter().map(|piece| piece.len() as u64).sum::<u64>();
        if cost > *budget {
            return Integrity::unchecked(Why::Budget);
        }
        *budget -= cost;
        let actual = hash.digest(pieces);

        // What the signature is over: the signed attributes when there are
        // any, else the covered bytes themselves --- whose digest is `actual`.
        let (to_verify, digest_matches) = match (&self.signed, &claimed) {
            (Some(attributes), Some(claimed)) => (hash.digest(&[attributes]), *claimed == actual),
            (None, None) => (actual.clone(), true),
            _ => return Integrity::unchecked(Why::Attributes),
        };

        if !verify(&key, method, hash, &to_verify, &self.signature) {
            return named(Verdict::Broken);
        }
        if !digest_matches {
            return named(Verdict::Altered);
        }
        if hash == Hash::Sha1 {
            return named(Verdict::Weak);
        }
        named(Verdict::Intact)
    }
}

/// Which way the signature value was made, and whether it agrees with `hash`.
///
/// A signature algorithm that names a hash of its own --- `sha256WithRSA`,
/// `ecdsa-with-SHA384`, PSS's parameters --- must name the one the digest
/// algorithm does. When they differ the signed data is hashed one way and the
/// value checked another, and neither answer would be a verdict.
fn method_of(info: &cms::signed_data::SignerInfo, hash: Result<Hash, Why>) -> Result<Method, Why> {
    let oid = info.signature_algorithm.oid.to_string();
    let hash = hash?;
    let agrees = |named: Hash| {
        if named == hash {
            Ok(())
        } else {
            Err(Why::Algorithm)
        }
    };
    match oid.as_str() {
        // rsaEncryption: the hash is the digest algorithm's.
        "1.2.840.113549.1.1.1" => Ok(Method::Rsa),
        "1.2.840.113549.1.1.5" => agrees(Hash::Sha1).map(|()| Method::Rsa),
        "1.2.840.113549.1.1.14" => agrees(Hash::Sha224).map(|()| Method::Rsa),
        "1.2.840.113549.1.1.11" => agrees(Hash::Sha256).map(|()| Method::Rsa),
        "1.2.840.113549.1.1.12" => agrees(Hash::Sha384).map(|()| Method::Rsa),
        "1.2.840.113549.1.1.13" => agrees(Hash::Sha512).map(|()| Method::Rsa),
        "1.2.840.113549.1.1.10" => pss(info, hash),
        // id-ecPublicKey: the hash is the digest algorithm's.
        "1.2.840.10045.2.1" => Ok(Method::Ecdsa),
        "1.2.840.10045.4.1" => agrees(Hash::Sha1).map(|()| Method::Ecdsa),
        "1.2.840.10045.4.3.1" => agrees(Hash::Sha224).map(|()| Method::Ecdsa),
        "1.2.840.10045.4.3.2" => agrees(Hash::Sha256).map(|()| Method::Ecdsa),
        "1.2.840.10045.4.3.3" => agrees(Hash::Sha384).map(|()| Method::Ecdsa),
        "1.2.840.10045.4.3.4" => agrees(Hash::Sha512).map(|()| Method::Ecdsa),
        _ => Err(Why::Algorithm),
    }
}

/// RSASSA-PSS's parameters, accepted only in the shape `rsa` implements.
///
/// `rsa` uses the message hash for MGF1 too, so a signature whose mask hash
/// differs --- legal, and rare --- is refused rather than verified wrongly.
/// The trailer field must be 1, the only value RFC 4055 defines.
fn pss(info: &cms::signed_data::SignerInfo, hash: Hash) -> Result<Method, Why> {
    let parameters = info
        .signature_algorithm
        .parameters
        .as_ref()
        .ok_or(Why::Algorithm)?
        .to_der()
        .map_err(|_| Why::Algorithm)?;
    let params = rsa::pkcs1::RsaPssParams::from_der(&parameters).map_err(|_| Why::Algorithm)?;
    let named = Hash::from_oid(&params.hash.oid.to_string()).ok_or(Why::Algorithm)?;
    // 1.2.840.113549.1.1.8, MGF1, whose own parameter is the hash it uses.
    let mask = params
        .mask_gen
        .parameters
        .as_ref()
        .map(|inner| inner.oid.to_string());
    let mgf1 = params.mask_gen.oid.to_string() == "1.2.840.113549.1.1.8";
    if named != hash
        || !mgf1
        || mask.as_deref().and_then(Hash::from_oid) != Some(hash)
        || params.trailer_field != rsa::pkcs1::TrailerField::BC
    {
        return Err(Why::Algorithm);
    }
    Ok(Method::Pss {
        salt: usize::from(params.salt_len),
    })
}

/// The one `messageDigest` value, with the content type checked beside it.
///
/// RFC 5652 §5.3: when there are signed attributes there must be exactly one
/// content-type attribute, equal to the encapsulated content type, and exactly
/// one message-digest attribute. Each carries exactly one value.
fn message_digest(
    attributes: &x509_cert::attr::Attributes,
    content_type: &der::asn1::ObjectIdentifier,
) -> Result<Option<Vec<u8>>, Why> {
    let only = |oid: &str| -> Result<&der::asn1::Any, Why> {
        let mut found = attributes.iter().filter(|a| a.oid.to_string() == oid);
        let attribute = found.next().ok_or(Why::Attributes)?;
        if found.next().is_some() {
            return Err(Why::Attributes);
        }
        match attribute.values.as_slice() {
            [value] => Ok(value),
            _ => Err(Why::Attributes),
        }
    };
    let stated_type: der::asn1::ObjectIdentifier = only("1.2.840.113549.1.9.3")?
        .decode_as()
        .map_err(|_| Why::Attributes)?;
    if &stated_type != content_type {
        return Err(Why::Attributes);
    }
    let digest: der::asn1::OctetString = only("1.2.840.113549.1.9.4")?
        .decode_as()
        .map_err(|_| Why::Attributes)?;
    Ok(Some(digest.as_bytes().to_vec()))
}

/// The signed attributes as the blob wrote them, re-tagged as a `SET OF`.
///
/// RFC 5652 §5.4: the signature is over the attributes' encoding with the
/// `[0] IMPLICIT` tag replaced by `SET OF`'s. **The bytes as written, not a
/// re-encoding**: `der` sorts a `SET OF` as it decodes one, so re-encoding the
/// parsed attributes gives different bytes from a signer that did not sort
/// them --- and that signer's signature is over what it wrote. pyHanko hashes
/// the written bytes too, which is what `signature-probe --mode integrity`
/// compares against.
///
/// Walked positionally through `SignedData` to the first `SignerInfo`. The
/// caller has already decoded the same blob in full, so every step here is
/// over a structure known to parse.
fn signed_attributes(blob: &[u8]) -> Option<Vec<u8>> {
    use der::asn1::AnyRef;
    use der::{Reader, SliceReader, Tag, TagNumber, Tagged};

    fn inner(any: AnyRef<'_>) -> Option<SliceReader<'_>> {
        SliceReader::new(any.value()).ok()
    }

    let content_info = AnyRef::from_der(blob).ok()?;
    let mut reader = inner(content_info)?;
    let _content_type: AnyRef<'_> = reader.decode().ok()?;
    let explicit: AnyRef<'_> = reader.decode().ok()?;
    let signed_data: AnyRef<'_> = inner(explicit)?.decode().ok()?;
    let mut reader = inner(signed_data)?;
    let mut signer_infos = None;
    while !reader.is_finished() {
        let any: AnyRef<'_> = reader.decode().ok()?;
        // version, digestAlgorithms, encapContentInfo, [0] certificates,
        // [1] crls, and last the signer infos --- the only SET after the
        // digest algorithms, so it is the last element.
        signer_infos = Some(any);
    }
    let first: AnyRef<'_> = inner(signer_infos?)?.decode().ok()?;
    let mut reader = inner(first)?;
    while !reader.is_finished() {
        let any: AnyRef<'_> = reader.decode().ok()?;
        if any.tag()
            == (Tag::ContextSpecific {
                constructed: true,
                number: TagNumber::N0,
            })
        {
            let mut encoded = any.to_der().ok()?;
            *encoded.first_mut()? = 0x31;
            return Some(encoded);
        }
    }
    None
}

/// The signer's key, from its certificate.
fn read_key(certificate: &x509_cert::Certificate) -> Result<Key, Why> {
    let spki = &certificate.tbs_certificate.subject_public_key_info;
    let bits = spki.subject_public_key.as_bytes().ok_or(Why::Certificate)?;
    match spki.algorithm.oid.to_string().as_str() {
        // rsaEncryption, and id-RSASSA-PSS for a key restricted to PSS: both
        // carry an RSAPublicKey in the bit string.
        "1.2.840.113549.1.1.1" | "1.2.840.113549.1.1.10" => {
            let parsed = rsa::pkcs1::RsaPublicKey::from_der(bits).map_err(|_| Why::Certificate)?;
            let n = rsa::BigUint::from_bytes_be(parsed.modulus.as_bytes());
            let e = rsa::BigUint::from_bytes_be(parsed.public_exponent.as_bytes());
            rsa::RsaPublicKey::new_with_max_size(n, e, MAX_RSA_BITS)
                .map(|key| Key::Rsa(Box::new(key)))
                .map_err(|_| Why::Certificate)
        }
        "1.2.840.10045.2.1" => {
            let curve = spki
                .algorithm
                .parameters
                .as_ref()
                .and_then(|p| p.decode_as::<der::asn1::ObjectIdentifier>().ok())
                .map(|oid| oid.to_string());
            match curve.as_deref() {
                Some("1.2.840.10045.3.1.7") => p256::ecdsa::VerifyingKey::from_sec1_bytes(bits)
                    .map(Key::P256)
                    .map_err(|_| Why::Certificate),
                Some("1.3.132.0.34") => p384::ecdsa::VerifyingKey::from_sec1_bytes(bits)
                    .map(Key::P384)
                    .map_err(|_| Why::Certificate),
                _ => Err(Why::Algorithm),
            }
        }
        _ => Err(Why::Algorithm),
    }
}

/// Whether `signature` is a signature over `digest` under `key`.
///
/// `digest` is already hashed: RSA is told which hash so it can check the
/// `DigestInfo` prefix, and ECDSA takes it as the prehash it is.
fn verify(key: &Key, method: Method, hash: Hash, digest: &[u8], signature: &[u8]) -> bool {
    use ecdsa::signature::hazmat::PrehashVerifier;

    match (key, method) {
        (Key::Rsa(key), Method::Rsa) => {
            let scheme = match hash {
                Hash::Sha1 => rsa::Pkcs1v15Sign::new::<sha1::Sha1>(),
                Hash::Sha224 => rsa::Pkcs1v15Sign::new::<sha2_10::Sha224>(),
                Hash::Sha256 => rsa::Pkcs1v15Sign::new::<sha2_10::Sha256>(),
                Hash::Sha384 => rsa::Pkcs1v15Sign::new::<sha2_10::Sha384>(),
                Hash::Sha512 => rsa::Pkcs1v15Sign::new::<sha2_10::Sha512>(),
            };
            key.verify(scheme, digest, signature).is_ok()
        }
        (Key::Rsa(key), Method::Pss { salt }) => {
            let scheme = match hash {
                Hash::Sha1 => rsa::Pss::new_with_salt::<sha1::Sha1>(salt),
                Hash::Sha224 => rsa::Pss::new_with_salt::<sha2_10::Sha224>(salt),
                Hash::Sha256 => rsa::Pss::new_with_salt::<sha2_10::Sha256>(salt),
                Hash::Sha384 => rsa::Pss::new_with_salt::<sha2_10::Sha384>(salt),
                Hash::Sha512 => rsa::Pss::new_with_salt::<sha2_10::Sha512>(salt),
            };
            key.verify(scheme, digest, signature).is_ok()
        }
        (Key::P256(key), Method::Ecdsa) => p256::ecdsa::Signature::from_der(signature)
            .is_ok_and(|sig| key.verify_prehash(digest, &sig).is_ok()),
        (Key::P384(key), Method::Ecdsa) => p384::ecdsa::Signature::from_der(signature)
            .is_ok_and(|sig| key.verify_prehash(digest, &sig).is_ok()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docinfo::scan;

    /// A generated fixture, or `None` with a `[SKIP]` line when it is absent.
    ///
    /// Absent is a fact about this checkout, not a pass: CI builds every one
    /// of these with `scripts/ci_fixtures.py --signed` and fails at that step
    /// when it cannot, which is what makes skipping here safe.
    fn fixture(name: &str) -> Option<Vec<u8>> {
        let path = std::path::Path::new("../testdata").join(name);
        match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(_) => {
                println!("[SKIP] {name}: not generated");
                None
            }
        }
    }

    /// Every signed signature's verdict, read the way the dialog reads it.
    fn verdicts(bytes: &[u8]) -> Vec<Integrity> {
        scan(bytes, 1, None)
            .expect("a signed fixture parses")
            .signatures
            .into_iter()
            .filter(|s| s.signed)
            .map(|s| s.integrity.expect("a signed field carries a verdict"))
            .collect()
    }

    fn only(bytes: &[u8]) -> Integrity {
        let mut all = verdicts(bytes);
        assert_eq!(all.len(), 1, "one signature");
        all.remove(0)
    }

    /// The four `/ByteRange` numbers of a one-signature fixture.
    fn range_of(bytes: &[u8]) -> [usize; 4] {
        let at = bytes
            .windows(11)
            .position(|w| w == b"/ByteRange ")
            .expect("a /ByteRange");
        let open = at + bytes[at..].iter().position(|b| *b == b'[').expect("[");
        let close = open + bytes[open..].iter().position(|b| *b == b']').expect("]");
        let text = std::str::from_utf8(&bytes[open + 1..close]).expect("ascii");
        let numbers: Vec<usize> = text
            .split_whitespace()
            .map(|n| n.parse().expect("an integer"))
            .collect();
        numbers.try_into().expect("four numbers")
    }

    /// `bytes` with its one signature's blob replaced by `edit` of it,
    /// re-hexed into the same reserved span so no offset moves.
    ///
    /// The edit is made on the decoded `SignedData` and re-encoded with `der`,
    /// which is the whole point: a test can then change exactly one field of
    /// a real, pyHanko-made signature --- the signer identifier, an algorithm,
    /// an attribute --- and leave every other byte as the signer wrote it.
    fn with_blob(bytes: &[u8], edit: impl FnOnce(&mut SignedData)) -> Vec<u8> {
        let [_, first, second, _] = range_of(bytes);
        let hex = &bytes[first + 1..second - 1];
        let raw = decode_hex(hex).expect("hex");
        let blob = crate::ber::to_definite_length(&raw).expect("a blob");
        let mut info = ContentInfo::from_der(&blob).expect("content info");
        let mut signed: SignedData = info.content.decode_as().expect("signed data");
        edit(&mut signed);
        info.content = der::Any::encode_from(&signed).expect("re-encoded");
        let encoded = info.to_der().expect("re-encoded");
        assert!(
            encoded.len() <= raw.len(),
            "the edit must fit the reservation"
        );
        let mut padded = encoded;
        padded.resize(raw.len(), 0);
        let digits: String = padded.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(digits.len(), hex.len(), "the reserved span must not move");
        let mut out = bytes.to_vec();
        out[first + 1..second - 1].copy_from_slice(digits.as_bytes());
        out
    }

    fn signers(infos: Vec<cms::signed_data::SignerInfo>) -> cms::signed_data::SignerInfos {
        cms::signed_data::SignerInfos(der::asn1::SetOfVec::try_from(infos).expect("a set"))
    }

    fn oid(text: &str) -> der::asn1::ObjectIdentifier {
        der::asn1::ObjectIdentifier::new_unwrap(text)
    }

    // ------------------------------------------------ the byte range itself

    /// A file of `head`, the hole, and `tail`, and the range that frames it.
    fn framed(head: &[u8], value: &[u8], tail: &[u8]) -> (Vec<u8>, Vec<i64>) {
        let digits: String = value.iter().map(|b| format!("{b:02X}")).collect();
        let mut file = head.to_vec();
        file.push(b'<');
        file.extend_from_slice(digits.as_bytes());
        file.push(b'>');
        let second = file.len();
        file.extend_from_slice(tail);
        let range = vec![0, head.len() as i64, second as i64, tail.len() as i64];
        (file, range)
    }

    #[test]
    fn a_range_that_leaves_out_exactly_its_own_value_is_accepted() {
        let (file, range) = framed(b"head", b"\x30\x03\x02\x01\x05", b"tail");
        let pieces = covered(&file, &range, b"\x30\x03\x02\x01\x05").expect("accepted");
        assert_eq!(pieces, [&b"head"[..], &b"tail"[..]]);
        // Padding the writer reserved is part of the value: `/Contents` holds
        // it too, and the comparison is with the whole string.
        let (file, range) = framed(b"h", b"\x01\x00\x00", b"t");
        assert!(covered(&file, &range, b"\x01\x00\x00").is_some());
    }

    #[test]
    fn a_range_whose_hole_holds_anything_but_this_signature_is_refused() {
        let value = b"\x30\x03\x02\x01\x05";
        let (file, range) = framed(b"head", value, b"tail");
        // A different value: the hole is some other hex string, such as one a
        // page draws as text. The wrapping attack's shape.
        assert!(covered(&file, &range, b"\x30\x03\x02\x01\x06").is_none());
        // One byte of the head moved into the hole: it would go uncovered.
        let mut wider = range.clone();
        wider[1] -= 1;
        assert!(covered(&file, &wider, value).is_none());
        // A hole that is not a hex string at all.
        let mut text = file.clone();
        text[5] = b'Z';
        assert!(covered(&text, &range, value).is_none());
    }

    #[test]
    fn a_range_not_starting_at_zero_or_not_in_two_pieces_is_refused() {
        let value = b"\x05";
        let (file, range) = framed(b"head", value, b"tail");
        let mut late = range.clone();
        late[0] = 1;
        assert!(covered(&file, &late, value).is_none());
        let mut three = range.clone();
        three.extend([10, 0]);
        assert!(covered(&file, &three, value).is_none());
        assert!(covered(&file, &range[..3], value).is_none());
    }

    #[test]
    fn a_range_the_file_cannot_hold_is_refused_without_arithmetic_overflow() {
        let value = b"\x05";
        let (file, range) = framed(b"head", value, b"tail");
        let mut past = range.clone();
        past[3] += 1;
        assert!(covered(&file, &past, value).is_none());
        let mut huge = range.clone();
        huge[3] = i64::MAX;
        assert!(covered(&file, &huge, value).is_none());
        huge[2] = i64::MAX;
        assert!(covered(&file, &huge, value).is_none());
        let mut negative = range.clone();
        negative[1] = -1;
        assert!(covered(&file, &negative, value).is_none());
    }

    // --------------------------------------------------- the real fixtures

    #[test]
    fn an_untouched_signature_is_intact() {
        // The control. A verifier that answered "altered" or "broken" for
        // everything would pass every refusal test below; this is what fails it.
        let mut examined = 0;
        for name in [
            "incr-signed.pdf",
            "incr-certified-1.pdf",
            "incr-certified-2.pdf",
            "incr-certified-3.pdf",
            "incr-certified-3-indirect.pdf",
            "incr-timestamped.pdf",
            "incr-two-signers.pdf",
        ] {
            let Some(bytes) = fixture(name) else { continue };
            let all = verdicts(&bytes);
            assert!(!all.is_empty(), "{name}");
            for verdict in all {
                assert_eq!(verdict.verdict, Verdict::Intact, "{name}: {verdict:?}");
                assert_eq!(verdict.why, None, "{name}");
                assert_eq!(verdict.digest, "SHA-256", "{name}");
                assert_eq!(verdict.method, "RSA", "{name}");
            }
            examined += 1;
        }
        println!("examined {examined} fixtures");
    }

    #[test]
    fn a_changed_byte_in_the_signed_range_is_altered() {
        let Some(bytes) = fixture("signed-altered.pdf") else {
            return;
        };
        let verdict = only(&bytes);
        assert_eq!(verdict.verdict, Verdict::Altered, "{verdict:?}");
        assert_eq!(verdict.why, None);
    }

    #[test]
    fn a_flipped_bit_in_the_signature_value_is_broken() {
        let Some(bytes) = fixture("signed-broken.pdf") else {
            return;
        };
        let verdict = only(&bytes);
        assert_eq!(verdict.verdict, Verdict::Broken, "{verdict:?}");
    }

    #[test]
    fn a_broken_signature_is_broken_even_when_the_bytes_also_changed() {
        // Both at once: the signature is tested first, because a digest it
        // does not vouch for is only the blob's own word. So this is Broken,
        // not Altered --- nothing the blob says, the digest included, stands.
        let Some(broken) = fixture("signed-broken.pdf") else {
            return;
        };
        let Some(altered) = fixture("signed-altered.pdf") else {
            return;
        };
        let [_, first, second, _] = range_of(&broken);
        let mut both = altered.clone();
        both[first..second].copy_from_slice(&broken[first..second]);
        assert_eq!(only(&both).verdict, Verdict::Broken);
    }

    #[test]
    fn each_signature_scheme_is_checked_by_its_own_mathematics() {
        let cases = [
            ("signed-p256.pdf", "SHA-256", "ECDSA P-256"),
            ("signed-p384.pdf", "SHA-384", "ECDSA P-384"),
            ("signed-pss.pdf", "SHA-256", "RSA-PSS"),
        ];
        for (name, digest, method) in cases {
            let Some(bytes) = fixture(name) else { continue };
            let verdict = only(&bytes);
            assert_eq!(verdict.verdict, Verdict::Intact, "{name}: {verdict:?}");
            assert_eq!(verdict.digest, digest, "{name}");
            assert_eq!(verdict.method, method, "{name}");
        }
    }

    #[test]
    fn a_corrupted_ecdsa_or_pss_value_is_broken_rather_than_passed() {
        // The schemes the intact test reaches, each with its value damaged:
        // without this, a scheme whose verification always succeeded would
        // pass the test above.
        for name in ["signed-p256.pdf", "signed-p384.pdf", "signed-pss.pdf"] {
            let Some(bytes) = fixture(name) else { continue };
            let damaged = with_blob(&bytes, |signed| {
                let mut infos: Vec<_> = signed.signer_infos.0.iter().cloned().collect();
                let mut value = infos[0].signature.as_bytes().to_vec();
                let last = value.len() - 1;
                value[last] ^= 0x01;
                infos[0].signature = der::asn1::OctetString::new(value).expect("octets");
                signed.signer_infos = signers(infos);
            });
            assert_eq!(only(&damaged).verdict, Verdict::Broken, "{name}");
        }
    }

    #[test]
    fn a_sha1_match_is_weak_and_never_intact() {
        let Some(bytes) = fixture("signed-sha1.pdf") else {
            return;
        };
        let verdict = only(&bytes);
        assert_eq!(verdict.verdict, Verdict::Weak, "{verdict:?}");
        assert_eq!(verdict.digest, "SHA-1");
    }

    #[test]
    fn a_range_that_does_not_frame_its_own_value_is_not_checked() {
        // `signed-nested-field.pdf` borrows a real blob and states a range the
        // blob was never computed over. pyHanko calls its coverage UNCLEAR.
        let Some(bytes) = fixture("signed-nested-field.pdf") else {
            return;
        };
        let verdict = only(&bytes);
        assert_eq!(verdict.verdict, Verdict::Unchecked);
        assert_eq!(verdict.why, Some(Why::Range));
    }

    #[test]
    fn an_indefinite_length_blob_is_checked_after_the_walk() {
        // The same signature as `incr-signed.pdf`, every length rewritten as
        // BER's indefinite form. The walk hands back the definite encoding the
        // signer signed, so the signed attributes hash to what was signed.
        let Some(bytes) = fixture("incr-ber.pdf") else {
            return;
        };
        assert_eq!(only(&bytes).verdict, Verdict::Intact);
    }

    // ------------------------------------ one field of a real signature changed

    #[test]
    fn a_blob_re_encoded_with_no_edit_is_still_intact() {
        // The control for every test below. `with_blob` decodes a real
        // signature and encodes it again; if that round trip changed a byte
        // the signature covers, every refusal below would be a refusal of the
        // re-encoding rather than of the one field the test changed.
        let Some(bytes) = fixture("incr-signed.pdf") else {
            return;
        };
        let same = with_blob(&bytes, |_| {});
        assert_eq!(only(&same).verdict, Verdict::Intact);
    }

    #[test]
    fn a_signer_the_signature_does_not_name_is_not_used_to_check_it() {
        // The one certificate, with the signer identifier pointing elsewhere.
        // The dialog still shows that certificate --- a set of one leaves
        // nothing to choose between --- but the verdict needs the key the
        // signature *names*, and verifying under another would be a guess.
        let Some(bytes) = fixture("incr-signed.pdf") else {
            return;
        };
        let renamed = with_blob(&bytes, |signed| {
            use cms::signed_data::SignerIdentifier;
            let mut infos: Vec<_> = signed.signer_infos.0.iter().cloned().collect();
            if let SignerIdentifier::IssuerAndSerialNumber(both) = &mut infos[0].sid {
                both.serial_number =
                    x509_cert::serial_number::SerialNumber::new(&[0x7f, 0x01]).expect("serial");
            }
            signed.signer_infos = signers(infos);
        });
        let verdict = only(&renamed);
        assert_eq!(verdict.verdict, Verdict::Unchecked, "{verdict:?}");
        assert_eq!(verdict.why, Some(Why::Certificate));
    }

    #[test]
    fn a_digest_algorithm_this_does_not_carry_is_not_checked() {
        let Some(bytes) = fixture("incr-signed.pdf") else {
            return;
        };
        let md5 = with_blob(&bytes, |signed| {
            let mut infos: Vec<_> = signed.signer_infos.0.iter().cloned().collect();
            infos[0].digest_alg.oid = oid("1.2.840.113549.2.5");
            signed.signer_infos = signers(infos);
        });
        let verdict = only(&md5);
        assert_eq!(
            (verdict.verdict, verdict.why),
            (Verdict::Unchecked, Some(Why::Algorithm))
        );
    }

    #[test]
    fn a_signature_algorithm_naming_another_hash_is_not_checked() {
        // sha384WithRSA over a SHA-256 digest. Verifying under the digest's
        // hash would be verifying a different claim from the one written.
        let Some(bytes) = fixture("incr-signed.pdf") else {
            return;
        };
        let mismatched = with_blob(&bytes, |signed| {
            let mut infos: Vec<_> = signed.signer_infos.0.iter().cloned().collect();
            infos[0].signature_algorithm.oid = oid("1.2.840.113549.1.1.12");
            signed.signer_infos = signers(infos);
        });
        let verdict = only(&mismatched);
        assert_eq!(
            (verdict.verdict, verdict.why),
            (Verdict::Unchecked, Some(Why::Algorithm))
        );
    }

    #[test]
    fn signed_attributes_without_a_message_digest_are_not_checked() {
        let Some(bytes) = fixture("incr-signed.pdf") else {
            return;
        };
        let stripped = with_blob(&bytes, |signed| {
            let mut infos: Vec<_> = signed.signer_infos.0.iter().cloned().collect();
            let kept: Vec<_> = infos[0]
                .signed_attrs
                .as_ref()
                .expect("pyHanko writes signed attributes")
                .iter()
                .filter(|a| a.oid.to_string() != "1.2.840.113549.1.9.4")
                .cloned()
                .collect();
            infos[0].signed_attrs = Some(kept.try_into().expect("a set"));
            signed.signer_infos = signers(infos);
        });
        let verdict = only(&stripped);
        assert_eq!(
            (verdict.verdict, verdict.why),
            (Verdict::Unchecked, Some(Why::Attributes))
        );
    }

    #[test]
    fn a_content_type_the_attributes_disagree_with_is_not_checked() {
        // The attributes say the signed content is `data`; the envelope is
        // relabelled. The signature and the digest both still hold, which is
        // exactly why only the equality check can refuse it.
        let Some(bytes) = fixture("incr-signed.pdf") else {
            return;
        };
        let relabelled = with_blob(&bytes, |signed| {
            signed.encap_content_info.econtent_type = oid("1.2.840.113549.1.7.5");
        });
        let verdict = only(&relabelled);
        assert_eq!(
            (verdict.verdict, verdict.why),
            (Verdict::Unchecked, Some(Why::Attributes))
        );
    }

    #[test]
    fn a_blob_with_two_signers_is_not_attributed_to_either() {
        let Some(bytes) = fixture("incr-signed.pdf") else {
            return;
        };
        let doubled = with_blob(&bytes, |signed| {
            let mut infos: Vec<_> = signed.signer_infos.0.iter().cloned().collect();
            let mut second = infos[0].clone();
            second.signature = der::asn1::OctetString::new(vec![1, 2, 3]).expect("octets");
            infos.push(second);
            signed.signer_infos = signers(infos);
        });
        let verdict = only(&doubled);
        assert_eq!(
            (verdict.verdict, verdict.why),
            (Verdict::Unchecked, Some(Why::Attributes))
        );
    }

    #[test]
    fn a_subfilter_this_does_not_check_is_not_checked() {
        // The raw PKCS#1 and the SHA-1-wrapped PKCS#7 sign something other
        // than the range's bytes directly, and a document timestamp is a token
        // rather than a signer. Checked with the detached computation, any of
        // them would be checked wrongly.
        let Some(bytes) = fixture("incr-signed.pdf") else {
            return;
        };
        let [start, first, second, last] = range_of(&bytes);
        let raw = decode_hex(&bytes[first + 1..second - 1]).expect("hex");
        let blob = crate::ber::to_definite_length(&raw);
        let range = [start, first, second, last].map(|n| n as i64);
        for kind in ["adbe.x509.rsa_sha1", "adbe.pkcs7.sha1", "ETSI.RFC3161", ""] {
            let verdict = check(
                &bytes,
                &range,
                &raw,
                blob.as_deref(),
                kind,
                &mut MAX_HASHED.clone(),
            );
            assert_eq!(
                (verdict.verdict, verdict.why),
                (Verdict::Unchecked, Some(Why::Format)),
                "{kind}"
            );
        }
        // And the control: the same inputs under the subfilter they were
        // written with, so the refusals above are about the name alone.
        let verdict = check(
            &bytes,
            &range,
            &raw,
            blob.as_deref(),
            "adbe.pkcs7.detached",
            &mut MAX_HASHED.clone(),
        );
        assert_eq!(verdict.verdict, Verdict::Intact);
    }

    #[test]
    fn a_budget_smaller_than_the_range_refuses_before_hashing() {
        let Some(bytes) = fixture("incr-signed.pdf") else {
            return;
        };
        let [start, first, second, last] = range_of(&bytes);
        let raw = decode_hex(&bytes[first + 1..second - 1]).expect("hex");
        let blob = crate::ber::to_definite_length(&raw);
        let range = [start, first, second, last].map(|n| n as i64);
        let cost = (first + last) as u64;

        let mut short = cost - 1;
        let verdict = check(
            &bytes,
            &range,
            &raw,
            blob.as_deref(),
            "adbe.pkcs7.detached",
            &mut short,
        );
        assert_eq!(
            (verdict.verdict, verdict.why),
            (Verdict::Unchecked, Some(Why::Budget))
        );
        assert_eq!(short, cost - 1, "a refusal spends nothing");

        let mut exact = cost;
        let verdict = check(
            &bytes,
            &range,
            &raw,
            blob.as_deref(),
            "adbe.pkcs7.detached",
            &mut exact,
        );
        assert_eq!(verdict.verdict, Verdict::Intact);
        assert_eq!(exact, 0, "what was hashed is charged");
    }

    #[test]
    fn an_unchecked_verdict_always_says_why() {
        // Over every verdict the fixtures produce, and the refusals above
        // built on them: `why` is set exactly when nothing was concluded.
        let mut seen = Vec::new();
        for name in [
            "incr-signed.pdf",
            "incr-two-signers.pdf",
            "signed-altered.pdf",
            "signed-broken.pdf",
            "signed-sha1.pdf",
            "signed-nested-field.pdf",
        ] {
            let Some(bytes) = fixture(name) else { continue };
            seen.extend(verdicts(&bytes));
        }
        seen.push(Integrity::unchecked(Why::Budget));
        seen.push(Integrity::default());
        for verdict in &seen[..seen.len() - 1] {
            assert_eq!(
                verdict.verdict == Verdict::Unchecked,
                verdict.why.is_some(),
                "{verdict:?}"
            );
        }
        // The one exception, and why it is harmless: a default is a field
        // nobody filled in, and it reads as unchecked rather than as a pass.
        assert_eq!(Integrity::default().verdict, Verdict::Unchecked);
    }
}
