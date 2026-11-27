//! Smart cards and tokens through PKCS#11: how Linux signs.
//!
//! macOS and Windows sign through a system service that holds the key and asks
//! for its PIN (`keystore.rs`). Linux has no such service, so tpdf talks to the
//! token itself, through p11-kit's proxy module, which loads every PKCS#11
//! module the system has registered: OpenSC for smart cards, a vendor's module
//! for its token, SoftHSM in the tests. Decided 2026-10-14 (`docs/PLAN.md` §1).
//!
//! ## What tpdf holds now that it did not on the other two
//!
//! **The PIN.** `C_Login` takes it from the caller, so tpdf asks for it
//! ([`PinAsker`]): the application in a native GTK dialog, never the webview,
//! and the command-line tool on the terminal with echo off. It is kept in a
//! `SecretString`, which is zeroed when dropped after each login.
//! A token with a PIN pad (`CKF_PROTECTED_AUTHENTICATION_PATH`) is logged in
//! with no PIN, and the pad asks.
//!
//! **Not the key.** The private key stays on the token, which signs one SHA-256
//! digest per signature, as `SecKeyCreateSignature` and `NCryptSignHash` do.
//!
//! ## How a certificate is matched to its key
//!
//! By `CKA_ID`, the attribute PKCS#11 tokens use to tie a certificate to its
//! key. A certificate with no ID is not listed. The key itself is usually
//! private (`CKA_PRIVATE`) and so invisible before login, which is why the
//! listing cannot confirm that a key is there; a certificate whose key is
//! missing is found out at signing, after the PIN. The system's trust module
//! (p11-kit-trust) lists CA certificates with no key; `sign_cms::usable`
//! refuses those, since none may sign.

use std::sync::OnceLock;

use cryptoki::context::{CInitializeArgs, CInitializeFlags, Pkcs11};
use cryptoki::mechanism::Mechanism;
use cryptoki::object::{Attribute, AttributeType, CertificateType, ObjectClass, ObjectHandle};
use cryptoki::session::{Session, SessionState, UserType};
use cryptoki::slot::Slot;
use cryptoki::types::AuthPin;

use crate::sign_cms::KeyKind;

/// Where p11-kit's proxy module is, first match wins: Debian and Ubuntu on
/// x86-64 and on ARM, then Fedora and openSUSE, then the loader's own search.
pub const PROXY: [&str; 4] = [
    "/usr/lib/x86_64-linux-gnu/p11-kit-proxy.so",
    "/usr/lib/aarch64-linux-gnu/p11-kit-proxy.so",
    "/usr/lib64/p11-kit-proxy.so",
    "p11-kit-proxy.so",
];

/// The DER prefix of a SHA-256 `DigestInfo`, which `CKM_RSA_PKCS` signs with
/// the digest after it. `SecKeyCreateSignature` and `NCryptSignHash` add the
/// same bytes themselves when told the hash is SHA-256.
const SHA256_DIGEST_INFO: [u8; 19] = [
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

/// Why the token refused the login, in the reader's words where the reader can
/// act on it. The library's own text for a wrong PIN is two sentences about
/// "authentication ... other than a PIN", which said nothing a reader needs.
fn login_refused(token: &str, error: &cryptoki::error::Error) -> String {
    use cryptoki::error::{Error, RvError};
    match error {
        Error::Pkcs11(RvError::PinIncorrect, _) => said!(
            "the PIN for the token {token:?} is not correct, and nothing was signed. \
             A token locks itself after a few wrong PINs."
        ),
        Error::Pkcs11(RvError::PinLocked, _) => said!(
            "the token {token:?} is locked after too many wrong PINs, and nothing was signed. \
             Its unblocking code (PUK) or its issuer can unlock it."
        ),
        Error::Pkcs11(RvError::PinLenRange, _) => said!(
            "the PIN has the wrong number of characters for the token {token:?}, and nothing was signed"
        ),
        other => said!("the token {token:?} did not accept the PIN: {other}"),
    }
}

/// What the reader is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked<'a> {
    /// A token's PIN, given the token's label.
    Pin(&'a str),
    /// A certificate file's password, given the file's name (`pkcs12.rs`).
    Password(&'a str),
}

/// Asks the reader for a PIN or a password; answers `Ok(None)` when the
/// reader cancelled.
pub type PinAsker = dyn Fn(Asked<'_>) -> Result<Option<AuthPin>, String> + Send + Sync;

static ASKER: OnceLock<Box<PinAsker>> = OnceLock::new();

/// Installs how this process asks for a PIN. Once per process: the
/// application installs its dialog at start, the command-line tool its
/// terminal prompt. A second call is ignored.
pub fn set_pin_asker(asker: Box<PinAsker>) {
    let _ = ASKER.set(asker);
}

/// Asks through the installed asker.
///
/// # Errors
///
/// No asker installed, or the asker's own failure.
pub fn ask(question: Asked<'_>) -> Result<Option<AuthPin>, String> {
    let asker = ASKER.get().ok_or_else(|| match question {
        Asked::Pin(token) => {
            said!("the token {token:?} needs its PIN, and nothing here can ask for it")
        }
        Asked::Password(file) => {
            said!(
                "the certificate file {file:?} needs its password, and nothing here can ask for it"
            )
        }
    })?;
    asker(question)
}

/// A certificate on a token, and where its key is.
#[derive(Debug, Clone)]
pub struct Found {
    /// The certificate, DER.
    pub certificate: Vec<u8>,
    /// Which slot the token is in.
    slot: Slot,
    /// The token's label, for the PIN prompt.
    pub token: String,
    /// `CKA_ID`, shared by the certificate and its key.
    id: Vec<u8>,
}

pub(crate) struct Module {
    pub(crate) context: Pkcs11,
    path: String,
}

/// The proxy module, loaded and initialised once per process.
///
/// # Errors
///
/// No proxy module at any of [`PROXY`], or one that refused to initialise.
pub(crate) fn proxy() -> Result<&'static Module, String> {
    static CONTEXT: OnceLock<Result<Module, String>> = OnceLock::new();
    CONTEXT
        .get_or_init(|| {
            let mut tried = Vec::new();
            for path in PROXY {
                match open(path) {
                    Ok(context) => {
                        return Ok(Module {
                            context,
                            path: path.into(),
                        })
                    }
                    Err(why) => tried.push(why),
                }
            }
            Err(said!(
                "signing with a smart card or token needs p11-kit (the package p11-kit-modules on \
                 Ubuntu and Debian), and it could not be loaded: {tried}",
                tried = tried.join("; ")
            ))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Loads and initialises one PKCS#11 module.
///
/// # Errors
///
/// The module not loading, or `C_Initialize` failing.
pub fn open(path: &str) -> Result<Pkcs11, String> {
    let context = Pkcs11::new(path).map_err(|e| format!("{path}: {e}"))?;
    context
        .initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK))
        .map_err(|e| format!("{path}: C_Initialize: {e}"))?;
    Ok(context)
}

/// Every X.509 certificate with a `CKA_ID` on every initialised token.
///
/// A token that cannot be read is skipped, so one broken reader does not hide
/// the others' certificates.
///
/// # Errors
///
/// The module not listing its slots.
pub fn certificates(context: &Pkcs11) -> Result<Vec<Found>, String> {
    let slots = context
        .get_slots_with_token()
        .map_err(|e| said!("listing smart cards and tokens: {e}"))?;
    let mut found = Vec::new();
    for slot in slots {
        let Ok(info) = context.get_token_info(slot) else {
            continue;
        };
        if !info.token_initialized() {
            continue;
        }
        let Ok(session) = context.open_ro_session(slot) else {
            continue;
        };
        let template = [
            Attribute::Class(ObjectClass::CERTIFICATE),
            Attribute::CertificateType(CertificateType::X_509),
        ];
        let Ok(objects) = session.find_objects(&template) else {
            continue;
        };
        for object in objects {
            let Ok(attributes) =
                session.get_attributes(object, &[AttributeType::Value, AttributeType::Id])
            else {
                continue;
            };
            let (mut certificate, mut id) = (Vec::new(), Vec::new());
            for attribute in attributes {
                match attribute {
                    Attribute::Value(value) => certificate = value,
                    Attribute::Id(value) => id = value,
                    _ => {}
                }
            }
            if !certificate.is_empty() && !id.is_empty() {
                found.push(Found {
                    certificate,
                    slot,
                    token: info.label().trim().to_string(),
                    id,
                });
            }
        }
    }
    Ok(found)
}

/// Signs `digest` with the key that shares `found`'s `CKA_ID`, logging in
/// first if the token wants it.
///
/// The session is closed when this returns. tpdf keeps no other session
/// open, so that logs the token out and the next signature asks for the PIN
/// again; while another session of this process is open, PKCS#11 keeps the
/// token logged in. Keys with CKA_ALWAYS_AUTHENTICATE still require a fresh
/// login after each signature operation is initialized.
///
/// # Errors
///
/// No way to ask for a PIN, the reader cancelling, a wrong PIN, no key with
/// the certificate's ID, or the token refusing to sign.
pub(crate) fn sign(
    module: &Module,
    found: &Found,
    kind: KeyKind,
    digest: &[u8; 32],
) -> Result<Vec<u8>, String> {
    let context = &module.context;
    let session = context
        .open_ro_session(found.slot)
        .map_err(|e| said!("opening the token {token:?}: {e}", token = found.token))?;
    let info = context
        .get_token_info(found.slot)
        .map_err(|e| said!("reading the token {token:?}: {e}", token = found.token))?;
    // Logged in already when another session of this process logged in and is
    // still open: PKCS#11 keeps the login per token, not per session. Asking
    // again would only be refused (CKR_USER_ALREADY_LOGGED_IN).
    let logged_in = session.get_session_info().is_ok_and(|now| {
        matches!(
            now.session_state(),
            SessionState::RoUser | SessionState::RwUser
        )
    });
    let login = |user| {
        let pin = if info.protected_authentication_path() {
            None
        } else {
            Some(ask(Asked::Pin(&found.token))?.ok_or_else(|| said!("signing was cancelled"))?)
        };
        session
            .login(user, pin.as_ref())
            .map_err(|e| login_refused(&found.token, &e))
    };
    if info.login_required() && !logged_in {
        login(UserType::User)?;
    }
    let keys = session
        .find_objects(&[
            Attribute::Class(ObjectClass::PRIVATE_KEY),
            Attribute::Id(found.id.clone()),
        ])
        .map_err(|e| said!("finding the key on {token:?}: {e}", token = found.token))?;
    let key = keys.first().copied().ok_or_else(|| {
        said!(
            "the token {token:?} has this certificate but no key for it",
            token = found.token
        )
    })?;
    let attributes = session
        .get_attributes(key, &[AttributeType::AlwaysAuthenticate])
        .map_err(sign_refused)?;
    if !attributes.contains(&Attribute::AlwaysAuthenticate(true)) {
        return sign_with(&session, key, kind, digest);
    }
    // cryptoki's sign() would initialize a second time, invalidating the
    // context login. Raw ECDSA and RSA-PKCS both require single-part C_Sign.
    // p11-kit's proxy and the SoftHSM test module export this entry point.
    // SAFETY: reopen the same loaded provider that owns the session and key.
    let library = unsafe { libloading::Library::new(&module.path) }.map_err(|e| e.to_string())?;
    // SAFETY: CK_C_Sign is the PKCS#11 ABI for the named symbol.
    let sign = unsafe { library.get::<cryptoki_sys::CK_C_Sign>(b"C_Sign\0") }
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "token module has no C_Sign".to_string())?;
    signature(kind, digest, |mechanism, data| {
        session.sign_init(mechanism, key).map_err(sign_refused)?;
        login(UserType::ContextSpecific)?;
        let size = match kind {
            KeyKind::Rsa(bits) => bits.div_ceil(8),
            KeyKind::P256 => 64,
            KeyKind::P384 => 96,
        };
        let mut bytes = vec![0; size];
        let mut length = size as cryptoki_sys::CK_ULONG;
        // C_Sign's input pointer is mutable in the C ABI. Give the provider
        // owned writable bytes, although the input is semantically read-only.
        let mut input = data.to_vec();
        // SAFETY: both buffers live through the call. length gives the exact
        // output capacity. The initialized session and library are still live.
        let rv = unsafe {
            sign(
                session.handle(),
                input.as_mut_ptr(),
                input.len() as _,
                bytes.as_mut_ptr(),
                &raw mut length,
            )
        };
        cryptoki::error::Rv::from(rv)
            .into_result(cryptoki::context::Function::Sign)
            .map_err(sign_refused)?;
        if length as usize != size {
            return Err(format!(
                "token returned {length} signature bytes, expected {size}"
            ));
        }
        Ok(bytes)
    })
}

fn sign_refused(e: cryptoki::error::Error) -> String {
    said!("the token did not sign: {e}")
}

/// One signature by `key` in `session`, in the form CMS wants.
///
/// The mechanism per kind, and the one place it is chosen: `CKM_RSA_PKCS` over
/// a SHA-256 `DigestInfo`, `CKM_ECDSA` over the bare digest, whose `r || s`
/// answer becomes DER. Public so the tests sign through this and not a copy.
///
/// # Errors
///
/// The token refusing.
pub fn sign_with(
    session: &Session,
    key: ObjectHandle,
    kind: KeyKind,
    digest: &[u8; 32],
) -> Result<Vec<u8>, String> {
    signature(kind, digest, |mechanism, data| {
        session.sign(mechanism, key, data).map_err(sign_refused)
    })
}

fn signature(
    kind: KeyKind,
    digest: &[u8; 32],
    sign: impl FnOnce(&Mechanism<'_>, &[u8]) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    match kind {
        KeyKind::Rsa(_) => {
            let mut data = SHA256_DIGEST_INFO.to_vec();
            data.extend_from_slice(digest);
            sign(&Mechanism::RsaPkcs, &data)
        }
        KeyKind::P256 | KeyKind::P384 => {
            crate::sign_cms::ecdsa_der(&sign(&Mechanism::Ecdsa, digest)?)
        }
    }
}

#[cfg(test)]
mod tests;
