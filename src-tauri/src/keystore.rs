//! The reader's own signing certificates, and their keys, through the OS.
//!
//! ## The key never leaves the OS, and that is the whole design
//!
//! tpdf holds no private key and could not hold one: nothing here reads a key,
//! exports one, or asks for a PIN. What this module does is find the
//! certificates the OS says have a key behind them, and ask the OS to sign one
//! SHA-256 digest with that key --- `SecKeyCreateSignature` on macOS,
//! `NCryptSignHash` on Windows. A smart card, a token or a key the OS has
//! marked as needing confirmation works the same way, because the OS does the
//! asking: **a prompt that appears while signing is the OS's, and it is
//! correct**. tpdf never sees what the reader types into it.
//!
//! This runs in the **app process**, which is the one that holds the reader's
//! authority --- the worker is sandboxed and must not be able to reach a key.
//! Nothing here reads a document: the inputs are certificates from the reader's
//! own store and a digest `sign_cms.rs` computed.
//!
//! ## Where the chain comes from
//!
//! The certificates above the signer's are whatever the OS chain API returns,
//! with network retrieval turned off: `SecTrust` with fetching disallowed on
//! macOS, `CertGetCertificateChain` with cache-only retrieval and AIA disabled
//! on Windows. Building the chain is not a trust decision here --- nothing is
//! concluded from whether it ends at a root --- it is only what a reader's
//! verifier will want in the `certificates` set. Fetching intermediates over
//! the network would be a second network authority beside the updater
//! (`docs/THREAT-MODEL.md` §T9), and that belongs to Phase 6 step 3.

use crate::sign_cms::{Key, KeyKind};

/// One certificate with a private key behind it.
pub struct Identity {
    /// The signer's certificate, DER.
    pub certificate: Vec<u8>,
    /// What the OS chain API returned above it, DER, the signer left out.
    pub chain: Vec<Vec<u8>>,
    handle: platform::Handle,
}

impl Identity {
    /// A stable name for this identity: the SHA-256 of its certificate, hex.
    ///
    /// What the chooser sends back. Not a secret and not an index --- a list
    /// can change between the listing and the click, and a hash names the same
    /// certificate either way or names none.
    #[must_use]
    pub fn id(&self) -> String {
        id_of(&self.certificate)
    }
}

/// The identifier [`Identity::id`] gives a certificate.
#[must_use]
pub fn id_of(certificate: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(certificate)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl Key for Identity {
    fn sign_digest(&self, kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        platform::sign(&self.handle, kind, digest)
    }
}

/// Every certificate in the reader's store that has a private key behind it.
///
/// Unfiltered: whether each one can sign is `sign_cms::usable`'s question,
/// which is pure and tested, and asked by the caller.
///
/// # Errors
///
/// The store could not be opened or searched.
pub fn identities() -> Result<Vec<Identity>, String> {
    platform::identities()
}

/// The identity whose certificate hashes to `id`.
///
/// # Errors
///
/// The store could not be searched, or no certificate in it has that hash ---
/// which is a certificate removed since the list was shown.
pub fn find(id: &str) -> Result<Identity, String> {
    identities()?
        .into_iter()
        .find(|identity| identity.id() == id)
        .ok_or_else(|| {
            "that certificate is no longer in your keychain or certificate store".to_string()
        })
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{Identity, KeyKind};
    use security_framework::certificate::SecCertificate;
    use security_framework::identity::SecIdentity;
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit, Reference, SearchResult};
    use security_framework::key::{Algorithm, SecKey};
    use security_framework::policy::SecPolicy;
    use security_framework::trust::SecTrust;

    /// `errSecItemNotFound`: a search that matched nothing, which for a
    /// reader with no signing certificate is the ordinary answer.
    const NOT_FOUND: i32 = -25300;

    pub struct Handle(SecIdentity);

    pub fn identities() -> Result<Vec<Identity>, String> {
        let found = match ItemSearchOptions::new()
            .class(ItemClass::identity())
            .load_refs(true)
            .limit(Limit::All)
            .search()
        {
            Ok(found) => found,
            Err(error) if error.code() == NOT_FOUND => return Ok(Vec::new()),
            Err(error) => return Err(format!("the keychain could not be searched: {error}")),
        };
        let mut out = Vec::new();
        for result in found {
            let SearchResult::Ref(Reference::Identity(identity)) = result else {
                continue;
            };
            let Ok(certificate) = identity.certificate() else {
                continue;
            };
            out.push(Identity {
                certificate: certificate.to_der(),
                chain: chain_of(&certificate),
                handle: Handle(identity),
            });
        }
        Ok(out)
    }

    /// The certificates above `certificate`, as `SecTrust` assembles them
    /// without the network. Empty when no chain could be built.
    #[allow(deprecated)] // `chain()` is macOS 12; the bundle targets 10.13.
    pub fn chain_of(certificate: &SecCertificate) -> Vec<Vec<u8>> {
        let Ok(mut trust) = SecTrust::create_with_certificates(
            std::slice::from_ref(certificate),
            &[SecPolicy::create_x509()],
        ) else {
            return Vec::new();
        };
        let _ = trust.set_network_fetch_allowed(false);
        // The outcome is not a verdict here; evaluating is what assembles the
        // chain, and an untrusted one is still the one a verifier wants.
        let _ = trust.evaluate_with_error();
        (1..trust.certificate_count())
            .filter_map(|at| trust.certificate_at_index(at))
            .map(|certificate| certificate.to_der())
            .collect()
    }

    pub fn sign(handle: &Handle, kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        let key = handle
            .0
            .private_key()
            .map_err(|e| format!("the keychain would not give access to this key: {e}"))?;
        sign_with(&key, kind, digest)
    }

    /// Signs `digest` with `key`. Split from [`sign`] so a test can hand it a
    /// key that lives in no keychain at all.
    pub fn sign_with(key: &SecKey, kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        let algorithm = match kind {
            KeyKind::Rsa(_) => Algorithm::RSASignatureDigestPKCS1v15SHA256,
            KeyKind::P256 | KeyKind::P384 => Algorithm::ECDSASignatureDigestX962SHA256,
        };
        key.create_signature(algorithm, digest)
            .map_err(|e| format!("macOS did not sign: {e}"))
    }
}

#[cfg(windows)]
mod platform {
    use super::{Identity, KeyKind};
    use windows_sys::Win32::Security::Cryptography::{
        CertCloseStore, CertDuplicateCertificateContext, CertEnumCertificatesInStore,
        CertFreeCertificateChain, CertFreeCertificateContext, CertGetCertificateChain,
        CertGetCertificateContextProperty, CertOpenSystemStoreW, CryptAcquireCertificatePrivateKey,
        NCryptFreeObject, NCryptSignHash, BCRYPT_PKCS1_PADDING_INFO, BCRYPT_SHA256_ALGORITHM,
        CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL, CERT_CHAIN_CONTEXT, CERT_CHAIN_DISABLE_AIA,
        CERT_CHAIN_DISABLE_AUTH_ROOT_AUTO_UPDATE, CERT_CHAIN_PARA, CERT_CONTEXT,
        CERT_KEY_PROV_INFO_PROP_ID, CERT_NCRYPT_KEY_SPEC, CRYPT_ACQUIRE_ONLY_NCRYPT_KEY_FLAG,
        NCRYPT_PAD_PKCS1_FLAG,
    };

    /// A certificate context this process holds a reference to.
    pub struct Handle(*const CERT_CONTEXT);

    // The context is reference-counted by crypt32 and immutable once made; it
    // is only read, and freed once, on whichever thread drops it.
    unsafe impl Send for Handle {}

    impl Drop for Handle {
        fn drop(&mut self) {
            // Obtained from `CertDuplicateCertificateContext`, so ours to free.
            unsafe {
                CertFreeCertificateContext(self.0);
            }
        }
    }

    fn encoded(context: *const CERT_CONTEXT) -> Vec<u8> {
        // A context crypt32 handed back is valid for as long as it is held,
        // and `pbCertEncoded` is `cbCertEncoded` bytes of it.
        unsafe {
            std::slice::from_raw_parts((*context).pbCertEncoded, (*context).cbCertEncoded as usize)
                .to_vec()
        }
    }

    pub fn identities() -> Result<Vec<Identity>, String> {
        let name: Vec<u16> = "MY".encode_utf16().chain(Some(0)).collect();
        // The current user's personal store, which is where Windows keeps
        // certificates that have a private key --- smart cards' included, once
        // their minidriver has propagated them.
        let store = unsafe { CertOpenSystemStoreW(0, name.as_ptr()) };
        if store.is_null() {
            return Err(format!(
                "the certificate store could not be opened: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut out = Vec::new();
        let mut context: *mut CERT_CONTEXT = std::ptr::null_mut();
        loop {
            // Takes ownership of the previous context and returns the next.
            context = unsafe { CertEnumCertificatesInStore(store, context) };
            if context.is_null() {
                break;
            }
            let mut size = 0u32;
            // A certificate with no key-provider property has no private key
            // this user can reach, so it cannot sign and is not an identity.
            let keyed = unsafe {
                CertGetCertificateContextProperty(
                    context,
                    CERT_KEY_PROV_INFO_PROP_ID,
                    std::ptr::null_mut(),
                    &mut size,
                )
            } != 0;
            if !keyed {
                continue;
            }
            out.push(Identity {
                certificate: encoded(context),
                chain: chain_of(context),
                handle: Handle(unsafe { CertDuplicateCertificateContext(context) }),
            });
        }
        unsafe {
            CertCloseStore(store, 0);
        }
        Ok(out)
    }

    /// The certificates above `context`, from the machine's caches only.
    fn chain_of(context: *const CERT_CONTEXT) -> Vec<Vec<u8>> {
        let para = CERT_CHAIN_PARA {
            cbSize: std::mem::size_of::<CERT_CHAIN_PARA>() as u32,
            ..Default::default()
        };
        let mut chain: *mut CERT_CHAIN_CONTEXT = std::ptr::null_mut();
        let flags = CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL
            | CERT_CHAIN_DISABLE_AIA
            | CERT_CHAIN_DISABLE_AUTH_ROOT_AUTO_UPDATE;
        let built = unsafe {
            CertGetCertificateChain(
                std::ptr::null_mut(),
                context,
                std::ptr::null(),
                (*context).hCertStore,
                &para,
                flags,
                std::ptr::null(),
                &mut chain,
            )
        } != 0;
        if !built || chain.is_null() {
            return Vec::new();
        }
        let mut out = Vec::new();
        // The first simple chain is the one ending at the end certificate;
        // its first element is the end certificate itself.
        unsafe {
            if (*chain).cChain > 0 {
                let simple = *(*chain).rgpChain;
                for at in 1..(*simple).cElement as usize {
                    let element = *(*simple).rgpElement.add(at);
                    out.push(encoded((*element).pCertContext));
                }
            }
            CertFreeCertificateChain(chain);
        }
        out
    }

    pub fn sign(handle: &Handle, kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        let mut key = 0usize;
        let mut spec = 0u32;
        let mut free = 0;
        // No silent flag: a key that needs the reader's PIN or confirmation
        // asks for it here, in Windows' own dialog.
        let acquired = unsafe {
            CryptAcquireCertificatePrivateKey(
                handle.0,
                CRYPT_ACQUIRE_ONLY_NCRYPT_KEY_FLAG,
                std::ptr::null(),
                &mut key,
                &mut spec,
                &mut free,
            )
        } != 0;
        if !acquired {
            return Err(format!(
                "Windows would not give access to this certificate's key (only keys held \
                 through CNG can sign): {}",
                std::io::Error::last_os_error()
            ));
        }
        let result = if spec == CERT_NCRYPT_KEY_SPEC {
            sign_with(key, kind, digest)
        } else {
            Err("this certificate's key is held by a provider tpdf does not use".into())
        };
        if free != 0 {
            unsafe {
                NCryptFreeObject(key);
            }
        }
        result
    }

    fn sign_with(key: usize, kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        let padding = BCRYPT_PKCS1_PADDING_INFO {
            pszAlgId: BCRYPT_SHA256_ALGORITHM,
        };
        let (info, flags) = match kind {
            KeyKind::Rsa(_) => (
                std::ptr::from_ref(&padding).cast::<core::ffi::c_void>(),
                NCRYPT_PAD_PKCS1_FLAG,
            ),
            KeyKind::P256 | KeyKind::P384 => (std::ptr::null(), 0),
        };
        let mut size = 0u32;
        let status = unsafe {
            NCryptSignHash(
                key,
                info,
                digest.as_ptr(),
                32,
                std::ptr::null_mut(),
                0,
                &mut size,
                flags,
            )
        };
        if status != 0 {
            return Err(format!("Windows did not sign (0x{status:08X})"));
        }
        let mut signature = vec![0u8; size as usize];
        let status = unsafe {
            NCryptSignHash(
                key,
                info,
                digest.as_ptr(),
                32,
                signature.as_mut_ptr(),
                size,
                &mut size,
                flags,
            )
        };
        if status != 0 {
            return Err(format!("Windows did not sign (0x{status:08X})"));
        }
        signature.truncate(size as usize);
        match kind {
            KeyKind::Rsa(_) => Ok(signature),
            // CNG answers ECDSA as r || s; CMS wants the DER form.
            KeyKind::P256 | KeyKind::P384 => crate::sign_cms::ecdsa_der(&signature),
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use super::{Identity, KeyKind};

    pub struct Handle;

    pub fn identities() -> Result<Vec<Identity>, String> {
        Err("signing with a certificate is not available on this platform".into())
    }

    pub fn sign(_: &Handle, _: KeyKind, _: &[u8; 32]) -> Result<Vec<u8>, String> {
        Err("signing with a certificate is not available on this platform".into())
    }
}

#[cfg(test)]
mod tests;
