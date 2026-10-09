//! Gathering long-term validation data while signing: Phase 6 step 3,
//! increment C2.
//!
//! ## What a B-LT signing adds, and who does what
//!
//! A B-T signature (increment B) says who signed and when. Whether the
//! signer's certificate --- and the timestamp authority's --- had been revoked
//! is said by their issuers, in OCSP responses (RFC 6960) and revocation lists
//! (RFC 5280 §5), which the issuers stop serving once the certificates expire.
//! PAdES baseline B-LT keeps them in the document: this module fetches them
//! while signing, and `sign_dss.rs` writes them into a `/DSS` revision.
//!
//! - **The app process (or the command-line tool's) fetches**, never a worker
//!   and never the webview, with increment B's client (`tsa::fetch`): http and
//!   https only, judged by the `url` crate; no redirect; connect and total
//!   timeouts; answers bounded by what is read. **Reading a document never goes
//!   online** (increment C1's decision); only signing does, and only when the
//!   reader asked for long-term data on this signing, which is offered only
//!   together with a timestamp.
//! - **Only for an authority this computer trusts** ([`vouched`]). Before
//!   anything is fetched, the timestamp authority's certificate must chain to a
//!   root the operating system trusts for timestamping, now and offline ---
//!   `trust::judge_for` with `Purpose::Timestamping`, the reader's own rule,
//!   over at most `trust::MAX_CERTIFICATES` of the token's certificates. The
//!   token came from the network, and over `http://` anybody on the path can
//!   answer with one from an authority of their own whose certificates name
//!   addresses of their choosing; a chain the OS trusts is what makes those
//!   addresses the certificate authority's --- and only for the certificates
//!   on that chain, so the chain is what [`vouched`] answers and [`plan`]
//!   takes no certificate above the authority's from anywhere else. They may
//!   still be on a private network or this machine, on purpose: a company's
//!   own PKI publishes its responder there. The signer's certificate needs no
//!   such gate: it comes from the reader's own keychain or certificate store,
//!   not from the network.
//! - **What is asked about** ([`plan`]): the signer's certificate and every
//!   certificate above it that is not a root, and the timestamp authority's
//!   certificate and every one above it that is not a root. Roots need nothing:
//!   nobody revokes a trust anchor through its own data. A certificate carrying
//!   `id-pkix-ocsp-nocheck` needs nothing either (RFC 6960 §4.2.2.2.1: it is a
//!   delegated responder's, which is trusted for the life of the certificate).
//!   The signer's issuers are the signature's own certificates and the chain
//!   the operating system assembles for the signer offline; the authority's
//!   **only the chain the operating system vouched for it by**, never a
//!   certificate its token carries beside that chain, since anybody on the
//!   path can add one (since 2026-10-05; until then a certificate added to a
//!   token could pass for an issuer by name and key, and its addresses were
//!   asked). **Every issuer goes into `/DSS /Certs`**, because the reader
//!   does not offer the OS chain as candidate issuers, so an issuer not in
//!   the document reads as `unchecked`, reason `issuer`. The walk itself is
//!   the reader's (`revocation::chain::walk`), over what the reader will
//!   have, so what is asked about here is exactly what the reader judges in
//!   the file written --- and a walk that meets a certificate outside those
//!   chains is a refusal, not a different walk.
//! - **How** ([`gather`]): OCSP first --- a `POST` of `application/ocsp-request`
//!   to each `id-ad-ocsp` address in the certificate's `authorityInfoAccess`, in
//!   the order written --- and the revocation list from its
//!   `cRLDistributionPoints` only when there is no responder or no responder
//!   gave an answer. `ldap:` and every other scheme is skipped. One at a time:
//!   a B-LT signing asks about two to five certificates, each answered in well
//!   under a second (measured, `docs/PLAN.md` §9), and a parallel fetch would
//!   be a second concurrency design for no reader-visible time.
//! - **Each answer is checked before it is kept**, with increment C1's own
//!   judge (`revocation::judge`, at the present): an answer that is `good` is
//!   kept; `revoked` is a refusal naming the certificate, and **no signature is
//!   ever written with a certificate its CA says is revoked**; `unknown`, and an
//!   answer that does not check out, is a refusal too. The fallback to the list
//!   is for a responder that did not answer, never for an answer tpdf did not
//!   like: a response whose signature fails is either an attacker on the path
//!   or a broken responder, and the list from the same CA over the same path
//!   would not settle which.
//! - **The worker writes and reads back** (`sign_dss::extend`): the app process
//!   hands it the signed bytes and the DER, and it answers the revision and
//!   `docinfo::scan` over the result. [`check`] then refuses unless the new
//!   signature is intact, its timestamp intact, and the signer's and the
//!   authority's revocation standings **`good`**, for their whole chains ---
//!   the answer a properties dialog would give the file.
//!
//! ## Why the `CertID` hashes with SHA-1
//!
//! An OCSP request names the certificate by its serial and two hashes, of its
//! issuer's name and of its issuer's key. They are written with SHA-1 because
//! that is what responders are required to understand (RFC 5019 §2.1.1) and
//! what OpenSSL and the public responders use; several answer nothing for
//! SHA-256. It is not a weakness: the hashes are an **identifier**, not a
//! signature. What makes a response believable is the responder's signature
//! over it, checked with the responder's own algorithm, and a matching serial
//! under an issuer found by its key --- a colliding name hash would still need a
//! responder the issuer authorised to sign an answer about it.
//!
//! No nonce is sent: the public responders serve pre-produced answers and
//! ignore one, and freshness is judged by the response's own dates, as
//! increment C1 judges every response in a document.
//!
//! ## When it fails
//!
//! **Nothing is written, and the reader is told why** ([`Refusal::sentence`]).
//! The window keeps the made, timestamped signature so the key is not asked
//! for again, and offers trying again or signing without the long-term data;
//! the command line exits 3 --- or 4 where the failure is tpdf's own
//! ([`Refusal::tpdf_failed`]). A revocation is the one refusal with no second
//! choice ([`Refusal::revoked`]).

use std::time::{Duration, Instant};

use der::{Decode as _, Encode as _};
use x509_cert::Certificate;
use x509_ocsp::{OcspResponse, OcspResponseStatus};

use crate::revocation::chain::{walk, End};
use crate::revocation::{Basis, Moment, Source, Status};
use crate::sign_dss::Gathered;
use crate::tsa;

/// id-ad-ocsp, RFC 5280 §4.2.2.1.
const ID_AD_OCSP: &str = "1.3.6.1.5.5.7.48.1";
/// id-pe-authorityInfoAccess.
const AUTHORITY_INFO_ACCESS: &str = "1.3.6.1.5.5.7.1.1";
/// id-ce-cRLDistributionPoints.
const CRL_DISTRIBUTION_POINTS: &str = "2.5.29.31";
/// id-aa-timeStampToken.
const TIME_STAMP_TOKEN: &str = "1.2.840.113549.1.9.16.2.14";
/// id-sha1, for the `CertID`.
const ID_SHA1: &str = "1.3.14.3.2.26";

/// The most certificates asked about in one signing: the signer's chain and
/// the authority's, below their roots. Real chains are two or three deep.
pub const MAX_SUBJECTS: usize = 8;

/// The most requests one signing makes: an OCSP request and a list for
/// every certificate at most.
pub const MAX_REQUESTS: usize = 2 * MAX_SUBJECTS;

/// The most DER one signing gathers, certificates included.
///
/// Smaller than increment C1's per-list bound of 8 MiB, which is the reader's,
/// because the worker's answer carries the revision back as JSON --- up to four
/// characters a byte --- under `worker_proto::MAX_REPLY_BYTES` (32 MB). A list
/// anywhere near it is an end-entity list from a CA whose responder should
/// have answered first. It is also `sign_dss::MAX_BYTES`.
pub const MAX_GATHERED: usize = crate::sign_dss::MAX_BYTES;

/// How long the whole gathering may take. Each request also has
/// [`OCSP`]'s or [`LIST`]'s limits, cut to what is left of this.
pub const TOTAL: Duration = Duration::from_secs(90);

/// One OCSP exchange: a response with its responder's certificate is 1.5 to
/// 5 KB, and the reader's own bound (`revocation::MAX_RESPONSE_BYTES`) is this.
pub const OCSP: tsa::Limits = tsa::Limits {
    connect: Duration::from_secs(10),
    total: Duration::from_secs(30),
    body: crate::revocation::MAX_RESPONSE_BYTES,
};

/// One list's download.
pub const LIST: tsa::Limits = tsa::Limits {
    connect: Duration::from_secs(10),
    total: Duration::from_secs(30),
    body: MAX_GATHERED,
};

/// Whose certificate a subject is, for the sentences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whose {
    /// The signer's own.
    Signer,
    /// One above the signer's.
    SignerIssuer,
    /// The timestamp authority's own.
    Authority,
    /// One above the authority's.
    AuthorityIssuer,
}

/// A certificate whose revocation is asked about, with its issuer.
#[derive(Clone, Debug)]
pub struct Subject {
    /// The certificate.
    pub certificate: Certificate,
    /// The certificate that issued it, found by name and by its key.
    pub issuer: Certificate,
    /// Whose it is.
    pub whose: Whose,
}

impl Subject {
    /// How the sentences name it.
    #[must_use]
    pub fn name(&self) -> String {
        named(&self.certificate, self.whose)
    }
}

/// A certificate, as the sentences name it: whose, and its common name.
fn named(certificate: &Certificate, whose: Whose) -> String {
    let mut cn = crate::docinfo::common_name(&certificate.tbs_certificate.subject);
    if cn.is_empty() {
        cn = crate::docinfo::distinguished_name(&certificate.tbs_certificate.subject);
    }
    match whose {
        Whose::Signer => format!("the signer's certificate ({cn})"),
        Whose::SignerIssuer => format!("the certificate above the signer's ({cn})"),
        Whose::Authority => format!("the timestamp authority's certificate ({cn})"),
        Whose::AuthorityIssuer => {
            format!("the certificate above the timestamp authority's ({cn})")
        }
    }
}

/// Why no long-term validation data was written. Nothing is written for any of
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The signature, or its timestamp, could not be read to find what to ask
    /// about --- tpdf made both, so this is tpdf's failure.
    Unreadable(String),
    /// Long-term data was asked for without a timestamp.
    NoTimestamp,
    /// The certificate names no OCSP responder and no list tpdf can fetch.
    NotPublished(String),
    /// The certificate that issued it is nowhere to be found.
    NoIssuer(String),
    /// The timestamp authority's certificate does not chain to a root this
    /// computer trusts for timestamping, so nothing is fetched for it.
    Untrusted {
        /// The authority, by its common name.
        name: String,
        /// Why not, as a clause.
        why: String,
    },
    /// Nobody answered: each attempt, as a clause.
    Unanswered {
        /// The certificate.
        name: String,
        /// What each address did.
        attempts: Vec<String>,
    },
    /// A checked answer says the responder does not know the certificate.
    Unknown {
        /// The certificate.
        name: String,
        /// The responder's host.
        host: String,
    },
    /// Checked data says the certificate is revoked.
    Revoked {
        /// The certificate.
        name: String,
        /// Since when, formatted.
        at: String,
        /// Which kind of data said so, and from where, as a phrase.
        by: String,
    },
    /// An answer that does not check out, and why.
    DoesNotCheck {
        /// The certificate.
        name: String,
        /// Which kind of data, and from where, as a phrase.
        by: String,
        /// Why, as a clause.
        why: String,
    },
    /// Past one of this module's bounds.
    Bound(String),
    /// The worker's revision, or its reading of the result, was not what could
    /// be written.
    Written(String),
    /// The archive timestamp over the whole --- PAdES B-LTA --- could not be
    /// had, or did not check out: the timestamp authority's sentence.
    Archive(String),
}

impl Refusal {
    /// The sentence the reader is told. No full stop: the caller adds what
    /// happens next.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Refusal::Unreadable(why) => {
                format!("the signature just made could not be read to gather its data: {why}")
            }
            Refusal::NoTimestamp => {
                "long-term validation data needs a timestamp: choose a timestamp authority too"
                    .into()
            }
            Refusal::NotPublished(name) => format!(
                "{name} does not say where its revocation data is published, so tpdf cannot \
                 add long-term validation data for it: that needs a certificate from a \
                 certificate authority that publishes revocation data (an OCSP responder or \
                 a revocation list over http or https). A timestamp alone works with this \
                 certificate"
            ),
            Refusal::NoIssuer(name) => format!(
                "tpdf could not find the certificate that issued {name}, so it cannot ask about \
                 its revocation"
            ),
            Refusal::Untrusted { name, why } => format!(
                "the timestamp authority {name} is not trusted by this computer, so tpdf will \
                 not fetch revocation data for it: {why}"
            ),
            Refusal::Unanswered { name, attempts } => format!(
                "tpdf could not get revocation data for {name}: {}",
                attempts.join("; ")
            ),
            Refusal::Unknown { name, host } => format!(
                "the OCSP responder at {host} says it does not know {name}, so there is no \
                 revocation data to keep for it"
            ),
            Refusal::Revoked { name, at, by } => format!(
                "{name} has been revoked{} according to {by}. tpdf will not write a signature \
                 made with a revoked certificate",
                if at.is_empty() {
                    String::new()
                } else {
                    format!(" since {at}")
                }
            ),
            Refusal::DoesNotCheck { name, by, why } => {
                format!("{by} about {name} does not check out: {why}")
            }
            Refusal::Bound(why) => format!("the long-term validation data {why}"),
            Refusal::Written(why) => format!(
                "tpdf's own check of the signed document with its long-term validation data \
                 did not pass: {why}"
            ),
            Refusal::Archive(why) => format!(
                "the archive timestamp, which keeps the signature checkable after the \
                 timestamp authority's own certificate expires, could not be added: {why}"
            ),
        }
    }

    /// Whether this refusal is tpdf's own failure rather than the document's,
    /// an authority's or a certificate authority's: a signature or token tpdf
    /// made that it cannot read, or a revision its own worker built or read
    /// back --- including a worker that died or did not answer, which reaches
    /// here as [`Refusal::Written`]. The command line exits 4 for these, as its
    /// README says of a worker that died, and 3 for every other refusal.
    #[must_use]
    pub fn tpdf_failed(&self) -> bool {
        matches!(self, Refusal::Unreadable(_) | Refusal::Written(_))
    }

    /// Whether this refusal is a certificate authority saying a certificate is
    /// revoked: the one refusal after which **no** signature is written, with
    /// or without the long-term data.
    #[must_use]
    pub fn revoked(&self) -> bool {
        matches!(self, Refusal::Revoked { .. })
    }
}

/// An `http` or `https` address with a host and no credentials, as
/// `tsa::authority` judges one; anything else is skipped rather than asked.
fn address(text: &str) -> Option<url::Url> {
    let url = url::Url::parse(text.trim()).ok()?;
    let fine = matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|host| !host.is_empty())
        && url.username().is_empty()
        && url.password().is_none();
    fine.then_some(url)
}

/// The extension `oid` names, when the certificate carries it.
fn extension<'a>(certificate: &'a Certificate, oid: &str) -> Option<&'a [u8]> {
    certificate
        .tbs_certificate
        .extensions
        .iter()
        .flatten()
        .find(|e| e.extn_id.to_string() == oid)
        .map(|e| e.extn_value.as_bytes())
}

/// The OCSP responders a certificate names, usable ones only, in order.
fn responders(certificate: &Certificate) -> Vec<url::Url> {
    use x509_cert::ext::pkix::name::GeneralName;
    let Some(value) = extension(certificate, AUTHORITY_INFO_ACCESS) else {
        return Vec::new();
    };
    let Ok(access) = x509_cert::ext::pkix::AuthorityInfoAccessSyntax::from_der(value) else {
        return Vec::new();
    };
    access
        .0
        .iter()
        .filter(|d| d.access_method.to_string() == ID_AD_OCSP)
        .filter_map(|d| match &d.access_location {
            GeneralName::UniformResourceIdentifier(uri) => address(uri.as_str()),
            _ => None,
        })
        .collect()
}

/// The lists a certificate names, `http` and `https` only, in order: an
/// `ldap:` point --- common in certificates from directory-based CAs --- is
/// skipped, never asked.
fn lists(certificate: &Certificate) -> Vec<url::Url> {
    use x509_cert::ext::pkix::name::{DistributionPointName, GeneralName};
    let Some(value) = extension(certificate, CRL_DISTRIBUTION_POINTS) else {
        return Vec::new();
    };
    let Ok(points) = x509_cert::ext::pkix::CrlDistributionPoints::from_der(value) else {
        return Vec::new();
    };
    points
        .0
        .iter()
        .filter_map(|point| match &point.distribution_point {
            Some(DistributionPointName::FullName(names)) => Some(names),
            _ => None,
        })
        .flatten()
        .filter_map(|name| match name {
            GeneralName::UniformResourceIdentifier(uri) => address(uri.as_str()),
            _ => None,
        })
        .collect()
}

/// A CMS `SignedData` and its signer's certificate.
fn signed_data(blob: &[u8]) -> Result<cms::signed_data::SignedData, String> {
    let info = cms::content_info::ContentInfo::from_der(blob).map_err(|e| e.to_string())?;
    info.content
        .decode_as::<cms::signed_data::SignedData>()
        .map_err(|e| e.to_string())
}

/// The signer's certificate of a `SignedData`, as `SignerInfo.sid` names it.
fn signer_of(signed: &cms::signed_data::SignedData) -> Option<Certificate> {
    match crate::docinfo::signer_certificate(signed) {
        Some((certificate, true)) => Some(certificate.clone()),
        _ => None,
    }
}

/// The timestamp token a signature carries, DER: its first signer's
/// `id-aa-timeStampToken` attribute.
fn token_of(signed: &cms::signed_data::SignedData) -> Option<Vec<u8>> {
    signed
        .signer_infos
        .0
        .as_slice()
        .first()
        .and_then(|info| info.unsigned_attrs.as_ref())
        .and_then(|attributes| {
            attributes
                .iter()
                .find(|a| a.oid.to_string() == TIME_STAMP_TOKEN)
        })
        .and_then(|attribute| attribute.values.as_slice().first())
        .and_then(|value| value.to_der().ok())
}

/// What [`Vouch`] answers about a timestamp authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vouched {
    /// Whether it is trusted for timestamping.
    pub trust: crate::trust::Trust,
    /// The chain that answer is about, the authority's certificate first and
    /// the root last, DER: the one the store assembled in the evaluation that
    /// passed. Empty unless trusted.
    pub chain: Vec<Vec<u8>>,
}

/// Who says whether the timestamp authority is trusted for timestamping, and
/// by which chain, given its token (a CMS `ContentInfo`, DER) and the present:
/// [`vouched_by_os`] in the application, the same rule over a test's own roots
/// in a test.
pub type Vouch<'a> = dyn Fn(&[u8], u64) -> Vouched + 'a;

/// [`Vouch`] for the application: the operating system's store, offline, for
/// timestamping --- `trust::of_blob_with_chain`, the rule the reader applies
/// to every token, including its cap on how many certificates the OS is
/// handed.
#[must_use]
pub fn vouched_by_os(token: &[u8], now: u64) -> Vouched {
    vouched_under(token, now, crate::trust::Anchors::System)
}

/// [`vouched_by_os`] over `anchors`: the system's store, or the roots a test
/// or the command-line tool's environment names.
#[must_use]
pub fn vouched_under(token: &[u8], now: u64, anchors: crate::trust::Anchors<'_>) -> Vouched {
    let (trust, chain) =
        crate::trust::of_blob_with_chain(token, crate::trust::Purpose::Timestamping, now, anchors);
    Vouched { trust, chain }
}

/// Refuses unless `vouch` says the authority that stamped `cms` is trusted
/// for timestamping now --- asked before anything is fetched, so an
/// authority nobody vouches for never chooses an address tpdf connects to.
/// Answers the chain it was vouched for by, which is the only place
/// [`plan`] then looks for the certificates above the authority's.
///
/// **Trusted, and nothing less**: a store that could not be asked, or a
/// token carrying more certificates than the OS is handed, is not a chain
/// anybody vouched for, and an authority whose certificate is out of its
/// dates cannot have stamped this signature soundly.
///
/// # Errors
///
/// [`Refusal::Untrusted`], naming the authority and why; a signature or
/// token tpdf cannot read; no timestamp.
pub fn vouched(cms: &[u8], now: u64, vouch: &Vouch<'_>) -> Result<Vec<Vec<u8>>, Refusal> {
    use crate::trust::Standing;
    let signed = signed_data(cms).map_err(Refusal::Unreadable)?;
    let token = token_of(&signed).ok_or(Refusal::NoTimestamp)?;
    let Vouched { trust, chain } = vouch(&token, now);
    if trust.standing == Standing::Trusted {
        return Ok(chain);
    }
    let computer = crate::words::computer(trust.store);
    let why = match (trust.standing, trust.why) {
        (Standing::Expired | Standing::NotYetValid, _) => {
            "its certificate is not in force now".to_string()
        }
        (_, Some(doubt)) => crate::words::doubt_about(doubt, computer, "the authority's"),
        (_, None) => "no reason was given".to_string(),
    };
    let name = signed_data(&token)
        .ok()
        .and_then(|token| signer_of(&token))
        .map_or_else(
            || "whose certificate is not identified".to_string(),
            |certificate| {
                let cn = crate::docinfo::common_name(&certificate.tbs_certificate.subject);
                if cn.is_empty() {
                    crate::docinfo::distinguished_name(&certificate.tbs_certificate.subject)
                } else {
                    cn
                }
            },
        );
    Err(Refusal::Untrusted { name, why })
}

/// What to ask about, and every certificate the `/DSS` should carry.
///
/// `cms` is the timestamped signature just made. `os_chain` answers the chain
/// the operating system assembles for a certificate from the ones given, DER,
/// offline --- `trust::platform::evaluate`'s chain in the application, nothing
/// in a test that wants none. `vouched_chain` is the chain the timestamp
/// authority was vouched for by, as [`vouched`] answers it.
///
/// **Where an issuer may come from** differs for the two chains, because
/// where the certificates came from does:
///
/// - **Above the signer's**, from the signature's own set --- the chain the
///   reader's key store gave when signing --- and what `os_chain` assembles
///   for the signer from it. Never from the token.
/// - **Above the authority's**, from `vouched_chain` and nowhere else. The
///   token's own set is outside its signature: anybody on the path can add a
///   certificate with an issuer's name and key under a root of their own,
///   naming addresses of their choosing, and a walk that took its links from
///   that set would ask those addresses and write that certificate. A
///   certificate the store did not put on the chain it accepted is not a
///   link, and a walk that meets one is refused ([`Refusal::NoIssuer`]).
///
/// The walk still runs over everything the reader will have --- the token's
/// set included --- because it has to be the reader's walk. So a token
/// carrying such a certificate where the walk meets it first is a refusal,
/// not a signing by the genuine chain: the file would read back with that
/// certificate on the chain, and nothing said about it. A cross-certificate
/// of a root is not a link (the walk ends at it), so the tokens that carry
/// one are not refused.
///
/// It does not ask whether the authority is trusted: [`extend`] asks that
/// first ([`vouched`]) and passes the chain on.
///
/// # Errors
///
/// A signature or token tpdf cannot read ([`Refusal::Unreadable`]), no
/// timestamp, an issuer nowhere to be found, a signer or authority whose
/// certificate is self-issued and publishes nothing, or more than
/// [`MAX_SUBJECTS`].
pub fn plan(
    cms: &[u8],
    os_chain: &OsChain<'_>,
    vouched_chain: &[Vec<u8>],
) -> Result<(Vec<Subject>, Vec<Vec<u8>>), Refusal> {
    planned(cms, os_chain, Some(vouched_chain), true)
}

/// [`plan`] for the timestamp authority's chain alone, taken from `os_chain`
/// over the token's certificates rather than from a chain anybody vouched
/// for.
///
/// **For `sign-probe`'s measurement against the real authorities**, whose
/// signer is a self-made certificate [`plan`] refuses: it measures the half a
/// real B-LT signing shares with every signer. Never the signing path, which
/// asks nothing about an authority before [`vouched`] has answered.
///
/// # Errors
///
/// As [`plan`], for the authority's chain.
pub fn plan_authority(
    cms: &[u8],
    os_chain: &OsChain<'_>,
) -> Result<(Vec<Subject>, Vec<Vec<u8>>), Refusal> {
    planned(cms, os_chain, None, false)
}

/// [`plan`], with the signer's chain walked or not, and the authority's
/// links held to `vouched_chain` --- or, for the probe alone, to `os_chain`'s.
fn planned(
    cms: &[u8],
    os_chain: &OsChain<'_>,
    vouched_chain: Option<&[Vec<u8>]>,
    with_signer: bool,
) -> Result<(Vec<Subject>, Vec<Vec<u8>>), Refusal> {
    let signed = signed_data(cms).map_err(Refusal::Unreadable)?;
    let signer = signer_of(&signed)
        .ok_or_else(|| Refusal::Unreadable("its certificate is not identified".into()))?;
    let token = token_of(&signed).ok_or(Refusal::NoTimestamp)?;
    let token = signed_data(&token).map_err(Refusal::Unreadable)?;
    let authority = signer_of(&token).ok_or_else(|| {
        Refusal::Unreadable("the timestamp's certificate is not identified".into())
    })?;

    let encoded = |certificates: Vec<&Certificate>| -> Vec<Vec<u8>> {
        certificates
            .iter()
            .filter_map(|certificate| certificate.to_der().ok())
            .collect()
    };
    let in_signature = encoded(crate::docinfo::certificates_of(&signed));
    let in_token = encoded(crate::docinfo::certificates_of(&token));
    // Which certificates may stand on each chain, DER. Above the signer's:
    // the signature's own set, and what the OS assembles for the signer from
    // it. Above the authority's: the chain it was vouched for by.
    let mut above_signer = in_signature.clone();
    if with_signer {
        if let Ok(der) = signer.to_der() {
            above_signer.extend(os_chain(&der, &in_signature));
        }
    }
    let above_authority = match vouched_chain {
        Some(chain) => chain.to_vec(),
        None => authority
            .to_der()
            .map(|der| os_chain(&der, &in_token))
            .unwrap_or_default(),
    };

    // Every candidate the reader will have, in the reader's order: the
    // signature's set, the token's, and the two chains, which go into the
    // `/DSS`. The token's stay candidates so the walk here is the walk the
    // reader makes over the file written; which of them may be a link is
    // decided below.
    let mut known: Vec<Vec<u8>> = Vec::new();
    for der in in_signature
        .iter()
        .chain(&in_token)
        .chain(&above_signer)
        .chain(&above_authority)
    {
        if !known.contains(der) {
            known.push(der.clone());
        }
    }
    let candidates: Vec<Certificate> = known
        .iter()
        .filter_map(|der| Certificate::from_der(der).ok())
        .collect();
    let everyone: Vec<&Certificate> = candidates.iter().collect();

    let mut subjects: Vec<Subject> = Vec::new();
    let mut carried: Vec<Vec<u8>> = Vec::new();
    let mut carry = |certificate: &Certificate| {
        if let Ok(der) = certificate.to_der() {
            if !carried.contains(&der) {
                carried.push(der);
            }
        }
    };
    for (leaf, own, above, admitted) in [
        (&signer, Whose::Signer, Whose::SignerIssuer, &above_signer),
        (
            &authority,
            Whose::Authority,
            Whose::AuthorityIssuer,
            &above_authority,
        ),
    ] {
        if own == Whose::Signer && !with_signer {
            continue;
        }
        // The walk is the reader's (`revocation::chain::walk`), so what is
        // asked about here is what the reader judges in the file written.
        // A root needs nothing --- unless it is the signer or the authority
        // itself, which then has nobody to vouch for it, and says so the way
        // a certificate with no addresses does. Nor does a **cross-certificate
        // of a root**, which the walk ends at too: DigiCert's and Sectigo's
        // tokens carry their timestamping roots in that form (measured
        // 2026-09-28, `docs/PLAN.md`).
        //
        // **The anchor is carried, and the cross-certificate is not.** Given
        // both, pyHanko resolves an OCSP response signed by the root's key to
        // the cross-certificate, which is not the issuer on its path, and calls
        // the response unauthorised --- measured on DigiCert's and Sectigo's
        // real data (`docs/TRAPS.md`). The token still carries the
        // cross-certificate for whoever builds that path.
        let walked = walk(leaf, &everyone);
        if walked.links.is_empty() {
            return Err(Refusal::NotPublished(named(leaf, own)));
        }
        for (at, link) in walked.links.iter().enumerate() {
            let whose = if at == 0 { own } else { above };
            // A certificate above the leaf that passes for an issuer by name
            // and key, and is not on the chain this leaf may have: for the
            // authority, one the token carries that the OS did not vouch for.
            // Refused here, before anything is fetched or carried: its
            // addresses were chosen by whoever made it, and the reader of
            // the file would walk into it as this walk did. The certificate
            // below it is the one whose issuer was not found.
            let der = link.certificate.to_der().unwrap_or_default();
            if at > 0 && !admitted.contains(&der) {
                let below = &walked.links[at - 1].certificate;
                let whose = if at == 1 { own } else { above };
                return Err(Refusal::NoIssuer(named(below, whose)));
            }
            carry(&link.certificate);
            let Some(issuer) = &link.issuer else {
                return Err(Refusal::NoIssuer(named(&link.certificate, whose)));
            };
            if link.no_check {
                continue;
            }
            if !subjects
                .iter()
                .any(|s| s.certificate.to_der().ok().as_deref() == Some(&der[..]))
            {
                subjects.push(Subject {
                    certificate: link.certificate.clone(),
                    issuer: issuer.clone(),
                    whose,
                });
            }
        }
        match (&walked.end, &walked.anchor) {
            (End::Root, Some(anchor)) => carry(anchor),
            (End::Loop, _) => {
                return Err(Refusal::Bound(
                    "would follow a chain of certificates that loops back on itself, which no \
                     verifier can end at a root"
                        .into(),
                ))
            }
            _ => {}
        }
        if walked.dropped > 0 || subjects.len() > MAX_SUBJECTS {
            return Err(Refusal::Bound(format!(
                "would cover more than {MAX_SUBJECTS} certificates, more than any real chain has"
            )));
        }
    }
    Ok((subjects, carried))
}

/// The DER `OCSPRequest` for `subject`: one `CertID` under SHA-1 (the module
/// note says why), no nonce, unsigned.
fn ocsp_request(subject: &Subject) -> Result<Vec<u8>, String> {
    use sha1::Digest as _;
    let name = subject
        .certificate
        .tbs_certificate
        .issuer
        .to_der()
        .map_err(|e| e.to_string())?;
    let key = subject
        .issuer
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .raw_bytes();
    let octets = |bytes: Vec<u8>| der::asn1::OctetString::new(bytes).map_err(|e| e.to_string());
    let request = x509_ocsp::OcspRequest {
        tbs_request: x509_ocsp::TbsRequest {
            version: x509_ocsp::Version::V1,
            requestor_name: None,
            request_list: vec![x509_ocsp::Request {
                req_cert: x509_ocsp::CertId {
                    hash_algorithm: x509_cert::spki::AlgorithmIdentifierOwned {
                        oid: der::asn1::ObjectIdentifier::new_unwrap(ID_SHA1),
                        parameters: Some(der::asn1::Null.into()),
                    },
                    issuer_name_hash: octets(sha1::Sha1::digest(&name).to_vec())?,
                    issuer_key_hash: octets(sha1::Sha1::digest(key).to_vec())?,
                    serial_number: subject.certificate.tbs_certificate.serial_number.clone(),
                },
                single_request_extensions: None,
            }],
            request_extensions: None,
        },
        optional_signature: None,
    };
    request.to_der().map_err(|e| e.to_string())
}

/// A transport refusal, as a clause about the address it came from.
fn failed(what: &str, url: &url::Url, why: &tsa::Refusal, limits: &tsa::Limits) -> String {
    let host = url.host_str().unwrap_or_default();
    let said = match why {
        tsa::Refusal::Unreachable(why) => format!("could not be reached ({why})"),
        tsa::Refusal::TimedOut => format!(
            "did not answer within {} seconds",
            limits.total.as_secs().max(1)
        ),
        tsa::Refusal::Http(code) => format!("answered with HTTP status {code}"),
        tsa::Refusal::TooLarge => format!("answered with more than {} KiB", limits.body / 1024),
        other => other.sentence(host),
    };
    format!("{what} at {host} {said}")
}

/// A revocation `Gap`, as a clause.
fn gap_words(gap: Option<crate::revocation::Gap>) -> String {
    use crate::revocation::Gap;
    match gap {
        Some(Gap::Unreadable) => "it could not be read".into(),
        Some(Gap::Bound) => "it is larger than tpdf reads".into(),
        Some(Gap::Issuer) => "the certificate's issuer could not be found".into(),
        Some(Gap::Signature) => "its signature does not check out".into(),
        Some(Gap::Unauthorised) => {
            "it is signed by a party the certificate's issuer did not authorise".into()
        }
        Some(Gap::Algorithm) => "it uses an algorithm tpdf does not implement".into(),
        Some(Gap::Unsupported) => "it is in a form tpdf does not interpret".into(),
        Some(Gap::Stale) => "it is out of date".into(),
        Some(Gap::Expired) => "it was issued after the certificate expired".into(),
        Some(Gap::Dates) => "its dates do not hang together".into(),
        Some(Gap::Budget) => "checking it would take more work than tpdf allows".into(),
        None => "it says nothing about this certificate".into(),
    }
}

/// What an answer about `subject` came to, judged by increment C1's reader at
/// `now`: kept (`Ok`), or the refusal it is.
fn judged(
    subject: &Subject,
    candidates: &[Certificate],
    material: &crate::revocation::Material,
    by: &str,
    host: &str,
    now: u64,
) -> Result<(), Refusal> {
    let pool = crate::revocation::Pool::new(material);
    let mut budget = crate::integrity::MAX_HASHED;
    let answer = crate::revocation::judge(
        &subject.certificate,
        candidates,
        &[&pool],
        Moment {
            basis: Basis::Now,
            at: now,
        },
        now,
        &mut budget,
    );
    match answer.standing {
        Status::Good => Ok(()),
        Status::Revoked => Err(Refusal::Revoked {
            name: subject.name(),
            at: answer.revoked,
            by: by.to_string(),
        }),
        Status::Unknown => Err(Refusal::Unknown {
            name: subject.name(),
            host: host.to_string(),
        }),
        Status::None | Status::Unchecked => Err(Refusal::DoesNotCheck {
            name: subject.name(),
            by: by.to_string(),
            why: gap_words(answer.why),
        }),
    }
}

/// Who assembles a certificate's chain from candidates, offline: [`os_chain`]
/// in the application and the tool, a test's own in a test.
pub type OsChain<'a> = dyn Fn(&[u8], &[Vec<u8>]) -> Vec<Vec<u8>> + 'a;

/// Who stamps the archive timestamp: given the covered pieces of the file,
/// the timestamp authority's token over them, or the sentence saying why not
/// --- `tsa::ask_over_range` against the authority the reader chose, or a
/// test's.
pub type Archive<'a> = dyn FnMut(&[&[u8]]) -> Result<Vec<u8>, String> + 'a;

/// Who fetches: `tsa::fetch` on the application's runtime, or a test's.
pub type Fetch<'a> = dyn FnMut(&url::Url, Option<(&str, Vec<u8>)>, &tsa::Limits) -> Result<Vec<u8>, tsa::Refusal>
    + 'a;

/// Fetches and checks revocation data for every subject, in order, within
/// [`MAX_REQUESTS`], [`MAX_GATHERED`] and `total`.
///
/// `certificates` are the ones [`plan`] found, which go into the `/DSS` and
/// are the candidate issuers every answer is checked against.
///
/// # Errors
///
/// Every [`Refusal`] from [`Refusal::NotPublished`] on but
/// [`Refusal::Written`].
pub fn gather(
    subjects: &[Subject],
    certificates: Vec<Vec<u8>>,
    now: u64,
    total: Duration,
    fetch: &mut Fetch<'_>,
) -> Result<Gathered, Refusal> {
    let started = Instant::now();
    let candidates: Vec<Certificate> = certificates
        .iter()
        .filter_map(|der| Certificate::from_der(der).ok())
        .collect();
    let mut gathered = Gathered {
        certificates,
        ..Gathered::default()
    };
    let mut requests = 0usize;
    // The limits for the next request, cut to what is left of `total`.
    let mut next = |limits: tsa::Limits| -> Result<tsa::Limits, Refusal> {
        requests += 1;
        if requests > MAX_REQUESTS {
            return Err(Refusal::Bound(format!(
                "would take more than {MAX_REQUESTS} requests"
            )));
        }
        let left = total.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Err(Refusal::Bound(format!(
                "took longer than {} seconds to gather",
                total.as_secs()
            )));
        }
        Ok(tsa::Limits {
            total: limits.total.min(left),
            connect: limits.connect.min(left),
            ..limits
        })
    };
    let room = |gathered: &Gathered, more: usize| {
        if gathered.bytes() + more > MAX_GATHERED {
            Err(Refusal::Bound(format!(
                "would be more than {} MiB, more than tpdf adds to a document",
                MAX_GATHERED >> 20
            )))
        } else {
            Ok(())
        }
    };
    room(&gathered, 0)?;

    for subject in subjects {
        let responders = responders(&subject.certificate);
        let lists = lists(&subject.certificate);
        if responders.is_empty() && lists.is_empty() {
            return Err(Refusal::NotPublished(subject.name()));
        }
        let mut attempts = Vec::new();
        let mut done = false;
        for url in &responders {
            let host = url.host_str().unwrap_or_default().to_string();
            let limits = next(OCSP)?;
            let body = ocsp_request(subject).map_err(Refusal::Unreadable)?;
            let answer = match fetch(url, Some(("application/ocsp-request", body)), &limits) {
                Ok(answer) => answer,
                Err(why) => {
                    attempts.push(failed("the OCSP responder", url, &why, &limits));
                    continue;
                }
            };
            // A responder that answered with no answer --- `tryLater`,
            // `internalError`, bytes that are not a response --- has not
            // answered, and the list may.
            let status = OcspResponse::from_der(&answer).map(|r| r.response_status);
            match status {
                Ok(OcspResponseStatus::Successful) => {}
                Ok(other) => {
                    attempts.push(format!(
                        "the OCSP responder at {host} answered {}",
                        format!("{other:?}").to_lowercase()
                    ));
                    continue;
                }
                Err(_) => {
                    attempts.push(format!(
                        "the OCSP responder at {host} answered with something that is not an \
                         OCSP response"
                    ));
                    continue;
                }
            }
            let mut material = crate::revocation::Material::default();
            for certificate in &gathered.certificates {
                material.certificate(certificate.clone());
            }
            material.response(&answer);
            let by = format!("the OCSP response from {host}");
            room(&gathered, answer.len())?;
            judged(subject, &candidates, &material, &by, &host, now)?;
            gathered.responses.push(answer);
            done = true;
            break;
        }
        if done {
            continue;
        }
        for url in &lists {
            let host = url.host_str().unwrap_or_default().to_string();
            let limits = next(LIST)?;
            let answer = match fetch(url, None, &limits) {
                Ok(answer) => answer,
                Err(why) => {
                    attempts.push(failed("the revocation list", url, &why, &limits));
                    continue;
                }
            };
            let mut material = crate::revocation::Material::default();
            material.list(answer.clone());
            let by = format!("the revocation list from {host}");
            // Measured before it is parsed: a list is the large half.
            room(&gathered, answer.len())?;
            judged(subject, &candidates, &material, &by, &host, now)?;
            gathered.lists.push(answer);
            done = true;
            break;
        }
        if !done {
            return Err(Refusal::Unanswered {
                name: subject.name(),
                attempts,
            });
        }
    }
    Ok(gathered)
}

/// Refuses unless the worker's reading of the finished bytes says what a B-LT
/// signing must: the new signature intact, its timestamp intact, and the
/// signer's and the authority's revocation standings `good` --- theirs and
/// their chains', since the reader judges the whole chain (2026-09-28).
///
/// `ours` is the new signature as the worker read it, chosen by
/// `sign_cms::ours`: by where its range ends, not by its name alone.
///
/// # Errors
///
/// [`Refusal::Revoked`] when either standing is `revoked`;
/// [`Refusal::Bound`] when the new signature was not checked because the
/// document's signatures together are past the hashing budget; otherwise
/// [`Refusal::Written`] with what was found instead.
pub fn check(ours: Option<&crate::docinfo::Signature>) -> Result<(), Refusal> {
    use crate::integrity::{Verdict, Why};
    let written = |why: &str| Err(Refusal::Written(why.to_string()));
    let Some(ours) = ours else {
        return written("the new signature is not in it");
    };
    match ours.integrity.as_ref() {
        Some(found) if found.verdict == Verdict::Intact => {}
        // The budget is one for the document, spent in the order the fields
        // are listed, and the new signature is last: on a large file with
        // several signatures it is the one left unchecked. That is a bound
        // and not a failed check --- the seal found this signature intact on
        // a budget of its own --- and until 2026-10-09 it was reported as
        // tpdf's own check not passing, with the tool's exit code for that.
        Some(found) if found.verdict == Verdict::Unchecked && found.why == Some(Why::Budget) => {
            return Err(Refusal::Bound(
                "cannot be checked before it is written: the document's signatures \
                 together cover more data than tpdf checks at once, so the new signature \
                 was not checked again with the data added"
                    .into(),
            ))
        }
        _ => return written("the new signature does not read as intact"),
    }
    let Some(stamp) = &ours.timestamp else {
        return written("the new signature's timestamp is not in it");
    };
    if stamp.integrity.as_ref().map(|i| i.verdict) != Some(Verdict::Intact) {
        return written("the new signature's timestamp does not read as intact");
    }
    let name = |certificate: Option<&crate::docinfo::Certificate>, whose: &str| {
        certificate.map_or_else(
            || whose.to_string(),
            |c| format!("{whose} ({})", c.subject_cn),
        )
    };
    for (revocation, whose) in [
        (
            ours.revocation.as_ref(),
            name(ours.certificate.as_ref(), "the signer's certificate"),
        ),
        (
            stamp.revocation.as_ref(),
            name(
                stamp.authority.as_ref(),
                "the timestamp authority's certificate",
            ),
        ),
    ] {
        let Some(revocation) = revocation else {
            return Err(Refusal::Written(format!(
                "no revocation answer for {whose}"
            )));
        };
        match revocation.standing {
            Status::Good => {}
            Status::Revoked => {
                return Err(Refusal::Revoked {
                    name: whose,
                    at: revocation.revoked.clone(),
                    by: match revocation.source {
                        Some(Source::Crl) => "the revocation list in the document".into(),
                        _ => "the OCSP response in the document".into(),
                    },
                })
            }
            other => {
                return Err(Refusal::Written(format!(
                    "{whose} reads {}{}",
                    format!("{other:?}").to_lowercase(),
                    revocation
                        .why
                        .map(|why| format!(" ({})", gap_words(Some(why))))
                        .unwrap_or_default()
                )))
            }
        }
    }
    // And every certificate above them, as the reader judges the file: the
    // ones the gathering asked about, each read back `good`, and none past
    // the bound. Above only --- the chain's first certificate is the leaf,
    // answered for above, and a second check of it would hide either one.
    for (chain, above) in [
        (ours.revocation_chain.as_ref(), "the signer's"),
        (stamp.revocation_chain.as_ref(), "the timestamp authority's"),
    ] {
        let Some(chain) = chain else {
            return written("no revocation answer for a chain");
        };
        let name = |judged: &crate::revocation::chain::Judged| {
            format!("the certificate above {above} ({})", judged.subject_cn)
        };
        let issuers = chain.certificates.iter().skip(1);
        if let Some(judged) = issuers
            .clone()
            .find(|c| c.revocation.standing == Status::Revoked)
        {
            return Err(Refusal::Revoked {
                name: name(judged),
                at: judged.revocation.revoked.clone(),
                by: match judged.revocation.source {
                    Some(Source::Crl) => "the revocation list in the document".into(),
                    _ => "the OCSP response in the document".into(),
                },
            });
        }
        if let Some(judged) = issuers
            .clone()
            .find(|c| c.revocation.standing != Status::Good)
        {
            return Err(Refusal::Written(format!(
                "{} reads {}",
                name(judged),
                format!("{:?}", judged.revocation.standing).to_lowercase()
            )));
        }
        if chain.dropped > 0 {
            return Err(Refusal::Written(format!(
                "the chain above {above} certificate is longer than tpdf judges"
            )));
        }
    }
    Ok(())
}

/// The chain the operating system assembles for `leaf` from `others`,
/// offline, signer first --- or nothing where there is no store to ask.
///
/// **Held to the reader's bounds** ([`bounded`]): for `sign-probe`'s
/// [`plan_authority`], `others` is the timestamp token's certificates, which
/// came from the network, and they are parsed by the OS in this process
/// rather than a worker's.
#[must_use]
pub fn os_chain(leaf: &[u8], others: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    os_chain_with(leaf, others, |leaf, others| {
        crate::trust::platform::evaluate(leaf, others, crate::trust::Anchors::System, now)
            .map(|found| found.chain)
            .ok()
    })
}

/// [`os_chain`], with `evaluate` the store: split so a test sees what the OS
/// would be handed.
fn os_chain_with(
    leaf: &[u8],
    others: &[Vec<u8>],
    evaluate: impl FnOnce(&[u8], &[Vec<u8>]) -> Option<Vec<Vec<u8>>>,
) -> Vec<Vec<u8>> {
    let Some(others) = bounded(leaf, others) else {
        return Vec::new();
    };
    evaluate(leaf, &others).unwrap_or_default()
}

/// What of `others` the OS is handed beside `leaf`: at most
/// `trust::MAX_CERTIFICATES`, in order --- the signature's own first --- each
/// no larger than `trust::MAX_CERTIFICATE_BYTES`. `None` when `leaf` itself is
/// over that size, which nothing real is. The same bounds the reader holds a
/// signature's set to in the worker (`trust::certificates_with`); one over
/// the count is left out rather than refusing the lot, since a candidate a
/// chain needed then reads as a missing link, which is what it is.
fn bounded(leaf: &[u8], others: &[Vec<u8>]) -> Option<Vec<Vec<u8>>> {
    if leaf.len() > crate::trust::MAX_CERTIFICATE_BYTES {
        return None;
    }
    Some(
        others
            .iter()
            .filter(|der| der.len() <= crate::trust::MAX_CERTIFICATE_BYTES)
            .take(crate::trust::MAX_CERTIFICATES)
            .cloned()
            .collect(),
    )
}

/// [`tsa::fetch`], waited for on the application's runtime: for a caller on
/// an ordinary thread, as `tsa::ask_blocking` is.
///
/// # Errors
///
/// As [`tsa::fetch`].
pub fn fetch_blocking(
    url: &url::Url,
    body: Option<(&str, Vec<u8>)>,
    limits: &tsa::Limits,
) -> Result<Vec<u8>, tsa::Refusal> {
    tauri::async_runtime::block_on(tsa::fetch(url, body, limits))
}

/// The whole of a long-term signing's second half: what to ask about, the
/// fetching, the worker's revision and its reading, the check, and the
/// archive timestamp over it all --- and the signed document with both
/// revisions appended, when every step passed (PAdES B-LTA since 2026-09-28).
///
/// `bytes` is the sealed B-T document; `cms` the timestamped signature in it;
/// `field` its field. `vouch` says whether the authority is trusted, and is
/// asked before anything else ([`vouched`]).
///
/// # Errors
///
/// Every [`Refusal`].
#[allow(clippy::too_many_arguments)]
pub fn extend(
    bytes: &[u8],
    cms: &[u8],
    field: &str,
    now: u64,
    worker: &dyn crate::save::Verifier,
    os_chain: &OsChain<'_>,
    vouch: &Vouch<'_>,
    fetch: &mut Fetch<'_>,
    archive: &mut Archive<'_>,
) -> Result<Vec<u8>, Refusal> {
    let vouched_chain = vouched(cms, now, vouch)?;
    let (subjects, certificates) = plan(cms, os_chain, &vouched_chain)?;
    let gathered = gather(&subjects, certificates, now, TOTAL, fetch)?;
    let extended = worker
        .validation(bytes, &gathered)
        .map_err(Refusal::Written)?;
    if extended.built_against != bytes.len() {
        return Err(Refusal::Written(format!(
            "the worker built its revision against {} bytes, and the signed document is {}",
            extended.built_against,
            bytes.len()
        )));
    }
    // The revision just built is all that follows the new signature's own.
    let ours = crate::sign_cms::ours(&extended.signatures, field, extended.update.len() as u64);
    check(ours.map(|at| &extended.signatures[at]))?;
    let mut whole = Vec::with_capacity(bytes.len() + extended.update.len());
    whole.extend_from_slice(bytes);
    whole.extend_from_slice(&extended.update);
    archived(whole, worker, archive)
}

/// `whole` --- the signed document with its validation data --- with an
/// archive timestamp over all of it appended (PAdES B-LTA): a document
/// timestamp, whose revision the worker builds and whose token `archive` asks
/// the reader's timestamp authority for, sealed only when it reads back intact.
///
/// **Why the same authority, and why always.** The validation data keeps the
/// signature checkable until the timestamp authority's own certificate
/// expires, which for the public authorities is within a few years; the
/// archive timestamp is what a reader then judges that authority at, and it is
/// what *Keep it verifiable after the certificates expire* promises
/// (`docs/PLAN.md` §9, *Archive timestamps*). The reader already chose whom to
/// ask for a time, so that authority is asked again rather than a second one
/// nobody chose.
///
/// # Errors
///
/// [`Refusal::Written`] for the worker's revision, [`Refusal::Archive`] for
/// the authority's answer or the sealed result not reading back intact.
fn archived(
    whole: Vec<u8>,
    worker: &dyn crate::save::Verifier,
    archive: &mut Archive<'_>,
) -> Result<Vec<u8>, Refusal> {
    // A revision built against other bytes is refused by the seal, which
    // checks it before splicing --- one check, not two that hide each other.
    let unsigned = worker
        .document_timestamp(&whole)
        .map_err(Refusal::Written)?;
    let token = {
        let [_, hole, rest, _] = unsigned
            .range
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX));
        let mut staged = whole.clone();
        staged.extend_from_slice(&unsigned.update);
        let (Some(before), Some(after)) = (staged.get(..hole), staged.get(rest..)) else {
            return Err(Refusal::Written(
                "the archive timestamp's revision states a range outside itself".into(),
            ));
        };
        archive(&[before, after]).map_err(Refusal::Archive)?
    };
    crate::sign_cms::seal_document_timestamp(whole, unsigned, &token).map_err(Refusal::Archive)
}

#[cfg(test)]
mod tests;
