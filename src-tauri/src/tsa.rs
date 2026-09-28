//! Asking a timestamp authority for an RFC 3161 token: Phase 6 step 3,
//! increment B.
//!
//! ## What this adds, and why it is the part that needed deciding
//!
//! A signature's `/M` is the signer's own clock. A timestamp token is another
//! party's signed statement that the signature existed at a moment --- PAdES
//! baseline B-T --- and it has to be **asked for over the network**, which makes
//! this the application's second network authority beside the updater
//! (`docs/THREAT-MODEL.md` §T10). Three decisions bound it, and they are the
//! owner's, recorded in `docs/PLAN.md` §9:
//!
//! - **Opt-in for each signing.** No request is made unless the reader asked for
//!   a timestamp on this signing: a server in the window's chooser, or
//!   `--timestamp` on the command line.
//! - **A short list plus the reader's own URL** ([`SERVERS`]), nothing
//!   preselected, the choice remembered by the window. Measured on 2026-09-28:
//!   all three grant tokens, and all three authorities' certificates carry
//!   `id-kp-timeStamping`, critical and alone.
//! - **`http://` is accepted**, because two of the three offer nothing else.
//!   That costs nothing in integrity: the token is itself a signature, and
//!   [`accept`] checks it before anything is written. What plain HTTP exposes
//!   is stated in the threat model: a hash of the signature value and a nonce
//!   go out, and an observer learns that somebody signed something then.
//!
//! ## Who asks, and what is checked before anything is believed
//!
//! **The app process, or the command-line tool's process --- never a worker.**
//! The worker has `(deny network*)` and keeps it. What leaves is
//! [`request`]: a `TimeStampReq` whose imprint is SHA-256 over **the value
//! octets of the signer's `signature`** (RFC 3161 Appendix A, and what
//! increment A's reader checks), a fresh 128-bit nonce, and `certReq TRUE`, so
//! the token carries the authority's certificate.
//!
//! What comes back is attacker-chosen bytes parsed in this process, which is
//! the one thing here the worker boundary does not cover. It is bounded before
//! it is parsed --- [`LIMITS`]: 64 KiB, a connect and a total timeout, no
//! redirect --- and parsed only by `der`, which is memory-safe and refuses
//! anything not canonical. [`accept`] then refuses unless, in this order:
//!
//! 1. the answer is a `TimeStampResp` with status `granted` or
//!    `grantedWithMods`, carrying a token --- anything else is the authority's
//!    refusal, told with its own words when it gave any;
//! 2. the token's verdict under **increment A's reader**
//!    (`integrity::token::check`, the function the properties dialog's row
//!    comes from --- no second verifier) is `intact`: its own signature, its
//!    `TSTInfo`, its ESS binding and its imprint over this signature's value.
//!    `weak` is refused too: SHA-1 anywhere the time rests on is a verdict a
//!    reader would see beside a signature made today;
//! 3. its imprint is **the one asked for** --- SHA-256, over these bytes. The
//!    verdict accepts any hash a token may use; a request that asked for
//!    SHA-256 and got something else did not get what it asked for;
//! 4. its nonce is **the one sent**. A replayed answer to an earlier request
//!    over the same signature would pass every check above; the nonce is what
//!    says this answer is to this request.
//!
//! `sign_cms::Made::seal` then checks the token a second time **in the
//! finished bytes**, so nothing is written whose timestamp tpdf's own reader
//! would not call attested.
//!
//! ## When it fails
//!
//! Nothing is written, and the reader is told why ([`Refusal::sentence`]).
//! [`stamp`] is where that is decided, for the window and the command line
//! alike: **a request asked for and not answered is never an untimestamped
//! signature**. The window then offers trying again or signing without one, as
//! a second, explicit choice, over the signature it already has --- so the OS
//! is not asked for the key a second time (`commands::sign`). The command line
//! has nobody to ask and exits 3.

use std::time::Duration;

use der::asn1::{ObjectIdentifier, OctetString};
use der::{Decode as _, Encode as _};
use sha2_10::Digest as _;

use crate::integrity::token::{self, MessageImprint};
use crate::integrity::Verdict;

/// id-sha256: the imprint's algorithm in every request.
const ID_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");

/// id-ct-TSTInfo.
const TST_INFO: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.1.4");

/// A timestamp authority the reader can pick by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Server {
    /// What `--timestamp` accepts in place of the URL, and what the window
    /// remembers.
    pub name: &'static str,
    /// As a reader reads it.
    pub label: &'static str,
    /// Where the request goes.
    pub url: &'static str,
}

/// The short list, decided by the owner on 2026-09-28 and measured live the
/// same day (`docs/PLAN.md` §9): each granted a token, and each authority's
/// certificate names `id-kp-timeStamping`, critical and alone.
///
/// Two are `http://` because that is what they serve: DigiCert and GlobalSign
/// did not connect over HTTPS at all. Sectigo answers both, and HTTPS is used.
/// `src/lib/signtimestamp.ts` holds the same three, and `cli::tests` writes
/// these to `wording.json` so a test holds the two lists together.
pub const SERVERS: [Server; 3] = [
    Server {
        name: "digicert",
        label: "DigiCert",
        url: "http://timestamp.digicert.com",
    },
    Server {
        name: "sectigo",
        label: "Sectigo",
        url: "https://timestamp.sectigo.com",
    },
    Server {
        name: "globalsign",
        label: "GlobalSign",
        url: "http://timestamp.globalsign.com/tsa/r6advanced1",
    },
];

/// How long a request may take, and how much of an answer is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Until the connection is open.
    pub connect: Duration,
    /// For the whole exchange, the answer's last byte included.
    pub total: Duration,
    /// The most bytes of an answer read. A token with its certificates is 6 to
    /// 8 KiB (measured, `docs/PLAN.md`); the signature's reserved span holds
    /// 32 KiB altogether, so an answer anywhere near this could not be used.
    pub body: usize,
}

/// The limits every signing uses. A signing that waits on a server must end:
/// a reader looking at a window that says *signing* for ever cannot tell a slow
/// server from a hung application.
pub const LIMITS: Limits = Limits {
    connect: Duration::from_secs(10),
    total: Duration::from_secs(30),
    body: 64 * 1024,
};

/// Why no token was used. Nothing is written for any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The address is not one tpdf asks: not `http` or `https`, not a URL, or
    /// carrying a user name or password. Refused before any socket is opened.
    Address(String),
    /// The server could not be reached, or the connection failed.
    Unreachable(String),
    /// It did not answer inside [`Limits`].
    TimedOut,
    /// It answered with an HTTP status other than 200.
    Http(u16),
    /// Its answer is longer than [`Limits::body`].
    TooLarge,
    /// Its answer is not a `TimeStampResp`, or its token not a token.
    Unreadable,
    /// It declined, with the status and whatever it said.
    Declined(String),
    /// It granted, and sent no token.
    NoToken,
    /// The token's verdict is not `intact`.
    Token(crate::integrity::Integrity),
    /// The token is of an imprint other than the one asked for.
    Imprint,
    /// The token does not carry the nonce sent.
    Nonce,
    /// The token could not be put into the signature (`sign_cms::Made::stamped`).
    Fit(String),
}

impl Refusal {
    /// The sentence the reader is told, naming the authority by `host`.
    #[must_use]
    pub fn sentence(&self, host: &str) -> String {
        let at = if host.is_empty() {
            "the timestamp authority".to_string()
        } else {
            format!("the timestamp authority at {host}")
        };
        match self {
            Refusal::Address(why) => format!("tpdf does not ask {why}"),
            Refusal::Unreachable(why) => format!("tpdf could not reach {at}: {why}"),
            Refusal::TimedOut => format!(
                "{at} did not answer within {} seconds",
                LIMITS.total.as_secs()
            ),
            Refusal::Http(code) => format!("{at} answered with HTTP status {code}"),
            Refusal::TooLarge => format!(
                "{at} answered with more than {} KiB, which no timestamp needs",
                LIMITS.body / 1024
            ),
            Refusal::Unreadable => format!("{at} answered with something that is not a timestamp"),
            Refusal::Declined(why) => format!("{at} declined: {why}"),
            Refusal::NoToken => format!("{at} said it granted a timestamp and sent none"),
            Refusal::Token(integrity) => format!(
                "{at} sent a timestamp that does not check out ({})",
                verdict_words(integrity)
            ),
            Refusal::Imprint => format!(
                "{at} sent a timestamp that is not of this signature as it was asked for \
                 (SHA-256)"
            ),
            Refusal::Nonce => format!(
                "{at} sent a timestamp that does not answer this request (its nonce is not the \
                 one sent)"
            ),
            Refusal::Fit(why) => format!("the timestamp from {at} could not be added: {why}"),
        }
    }
}

/// A token's verdict, briefly, for [`Refusal::sentence`].
fn verdict_words(integrity: &crate::integrity::Integrity) -> String {
    match integrity.verdict {
        Verdict::Intact => "intact".into(),
        Verdict::Weak => format!("it rests on {}", integrity.digest),
        Verdict::Altered => "it is of something other than this signature".into(),
        Verdict::Broken => "its own signature fails".into(),
        Verdict::Unchecked => format!(
            "it could not be checked: {}",
            integrity.why.map_or_else(
                || "no reason".to_string(),
                |why| format!("{why:?}").to_lowercase()
            )
        ),
    }
}

/// The authority `text` names: one of [`SERVERS`] by name, or a URL.
///
/// **Parsed by the `url` crate and judged by its scheme**, never by a prefix:
/// `weburl.rs`'s reason, since `https:/\host` and a URL with a leading control
/// character are what a `starts_with` test accepts. Only `http` and `https`,
/// with a host, and with no user name or password --- credentials in a URL
/// would travel in the clear over `http`, and no public authority needs one.
///
/// # Errors
///
/// [`Refusal::Address`], with the reason as a clause.
pub fn authority(text: &str) -> Result<url::Url, Refusal> {
    let text = text.trim();
    if let Some(server) = SERVERS
        .iter()
        .find(|server| server.name.eq_ignore_ascii_case(text))
    {
        return url::Url::parse(server.url).map_err(|e| Refusal::Address(e.to_string()));
    }
    let url = url::Url::parse(text).map_err(|_| {
        Refusal::Address(format!(
            "`{text}` for a timestamp: it is not a URL, nor one of {}",
            SERVERS.map(|s| s.name).join(", ")
        ))
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Refusal::Address(format!(
            "a timestamp authority over `{}:` --- only http and https",
            url.scheme()
        )));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(Refusal::Address(format!(
            "`{text}` for a timestamp: it names no server"
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Refusal::Address(
            "a timestamp authority with a user name or password in its address".into(),
        ));
    }
    Ok(url)
}

/// `TimeStampReq`, RFC 3161 §2.4.1, as tpdf writes it: version 1, the
/// imprint, no policy, a nonce, `certReq TRUE`, no extensions.
#[derive(Clone, Debug, der::Sequence)]
pub(crate) struct TimeStampReq {
    pub(crate) version: u8,
    pub(crate) message_imprint: MessageImprint,
    #[asn1(optional = "true")]
    pub(crate) req_policy: Option<ObjectIdentifier>,
    #[asn1(optional = "true")]
    pub(crate) nonce: Option<der::asn1::Uint>,
    /// `DEFAULT FALSE`, so DER writes it only because it is true.
    pub(crate) cert_req: bool,
}

/// The DER `TimeStampReq` for a signature whose value octets hash to
/// `digest`, carrying `nonce`.
///
/// # Errors
///
/// None in practice; every value here is built here.
pub fn request(digest: &[u8; 32], nonce: &[u8; 16]) -> Result<Vec<u8>, String> {
    TimeStampReq {
        version: 1,
        message_imprint: MessageImprint {
            // RFC 5754 §2: SHA-2 identifiers are written with the parameters
            // absent, which every authority measured accepts.
            hash_algorithm: x509_cert::spki::AlgorithmIdentifierOwned {
                oid: ID_SHA256,
                parameters: None,
            },
            hashed_message: OctetString::new(digest.to_vec()).map_err(|e| e.to_string())?,
        },
        req_policy: None,
        nonce: Some(der::asn1::Uint::new(nonce).map_err(|e| e.to_string())?),
        cert_req: true,
    }
    .to_der()
    .map_err(|e| e.to_string())
}

/// `PKIStatusInfo`, RFC 3161 §2.4.2.
#[derive(Clone, Debug, der::Sequence)]
struct PkiStatusInfo {
    status: u32,
    #[asn1(optional = "true")]
    status_string: Option<Vec<String>>,
    #[asn1(optional = "true")]
    fail_info: Option<der::asn1::BitString>,
}

/// `TimeStampResp`, RFC 3161 §2.4.2.
#[derive(Clone, Debug, der::Sequence)]
struct TimeStampResp {
    status: PkiStatusInfo,
    #[asn1(optional = "true")]
    time_stamp_token: Option<der::Any>,
}

/// `PKIFailureInfo`'s bits, by number, as RFC 3161 names them.
const FAILURES: [(usize, &str); 8] = [
    (0, "the hash algorithm is not supported"),
    (2, "the request is malformed"),
    (5, "the data is in a form it does not accept"),
    (14, "its time source is not available"),
    (15, "the policy is not supported"),
    (16, "an extension is not supported"),
    (17, "the information asked for is not available"),
    (25, "it failed internally"),
];

/// What a declining authority said, as a clause.
fn declined(info: &PkiStatusInfo) -> String {
    let status = match info.status {
        2 => "rejection",
        3 => "waiting",
        4 => "revocation warning",
        5 => "revocation notification",
        _ => "an unknown status",
    };
    let mut why = format!("status {} ({status})", info.status);
    if let Some(bits) = &info.fail_info {
        let set: Vec<&str> = FAILURES
            .iter()
            .filter(|(bit, _)| {
                bits.raw_bytes()
                    .get(bit / 8)
                    .is_some_and(|byte| byte & (0x80 >> (bit % 8)) != 0)
            })
            .map(|(_, words)| *words)
            .collect();
        if !set.is_empty() {
            why.push_str(&format!(", {}", set.join(", ")));
        }
    }
    if let Some(text) = &info.status_string {
        // The authority's own words, trimmed to a sentence's worth and kept to
        // what can be shown: they are a stranger's text.
        let said: String = text
            .join(" ")
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        if !said.trim().is_empty() {
            why.push_str(&format!(": \u{201c}{}\u{201d}", said.trim()));
        }
    }
    why
}

/// The `TSTInfo` a token encapsulates, DER.
fn tst_info(token: &[u8]) -> Option<Vec<u8>> {
    let info = cms::content_info::ContentInfo::from_der(token).ok()?;
    let signed: cms::signed_data::SignedData = info.content.decode_as().ok()?;
    if signed.encap_content_info.econtent_type != TST_INFO {
        return None;
    }
    let wrapped: OctetString = signed.encap_content_info.econtent?.decode_as().ok()?;
    Some(wrapped.as_bytes().to_vec())
}

/// The nonce a `TSTInfo` carries, its leading zeros dropped, or `None`.
///
/// After `genTime` come `accuracy` (a `SEQUENCE`), `ordering` (a `BOOLEAN`),
/// `nonce`, and two context-tagged fields --- so the nonce is the one
/// `INTEGER` there, and it is found by tag rather than by position, since each
/// of the three ahead of it may be absent.
fn nonce_of(tst_info: &[u8]) -> Option<Vec<u8>> {
    use der::{Reader as _, Tagged as _};

    let mut outer = der::SliceReader::new(tst_info).ok()?;
    let sequence = der::asn1::AnyRef::decode(&mut outer).ok()?;
    if sequence.tag() != der::Tag::Sequence || !outer.is_finished() {
        return None;
    }
    let mut inner = der::SliceReader::new(sequence.value()).ok()?;
    let _version = der::asn1::Int::decode(&mut inner).ok()?;
    let _policy = ObjectIdentifier::decode(&mut inner).ok()?;
    let _imprint = MessageImprint::decode(&mut inner).ok()?;
    let _serial = der::asn1::Int::decode(&mut inner).ok()?;
    let _time = der::asn1::GeneralizedTime::decode(&mut inner).ok()?;
    while !inner.is_finished() {
        let field = der::asn1::AnyRef::decode(&mut inner).ok()?;
        if field.tag() == der::Tag::Integer {
            return Some(unpadded(field.value()));
        }
    }
    None
}

/// An unsigned big-endian integer with its leading zero bytes dropped, so two
/// encodings of one number compare equal.
fn unpadded(bytes: &[u8]) -> Vec<u8> {
    let leading = bytes.iter().take_while(|b| **b == 0).count();
    bytes[leading..].to_vec()
}

/// Checks an authority's answer to the request for `value` carrying `nonce`,
/// and returns the token, DER.
///
/// The order is the module note's, and a check that passes says nothing about
/// the ones after it.
///
/// # Errors
///
/// Every [`Refusal`] from [`Refusal::Unreadable`] on.
pub fn accept(answer: &[u8], value: &[u8], nonce: &[u8]) -> Result<Vec<u8>, Refusal> {
    let response = TimeStampResp::from_der(answer).map_err(|_| Refusal::Unreadable)?;
    // 0 granted, 1 grantedWithMods: both carry a token. Every other status is
    // the authority declining, whatever else the answer holds.
    if response.status.status > 1 {
        return Err(Refusal::Declined(declined(&response.status)));
    }
    let token = response
        .time_stamp_token
        .ok_or(Refusal::NoToken)?
        .to_der()
        .map_err(|_| Refusal::Unreadable)?;

    let verdict = token::check(
        &token,
        token::Target::Signature(value),
        &mut crate::integrity::MAX_HASHED.clone(),
    );
    if verdict.verdict != Verdict::Intact {
        return Err(Refusal::Token(verdict));
    }

    let statement = tst_info(&token).ok_or(Refusal::Unreadable)?;
    let imprint = token::imprint_of(&statement).ok_or(Refusal::Unreadable)?;
    // The imprint's bytes, not its algorithm, are compared, and that is enough:
    // an `intact` verdict already says they are the digest of `value` under the
    // algorithm the token names, so equal to SHA-256 of it means that algorithm
    // is SHA-256 --- short of one hash colliding with another. A comparison of
    // the identifier as well was written first, and a mutation removing it
    // showed no input could make it matter.
    let asked: [u8; 32] = sha2_10::Sha256::digest(value).into();
    if imprint.hashed_message.as_bytes() != asked {
        return Err(Refusal::Imprint);
    }
    if nonce_of(&statement) != Some(unpadded(nonce)) {
        return Err(Refusal::Nonce);
    }
    Ok(token)
}

/// A fresh nonce, from the operating system's random source through the same
/// `ring` provider the TLS connection uses.
///
/// # Errors
///
/// The system's random source failed, which is not worth guessing around.
fn fresh_nonce() -> Result<[u8; 16], Refusal> {
    let mut nonce = [0u8; 16];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut nonce)
        .map_err(|_| Refusal::Unreachable("the system's random source failed".into()))?;
    Ok(nonce)
}

/// The HTTP client, under `limits`.
///
/// `rustls` needs a process-wide crypto provider before a client can be built,
/// and `reqwest` with `rustls-no-provider` panics without one. The updater
/// installs `ring`'s; this installs the same one, and does nothing when either
/// got there first --- `install_default` refuses a second, which is the
/// idempotence wanted.
fn client(limits: &Limits) -> Result<reqwest::Client, Refusal> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .connect_timeout(limits.connect)
        .timeout(limits.total)
        // A timestamp authority answers where it is asked. A redirect is a
        // second address nobody chose, possibly under another scheme.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| Refusal::Unreachable(e.to_string()))
}

/// Posts `body` to `url` and reads at most `limits.body` bytes of the answer.
async fn post(url: &url::Url, body: Vec<u8>, limits: &Limits) -> Result<Vec<u8>, Refusal> {
    fetch(url, Some(("application/timestamp-query", body)), limits).await
}

/// One exchange with `url` under `limits`: a `POST` of `body` with its content
/// type, or a `GET` for `None`, and at most `limits.body` bytes of the answer.
///
/// **Every request tpdf makes while signing comes through here**: the
/// timestamp above, and the OCSP responses and revocation lists `longterm.rs`
/// gathers --- so the rules are one set, stated once: no redirect, a connect
/// and a total timeout, an answer bounded by what is read, HTTP 200 or a
/// refusal. The caller has judged the address (`authority`,
/// `longterm::address`) before it gets here.
///
/// # Errors
///
/// [`Refusal::Unreachable`], [`Refusal::TimedOut`], [`Refusal::Http`] and
/// [`Refusal::TooLarge`].
pub(crate) async fn fetch(
    url: &url::Url,
    body: Option<(&str, Vec<u8>)>,
    limits: &Limits,
) -> Result<Vec<u8>, Refusal> {
    let failed = |e: reqwest::Error| {
        if e.is_timeout() {
            Refusal::TimedOut
        } else {
            Refusal::Unreachable(without_url(&e))
        }
    };
    let client = client(limits)?;
    let request = match body {
        Some((kind, body)) => client
            .post(url.clone())
            .header("Content-Type", kind)
            .body(body),
        None => client.get(url.clone()),
    };
    let mut response = request.send().await.map_err(failed)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(Refusal::Http(response.status().as_u16()));
    }
    // Bounded by what is read, not by what the server announces: a length
    // ahead of the body is the server's word, and chunked answers carry none.
    let mut answer = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(failed)? {
        if answer.len() + chunk.len() > limits.body {
            return Err(Refusal::TooLarge);
        }
        answer.extend_from_slice(&chunk);
    }
    Ok(answer)
}

/// A transport error's message without the URL `reqwest` puts in it, since the
/// sentence names the host already.
fn without_url(error: &reqwest::Error) -> String {
    use std::error::Error as _;
    let mut words = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        words = cause.to_string();
        source = cause.source();
    }
    words
}

/// Asks `url` for a token over the signature whose value octets are `value`,
/// and returns it once [`accept`] has.
///
/// # Errors
///
/// Every [`Refusal`] but [`Refusal::Address`] and [`Refusal::Fit`].
pub async fn ask(url: &url::Url, value: &[u8], limits: &Limits) -> Result<Vec<u8>, Refusal> {
    let nonce = fresh_nonce()?;
    let digest: [u8; 32] = sha2_10::Sha256::digest(value).into();
    let body = request(&digest, &nonce).map_err(Refusal::Unreachable)?;
    let answer = post(url, body, limits).await?;
    accept(&answer, value, &nonce)
}

/// [`ask`], waited for on the application's async runtime.
///
/// For a caller on an ordinary thread: the command-line tool's `main`, or a
/// Tauri command's `spawn_blocking` closure --- never from inside a task on the
/// runtime, which is what `block_on` refuses.
///
/// # Errors
///
/// As [`ask`].
pub fn ask_blocking(url: &url::Url, value: &[u8], limits: &Limits) -> Result<Vec<u8>, Refusal> {
    tauri::async_runtime::block_on(ask(url, value, limits))
}

/// The timestamped CMS for `made`, when one was asked for.
///
/// `Ok(None)` means **none was asked**; `Ok(Some(..))` is the CMS with the
/// token in it; an error is a request that was made and did not produce a
/// usable token. **There is no path from an error to `Ok(None)`**, and that is
/// the point of this function: the reader who asked for a timestamp gets one
/// or gets told, never a signature without one in silence. The window and the
/// command line both come through here, and `ask` is theirs to supply (a fake
/// authority, in the tests).
///
/// # Errors
///
/// What `ask` refused, or [`Refusal::Fit`].
pub fn stamp(
    made: &crate::sign_cms::Made,
    authority: Option<&url::Url>,
    ask: impl FnOnce(&url::Url, &[u8]) -> Result<Vec<u8>, Refusal>,
) -> Result<Option<Vec<u8>>, Refusal> {
    let Some(url) = authority else {
        return Ok(None);
    };
    let value = made.value().map_err(Refusal::Fit)?;
    let token = ask(url, &value)?;
    made.stamped(&token).map(Some).map_err(Refusal::Fit)
}

#[cfg(test)]
mod tests;
