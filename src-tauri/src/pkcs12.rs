//! Certificate files (PKCS#12, `.p12` and `.pfx`) on Linux, ECDSA keys only.
//!
//! A smart card signs with a key that never leaves it (`pkcs11.rs`). A
//! certificate file holds its key encrypted under a password, so signing with
//! one means **decrypting the key inside tpdf**: the one place tpdf performs a
//! private-key operation. Decided 2026-10-14 for ECDSA keys only, P-256 and
//! P-384. An RSA key is refused with a sentence, because `rsa`'s
//! RUSTSEC-2023-0071, a timing leak in its private-key operations, is accepted
//! in `.cargo/audit.toml` only because tpdf performs none with it
//! (`sign_cms::tests::no_private_key_type_is_named_outside_the_tests` holds
//! that, and names this file as its one exception, for `SigningKey`).
//!
//! The key lives only as long as one signature: [`open`] decrypts it, the
//! caller signs one digest, and the `SigningKey` zeroes itself when dropped.
//! Every decrypted buffer on the way is `Zeroizing`.
//!
//! ## What is read
//!
//! * The integrity MAC (HMAC with SHA-1 or SHA-2, key from the PKCS#12 key
//!   derivation), checked first, so a wrong password is named as one rather
//!   than as a broken file. PBMAC1, which only the newest OpenSSL writes, is
//!   refused.
//! * Encrypted data and shrouded key bags under PBES2 (PBKDF2 with AES-CBC,
//!   what OpenSSL 3 and current Windows write), or under the older PKCS#12
//!   schemes: triple DES, which older exports use for keys, and 40- or 128-bit
//!   RC2, which they use for certificates. RC4 is refused.
//! * Certificate bags with X.509 certificates, and key bags, shrouded or not.
//!
//! The key is paired with the certificate whose public key it has. The other
//! certificates in the file are the chain.

use cms::content_info::ContentInfo;
use cms::encrypted_data::EncryptedData;
use der::asn1::{BitString, ObjectIdentifier, OctetString};
use der::{Decode, Encode, Sequence};
use x509_cert::spki::AlgorithmIdentifierOwned;
use zeroize::Zeroizing;

use crate::sign_cms::KeyKind;

const ID_DATA: &str = "1.2.840.113549.1.7.1";
const ID_ENCRYPTED_DATA: &str = "1.2.840.113549.1.7.6";
const PBES2: &str = "1.2.840.113549.1.5.13";
const PBMAC1: &str = "1.2.840.113549.1.5.14";
const RSA: &str = "1.2.840.113549.1.1.1";
const EC_PUBLIC_KEY: &str = "1.2.840.10045.2.1";
const P256: &str = "1.2.840.10045.3.1.7";
const P384: &str = "1.3.132.0.34";

/// Why a certificate file cannot sign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The MAC did not match, or a decryption failed its padding.
    WrongPassword,
    /// The key is RSA; see the module header.
    Rsa,
    /// No key in the file, or none belonging to its certificates.
    NoKey,
    /// A scheme, curve or structure this does not read, named.
    Unsupported(String),
    /// Not a PKCS#12 file, or a damaged one.
    Malformed(String),
}

impl Refusal {
    /// The sentence a reader is shown.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Self::WrongPassword => said!("The password is not the one this certificate file was saved with."),
            // linux-difference: signing
            Self::Rsa => said!("This certificate file holds an RSA key, and tpdf signs from a file only with an ECDSA key. \
                          A smart card or token with this certificate can sign."),
            Self::NoKey => said!("This certificate file has no private key for its certificate, so it cannot sign."),
            Self::Unsupported(what) => said!("This certificate file uses {what}, which tpdf does not read."),
            Self::Malformed(why) => said!("This is not a certificate file tpdf can read: {why}."),
        }
    }
}

/// A certificate file opened with its password: the signer, the chain, and
/// the key, for one signature.
pub struct Opened {
    /// The signer's certificate, DER.
    pub certificate: Vec<u8>,
    /// The file's other certificates, DER.
    pub chain: Vec<Vec<u8>>,
    key: EcKey,
}

enum EcKey {
    P256(p256::ecdsa::SigningKey),
    P384(p384::ecdsa::SigningKey),
}

impl Opened {
    /// The key's kind.
    #[must_use]
    pub fn kind(&self) -> KeyKind {
        match self.key {
            EcKey::P256(_) => KeyKind::P256,
            EcKey::P384(_) => KeyKind::P384,
        }
    }

    /// Signs one SHA-256 digest, DER as CMS wants it.
    ///
    /// # Errors
    ///
    /// The curve library refusing, which a valid key does not.
    pub fn sign(&self, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        use ecdsa::signature::hazmat::PrehashSigner as _;
        let refused = |e: ecdsa::Error| said!("the key did not sign: {e}");
        Ok(match &self.key {
            EcKey::P256(key) => {
                let signature: p256::ecdsa::Signature =
                    key.sign_prehash(digest).map_err(refused)?;
                signature.to_der().as_bytes().to_vec()
            }
            EcKey::P384(key) => {
                let signature: p384::ecdsa::Signature =
                    key.sign_prehash(digest).map_err(refused)?;
                signature.to_der().as_bytes().to_vec()
            }
        })
    }
}

/// `EncryptedPrivateKeyInfo`, read here rather than through `pkcs8`, whose
/// type accepts only PBES1 and PBES2 and not the PKCS#12 schemes.
#[derive(Sequence)]
struct EncryptedKey {
    algorithm: AlgorithmIdentifierOwned,
    data: OctetString,
}

/// SEC 1's `ECPrivateKey`.
#[derive(Sequence)]
struct EcPrivateKey {
    version: u8,
    private_key: OctetString,
    #[asn1(context_specific = "0", optional = "true", tag_mode = "EXPLICIT")]
    parameters: Option<ObjectIdentifier>,
    #[asn1(context_specific = "1", optional = "true", tag_mode = "EXPLICIT")]
    public_key: Option<BitString>,
}

fn malformed(what: &str) -> impl Fn(der::Error) -> Refusal + '_ {
    move |e| Refusal::Malformed(format!("{what} ({e})"))
}

/// Opens a certificate file with its password.
///
/// # Errors
///
/// See [`Refusal`].
pub fn open(file: &[u8], password: &str) -> Result<Opened, Refusal> {
    let pfx = pkcs12::pfx::Pfx::from_der(file).map_err(malformed("the outer structure"))?;
    let auth = data_of(&pfx.auth_safe)?;
    if let Some(mac) = &pfx.mac_data {
        check_mac(mac, &auth, password)?;
    }
    let safes = Vec::<ContentInfo>::from_der(&auth).map_err(malformed("the list of contents"))?;
    let (mut certificates, mut keys) = (Vec::new(), Vec::new());
    for safe in &safes {
        let contents: Zeroizing<Vec<u8>> = match safe.content_type.to_string().as_str() {
            ID_DATA => Zeroizing::new(data_of(safe)?),
            ID_ENCRYPTED_DATA => {
                let encrypted = safe
                    .content
                    .decode_as::<EncryptedData>()
                    .map_err(malformed("encrypted contents"))?;
                let info = encrypted.enc_content_info;
                let bytes = info.encrypted_content.ok_or_else(|| {
                    Refusal::Malformed(said!("encrypted contents with no content"))
                })?;
                decrypt(&info.content_enc_alg, bytes.as_bytes(), password)?
            }
            other => return Err(Refusal::Unsupported(said!("contents of type {other}"))),
        };
        let bags = pkcs12::safe_bag::SafeContents::from_der(&contents)
            .map_err(malformed("a list of bags"))?;
        for bag in bags {
            let value = bag_value(&bag)?;
            match bag.bag_id {
                pkcs12::PKCS_12_CERT_BAG_OID => {
                    let cert = pkcs12::cert_type::CertBag::from_der(value)
                        .map_err(malformed("a certificate bag"))?;
                    if cert.cert_id == pkcs12::PKCS_12_X509_CERT_OID {
                        certificates.push(cert.cert_value.as_bytes().to_vec());
                    }
                }
                pkcs12::PKCS_12_PKCS8_KEY_BAG_OID => {
                    let shrouded = EncryptedKey::from_der(value).map_err(malformed("a key bag"))?;
                    keys.push(decrypt(
                        &shrouded.algorithm,
                        shrouded.data.as_bytes(),
                        password,
                    )?);
                }
                pkcs12::PKCS_12_KEY_BAG_OID => keys.push(Zeroizing::new(value.to_vec())),
                _ => {}
            }
        }
    }
    let key = match keys.first() {
        Some(key) => ec_key(key)?,
        None => return Err(Refusal::NoKey),
    };
    let public = match &key {
        EcKey::P256(k) => k.verifying_key().to_sec1_point(false).as_bytes().to_vec(),
        EcKey::P384(k) => k.verifying_key().to_sec1_point(false).as_bytes().to_vec(),
    };
    let signer = certificates
        .iter()
        .position(|der| public_key_of(der).as_deref() == Some(public.as_slice()))
        .ok_or(Refusal::NoKey)?;
    let certificate = certificates.remove(signer);
    Ok(Opened {
        certificate,
        chain: certificates,
        key,
    })
}

/// A bag's value without its `[0] EXPLICIT` wrapper.
///
/// `pkcs12` 0.1 decodes `bagValue` as the whole tagged field and encodes it as
/// the bare content, so what it hands back still has the tag on: measured on
/// 2026-10-14, every certificate bag OpenSSL wrote failed to parse as a
/// `CertBag` until this unwrapped it.
fn bag_value(bag: &pkcs12::safe_bag::SafeBag) -> Result<&[u8], Refusal> {
    Ok(der::AnyRef::from_der(&bag.bag_value)
        .map_err(malformed("a bag"))?
        .value())
}

/// The bytes of an `id-data` content: the OCTET STRING inside.
fn data_of(info: &ContentInfo) -> Result<Vec<u8>, Refusal> {
    if info.content_type.to_string() != ID_DATA {
        return Err(Refusal::Unsupported(said!(
            "outer contents of type {kind}",
            kind = info.content_type
        )));
    }
    Ok(info
        .content
        .decode_as::<OctetString>()
        .map_err(malformed("data contents"))?
        .as_bytes()
        .to_vec())
}

/// A certificate's public key bits, or `None` for one that does not parse.
fn public_key_of(certificate: &[u8]) -> Option<Vec<u8>> {
    let parsed = x509_cert::Certificate::from_der(certificate).ok()?;
    Some(
        parsed
            .tbs_certificate
            .subject_public_key_info
            .subject_public_key
            .raw_bytes()
            .to_vec(),
    )
}

/// The file's MAC, against the password.
fn check_mac(mac: &pkcs12::mac_data::MacData, auth: &[u8], password: &str) -> Result<(), Refusal> {
    use hmac::Mac as _;
    use pkcs12::kdf::{derive_key_utf8, Pkcs12KeyType};

    let salt = mac.mac_salt.as_bytes();
    let rounds = mac.iterations;
    let expected = mac.mac.digest.as_bytes();
    let unreadable = |_| {
        Refusal::Unsupported(said!(
            "a password with characters outside the Basic Multilingual Plane"
        ))
    };
    macro_rules! with {
        ($digest:ty, $len:expr) => {{
            let key = Zeroizing::new(
                derive_key_utf8::<$digest>(password, salt, Pkcs12KeyType::Mac, rounds, $len)
                    .map_err(unreadable)?,
            );
            let mut hmac = hmac::Hmac::<$digest>::new_from_slice(&key)
                .map_err(|_| Refusal::Malformed(said!("a MAC key of no length")))?;
            hmac.update(auth);
            hmac.verify_slice(expected)
                .map_err(|_| Refusal::WrongPassword)
        }};
    }
    match mac.mac.algorithm.oid.to_string().as_str() {
        "1.3.14.3.2.26" => with!(sha1::Sha1, 20),
        "2.16.840.1.101.3.4.2.1" => with!(sha2_10::Sha256, 32),
        "2.16.840.1.101.3.4.2.2" => with!(sha2_10::Sha384, 48),
        "2.16.840.1.101.3.4.2.3" => with!(sha2_10::Sha512, 64),
        PBMAC1 => Err(Refusal::Unsupported(said!("a PBMAC1 integrity check"))),
        other => Err(Refusal::Unsupported(said!(
            "an integrity check with the hash {other}"
        ))),
    }
}

/// Decrypts one encrypted content or key under the scheme `algorithm` names.
fn decrypt(
    algorithm: &AlgorithmIdentifierOwned,
    data: &[u8],
    password: &str,
) -> Result<Zeroizing<Vec<u8>>, Refusal> {
    use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, InnerIvInit, KeyIvInit};
    use pkcs12::kdf::{derive_key_utf8, Pkcs12KeyType};

    let oid = algorithm.oid.to_string();
    if oid == PBES2 {
        // `pkcs5` reads the scheme from the borrowed form of the identifier.
        let encoded = algorithm
            .to_der()
            .map_err(malformed("an encryption scheme"))?;
        let borrowed = x509_cert::spki::AlgorithmIdentifierRef::from_der(&encoded)
            .map_err(malformed("an encryption scheme"))?;
        let scheme = pkcs5::EncryptionScheme::try_from(borrowed)
            .map_err(|e| Refusal::Unsupported(said!("the encryption scheme it names ({e})")))?;
        return scheme
            .decrypt(password.as_bytes(), data)
            .map(Zeroizing::new)
            .map_err(|_| Refusal::WrongPassword);
    }
    let params = algorithm
        .parameters
        .as_ref()
        .ok_or_else(|| Refusal::Malformed(said!("an encryption scheme with no parameters")))?
        .decode_as::<pkcs12::pbe_params::Pkcs12PbeParams>()
        .map_err(malformed("encryption parameters"))?;
    let (salt, rounds) = (params.salt.as_bytes(), params.iterations);
    let unreadable = |_| {
        Refusal::Unsupported(said!(
            "a password with characters outside the Basic Multilingual Plane"
        ))
    };
    let derive = |kind, len| -> Result<Zeroizing<Vec<u8>>, Refusal> {
        Ok(Zeroizing::new(
            derive_key_utf8::<sha1::Sha1>(password, salt, kind, rounds, len).map_err(unreadable)?,
        ))
    };
    let iv = derive(Pkcs12KeyType::Iv, 8)?;
    let padding = |_| Refusal::WrongPassword;
    let plain = match oid.as_str() {
        // pbeWithSHAAnd3-KeyTripleDES-CBC, pbeWithSHAAnd2-KeyTripleDES-CBC
        "1.2.840.113549.1.12.1.3" => {
            let key = derive(Pkcs12KeyType::EncryptionKey, 24)?;
            cbc::Decryptor::<des::TdesEde3>::new_from_slices(&key, &iv)
                .map_err(|_| Refusal::Malformed(said!("a triple-DES key of the wrong length")))?
                .decrypt_padded_vec_mut::<Pkcs7>(data)
                .map_err(padding)?
        }
        "1.2.840.113549.1.12.1.4" => {
            let key = derive(Pkcs12KeyType::EncryptionKey, 16)?;
            cbc::Decryptor::<des::TdesEde2>::new_from_slices(&key, &iv)
                .map_err(|_| Refusal::Malformed(said!("a triple-DES key of the wrong length")))?
                .decrypt_padded_vec_mut::<Pkcs7>(data)
                .map_err(padding)?
        }
        // pbeWithSHAAnd128BitRC2-CBC, pbewithSHAAnd40BitRC2-CBC
        "1.2.840.113549.1.12.1.5" | "1.2.840.113549.1.12.1.6" => {
            let bits = if oid.ends_with(".5") { 128 } else { 40 };
            let key = derive(Pkcs12KeyType::EncryptionKey, bits / 8)?;
            let cipher = rc2::Rc2::new_with_eff_key_len(&key, bits);
            cbc::Decryptor::<rc2::Rc2>::inner_iv_slice_init(cipher, &iv)
                .map_err(|_| Refusal::Malformed(said!("an RC2 IV of the wrong length")))?
                .decrypt_padded_vec_mut::<Pkcs7>(data)
                .map_err(padding)?
        }
        "1.2.840.113549.1.12.1.1" | "1.2.840.113549.1.12.1.2" => {
            return Err(Refusal::Unsupported(said!("RC4 encryption")))
        }
        other => return Err(Refusal::Unsupported(said!("the encryption scheme {other}"))),
    };
    Ok(Zeroizing::new(plain))
}

/// The ECDSA key in a PKCS#8 `PrivateKeyInfo`.
fn ec_key(info: &[u8]) -> Result<EcKey, Refusal> {
    let info =
        pkcs12::safe_bag::PrivateKeyInfo::from_der(info).map_err(malformed("a private key"))?;
    match info.algorithm.oid.to_string().as_str() {
        RSA => return Err(Refusal::Rsa),
        EC_PUBLIC_KEY => {}
        other => return Err(Refusal::Unsupported(said!("a key of type {other}"))),
    }
    let curve = info
        .algorithm
        .parameters
        .as_ref()
        .and_then(|any| any.decode_as::<ObjectIdentifier>().ok())
        .map(|oid| oid.to_string());
    let inner = Zeroizing::new(info.private_key.as_bytes().to_vec());
    let sec1 = EcPrivateKey::from_der(&inner).map_err(malformed("an EC private key"))?;
    let scalar = Zeroizing::new(sec1.private_key.as_bytes().to_vec());
    let curve = curve.or_else(|| sec1.parameters.map(|oid| oid.to_string()));
    let bad = |_| Refusal::Malformed(said!("an EC private key that is not a valid scalar"));
    match curve.as_deref() {
        Some(P256) => Ok(EcKey::P256(
            p256::ecdsa::SigningKey::from_slice(&scalar).map_err(bad)?,
        )),
        Some(P384) => Ok(EcKey::P384(
            p384::ecdsa::SigningKey::from_slice(&scalar).map_err(bad)?,
        )),
        Some(other) => Err(Refusal::Unsupported(said!("the curve {other}"))),
        None => Err(Refusal::Malformed(said!(
            "an EC private key that names no curve"
        ))),
    }
}

#[cfg(test)]
mod tests;
