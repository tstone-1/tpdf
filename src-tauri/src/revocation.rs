//! Whether a certificate was revoked, judged only from data the document carries.
//!
//! ## The question, and the decision that bounds it
//!
//! [`crate::trust`] says whether an issuer this computer trusts vouches for a
//! certificate. It cannot say whether that issuer has since **withdrawn** it:
//! a key reported stolen, a certificate issued in error. The issuer says that
//! in revocation data --- an OCSP response (RFC 6960) or a certificate
//! revocation list (RFC 5280 §5) --- and a document made for long-term
//! validation (PAdES baseline B-LT) carries the data it was signed against:
//! in its `/DSS` dictionary, in the Adobe `adbe-revocationInfoArchival`
//! signed attribute, or in the CMS `crls` set.
//!
//! **Reading and verifying never touch the network** (`docs/PLAN.md` §9,
//! Phase 6, decided 2026-09-28). Asking an OCSP responder while a document is
//! opened would tell each certificate authority which signed documents the
//! reader opens, and would put a network authority in the read path, which
//! runs in a worker with `(deny network*)`. So this module judges **only what
//! the document carries**, and the ordinary answer --- a document with no
//! revocation data at all --- is [`Status::None`], which every sentence reads
//! as *not checked*, never as reassurance.
//!
//! ## What each answer claims
//!
//! - [`Status::Good`] --- a response or list this could check says the
//!   certificate was not revoked, and it is fresh for the moment judged
//!   (below). Checked means: signed by the certificate's issuer, or for OCSP
//!   by a responder that issuer authorised with `id-kp-OCSPSigning`; about
//!   this certificate (OCSP's `CertID`, the list's issuer name and scope);
//!   dates that hang together.
//! - [`Status::Revoked`] --- such data lists it as revoked, with the time and
//!   the reason it gives. [`Revocation::after_moment`] says the revocation
//!   came **after** a moment a trusted timestamp attests, which does not undo
//!   the signature: that is the whole point of long-term validation. Without
//!   an attested moment it is never set, because the date a signature gives
//!   is the signer's own claim.
//! - [`Status::Unknown`] --- a checked OCSP response says its responder does
//!   not know the certificate.
//! - [`Status::None`] --- the document carries nothing about it.
//! - [`Status::Unchecked`] --- data was there and nothing could be concluded
//!   from it, with [`Gap`] saying why.
//!
//! ## Freshness, and whose moment it is
//!
//! A response saying *good* speaks for the moment it was issued. What makes
//! it relevant to the moment being judged is ETSI EN 319 102-1 V1.3.1
//! §5.2.5.4's default: absent a policy value, the maximum accepted freshness
//! is the interval between `thisUpdate` and `nextUpdate`, and the data passes
//! when it was issued after *the moment minus that interval* --- which
//! reduces to **`nextUpdate` after the moment**. Data with no `nextUpdate`
//! fails there, as the standard says. The moment is, in order: the time an
//! intact timestamp from a trusted authority attests ([`Basis::Attested`]);
//! for the timestamp authority's own certificate, the time its token states
//! ([`Basis::Stated`]); the signer's `/M` ([`Basis::Claimed`], the signer's
//! own word, said so); and failing all of them the present ([`Basis::Now`]).
//! **The standard's own NOTE 2 suggests a stricter policy** --- a freshness of
//! zero, data issued after the moment --- once the signing time is known;
//! that is a policy choice this does not make, and `docs/PLAN.md` puts it to
//! the owner.
//!
//! A *good* issued after the certificate itself expired proves less than it
//! says: lists drop expired certificates, and a responder may forget them.
//! So such an answer counts only when it states that it keeps them ---
//! `expiredCertsOnCRL` or OCSP `archiveCutoff` at or before the certificate's
//! last day --- and is [`Gap::Expired`] otherwise.
//!
//! ## Where this runs
//!
//! In the **worker**, from `docinfo::scan_from`. Every response, list and
//! certificate here is attacker-chosen DER, parsed by `x509-ocsp` and
//! `x509-cert` after the counts and sizes in this module's constants bound
//! it, and every signature is checked by [`crate::integrity::signed_by`] ---
//! the arithmetic a signature's own verdict rests on, not a second copy.
//! `docs/THREAT-MODEL.md` §T6.25 states the bounds.

use std::cell::RefCell;
use std::collections::HashMap;

use der::{Decode, Encode};
use x509_cert::crl::CertificateList;
use x509_cert::Certificate;
use x509_ocsp::{BasicOcspResponse, CertStatus, OcspResponse, OcspResponseStatus, ResponderId};

use crate::integrity::Hash;

pub mod chain;

/// The most OCSP responses read for one signature, from the document and its
/// CMS together.
///
/// A B-LT signature carries one per certificate in its chains --- the
/// signer's, the authority's, and sometimes their issuers' --- so a document
/// with several signatures carries a handful. Past the bound the rest are
/// counted in [`Material::dropped`], and no answer about a certificate may
/// then be *good* or *none*: what was dropped might have said revoked.
pub const MAX_RESPONSES: usize = 32;

/// The largest single OCSP response read, in DER bytes.
///
/// A real response is 1.5 to 5 KB with its responder's certificate; the
/// bound is generous by an order of magnitude and small enough that 32 of
/// them are two megabytes.
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// The most revocation lists read for one signature.
///
/// A list is per issuer, so a signature and its timestamp need two or three.
/// Lists are the expensive half --- parsed whole, and hashed whole to check
/// their signature --- which is why this bound is the smaller one.
pub const MAX_LISTS: usize = 8;

/// The largest single revocation list read, in DER bytes.
///
/// Measured 2026-09-28: the lists the three public timestamp authorities'
/// certificates name are 0.7 to 1.2 KB, and a large public CA's end-entity
/// list is a few hundred kilobytes to a few megabytes. Eight megabytes admits
/// those and bounds the parse: `x509-cert` builds one entry per revoked
/// serial, about 100 bytes each.
pub const MAX_LIST_BYTES: usize = 8 * 1024 * 1024;

/// The most certificates the `/DSS` contributes as candidate issuers.
///
/// Twice [`crate::trust::MAX_CERTIFICATES`]: the `/DSS` is shared by every
/// signature in the document, so it holds several signatures' chains at once.
/// Each is also held to [`crate::trust::MAX_CERTIFICATE_BYTES`].
pub const MAX_DSS_CERTIFICATES: usize = 32;

/// The most `/VRI` entries walked. One per signature the document's
/// validation data was gathered for; [`crate::docinfo`] walks at most 32
/// signatures, so twice that covers any document it reads in full.
pub const MAX_VRI: usize = 64;

/// How far in the future a response's `thisUpdate` may be before its dates
/// are called incoherent: clock skew between the responder and this
/// computer, not a window for postdating.
const SKEW: u64 = 5 * 60;

/// id-kp-OCSPSigning, RFC 6960 §4.2.2.2: the purpose a delegated responder's
/// certificate must name.
pub const OCSP_SIGNING: &str = "1.3.6.1.5.5.7.3.9";

/// id-pkix-ocsp-basic, the only response type RFC 6960 defines.
const OCSP_BASIC: &str = "1.3.6.1.5.5.7.48.1.1";

/// id-pkix-ocsp-nonce, the one critical-capable response extension read as
/// harmless: it means something only to the party that asked.
const OCSP_NONCE: &str = "1.3.6.1.5.5.7.48.1.2";

/// id-pkix-ocsp-archive-cutoff, RFC 6960 §4.4.4.
const ARCHIVE_CUTOFF: &str = "1.3.6.1.5.5.7.48.1.6";

/// id-ce-expiredCertsOnCRL (X.509 2019 §9.6.2.9).
const EXPIRED_CERTS_ON_CRL: &str = "2.5.29.60";

/// The CRL extensions a list may mark critical and still be read here.
/// `issuingDistributionPoint` is read for its scope ([`in_scope`]); anything
/// else critical --- a delta indicator, above all --- is [`Gap::Unsupported`].
const ISSUING_DISTRIBUTION_POINT: &str = "2.5.29.28";

/// The answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The document carries no revocation data about this certificate.
    /// **Not checked**, and never to be read as reassurance.
    #[default]
    None,
    /// Data about it was there, and nothing could be concluded. [`Revocation::why`]
    /// says what stopped it.
    Unchecked,
    /// A checked OCSP response says its responder does not know it.
    Unknown,
    /// Checked data lists it as revoked.
    Revoked,
    /// Checked, fresh data says it was not revoked.
    Good,
}

/// Why data about a certificate led to no conclusion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gap {
    /// Revocation data in the document could not be read, so what it said is
    /// not known.
    Unreadable,
    /// The document carries more revocation data than tpdf reads, and what
    /// was left unread might have said revoked.
    Bound,
    /// The certificate's issuer is not in the document, so data about it
    /// cannot be checked.
    Issuer,
    /// The data's own signature does not check out.
    Signature,
    /// The data is signed by a party the certificate's issuer did not
    /// authorise to answer for it.
    Unauthorised,
    /// A hash or signature algorithm tpdf does not implement.
    Algorithm,
    /// A form tpdf does not interpret: a delta or indirect list, a partial
    /// scope, or an extension marked critical that it does not know.
    Unsupported,
    /// The latest data does not reach the moment being judged.
    Stale,
    /// The data was issued after the certificate expired, and does not say
    /// that it keeps expired certificates.
    Expired,
    /// The data's own dates do not hang together.
    Dates,
    /// The document's signatures together asked for more hashing than
    /// [`crate::integrity::MAX_HASHED`].
    Budget,
}

/// Which kind of data answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// An OCSP response, RFC 6960.
    Ocsp,
    /// A certificate revocation list, RFC 5280 §5.
    Crl,
}

/// The reason a revocation states, RFC 5280 §5.3.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Unspecified,
    KeyCompromise,
    CaCompromise,
    AffiliationChanged,
    Superseded,
    CessationOfOperation,
    CertificateHold,
    RemoveFromCrl,
    PrivilegeWithdrawn,
    AaCompromise,
}

/// Whose clock the moment judged is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// The present, on this computer's clock: nothing better was known.
    #[default]
    Now,
    /// The signature's `/M`: the signer's own claim.
    Claimed,
    /// The time a timestamp token states --- for the authority's own
    /// certificate, whose word it is.
    Stated,
    /// The time an intact timestamp from an authority this computer trusts
    /// attests.
    Attested,
}

/// The moment a certificate's revocation is judged at, and whose it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moment {
    /// Whose clock.
    pub basis: Basis,
    /// Seconds since the epoch.
    pub at: u64,
}

/// What the document's own revocation data says about one certificate.
///
/// **A verdict**, shown only where [`crate::trust::Trust`] is: beside an
/// intact or weak signature, and beside an attested timestamp's authority.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Revocation {
    /// The answer.
    pub standing: Status,
    /// Why nothing was concluded, for `unchecked`; `None` otherwise.
    pub why: Option<Gap>,
    /// Which data answered, for `good`, `revoked` and `unknown`.
    pub source: Option<Source>,
    /// When that data was issued (`thisUpdate`), formatted; empty otherwise.
    pub issued: String,
    /// When its issuer promised the next (`nextUpdate`), formatted; empty
    /// when it states none or nothing answered.
    pub next: String,
    /// When it was revoked, formatted, for `revoked`.
    pub revoked: String,
    /// The reason a revocation states, when it states one.
    pub reason: Option<Reason>,
    /// Whose clock [`Revocation::moment`] is.
    pub basis: Basis,
    /// The moment judged, formatted.
    pub moment: String,
    /// `revoked`, **after** a moment a trusted timestamp attests --- which does
    /// not undo the signature. Only ever true for [`Basis::Attested`].
    pub after_moment: bool,
}

impl Revocation {
    fn at(moment: Moment) -> Self {
        Revocation {
            basis: moment.basis,
            moment: format_time(moment.at),
            ..Revocation::default()
        }
    }

    fn unchecked(moment: Moment, why: Gap) -> Self {
        Revocation {
            standing: Status::Unchecked,
            why: Some(why),
            ..Revocation::at(moment)
        }
    }

    /// Whether this answer fails `verify --strict`: revoked, unless after an
    /// attested moment. `none`, `unknown` and `unchecked` do not, because
    /// they would fail nearly every document --- `docs/PLAN.md` records it.
    #[must_use]
    pub fn undoes(&self) -> bool {
        self.standing == Status::Revoked && !self.after_moment
    }
}

/// Revocation data as found, before any of it is parsed: DER, bounded.
#[derive(Clone, Debug, Default)]
pub struct Material {
    /// Candidate issuer certificates, DER.
    pub certificates: Vec<Vec<u8>>,
    /// `BasicOCSPResponse`s, DER --- unwrapped from an `OCSPResponse` where
    /// the carrier held one.
    pub responses: Vec<Vec<u8>>,
    /// `CertificateList`s, DER.
    pub lists: Vec<Vec<u8>>,
    /// Entries present and not readable: an undecodable stream, a response
    /// that is not a successful basic one, anything over its size bound.
    pub unread: usize,
    /// Entries not read at a count bound.
    pub dropped: usize,
}

impl Material {
    /// Adds a certificate, within [`MAX_DSS_CERTIFICATES`] and
    /// [`crate::trust::MAX_CERTIFICATE_BYTES`].
    pub fn certificate(&mut self, der: Vec<u8>) {
        if der.len() > crate::trust::MAX_CERTIFICATE_BYTES {
            self.unread += 1;
        } else if self.certificates.contains(&der) {
        } else if self.certificates.len() >= MAX_DSS_CERTIFICATES {
            self.dropped += 1;
        } else {
            self.certificates.push(der);
        }
    }

    /// Adds a full `OCSPResponse`, as a `/DSS` and the Adobe attribute carry
    /// it: only a successful one of the basic type is kept.
    pub fn response(&mut self, der: &[u8]) {
        if der.len() > MAX_RESPONSE_BYTES {
            self.unread += 1;
            return;
        }
        let basic = OcspResponse::from_der(der).ok().and_then(|response| {
            let bytes = response.response_bytes?;
            (response.response_status == OcspResponseStatus::Successful
                && bytes.response_type.to_string() == OCSP_BASIC)
                .then(|| bytes.response.as_bytes().to_vec())
        });
        match basic {
            Some(basic) => self.basic(basic),
            None => self.unread += 1,
        }
    }

    /// Adds a `BasicOCSPResponse`, as the CMS `crls` set carries one.
    pub fn basic(&mut self, der: Vec<u8>) {
        if der.len() > MAX_RESPONSE_BYTES {
            self.unread += 1;
        } else if self.responses.contains(&der) {
        } else if self.responses.len() >= MAX_RESPONSES {
            self.dropped += 1;
        } else {
            self.responses.push(der);
        }
    }

    /// Adds a revocation list.
    pub fn list(&mut self, der: Vec<u8>) {
        if der.len() > MAX_LIST_BYTES {
            self.unread += 1;
        } else if self.lists.contains(&der) {
        } else if self.lists.len() >= MAX_LISTS {
            self.dropped += 1;
        } else {
            self.lists.push(der);
        }
    }

    /// Everything in `other` added to this, each entry through the same bound.
    pub fn extend(&mut self, other: &Material) {
        for certificate in &other.certificates {
            self.certificate(certificate.clone());
        }
        for response in &other.responses {
            self.basic(response.clone());
        }
        for list in &other.lists {
            self.list(list.clone());
        }
        self.unread += other.unread;
        self.dropped += other.dropped;
    }

    /// What a CMS `SignedData` carries: the Adobe `adbe-revocationInfoArchival`
    /// signed attribute of its signer (1.2.840.113583.1.1.8), its `crls` set
    /// --- lists, and OCSP responses in `id-ri-ocsp-response` form --- and its
    /// certificates as candidate issuers.
    #[must_use]
    pub fn of_cms(blob: &[u8]) -> Material {
        let mut out = Material::default();
        let Some(signed) = cms::content_info::ContentInfo::from_der(blob)
            .ok()
            .and_then(|info| {
                info.content
                    .decode_as::<cms::signed_data::SignedData>()
                    .ok()
            })
        else {
            return out;
        };
        for certificate in crate::docinfo::certificates_of(&signed) {
            match certificate.to_der() {
                Ok(der) => out.certificate(der),
                Err(_) => out.unread += 1,
            }
        }
        if let Some(choices) = &signed.crls {
            for choice in choices.0.iter() {
                match choice {
                    cms::revocation::RevocationInfoChoice::Crl(list) => match list.to_der() {
                        Ok(der) => out.list(der),
                        Err(_) => out.unread += 1,
                    },
                    cms::revocation::RevocationInfoChoice::Other(other) => {
                        // id-ri-ocsp-response, RFC 5940 §4.1: a BasicOCSPResponse.
                        if other.other_format.oid.to_string() == "1.3.6.1.5.5.7.16.2" {
                            match other.other.to_der() {
                                Ok(der) => out.basic(der),
                                Err(_) => out.unread += 1,
                            }
                        } else {
                            out.unread += 1;
                        }
                    }
                }
            }
        }
        let archival = signed
            .signer_infos
            .0
            .as_slice()
            .first()
            .and_then(|info| info.signed_attrs.as_ref())
            .and_then(|attributes| {
                attributes
                    .iter()
                    .find(|a| a.oid.to_string() == "1.2.840.113583.1.1.8")
            });
        if let Some(attribute) = archival {
            match attribute.values.as_slice() {
                [value] => match value.to_der() {
                    Ok(der) => archival_into(&der, &mut out),
                    Err(_) => out.unread += 1,
                },
                _ => out.unread += 1,
            }
        }
        out
    }
}

/// `RevocationInfoArchival` (Adobe, PDF 32000-1 §12.8.3.3.2):
///
/// ```text
/// SEQUENCE {
///   crl          [0] EXPLICIT SEQUENCE of CRLs OPTIONAL,
///   ocsp         [1] EXPLICIT SEQUENCE of OCSPResponse OPTIONAL,
///   otherRevInfo [2] EXPLICIT SEQUENCE of OtherRevInfo OPTIONAL }
/// ```
///
/// Walked by tag rather than typed, so an `otherRevInfo` --- which this does
/// not read --- is counted as unread rather than refusing the rest.
fn archival_into(der: &[u8], out: &mut Material) {
    use der::asn1::AnyRef;
    use der::{Reader as _, Tag, TagNumber, Tagged as _};

    let walk = || -> Option<Vec<(TagNumber, Vec<Vec<u8>>)>> {
        let outer = AnyRef::from_der(der).ok()?;
        if outer.tag() != Tag::Sequence {
            return None;
        }
        let mut reader = der::SliceReader::new(outer.value()).ok()?;
        let mut found = Vec::new();
        while !reader.is_finished() {
            let tagged: AnyRef<'_> = reader.decode().ok()?;
            let Tag::ContextSpecific { number, .. } = tagged.tag() else {
                return None;
            };
            let inner = AnyRef::from_der(tagged.value()).ok()?;
            let mut items = der::SliceReader::new(inner.value()).ok()?;
            let mut each = Vec::new();
            while !items.is_finished() {
                let item: AnyRef<'_> = items.decode().ok()?;
                each.push(item.to_der().ok()?);
            }
            found.push((number, each));
        }
        Some(found)
    };
    let Some(found) = walk() else {
        out.unread += 1;
        return;
    };
    for (number, items) in found {
        for item in items {
            match number {
                TagNumber::N0 => out.list(item),
                TagNumber::N1 => out.response(&item),
                _ => out.unread += 1,
            }
        }
    }
}

/// One OCSP response, parsed, with the bytes its signature is over.
struct Response {
    basic: BasicOcspResponse,
    /// `tbsResponseData` exactly as written.
    signed: Vec<u8>,
}

/// One revocation list, parsed, with the bytes its signature is over.
struct List {
    list: CertificateList,
    /// `tbsCertList` exactly as written.
    signed: Vec<u8>,
}

/// Whether a response or list at an index checked out under an issuer, keyed
/// by the kind, the index and the issuer's DER.
type Checked = RefCell<HashMap<(Source, usize, Vec<u8>), Result<(), Gap>>>;

/// Revocation data, parsed once for every certificate asked about.
pub struct Pool {
    certificates: Vec<Certificate>,
    responses: Vec<Response>,
    lists: Vec<List>,
    unread: usize,
    dropped: usize,
    /// Whether a response or list at an index checked out under an issuer,
    /// by the issuer's DER: a list is hashed whole to check it, and eight
    /// megabytes hashed once per certificate would be the cost of asking.
    checked: Checked,
}

impl Pool {
    /// Parses `material`. What will not parse is counted as unread, never
    /// dropped in silence.
    #[must_use]
    pub fn new(material: &Material) -> Pool {
        let mut unread = material.unread;
        let certificates = material
            .certificates
            .iter()
            .filter_map(|der| {
                let parsed = Certificate::from_der(der).ok();
                unread += usize::from(parsed.is_none());
                parsed
            })
            .collect();
        let responses = material
            .responses
            .iter()
            .filter_map(|der| {
                let parsed = BasicOcspResponse::from_der(der).ok().and_then(|basic| {
                    Some(Response {
                        signed: first_element(der)?,
                        basic,
                    })
                });
                unread += usize::from(parsed.is_none());
                parsed
            })
            .collect();
        let lists = material
            .lists
            .iter()
            .filter_map(|der| {
                let parsed = CertificateList::from_der(der).ok().and_then(|list| {
                    Some(List {
                        signed: first_element(der)?,
                        list,
                    })
                });
                unread += usize::from(parsed.is_none());
                parsed
            })
            .collect();
        Pool {
            certificates,
            responses,
            lists,
            unread,
            dropped: material.dropped,
            checked: RefCell::new(HashMap::new()),
        }
    }

    /// Every certificate the pool and its responses carry: candidate issuers
    /// and responders.
    fn candidates(&self) -> impl Iterator<Item = &Certificate> {
        self.certificates.iter().chain(
            self.responses
                .iter()
                .flat_map(|r| r.basic.certs.iter().flatten()),
        )
    }
}

/// The first element of a DER `SEQUENCE`, as written: the bytes an X.509
/// signature is over (`tbsCertList`, `tbsResponseData`).
///
/// Sliced out rather than re-encoded from the parsed value, for the reason
/// `integrity::signed_attributes` gives: the signature is over what was
/// written.
fn first_element(der: &[u8]) -> Option<Vec<u8>> {
    use der::asn1::AnyRef;
    use der::Reader as _;

    let outer = AnyRef::from_der(der).ok()?;
    let value = outer.value();
    let mut reader = der::SliceReader::new(value).ok()?;
    let _: AnyRef<'_> = reader.decode().ok()?;
    let end = usize::try_from(u32::from(reader.position())).ok()?;
    value.get(..end).map(<[u8]>::to_vec)
}

/// What one piece of data said, before the pieces are combined.
#[derive(Clone, Debug)]
enum Finding {
    Said(Said),
    Gap(Gap),
}

#[derive(Clone, Debug)]
struct Said {
    source: Source,
    status: Stated,
    this_update: u64,
    next_update: Option<u64>,
    /// `archiveCutoff` or `expiredCertsOnCRL`, when stated.
    keeps_expired_since: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
enum Stated {
    Good,
    Revoked { at: u64, reason: Option<Reason> },
    Unknown,
}

/// What the document's revocation data says about `subject`, at `moment`.
///
/// `candidates` are certificates the subject's issuer may be among beyond the
/// pools' own: the signature's certificates, or the token's. `pools` are the
/// document's `/DSS` and the signature's own CMS data. `now` bounds how far in
/// the future data may claim to have been issued. `budget` is the document's
/// hashing budget, charged before each signature is checked.
#[must_use]
pub fn judge(
    subject: &Certificate,
    candidates: &[Certificate],
    pools: &[&Pool],
    moment: Moment,
    now: u64,
    budget: &mut u64,
) -> Revocation {
    let everything: Vec<&Certificate> = candidates
        .iter()
        .chain(pools.iter().flat_map(|pool| pool.candidates()))
        .collect();
    let issuer = issuer_of(subject, &everything);
    let issuer_name = subject.tbs_certificate.issuer.to_der().unwrap_or_default();

    let mut findings = Vec::new();
    for pool in pools {
        for (index, response) in pool.responses.iter().enumerate() {
            findings.extend(from_response(
                subject,
                issuer,
                &issuer_name,
                &everything,
                pool,
                index,
                response,
                budget,
            ));
        }
        for (index, list) in pool.lists.iter().enumerate() {
            findings.extend(from_list(
                subject,
                issuer,
                &issuer_name,
                pool,
                index,
                list,
                budget,
            ));
        }
    }

    let unread = pools.iter().map(|pool| pool.unread).sum::<usize>();
    let dropped = pools.iter().map(|pool| pool.dropped).sum::<usize>();
    let answer = combine(subject, findings, moment, now);
    // An answer that reassures, or that says nothing was there, cannot stand
    // beside data that was there and not read: that data might have said
    // revoked. A revocation found stands whatever else was missed.
    if matches!(answer.standing, Status::Good | Status::None) {
        if dropped > 0 {
            return Revocation::unchecked(moment, Gap::Bound);
        }
        if unread > 0 {
            return Revocation::unchecked(moment, Gap::Unreadable);
        }
    }
    answer
}

/// The certificate that issued `subject`: named as its issuer, and whose key
/// verifies its signature. A name alone is not enough --- two certificates may
/// share one, and the one whose key the subject's signature does not verify
/// under did not issue it.
pub(crate) fn issuer_of<'a>(
    subject: &Certificate,
    candidates: &[&'a Certificate],
) -> Option<&'a Certificate> {
    let wanted = subject.tbs_certificate.issuer.to_der().ok()?;
    let signed = subject.tbs_certificate.to_der().ok()?;
    let value = subject.signature.as_bytes()?;
    candidates.iter().copied().find(|candidate| {
        candidate.tbs_certificate.subject.to_der().ok().as_ref() == Some(&wanted)
            && crate::integrity::signed_by(candidate, &subject.signature_algorithm, &signed, value)
                == Ok(true)
    })
}

/// Charges `cost` to the budget, or refuses before anything is hashed.
fn charge(budget: &mut u64, cost: usize) -> Result<(), Gap> {
    let cost = cost as u64;
    if cost > *budget {
        return Err(Gap::Budget);
    }
    *budget -= cost;
    Ok(())
}

/// A certificate's public key as a `CertID` hashes it: the `subjectPublicKey`
/// BIT STRING's value, without its tag, length or unused-bits octet.
fn key_bits(certificate: &Certificate) -> &[u8] {
    certificate
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .raw_bytes()
}

/// Seconds since the epoch of an X.509 time.
fn seconds(time: x509_cert::time::Time) -> u64 {
    time.to_unix_duration().as_secs()
}

fn general_seconds(time: der::asn1::GeneralizedTime) -> u64 {
    time.to_unix_duration().as_secs()
}

/// A time, formatted as every date in `docinfo` is.
#[must_use]
pub fn format_time(at: u64) -> String {
    match der::DateTime::from_unix_duration(std::time::Duration::from_secs(at)) {
        Ok(at) => format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
            at.year(),
            at.month(),
            at.day(),
            at.hour(),
            at.minutes(),
            at.seconds()
        ),
        Err(_) => String::new(),
    }
}

/// What the responses in one OCSP response say about `subject`.
#[allow(clippy::too_many_arguments)]
fn from_response(
    subject: &Certificate,
    issuer: Option<&Certificate>,
    issuer_name: &[u8],
    everything: &[&Certificate],
    pool: &Pool,
    index: usize,
    response: &Response,
    budget: &mut u64,
) -> Vec<Finding> {
    let serial = &subject.tbs_certificate.serial_number;
    let mut out = Vec::new();
    for single in &response.basic.tbs_response_data.responses {
        let id = &single.cert_id;
        if &id.serial_number != serial {
            continue;
        }
        // A `CertID` names the issuer by two hashes. Without the issuer in
        // hand the key hash cannot be computed, so a response naming this
        // serial under this issuer's *name* is data about this certificate
        // that cannot be checked.
        let Some(hash) = Hash::from_oid(&id.hash_algorithm.oid.to_string()) else {
            out.push(Finding::Gap(Gap::Algorithm));
            continue;
        };
        if hash.digest(&[issuer_name]) != id.issuer_name_hash.as_bytes() {
            continue;
        }
        let Some(issuer) = issuer else {
            out.push(Finding::Gap(Gap::Issuer));
            continue;
        };
        if hash.digest(&[key_bits(issuer)]) != id.issuer_key_hash.as_bytes() {
            continue;
        }
        if let Err(gap) = response_checks_out(issuer, everything, pool, index, response, budget) {
            out.push(Finding::Gap(gap));
            continue;
        }
        if critical_unknown(single.single_extensions.as_deref(), &[]) {
            out.push(Finding::Gap(Gap::Unsupported));
            continue;
        }
        let status = match single.cert_status {
            CertStatus::Good(_) => Stated::Good,
            CertStatus::Revoked(info) => Stated::Revoked {
                at: general_seconds(info.revocation_time.0),
                reason: info.revocation_reason.map(reason_of),
            },
            CertStatus::Unknown(_) => Stated::Unknown,
        };
        out.push(Finding::Said(Said {
            source: Source::Ocsp,
            status,
            this_update: general_seconds(single.this_update.0),
            next_update: single.next_update.map(|t| general_seconds(t.0)),
            keeps_expired_since: extension_time(
                response
                    .basic
                    .tbs_response_data
                    .response_extensions
                    .as_deref(),
                ARCHIVE_CUTOFF,
            ),
        }));
    }
    out
}

/// Whether an OCSP response is signed by `issuer`, or by a responder `issuer`
/// authorised: RFC 6960 §4.2.2.2.
///
/// The responder is found by its `ResponderID` among the issuer, the
/// response's own certificates and the document's. It is authorised when it
/// **is** the issuer, or when the issuer issued its certificate --- its key
/// verifies that certificate's signature --- with `id-kp-OCSPSigning` among
/// its purposes, and in force when the response was produced. Then, and only
/// then, is the response's own signature asked about.
fn response_checks_out(
    issuer: &Certificate,
    everything: &[&Certificate],
    pool: &Pool,
    index: usize,
    response: &Response,
    budget: &mut u64,
) -> Result<(), Gap> {
    let key = (Source::Ocsp, index, issuer.to_der().unwrap_or_default());
    if let Some(known) = pool.checked.borrow().get(&key) {
        return *known;
    }
    let answer = (|| {
        let data = &response.basic.tbs_response_data;
        if critical_unknown(data.response_extensions.as_deref(), &[OCSP_NONCE]) {
            return Err(Gap::Unsupported);
        }
        let produced = general_seconds(data.produced_at.0);
        let own = response.basic.certs.iter().flatten();
        let mut named: Vec<&Certificate> = Vec::new();
        for candidate in std::iter::once(issuer)
            .chain(own)
            .chain(everything.iter().copied())
        {
            let fits = match &data.responder_id {
                ResponderId::ByName(name) => {
                    candidate.tbs_certificate.subject.to_der().ok() == name.to_der().ok()
                }
                ResponderId::ByKey(hash) => {
                    Hash::Sha1.digest(&[key_bits(candidate)]) == hash.as_bytes()
                }
            };
            if fits && !named.contains(&candidate) {
                named.push(candidate);
            }
        }
        let value = response.basic.signature.as_bytes().ok_or(Gap::Signature)?;
        let mut worst = Gap::Unauthorised;
        for responder in named {
            let authorised = responder == issuer || delegated(responder, issuer, produced);
            charge(budget, response.signed.len())?;
            let signed = crate::integrity::signed_by(
                responder,
                &response.basic.signature_algorithm,
                &response.signed,
                value,
            );
            match (authorised, signed) {
                (true, Ok(true)) => return Ok(()),
                (true, Ok(false)) => worst = Gap::Signature,
                (true, Err(_)) => worst = Gap::Algorithm,
                // Signed by somebody the issuer did not authorise: the
                // arithmetic may hold, and it proves nothing about the issuer.
                (false, _) => {}
            }
        }
        Err(worst)
    })();
    pool.checked.borrow_mut().insert(key, answer);
    answer
}

/// Whether `responder` is a responder `issuer` delegated OCSP to, at `at`:
/// issued by it --- `issuer`'s key verifies its certificate --- naming
/// `id-kp-OCSPSigning`, and in force then.
///
/// The responder certificate's *issuer name* is not compared with `issuer`'s:
/// the signature under `issuer`'s key already says who issued it, and a name
/// comparison after that could not fail on any certificate that key signed ---
/// a mutation removing it survived, which is how it came out (2026-09-28).
/// The responder's own revocation is not asked: real responders' certificates
/// carry `id-pkix-ocsp-nocheck`, and tpdf checks no chain above the one
/// certificate asked about (`docs/THREAT-MODEL.md` residual 31).
fn delegated(responder: &Certificate, issuer: &Certificate, at: u64) -> bool {
    let tbs = &responder.tbs_certificate;
    let from = seconds(tbs.validity.not_before);
    let until = seconds(tbs.validity.not_after);
    let purpose = tbs
        .extensions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .find(|e| e.extn_id.to_string() == "2.5.29.37")
        .and_then(|e| {
            x509_cert::ext::pkix::ExtendedKeyUsage::from_der(e.extn_value.as_bytes()).ok()
        })
        .is_some_and(|usage| usage.0.iter().any(|p| p.to_string() == OCSP_SIGNING));
    let signed = tbs.to_der().ok().zip(responder.signature.as_bytes());
    purpose
        && (from..=until).contains(&at)
        && signed.is_some_and(|(tbs, value)| {
            crate::integrity::signed_by(issuer, &responder.signature_algorithm, &tbs, value)
                == Ok(true)
        })
}

/// What one revocation list says about `subject`.
fn from_list(
    subject: &Certificate,
    issuer: Option<&Certificate>,
    issuer_name: &[u8],
    pool: &Pool,
    index: usize,
    list: &List,
    budget: &mut u64,
) -> Option<Finding> {
    let tbs = &list.list.tbs_cert_list;
    // A list is about the certificates its issuer issued. Another issuer's
    // list says nothing about this one, whatever serials it holds.
    if tbs.issuer.to_der().ok().as_deref() != Some(issuer_name) {
        return None;
    }
    match in_scope(subject, tbs.crl_extensions.as_deref()) {
        Ok(true) => {}
        Ok(false) => return None,
        Err(gap) => return Some(Finding::Gap(gap)),
    }
    let Some(issuer) = issuer else {
        return Some(Finding::Gap(Gap::Issuer));
    };
    if let Err(gap) = list_checks_out(issuer, pool, index, list, budget) {
        return Some(Finding::Gap(gap));
    }
    let serial = &subject.tbs_certificate.serial_number;
    let entry = tbs
        .revoked_certificates
        .iter()
        .flatten()
        .find(|entry| &entry.serial_number == serial);
    let status = match entry {
        None => Stated::Good,
        Some(entry) => {
            if critical_unknown(entry.crl_entry_extensions.as_deref(), &["2.5.29.21"]) {
                return Some(Finding::Gap(Gap::Unsupported));
            }
            Stated::Revoked {
                at: seconds(entry.revocation_date),
                reason: entry
                    .crl_entry_extensions
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .find(|e| e.extn_id.to_string() == "2.5.29.21")
                    .and_then(|e| {
                        x509_cert::ext::pkix::CrlReason::from_der(e.extn_value.as_bytes()).ok()
                    })
                    .map(reason_of),
            }
        }
    };
    Some(Finding::Said(Said {
        source: Source::Crl,
        status,
        this_update: seconds(tbs.this_update),
        next_update: tbs.next_update.map(seconds),
        keeps_expired_since: extension_time(tbs.crl_extensions.as_deref(), EXPIRED_CERTS_ON_CRL),
    }))
}

/// Whether a list is signed by `issuer`, as RFC 5280 §6.3.3 requires: the
/// issuer's key verifies it, under the algorithm it names inside and outside
/// alike, and the issuer's key usage --- when it states one --- permits
/// `cRLSign`. Asked once per list and issuer.
fn list_checks_out(
    issuer: &Certificate,
    pool: &Pool,
    index: usize,
    list: &List,
    budget: &mut u64,
) -> Result<(), Gap> {
    let key = (Source::Crl, index, issuer.to_der().unwrap_or_default());
    if let Some(known) = pool.checked.borrow().get(&key) {
        return *known;
    }
    let answer = (|| {
        let tbs = &list.list.tbs_cert_list;
        if tbs.signature != list.list.signature_algorithm {
            return Err(Gap::Algorithm);
        }
        if critical_unknown(tbs.crl_extensions.as_deref(), &[ISSUING_DISTRIBUTION_POINT]) {
            return Err(Gap::Unsupported);
        }
        if !signs_lists(issuer) {
            return Err(Gap::Unauthorised);
        }
        let value = list.list.signature.as_bytes().ok_or(Gap::Signature)?;
        charge(budget, list.signed.len())?;
        match crate::integrity::signed_by(
            issuer,
            &list.list.signature_algorithm,
            &list.signed,
            value,
        ) {
            Ok(true) => Ok(()),
            Ok(false) => Err(Gap::Signature),
            Err(_) => Err(Gap::Algorithm),
        }
    })();
    pool.checked.borrow_mut().insert(key, answer);
    answer
}

/// Whether `issuer`'s key usage, when it states one, includes `cRLSign`.
fn signs_lists(issuer: &Certificate) -> bool {
    let Some(extension) = issuer
        .tbs_certificate
        .extensions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .find(|e| e.extn_id.to_string() == "2.5.29.15")
    else {
        return true;
    };
    x509_cert::ext::pkix::KeyUsage::from_der(extension.extn_value.as_bytes())
        .is_ok_and(|usage| usage.crl_sign())
}

/// Whether a list's `issuingDistributionPoint` covers `subject`: RFC 5280
/// §6.3.3 (b). `Ok(false)` for a list that is about other certificates of the
/// same issuer --- a partition, or only its authorities --- so its silence
/// about this one says nothing; `Err` for a scope this does not interpret.
fn in_scope(
    subject: &Certificate,
    extensions: Option<&[x509_cert::ext::Extension]>,
) -> Result<bool, Gap> {
    use x509_cert::ext::pkix::crl::dp::IssuingDistributionPoint;
    use x509_cert::ext::pkix::name::DistributionPointName;

    let Some(extension) = extensions
        .unwrap_or(&[])
        .iter()
        .find(|e| e.extn_id.to_string() == ISSUING_DISTRIBUTION_POINT)
    else {
        return Ok(true);
    };
    let point = IssuingDistributionPoint::from_der(extension.extn_value.as_bytes())
        .map_err(|_| Gap::Unsupported)?;
    if point.indirect_crl || point.only_some_reasons.is_some() {
        return Err(Gap::Unsupported);
    }
    let authority = subject
        .tbs_certificate
        .extensions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .find(|e| e.extn_id.to_string() == "2.5.29.19")
        .and_then(|e| {
            x509_cert::ext::pkix::BasicConstraints::from_der(e.extn_value.as_bytes()).ok()
        })
        .is_some_and(|constraints| constraints.ca);
    if point.only_contains_attribute_certs
        || (point.only_contains_user_certs && authority)
        || (point.only_contains_ca_certs && !authority)
    {
        return Ok(false);
    }
    match &point.distribution_point {
        None => Ok(true),
        Some(DistributionPointName::NameRelativeToCRLIssuer(_)) => Err(Gap::Unsupported),
        Some(DistributionPointName::FullName(names)) => {
            // The certificate must name this list among its own distribution
            // points: a partitioned issuer's other lists do not cover it.
            let theirs: Vec<Vec<u8>> = names.iter().filter_map(|n| n.to_der().ok()).collect();
            let points = subject
                .tbs_certificate
                .extensions
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .find(|e| e.extn_id.to_string() == "2.5.29.31")
                .and_then(|e| {
                    x509_cert::ext::pkix::CrlDistributionPoints::from_der(e.extn_value.as_bytes())
                        .ok()
                });
            Ok(points.is_some_and(|points| {
                points
                    .0
                    .iter()
                    .any(|point| match &point.distribution_point {
                        Some(DistributionPointName::FullName(ours)) => ours
                            .iter()
                            .filter_map(|n| n.to_der().ok())
                            .any(|n| theirs.contains(&n)),
                        _ => false,
                    })
            }))
        }
    }
}

/// Whether any extension is marked critical and is not one of `known`.
fn critical_unknown(extensions: Option<&[x509_cert::ext::Extension]>, known: &[&str]) -> bool {
    extensions
        .unwrap_or(&[])
        .iter()
        .any(|e| e.critical && !known.contains(&e.extn_id.to_string().as_str()))
}

/// A `GeneralizedTime` extension's value, by OID.
fn extension_time(extensions: Option<&[x509_cert::ext::Extension]>, oid: &str) -> Option<u64> {
    extensions
        .unwrap_or(&[])
        .iter()
        .find(|e| e.extn_id.to_string() == oid)
        .and_then(|e| der::asn1::GeneralizedTime::from_der(e.extn_value.as_bytes()).ok())
        .map(general_seconds)
}

fn reason_of(reason: x509_cert::ext::pkix::CrlReason) -> Reason {
    use x509_cert::ext::pkix::CrlReason as R;
    match reason {
        R::Unspecified => Reason::Unspecified,
        R::KeyCompromise => Reason::KeyCompromise,
        R::CaCompromise => Reason::CaCompromise,
        R::AffiliationChanged => Reason::AffiliationChanged,
        R::Superseded => Reason::Superseded,
        R::CessationOfOperation => Reason::CessationOfOperation,
        R::CertificateHold => Reason::CertificateHold,
        R::RemoveFromCRL => Reason::RemoveFromCrl,
        R::PrivilegeWithdrawn => Reason::PrivilegeWithdrawn,
        R::AaCompromise => Reason::AaCompromise,
    }
}

/// Every finding about one certificate, read as one answer.
///
/// A revocation any checked data states is the answer, the earliest if
/// several: once revoked, a certificate stays revoked, so a later *good* does
/// not outweigh it. Otherwise the latest-issued *good* is judged for
/// freshness --- EN 319 102-1 §5.2.6 takes the latest too --- then an
/// *unknown*, then the first reason nothing could be checked, and only when
/// nothing at all concerned the certificate, `none`.
fn combine(subject: &Certificate, findings: Vec<Finding>, moment: Moment, now: u64) -> Revocation {
    let mut said = Vec::new();
    let mut gaps = Vec::new();
    for finding in findings {
        match finding {
            Finding::Said(s) => {
                // Dates that do not hang together are no dates: a response
                // issued in the future, or promising its successor before
                // itself.
                let incoherent = s.this_update > now.saturating_add(SKEW)
                    || s.next_update.is_some_and(|next| next < s.this_update);
                if incoherent {
                    gaps.push(Gap::Dates);
                } else {
                    said.push(s);
                }
            }
            Finding::Gap(gap) => gaps.push(gap),
        }
    }
    let answered = |s: &Said, standing: Status| Revocation {
        standing,
        source: Some(s.source),
        issued: format_time(s.this_update),
        next: s.next_update.map(format_time).unwrap_or_default(),
        ..Revocation::at(moment)
    };

    let revoked = said
        .iter()
        .filter_map(|s| match s.status {
            Stated::Revoked { at, reason } => Some((s, at, reason)),
            _ => None,
        })
        .min_by_key(|(_, at, _)| *at);
    if let Some((s, at, reason)) = revoked {
        return Revocation {
            revoked: format_time(at),
            reason,
            after_moment: moment.basis == Basis::Attested && at > moment.at,
            ..answered(s, Status::Revoked)
        };
    }
    let good = said
        .iter()
        .filter(|s| matches!(s.status, Stated::Good))
        .max_by_key(|s| s.this_update);
    if let Some(s) = good {
        let until = seconds(subject.tbs_certificate.validity.not_after);
        if s.this_update > until && !s.keeps_expired_since.is_some_and(|since| since <= until) {
            return Revocation::unchecked(moment, Gap::Expired);
        }
        // EN 319 102-1 §5.2.5.4: fresh when issued after the moment minus
        // (nextUpdate - thisUpdate), which is nextUpdate after the moment;
        // with no nextUpdate, not fresh.
        if !s.next_update.is_some_and(|next| next > moment.at) {
            return Revocation::unchecked(moment, Gap::Stale);
        }
        return answered(s, Status::Good);
    }
    if let Some(s) = said.iter().find(|s| matches!(s.status, Stated::Unknown)) {
        return answered(s, Status::Unknown);
    }
    match gaps.first() {
        Some(gap) => Revocation::unchecked(moment, *gap),
        None => Revocation::at(moment),
    }
}

#[cfg(test)]
mod tests;
