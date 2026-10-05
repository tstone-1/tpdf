//! The certificates above a signer's, and above a timestamp authority's: the
//! walk up to a root, and what the document's revocation data says about each
//! certificate on it.
//!
//! ## Why the leaf is not enough
//!
//! A certificate is only as good as the one that issued it. An issuing
//! authority whose own certificate has been revoked --- its key reported
//! stolen, say --- vouches for nothing, and every certificate below it falls
//! with it. So a revocation of the certificate **above** the signer's undoes
//! the signature exactly as a revocation of the signer's own does, and a
//! reader that judges only the two leaves reads such a signature as sound.
//! Increment C2 already gathers, checks and writes revocation data for every
//! certificate on both chains; this is the reader's half (`docs/PLAN.md` §9,
//! *The whole chain*).
//!
//! ## One walk, for reading and for signing
//!
//! [`walk`] is the only place a chain is followed. `longterm::plan` walks it to
//! decide what to ask the certificate authorities about while signing, and the
//! worker walks it to decide what to judge while reading; two walks would be two
//! answers to *which certificates are on this chain*, and the file one writes
//! would be read by the other. The rules, each measured before it was written:
//!
//! - **The issuer is found by name and by key** ([`crate::revocation::issuer_of`]),
//!   among the candidates given --- never by name alone.
//! - **A root ends the chain**: self-issued, and verified by its own key.
//!   Nobody revokes a trust anchor through its own data.
//! - **So does a cross-certificate of a root among the candidates**: the same
//!   name and key as a self-issued certificate, issued by another root.
//!   DigiCert's and Sectigo's tokens carry their roots in that form, and the
//!   chain a verifier builds ends at the self-issued one (`docs/TRAPS.md`).
//! - **A certificate carrying `id-pkix-ocsp-nocheck` is walked through and not
//!   judged** (RFC 6960 §4.2.2.2.1): it is a delegated responder's.
//! - **A certificate seen twice ends the walk**, so two authorities that
//!   cross-certify each other with neither root present cannot loop it.
//! - **At most [`MAX_CHAIN`] certificates are judged**; the rest are counted to
//!   the walk's end, never dropped in silence, and a chain with any counted
//!   cannot read `good`.
//!
//! ## Which candidates the reader offers
//!
//! The signature's certificates, its token's, the `/DSS`'s and those inside
//! the document's OCSP responses: exactly the population
//! [`crate::revocation::judge`] looks for an issuer in, so a certificate whose
//! data checked out always has its issuer on the walk too. **The chain the
//! operating system assembles is not offered**, though `trust.rs` builds one:
//! a revocation answer is a property of the file and reads the same on every
//! computer, which a chain completed from one computer's store would not; a
//! B-LT document carries its certificates (EN 319 142-1 §5.4.2), and tpdf's own
//! writer puts every issuer and the self-issued anchor in its `/DSS`; and a
//! certificate the walk cannot continue past reads *not checked*, which is the
//! safe direction. Residual 34 of `docs/THREAT-MODEL.md` names the price.

use der::Encode as _;
use x509_cert::Certificate;

use super::{issuer_of, judge, Moment, Pool, Revocation, Status};

/// id-pkix-ocsp-nocheck, RFC 6960 §4.2.2.2.1.
pub const OCSP_NO_CHECK: &str = "1.3.6.1.5.5.7.48.1.5";

/// The most certificates judged on one chain, the leaf included.
///
/// Real chains are two to four deep: a signer or an authority, one or two
/// issuing authorities, a root (measured on the three public timestamp
/// authorities, 2026-09-28). Eight is twice the deepest, and bounds the
/// hashing a hostile `/DSS` can ask for per chain.
pub const MAX_CHAIN: usize = 8;

/// How a walk ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// At a root among the candidates, or a cross-certificate of one.
    #[default]
    Root,
    /// At a certificate whose issuer is not among the candidates.
    NoIssuer,
    /// At a certificate already walked.
    Loop,
}

/// One certificate on a walk, with the one that issued it.
#[derive(Clone, Debug)]
pub struct Link {
    /// The certificate.
    pub certificate: Certificate,
    /// Its issuer, found by name and key; `None` only for the last link of a
    /// walk that ended at [`End::NoIssuer`].
    pub issuer: Option<Certificate>,
    /// It carries `id-pkix-ocsp-nocheck`, so nobody need ask about it.
    pub no_check: bool,
}

/// A chain, from the leaf up.
#[derive(Clone, Debug)]
pub struct Walk {
    /// Every certificate below the root, the leaf first, at most
    /// [`MAX_CHAIN`]. Empty when the leaf is itself a root.
    pub links: Vec<Link>,
    /// The root the walk ended at: the self-issued certificate, when the one
    /// met was a cross-certificate of it. `None` unless [`End::Root`].
    pub anchor: Option<Certificate>,
    /// How it ended.
    pub end: End,
    /// Certificates past [`MAX_CHAIN`], counted to the walk's end.
    pub dropped: usize,
}

/// Whether the certificate says nobody need ask about its own revocation.
#[must_use]
pub fn no_check(certificate: &Certificate) -> bool {
    certificate
        .tbs_certificate
        .extensions
        .iter()
        .flatten()
        .any(|e| e.extn_id.to_string() == OCSP_NO_CHECK)
}

/// Whether `certificate` is a root: self-issued, and verified by its own key.
#[must_use]
pub fn root(certificate: &Certificate) -> bool {
    certificate.tbs_certificate.subject == certificate.tbs_certificate.issuer
        && issuer_of(certificate, &[certificate]).is_some()
}

/// The root `certificate` is the same authority as: itself when it is one,
/// otherwise a self-issued certificate among `candidates` with its name and
/// its key --- the twin of a cross-certificate.
#[must_use]
pub fn anchor_of<'a>(
    certificate: &'a Certificate,
    candidates: &[&'a Certificate],
) -> Option<&'a Certificate> {
    if root(certificate) {
        return Some(certificate);
    }
    candidates.iter().copied().find(|candidate| {
        candidate.tbs_certificate.subject == certificate.tbs_certificate.subject
            && candidate.tbs_certificate.subject_public_key_info
                == certificate.tbs_certificate.subject_public_key_info
            && root(candidate)
    })
}

/// The chain from `leaf` up, its issuers found among `candidates`.
#[must_use]
pub fn walk(leaf: &Certificate, candidates: &[&Certificate]) -> Walk {
    let mut links = Vec::new();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut dropped = 0usize;
    let mut current = leaf.clone();
    loop {
        if let Some(anchor) = anchor_of(&current, candidates) {
            return Walk {
                links,
                anchor: Some(anchor.clone()),
                end: End::Root,
                dropped,
            };
        }
        let der = current.to_der().unwrap_or_default();
        if seen.contains(&der) {
            return Walk {
                links,
                anchor: None,
                end: End::Loop,
                dropped,
            };
        }
        seen.push(der);
        let issuer = issuer_of(&current, candidates).cloned();
        if links.len() < MAX_CHAIN {
            links.push(Link {
                no_check: no_check(&current),
                certificate: current,
                issuer: issuer.clone(),
            });
        } else {
            dropped += 1;
        }
        match issuer {
            Some(issuer) => current = issuer,
            None => {
                return Walk {
                    links,
                    anchor: None,
                    end: End::NoIssuer,
                    dropped,
                }
            }
        }
    }
}

/// One certificate on a chain, and what the document's data says about it.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Judged {
    /// The subject's distinguished name.
    pub subject: String,
    /// The subject's common name, empty when it has none.
    pub subject_cn: String,
    /// The serial number, uppercase hex.
    pub serial: String,
    /// The answer, at the moment the leaf is judged.
    pub revocation: Revocation,
}

/// What the document's revocation data says about a whole chain: the
/// signer's, or a timestamp authority's.
///
/// **The rule** ([`combine`]): any certificate revoked before the moment makes
/// the chain revoked, and names it; otherwise every certificate `good` makes it
/// good; otherwise the chain reads as its most telling answer --- `unknown`,
/// then `unchecked`, then `none`, then a revocation after an attested moment ---
/// the certificate nearest the leaf first among equals. Past [`MAX_CHAIN`], a
/// chain that would read `good`, `none` or revoked after the moment reads
/// `unchecked` instead, naming no certificate: what was not judged might have
/// been revoked before it.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Chain {
    /// Every certificate judged, the leaf first. Roots are not here, and
    /// neither is a certificate that needs no check.
    pub certificates: Vec<Judged>,
    /// The chain's answer.
    pub standing: Status,
    /// `revoked`, and only after an attested moment: the answer of
    /// [`Chain::decided_by`], which does not undo the signature.
    pub after_moment: bool,
    /// The index in [`Chain::certificates`] of the certificate that decides
    /// the answer; `None` when every one is `good`, or the bound decided it.
    pub decided_by: Option<usize>,
    /// Certificates on the chain past [`MAX_CHAIN`], not judged.
    pub dropped: usize,
    /// How the walk ended.
    pub end: End,
}

impl Chain {
    /// Whether the chain fails `verify --strict`: a certificate on it revoked,
    /// unless after an attested moment --- [`Revocation::undoes`], for the chain.
    #[must_use]
    pub fn undoes(&self) -> bool {
        self.standing == Status::Revoked && !self.after_moment
    }

    /// The certificate that decides the answer.
    #[must_use]
    pub fn deciding(&self) -> Option<&Judged> {
        self.decided_by.and_then(|at| self.certificates.get(at))
    }
}

/// How telling an answer is about a chain, most telling highest: what
/// [`combine`] names.
fn weight(revocation: &Revocation) -> u8 {
    match revocation.standing {
        Status::Revoked if !revocation.after_moment => 5,
        Status::Unknown => 4,
        Status::Unchecked => 3,
        Status::None => 2,
        Status::Revoked => 1,
        Status::Good => 0,
    }
}

/// The chain's answer from its certificates' answers --- the rule
/// [`Chain`] states.
#[must_use]
pub fn combine(certificates: Vec<Judged>, dropped: usize, end: End) -> Chain {
    // The first of the heaviest: `max_by_key` keeps the last, so the index
    // breaks the tie toward the leaf.
    let heaviest = certificates
        .iter()
        .enumerate()
        .filter(|(_, c)| weight(&c.revocation) > 0)
        .max_by_key(|(at, c)| (weight(&c.revocation), std::cmp::Reverse(*at)))
        .map(|(at, _)| at);
    let (standing, after_moment, decided_by) = match heaviest {
        Some(at) => {
            let r = &certificates[at].revocation;
            (r.standing, r.after_moment, Some(at))
        }
        None if certificates.is_empty() => (Status::None, false, None),
        None => (Status::Good, false, None),
    };
    // What was not judged might have been revoked, so nothing less telling
    // than `unchecked` can stand beside it: not `good`, not `none`, and not a
    // revocation after the moment, which reassures.
    let reassures = weight(&Revocation {
        standing,
        after_moment,
        ..Revocation::default()
    }) < weight(&Revocation {
        standing: Status::Unchecked,
        ..Revocation::default()
    });
    let (standing, after_moment, decided_by) = if dropped > 0 && reassures {
        (Status::Unchecked, false, None)
    } else {
        (standing, after_moment, decided_by)
    };
    Chain {
        certificates,
        standing,
        after_moment,
        decided_by,
        dropped,
        end,
    }
}

/// A certificate on a chain, as the answer names it.
fn judged(certificate: &Certificate, revocation: Revocation) -> Judged {
    let subject = &certificate.tbs_certificate.subject;
    Judged {
        subject: crate::certificate::distinguished_name(subject),
        subject_cn: crate::certificate::common_name(subject),
        serial: crate::certificate::hex_of(certificate.tbs_certificate.serial_number.as_bytes()),
        revocation,
    }
}

/// What the document's revocation data says about the chain above `leaf`,
/// whose own answer is `own`: every certificate below the root judged at
/// `moment`, as [`judge`] judges the leaf, with the same candidates.
///
/// `own` is passed rather than asked again so the leaf is judged once and the
/// chain's first certificate is, byte for byte, the answer beside it.
#[must_use]
pub fn chain(
    leaf: &Certificate,
    own: &Revocation,
    candidates: &[Certificate],
    pools: &[&Pool],
    moment: Moment,
    now: u64,
    budget: &mut u64,
) -> Chain {
    let everything: Vec<&Certificate> = candidates
        .iter()
        .chain(pools.iter().flat_map(|pool| pool.candidates()))
        .collect();
    let walked = walk(leaf, &everything);
    let mut certificates = vec![judged(leaf, own.clone())];
    for link in walked.links.iter().skip(1).filter(|link| !link.no_check) {
        let answer = judge(&link.certificate, candidates, pools, moment, now, budget);
        certificates.push(judged(&link.certificate, answer));
    }
    combine(certificates, walked.dropped, walked.end)
}

#[cfg(test)]
mod tests;
