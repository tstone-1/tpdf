//! Ephemeral native P-256 keys for the private-PKI integration check.
//!
//! Imports only synthetic fixture key bytes, never creates a named/persistent
//! key, and never opens a certificate store or keychain. These adapters exercise
//! SecKeyCreateSignature / NCryptSignHash, and on Linux `pkcs11::sign_with`
//! against a SoftHSM token of this process's own; identity discovery is a
//! separate test.

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
                .to_sec1_point(false)
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
            let point = soft.verifying_key().to_sec1_point(false);
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

/// Linux: a SoftHSM token in a folder of this process's own, the key a session
/// object that the token forgets when the session closes, and the signature
/// made by `tpdf_lib::pkcs11::sign_with`, the function the shipped keystore
/// signs through.
#[cfg(target_os = "linux")]
mod platform {
    use std::sync::OnceLock;

    use cryptoki::context::Pkcs11;
    use cryptoki::object::{Attribute, KeyType, ObjectClass, ObjectHandle};
    use cryptoki::session::{Session, SessionState, UserType};
    use cryptoki::types::AuthPin;
    use tpdf_lib::sign_cms::KeyKind;

    const SOFTHSM: [&str; 3] = [
        "/usr/lib/x86_64-linux-gnu/softhsm/libsofthsm2.so",
        "/usr/lib/softhsm/libsofthsm2.so",
        "/usr/lib64/pkcs11/libsofthsm2.so",
    ];
    /// The DER of P-256's object identifier, `CKA_EC_PARAMS` for that curve.
    const P256_PARAMS: [u8; 10] = [0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
    const USER_PIN: &str = "4711";

    fn pin(text: &str) -> AuthPin {
        AuthPin::new(text.to_string().into())
    }

    /// The token, set up once per process: `C_Initialize` reads the
    /// configuration once.
    fn token() -> Result<&'static Pkcs11, String> {
        static TOKEN: OnceLock<Result<Pkcs11, String>> = OnceLock::new();
        TOKEN
            .get_or_init(|| {
                let module = SOFTHSM
                    .iter()
                    .find(|path| std::path::Path::new(path).exists())
                    .ok_or("SoftHSM is not installed (the package softhsm2)")?;
                let dir =
                    std::env::temp_dir().join(format!("tpdf-cli-softhsm-{}", std::process::id()));
                let tokens = dir.join("tokens");
                std::fs::create_dir_all(&tokens).map_err(|e| e.to_string())?;
                let conf = dir.join("softhsm2.conf");
                std::fs::write(
                    &conf,
                    format!(
                        "directories.tokendir = {}\nobjectstore.backend = file\n",
                        tokens.display()
                    ),
                )
                .map_err(|e| e.to_string())?;
                std::env::set_var("SOFTHSM2_CONF", &conf);
                let context = tpdf_lib::pkcs11::open(module)?;
                let fail = |e: cryptoki::error::Error| e.to_string();
                let slot = context.get_all_slots().map_err(fail)?[0];
                context
                    .init_token(slot, &pin("so-pin"), "tpdf cli")
                    .map_err(fail)?;
                let session = context.open_rw_session(slot).map_err(fail)?;
                session
                    .login(UserType::So, Some(&pin("so-pin")))
                    .map_err(fail)?;
                session.init_pin(&pin(USER_PIN)).map_err(fail)?;
                Ok(context)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub struct Handle {
        session: Session,
        key: ObjectHandle,
    }

    impl Handle {
        pub fn import(soft: &p256::ecdsa::SigningKey) -> Result<Self, String> {
            let context = token()?;
            let fail = |e: cryptoki::error::Error| e.to_string();
            let slot = context
                .get_slots_with_initialized_token()
                .map_err(fail)?
                .into_iter()
                .next()
                .ok_or("the SoftHSM token is gone")?;
            let session = context.open_rw_session(slot).map_err(fail)?;
            let logged_in = session.get_session_info().is_ok_and(|now| {
                matches!(
                    now.session_state(),
                    SessionState::RwUser | SessionState::RoUser
                )
            });
            if !logged_in {
                session
                    .login(UserType::User, Some(&pin(USER_PIN)))
                    .map_err(fail)?;
            }
            let key = session
                .create_object(&[
                    Attribute::Class(ObjectClass::PRIVATE_KEY),
                    Attribute::KeyType(KeyType::EC),
                    Attribute::EcParams(P256_PARAMS.to_vec()),
                    Attribute::Value(soft.to_bytes().to_vec()),
                    Attribute::Token(false),
                    Attribute::Sign(true),
                ])
                .map_err(fail)?;
            Ok(Self { session, key })
        }

        pub fn sign(&self, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
            tpdf_lib::pkcs11::sign_with(&self.session, self.key, KeyKind::P256, digest)
        }
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod platform {
    pub struct Handle;
    impl Handle {
        pub fn import(_: &p256::ecdsa::SigningKey) -> Result<Self, String> {
            Err("native signing is available only on macOS, Windows and Linux".into())
        }
        pub fn sign(&self, _: &[u8; 32]) -> Result<Vec<u8>, String> {
            unreachable!()
        }
    }
}
