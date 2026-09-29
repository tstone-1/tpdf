//! Ephemeral native P-256 keys for the private-PKI integration check.
//!
//! Imports only synthetic fixture key bytes, never creates a named/persistent
//! key, and never opens a certificate store or keychain. These adapters exercise
//! SecKeyCreateSignature / NCryptSignHash; identity discovery is a separate test.

use std::cell::Cell;
use std::rc::Rc;
use tpdf_lib::sign_cms::{Key, KeyKind};

#[derive(Default)]
pub struct Calls {
    pub signatures: Cell<usize>,
    pub live_keys: Cell<usize>,
}

struct Native {
    key: platform::Handle,
    calls: Rc<Calls>,
}

impl Key for Native {
    fn sign_digest(&self, kind: KeyKind, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        if !matches!(kind, KeyKind::P256) {
            return Err("the private PKI uses only P-256".into());
        }
        self.calls.signatures.set(self.calls.signatures.get() + 1);
        self.key.sign(digest)
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        self.calls.live_keys.set(self.calls.live_keys.get() - 1);
    }
}

/// Import the fixture's key into an unnamed OS handle, released when dropped.
pub fn p256(seed: u8, calls: Rc<Calls>) -> Result<Box<dyn Key>, String> {
    let soft =
        p256::ecdsa::SigningKey::from_bytes(&[seed; 32].into()).map_err(|e| e.to_string())?;
    let key = platform::Handle::import(&soft)?;
    calls.live_keys.set(calls.live_keys.get() + 1);
    Ok(Box::new(Native { key, calls }))
}

#[cfg(target_os = "macos")]
mod platform {
    use core_foundation::base::TCFType as _;
    use core_foundation::data::CFData;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use security_framework::key::{Algorithm, SecKey};
    use security_framework_sys::item::{
        kSecAttrKeyClass, kSecAttrKeyClassPrivate, kSecAttrKeyType, kSecAttrKeyTypeECSECPrimeRandom,
    };

    pub struct Handle(SecKey);

    impl Handle {
        pub fn import(soft: &p256::ecdsa::SigningKey) -> Result<Self, String> {
            // X9.63 uncompressed public point followed by the private scalar.
            let mut bytes = soft
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()
                .to_vec();
            bytes.extend_from_slice(&soft.to_bytes());
            let attributes = CFDictionary::from_CFType_pairs(&[
                (
                    unsafe { CFString::wrap_under_get_rule(kSecAttrKeyType) },
                    unsafe { CFString::wrap_under_get_rule(kSecAttrKeyTypeECSECPrimeRandom) },
                ),
                (
                    unsafe { CFString::wrap_under_get_rule(kSecAttrKeyClass) },
                    unsafe { CFString::wrap_under_get_rule(kSecAttrKeyClassPrivate) },
                ),
            ]);
            let data = CFData::from_buffer(&bytes);
            let mut error = std::ptr::null_mut();
            let key = unsafe {
                security_framework_sys::key::SecKeyCreateWithData(
                    data.as_concrete_TypeRef(),
                    attributes.as_concrete_TypeRef(),
                    &mut error,
                )
            };
            if key.is_null() {
                if !error.is_null() {
                    unsafe { core_foundation::base::CFRelease(error.cast()) };
                }
                return Err("SecKeyCreateWithData refused the synthetic key".into());
            }
            Ok(Self(unsafe { SecKey::wrap_under_create_rule(key) }))
        }

        pub fn sign(&self, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
            self.0
                .create_signature(Algorithm::ECDSASignatureDigestX962SHA256, digest)
                .map_err(|e| e.to_string())
        }
    }
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Security::Cryptography::{
        NCryptFreeObject, NCryptImportKey, NCryptOpenStorageProvider, NCryptSignHash,
        BCRYPT_ECCPRIVATE_BLOB, BCRYPT_ECDSA_PRIVATE_P256_MAGIC, MS_KEY_STORAGE_PROVIDER,
        NCRYPT_SILENT_FLAG,
    };

    pub struct Handle {
        provider: usize,
        key: usize,
    }

    fn checked(status: i32) -> Result<(), String> {
        if status == 0 {
            Ok(())
        } else {
            Err(format!("CNG failed: 0x{status:08X}"))
        }
    }

    impl Handle {
        pub fn import(soft: &p256::ecdsa::SigningKey) -> Result<Self, String> {
            let mut handle = Self {
                provider: 0,
                key: 0,
            };
            checked(unsafe {
                NCryptOpenStorageProvider(&mut handle.provider, MS_KEY_STORAGE_PROVIDER, 0)
            })?;
            // BCRYPT_ECCKEY_BLOB header, big-endian X, Y and private scalar.
            let point = soft.verifying_key().to_encoded_point(false);
            let mut blob = BCRYPT_ECDSA_PRIVATE_P256_MAGIC.to_le_bytes().to_vec();
            blob.extend_from_slice(&32u32.to_le_bytes());
            blob.extend_from_slice(&point.as_bytes()[1..]);
            blob.extend_from_slice(&soft.to_bytes());
            // No key-name parameter: the software KSP keeps this key ephemeral.
            // https://learn.microsoft.com/windows/win32/api/ncrypt/nf-ncrypt-ncryptimportkey
            checked(unsafe {
                NCryptImportKey(
                    handle.provider,
                    0,
                    BCRYPT_ECCPRIVATE_BLOB,
                    std::ptr::null(),
                    &mut handle.key,
                    blob.as_ptr(),
                    u32::try_from(blob.len()).map_err(|e| e.to_string())?,
                    NCRYPT_SILENT_FLAG,
                )
            })?;
            Ok(handle)
        }

        pub fn sign(&self, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
            let mut bytes = [0u8; 64];
            let mut size = 0;
            checked(unsafe {
                NCryptSignHash(
                    self.key,
                    std::ptr::null(),
                    digest.as_ptr(),
                    32,
                    bytes.as_mut_ptr(),
                    64,
                    &mut size,
                    NCRYPT_SILENT_FLAG,
                )
            })?;
            if size != 64 {
                return Err("CNG returned an unexpected P-256 signature size".into());
            }
            tpdf_lib::sign_cms::ecdsa_der(&bytes)
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                if self.key != 0 {
                    NCryptFreeObject(self.key);
                }
                if self.provider != 0 {
                    NCryptFreeObject(self.provider);
                }
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    pub struct Handle;
    impl Handle {
        pub fn import(_: &p256::ecdsa::SigningKey) -> Result<Self, String> {
            Err("native signing is available only on macOS and Windows".into())
        }
        pub fn sign(&self, _: &[u8; 32]) -> Result<Vec<u8>, String> {
            unreachable!()
        }
    }
}
