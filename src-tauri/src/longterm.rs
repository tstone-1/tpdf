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
//! - **What is asked about** ([`plan`]): the signer's certificate and every
//!   certificate above it that is not a root, and the timestamp authority's
//!   certificate and every one above it that is not a root. Roots need nothing:
//!   nobody revokes a trust anchor through its own data. A certificate carrying
//!   `id-pkix-ocsp-nocheck` needs nothing either (RFC 6960 §4.2.2.2.1: it is a
//!   delegated responder's, which is trusted for the life of the certificate).
//!   The issuers are found among the signature's own certificates, the token's,
//!   and the chain the operating system assembles offline --- and **every one of
//!   them goes into `/DSS /Certs`**, because increment C1's reader does not offer
//!   the OS chain as candidate issuers, so an issuer not in the document reads
//!   as `unchecked`, reason `issuer`.
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
//!   authority's revocation standings **`good`** --- the answer a properties
//!   dialog would give the file.
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
//! the command line exits 3. A revocation is the one refusal with no second
//! choice ([`Refusal::revoked`]).

use std::time::{Duration, Instant};

use der::{Decode as _, Encode as _};
use x509_cert::Certificate;
use x509_ocsp::{OcspResponse, OcspResponseStatus};

use crate::revocation::{Basis, Moment, Source, Status};
use crate::sign_dss::Gathered;
use crate::tsa;

/// id-ad-ocsp, RFC 5280 §4.2.2.1.
const ID_AD_OCSP: &str = "1.3.6.1.5.5.7.48.1";
/// id-pe-authorityInfoAccess.
const AUTHORITY_INFO_ACCESS: &str = "1.3.6.1.5.5.7.1.1";
/// id-ce-cRLDistributionPoints.
const CRL_DISTRIBUTION_POINTS: &str = "2.5.29.31";
/// id-pkix-ocsp-nocheck, RFC 6960 §4.2.2.2.1.
const OCSP_NO_CHECK: &str = "1.3.6.1.5.5.7.48.1.5";
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
        }
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

/// Whether the certificate says nobody need ask about its own revocation.
fn no_check(certificate: &Certificate) -> bool {
    extension(certificate, OCSP_NO_CHECK).is_some()
}

/// Whether `certificate` is a root: self-issued, and verified by its own key.
fn root(certificate: &Certificate) -> bool {
    certificate.tbs_certificate.subject == certificate.tbs_certificate.issuer
        && crate::revocation::issuer_of(certificate, &[certificate]).is_some()
}

/// A root among `candidates` that `certificate` is the same authority as:
/// self-issued, with its name and its key. `certificate` itself when it is a
/// root; the self-issued twin when it is a cross-certificate.
fn anchor_of<'a>(
    certificate: &Certificate,
    candidates: &'a [Certificate],
) -> Option<&'a Certificate> {
    candidates.iter().find(|candidate| {
        candidate.tbs_certificate.subject == certificate.tbs_certificate.subject
            && candidate.tbs_certificate.subject_public_key_info
                == certificate.tbs_certificate.subject_public_key_info
            && root(candidate)
    })
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

/// What to ask about, and every certificate the `/DSS` should carry.
///
/// `cms` is the timestamped signature just made. `os_chain` answers the chain
/// the operating system assembles for a certificate from the ones given, DER,
/// offline --- `trust::platform::evaluate`'s chain in the application, nothing
/// in a test that wants none.
///
/// # Errors
///
/// A signature or token tpdf cannot read ([`Refusal::Unreadable`]), no
/// timestamp, an issuer nowhere to be found, a signer or authority whose
/// certificate is self-issued and publishes nothing, or more than
/// [`MAX_SUBJECTS`].
pub fn plan(cms: &[u8], os_chain: &OsChain<'_>) -> Result<(Vec<Subject>, Vec<Vec<u8>>), Refusal> {
    planned(cms, os_chain, true)
}

/// [`plan`] for the timestamp authority's chain alone.
///
/// **For `sign-probe`'s measurement against the real authorities**, whose
/// signer is a self-made certificate [`plan`] refuses: it measures the half a
/// real B-LT signing shares with every signer. Never the signing path.
///
/// # Errors
///
/// As [`plan`], for the authority's chain.
pub fn plan_authority(
    cms: &[u8],
    os_chain: &OsChain<'_>,
) -> Result<(Vec<Subject>, Vec<Vec<u8>>), Refusal> {
    planned(cms, os_chain, false)
}

/// [`plan`], with the signer's chain walked or not.
fn planned(
    cms: &[u8],
    os_chain: &OsChain<'_>,
    with_signer: bool,
) -> Result<(Vec<Subject>, Vec<Vec<u8>>), Refusal> {
    let signed = signed_data(cms).map_err(Refusal::Unreadable)?;
    let signer = signer_of(&signed)
        .ok_or_else(|| Refusal::Unreadable("its certificate is not identified".into()))?;
    let token = signed
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
        .ok_or(Refusal::NoTimestamp)?;
    let token = signed_data(&token).map_err(Refusal::Unreadable)?;
    let authority = signer_of(&token).ok_or_else(|| {
        Refusal::Unreadable("the timestamp's certificate is not identified".into())
    })?;

    // Every candidate issuer: the signature's set, the token's, and what the
    // OS assembles for each leaf from them.
    let mut known: Vec<Vec<u8>> = Vec::new();
    let keep = |der: Vec<u8>, known: &mut Vec<Vec<u8>>| {
        if !known.contains(&der) {
            known.push(der);
        }
    };
    for certificate in crate::docinfo::certificates_of(&signed)
        .into_iter()
        .chain(crate::docinfo::certificates_of(&token))
    {
        if let Ok(der) = certificate.to_der() {
            keep(der, &mut known);
        }
    }
    for leaf in [&signer, &authority] {
        let Ok(der) = leaf.to_der() else { continue };
        for found in os_chain(&der, &known.clone()) {
            keep(found, &mut known);
        }
    }
    let candidates: Vec<Certificate> = known
        .iter()
        .filter_map(|der| Certificate::from_der(der).ok())
        .collect();
    let everyone: Vec<&Certificate> = candidates.iter().collect();

    let mut subjects: Vec<Subject> = Vec::new();
    let mut carried: Vec<Vec<u8>> = Vec::new();
    for (leaf, own, above) in [
        (&signer, Whose::Signer, Whose::SignerIssuer),
        (&authority, Whose::Authority, Whose::AuthorityIssuer),
    ] {
        if own == Whose::Signer && !with_signer {
            continue;
        }
        let mut current = leaf.clone();
        let mut whose = own;
        for _ in 0..=MAX_SUBJECTS {
            // A root needs nothing --- unless it is the signer or the
            // authority itself, which then has nobody to vouch for it, and
            // says so the way a certificate with no addresses does. Nor does
            // a **cross-certificate of a root**: the same name and key as a
            // self-issued certificate the token or the OS holds, issued by
            // another root. DigiCert's and Sectigo's tokens carry their
            // timestamping roots in that form, and the chain a verifier builds
            // ends at the self-issued one (measured 2026-09-28, `docs/PLAN.md`).
            //
            // **The anchor is carried, and the cross-certificate is not.**
            // Given both, pyHanko resolves an OCSP response signed by the
            // root's key to the cross-certificate, which is not the issuer on
            // its path, and calls the response unauthorised --- measured on
            // DigiCert's and Sectigo's real data (`docs/TRAPS.md`). The token
            // still carries the cross-certificate for whoever builds that path.
            let anchor = anchor_of(&current, &candidates);
            if let Ok(der) = anchor.unwrap_or(&current).to_der() {
                if !carried.contains(&der) {
                    carried.push(der);
                }
            }
            if anchor.is_some() {
                if whose == own {
                    return Err(Refusal::NotPublished(named(&current, whose)));
                }
                break;
            }
            let issuer = crate::revocation::issuer_of(&current, &everyone)
                .cloned()
                .ok_or_else(|| Refusal::NoIssuer(named(&current, whose)))?;
            if !no_check(&current) {
                let der = current.to_der().unwrap_or_default();
                if !subjects
                    .iter()
                    .any(|s| s.certificate.to_der().ok().as_deref() == Some(&der[..]))
                {
                    subjects.push(Subject {
                        certificate: current.clone(),
                        issuer: issuer.clone(),
                        whose,
                    });
                }
            }
            current = issuer;
            whose = above;
        }
        if subjects.len() > MAX_SUBJECTS {
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
/// signing must: the new signature `field` intact, its timestamp intact, and
/// the signer's and the authority's revocation standings `good`.
///
/// # Errors
///
/// [`Refusal::Revoked`] when either standing is `revoked`; otherwise
/// [`Refusal::Written`] with what was found instead.
pub fn check(signatures: &[crate::docinfo::Signature], field: &str) -> Result<(), Refusal> {
    use crate::integrity::Verdict;
    let written = |why: &str| Err(Refusal::Written(why.to_string()));
    let Some(ours) = signatures.iter().find(|s| s.signed && s.field == field) else {
        return written("the new signature is not in it");
    };
    if ours.integrity.as_ref().map(|i| i.verdict) != Some(Verdict::Intact) {
        return written("the new signature does not read as intact");
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
    Ok(())
}

/// The chain the operating system assembles for `leaf` from `others`,
/// offline, signer first --- or nothing where there is no store to ask.
#[must_use]
pub fn os_chain(leaf: &[u8], others: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    crate::trust::platform::evaluate(leaf, others, crate::trust::Anchors::System, now)
        .map(|found| found.chain)
        .unwrap_or_default()
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

/// The whole of a B-LT signing's second half: what to ask about, the
/// fetching, the worker's revision and its reading, and the check --- and the
/// signed document with the revision appended, when every step passed.
///
/// `bytes` is the sealed B-T document; `cms` the timestamped signature in it;
/// `field` its field.
///
/// # Errors
///
/// Every [`Refusal`].
pub fn extend(
    bytes: &[u8],
    cms: &[u8],
    field: &str,
    now: u64,
    worker: &dyn crate::save::Verifier,
    os_chain: &OsChain<'_>,
    fetch: &mut Fetch<'_>,
) -> Result<Vec<u8>, Refusal> {
    let (subjects, certificates) = plan(cms, os_chain)?;
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
    check(&extended.signatures, field)?;
    let mut whole = Vec::with_capacity(bytes.len() + extended.update.len());
    whole.extend_from_slice(bytes);
    whole.extend_from_slice(&extended.update);
    Ok(whole)
}

#[cfg(test)]
mod tests;
