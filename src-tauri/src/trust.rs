//! Whether a signer's certificate chains to a root this operating system trusts.
//!
//! ## The question, and the one it is not
//!
//! [`crate::integrity`] says whether the bytes are the ones signed and whether
//! the key in the certificate made the signature. It cannot say whose key that
//! is: anybody can make a certificate naming anybody. This module asks the one
//! party the reader already relies on for that --- **the operating system's
//! own trust store**, the one their mail client and browser use --- whether an
//! issuer it trusts vouches for the signer's certificate. `SecTrust` with a
//! basic X.509 policy on macOS; `CertGetCertificateChain` and
//! `CertVerifyCertificateChainPolicy(CERT_CHAIN_POLICY_BASE)` on Windows.
//!
//! It is asked **only on top of an `intact` or `weak` integrity verdict**
//! ([`crate::docinfo`] enforces it). For an altered, broken or unchecked
//! signature there is nothing to attribute to anybody, and a "trusted" beside
//! one would be read as mitigating it.
//!
//! **Not Adobe's list.** Most signed PDFs are made against Adobe's Approved
//! Trust List, which is not in any operating system's store, so a signature
//! that chains only to an AATL root reads here as ending at a root this
//! computer does not trust. The verdict names the store for that reason
//! (`docs/PLAN.md` §9, Phase 6, records the decision).
//!
//! ## No network, and no revocation here
//!
//! Network retrieval is off: `SecTrustSetNetworkFetchAllowed(false)`, and
//! cache-only retrieval with AIA and root auto-update disabled on Windows.
//! Nothing here fetches an intermediate, an OCSP response or a CRL, and nothing
//! here asks about revocation at all: that is [`crate::revocation`]'s, from the
//! data the document carries and never fetched (`docs/PLAN.md` §9). An issuer
//! missing from the signature, the document's `/DSS` and this machine reads as
//! a missing link rather than being looked up. On Windows a third: a root Microsoft distributes on demand, and which
//! this machine has not yet downloaded, reads as not trusted.
//!
//! ## Time
//!
//! The chain is evaluated **now**, unless the signature's timestamp earns an
//! attested moment ([`judge_at`], since 2026-09-28). `/M` is the signer's own
//! clock and is not evidence, and without such a timestamp tpdf cannot know that the
//! certificate was in force when the signature was made. So a certificate that
//! has expired since is reported as exactly that: [`Standing::Expired`] means
//! the chain is sound and ends at a trusted root **at the certificate's own
//! last moment**, and is out of date now. That needs a second evaluation, at a
//! moment inside the signer's certificate's dates; one that fails there too is
//! [`Standing::Untrusted`] with the reason it gives, because the certificate
//! was never vouched for and its dates are not the news.
//!
//! ## What the signer's certificate was issued for
//!
//! A chain the OS trusts, ending at a certificate whose extended key usage
//! names only purposes a document signature does not serve --- a web server's,
//! a code signer's --- is [`Doubt::Purpose`], not trusted. The issuer vouched
//! that the key belongs to a server name or a software publisher, not that its
//! holder signs documents, and a basic X.509 policy checks no usage at all.
//! The list is `sign_cms::DOCUMENT_PURPOSES`, the same one that decides which
//! of the reader's own certificates tpdf offers for signing: a certificate
//! tpdf would refuse to sign with is not one it calls trusted for signing.
//!
//! ## A timestamp authority is asked about the same way, for another purpose
//!
//! Since 2026-09-28 a timestamp token that [`crate::integrity::token`] finds
//! intact or weak has its authority asked about too, through the same store,
//! at the same present moment, with [`Purpose::Timestamping`]: RFC 3161 §2.3
//! requires the authority's certificate to name `id-kp-timeStamping`, so there
//! a certificate stating no purpose fails ([`Doubt::Timestamping`]) where a
//! document signer's passes. Now and not the token's `genTime`, because that
//! time is the authority's own statement; `docs/PLAN.md` §9 records it.
//!
//! ## Where this runs
//!
//! In the **worker**, inside `docinfo::scan_from`, beside the integrity check.
//! The certificates are attacker-chosen bytes and the worker is where those
//! are parsed. Measured 2026-09-27 by `signature-probe --mode trust`, which
//! repeats the scan in a sandboxed child: the worker's sandbox profile allows
//! the Mach lookups `SecTrust` makes to `trustd`, and the verdicts under it
//! equal the unsandboxed ones. A `trustd` that cannot be reached is
//! [`Standing::Unchecked`], never a refusal (`platform::failure_of`). On Windows,
//! `windows_worker_trust_matches_uncontained_controls` starts a child through
//! the production low-integrity/job launcher and compares real ROOT-store
//! verdicts and synthetic chain controls; the scope is in the threat model's
//! T6.22. What the OS
//! receives is not the document's bytes: each certificate is decoded by
//! `x509-cert` and re-encoded as DER, at most [`MAX_CERTIFICATES`] of them and
//! each under [`MAX_CERTIFICATE_BYTES`].

use der::{Decode, Encode};
use x509_cert::Certificate;

/// The most certificates from one signature handed to the OS.
///
/// A real signature carries its signer and a few issuers above it --- the
/// largest seen here carries four. The set is attacker-chosen, and every
/// member is a candidate the chain builder may try.
pub const MAX_CERTIFICATES: usize = 16;

/// The largest single certificate handed to the OS, in DER bytes.
///
/// A certificate is one to two kilobytes; a large one with a long extension
/// list is under ten. The bound is for what is re-encoded and handed over, not
/// for what was parsed, which `docinfo::MAX_SIG_BLOB` already bounds.
pub const MAX_CERTIFICATE_BYTES: usize = 64 * 1024;

/// What the OS store says about the signer's certificate.
///
/// **A verdict, and only ever shown beside an intact or weak signature.** See
/// the module note for what each answer claims.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Trust {
    /// The answer.
    pub standing: Standing,
    /// Why the answer is not `Trusted`, for `Untrusted` and `Unchecked`.
    ///
    /// `None` for every other standing, and always set for those two ---
    /// `a_standing_that_is_not_trusted_says_why` asserts both halves.
    pub why: Option<Doubt>,
    /// The store that was asked. `None` when none was.
    pub store: Option<Store>,
    /// The moment the chain was judged at, formatted, when it was the time an
    /// intact timestamp from a trusted authority attests
    /// ([`Standing::TrustedAtTimestamp`], or `untrusted` there); empty when it
    /// was judged now. Added 2026-09-28.
    pub attested_at: String,
}

/// The answer, in increasing order of what it establishes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// The store was not asked, or could not answer. [`Trust::why`] says which.
    #[default]
    Unchecked,
    /// The store does not vouch for the certificate. [`Trust::why`] says why.
    Untrusted,
    /// The chain ends at a trusted root, and the signer's certificate does not
    /// come into force until later.
    NotYetValid,
    /// The chain ended at a trusted root at the signer's certificate's own last
    /// moment, and that certificate has expired since.
    Expired,
    /// The chain ends at a root this operating system trusts, now.
    Trusted,
    /// The chain ended at a root this operating system trusts **at the time
    /// an intact timestamp attests**, from an authority it trusts for
    /// timestamping --- whatever has happened to the certificate since. The
    /// moment is the authority's, in [`Trust::attested_at`]. Since 2026-09-28.
    TrustedAtTimestamp,
}

/// Which store answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    /// The macOS keychains' trust settings, through `trustd`.
    Mac,
    /// The Windows certificate stores, through CryptoAPI.
    Windows,
}

/// Why the store does not vouch, or was not asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Doubt {
    /// An issuer between the signer and a root is neither in the signature nor
    /// on this machine, and nothing here looks it up.
    Incomplete,
    /// The chain ends at a root this machine does not trust --- a certificate
    /// its holder made for themselves, or a root only Adobe's list carries.
    Root,
    /// A certificate above the signer's is outside its dates.
    Dates,
    /// The signer's certificate names only purposes a document signature does
    /// not serve.
    Purpose,
    /// A timestamp authority's certificate does not name timestamping among
    /// its purposes, which RFC 3161 §2.3 requires of it --- or names none.
    Timestamping,
    /// The signer's certificate was not in force at the time a trusted
    /// timestamp attests: it had expired, or had not begun. Since 2026-09-28.
    NotInForce,
    /// The operating system refused the chain for another reason.
    Rejected,
    /// The signature's certificates could not be prepared for the store:
    /// too many, too large, or not re-encodable.
    Certificate,
    /// The store could not be asked: an error from the trust API, or a
    /// platform with no store tpdf uses.
    Unavailable,
}

impl Trust {
    /// Nothing concluded, and why.
    #[must_use]
    pub fn unchecked(why: Doubt) -> Self {
        Trust {
            standing: Standing::Unchecked,
            why: Some(why),
            store: None,
            attested_at: String::new(),
        }
    }
}

/// What the certificate is being trusted **for**.
///
/// A basic X.509 policy checks no usage, so the purpose is asked here, and it
/// is asked differently for the two parties a signature can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// Signing documents: a certificate stating no extended key usage
    /// restricts nothing and passes, and one stating some must name a purpose
    /// in `sign_cms::DOCUMENT_PURPOSES`.
    Documents,
    /// Attesting a time. RFC 3161 §2.3: the authority's certificate **must**
    /// carry the extended key usage `id-kp-timeStamping`, so a certificate
    /// stating none fails here where it would pass for a document. It must also
    /// be critical and the only purpose named, as §2.3 says and
    /// `openssl ts -verify` enforces (pyHanko does not).
    Timestamping,
}

/// Which roots the chain may end at.
///
/// Production always passes [`Anchors::System`]. `Only` is the test seam, and
/// the reason it exists is the rule the tests work under: **nothing here may
/// modify the reader's keychain, trust settings or certificate store**. With
/// `Only`, the roots are certificates held in memory for one evaluation ---
/// `SecTrustSetAnchorCertificates` with `SecTrustSetAnchorCertificatesOnly` on
/// macOS, an exclusive-root chain engine over an in-memory store on Windows ---
/// and the system's roots are not consulted at all.
#[derive(Clone, Copy, Debug)]
pub enum Anchors<'a> {
    /// The roots this operating system trusts.
    System,
    /// These roots, DER, and no others.
    Only(&'a [Vec<u8>]),
}

/// What one evaluation found, before it is read as a [`Standing`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evaluation {
    /// The OS accepted the chain.
    pub passed: bool,
    /// Why not, when it did not. Ignored when `passed`.
    pub failure: Doubt,
    /// The chain the OS assembled, signer first, DER.
    pub chain: Vec<Vec<u8>>,
}

/// The signer's certificate's standing, from a signature's CMS blob.
///
/// `blob` is the definite-length `SignedData` the integrity check read; `now`
/// is seconds since the epoch. The signer is the certificate `SignerInfo.sid`
/// names --- the one whose key the integrity check verified under --- and the
/// rest of the set is offered to the chain builder as candidate issuers.
#[must_use]
pub fn of_blob(blob: &[u8], now: u64, anchors: Anchors<'_>) -> Trust {
    of_blob_for(blob, Purpose::Documents, now, anchors)
}

/// [`of_blob`] for a stated purpose: a timestamp token's authority is asked
/// about with [`Purpose::Timestamping`], through the same store and at the
/// same moment --- `now`, not the token's `genTime`, for the reason
/// `docs/PLAN.md` §9 records: `genTime` is the authority's own statement, so
/// judging the authority at it would let the party under question choose the
/// moment it is judged at.
#[must_use]
pub fn of_blob_for(blob: &[u8], purpose: Purpose, now: u64, anchors: Anchors<'_>) -> Trust {
    of_blob_with(blob, &[], purpose, now, anchors)
}

/// [`of_blob_for`] with `extra` certificates --- a document's `/DSS` --- offered
/// to the chain builder beside the signature's own.
#[must_use]
pub fn of_blob_with(
    blob: &[u8],
    extra: &[Vec<u8>],
    purpose: Purpose,
    now: u64,
    anchors: Anchors<'_>,
) -> Trust {
    let Some((leaf, others)) = certificates_with(blob, extra) else {
        return Trust::unchecked(Doubt::Certificate);
    };
    let Ok(parsed) = Certificate::from_der(&leaf) else {
        return Trust::unchecked(Doubt::Certificate);
    };
    judge_for(&parsed, purpose, now, |at| {
        platform::evaluate(&leaf, &others, anchors, at)
    })
}

/// [`of_blob_for`], and the chain the store vouched for: the one it assembled
/// in the evaluation that passed, signer first, DER, the root it ended at
/// last. Empty unless the answer is [`Standing::Trusted`].
///
/// For `longterm`, which fetches revocation data from the addresses the
/// certificates above a timestamp authority's name. Knowing that the store
/// vouches is not enough there: a token's set is outside its signature, and a
/// certificate added to it can pass for an issuer by name and key while being
/// on no chain the store accepted. One evaluation answers both questions, so
/// the chain returned is the one the verdict is about, not a second one built
/// a moment later.
#[must_use]
pub fn of_blob_with_chain(
    blob: &[u8],
    purpose: Purpose,
    now: u64,
    anchors: Anchors<'_>,
) -> (Trust, Vec<Vec<u8>>) {
    let Some((leaf, others)) = certificates_with(blob, &[]) else {
        return (Trust::unchecked(Doubt::Certificate), Vec::new());
    };
    let Ok(parsed) = Certificate::from_der(&leaf) else {
        return (Trust::unchecked(Doubt::Certificate), Vec::new());
    };
    let mut chain = Vec::new();
    let trust = judge_for(&parsed, purpose, now, |at| {
        let evaluation = platform::evaluate(&leaf, &others, anchors, at)?;
        chain.clone_from(&evaluation.chain);
        Ok(evaluation)
    });
    // `Trusted` is only ever the answer to the first evaluation, the one at
    // `now`, so the chain kept is that one's. Anything else --- a chain the
    // store refused, one that passed inside the certificate's dates and not
    // now, one whose certificate does not serve the purpose --- is not a
    // chain vouched for, whatever was assembled.
    if trust.standing != Standing::Trusted {
        chain.clear();
    }
    (trust, chain)
}

/// The signer's certificate and the other members of the set, re-encoded,
/// with `extra` offered as further candidate issuers: the certificates a
/// document's `/DSS` carries, since 2026-09-28.
///
/// `None` when there is no certificate `SignerInfo.sid` identifies, or when the
/// signature's own set is larger or any member bigger than this hands to the
/// OS. Held to the
/// same bounds as the signature's own set, separately --- at most
/// [`MAX_CERTIFICATES`] of them, each under [`MAX_CERTIFICATE_BYTES`] --- and
/// one over its size is left out rather than refusing the lot: an extra
/// candidate a chain needed reads as a missing link, which is what it is.
#[cfg(test)]
fn certificates(blob: &[u8]) -> Option<(Vec<u8>, Vec<Vec<u8>>)> {
    certificates_with(blob, &[])
}

fn certificates_with(blob: &[u8], extra: &[Vec<u8>]) -> Option<(Vec<u8>, Vec<Vec<u8>>)> {
    use cms::content_info::ContentInfo;
    use cms::signed_data::SignedData;

    let info = ContentInfo::from_der(blob).ok()?;
    let signed: SignedData = info.content.decode_as().ok()?;
    let (signer, matched) = crate::certificate::signer_certificate(&signed)?;
    if !matched {
        return None;
    }
    let all = crate::certificate::certificates_of(&signed);
    if all.len() > MAX_CERTIFICATES {
        return None;
    }
    let encode = |certificate: &Certificate| {
        certificate
            .to_der()
            .ok()
            .filter(|der| der.len() <= MAX_CERTIFICATE_BYTES)
    };
    let leaf = encode(signer)?;
    let mut others = Vec::new();
    for certificate in all {
        let der = encode(certificate)?;
        if der != leaf {
            others.push(der);
        }
    }
    let mut added = 0;
    for der in extra {
        if added == MAX_CERTIFICATES {
            break;
        }
        if der.len() <= MAX_CERTIFICATE_BYTES && *der != leaf && !others.contains(der) {
            others.push(der.clone());
            added += 1;
        }
    }
    Some((leaf, others))
}

/// Reads evaluations as a [`Standing`]; `evaluate` runs one at a given time.
///
/// Split from the platform call so the order of the questions --- the chain
/// now, then the chain inside the signer's dates, then the purpose --- is one
/// pure function whatever the platform, and the tests hold that order rather
/// than one operating system's error codes.
pub fn judge(
    signer: &Certificate,
    now: u64,
    evaluate: impl FnMut(u64) -> Result<Evaluation, String>,
) -> Trust {
    judge_for(signer, Purpose::Documents, now, evaluate)
}

/// [`judge`] for a stated purpose. The order of the questions is the same;
/// only the purpose asked last differs, and so does the doubt it answers with.
pub fn judge_for(
    signer: &Certificate,
    purpose: Purpose,
    now: u64,
    mut evaluate: impl FnMut(u64) -> Result<Evaluation, String>,
) -> Trust {
    let store = platform::STORE;
    let answer = |standing: Standing, why: Option<Doubt>| Trust {
        standing,
        why,
        store,
        attested_at: String::new(),
    };
    let Some(serves) = serves(signer, purpose) else {
        return Trust::unchecked(Doubt::Certificate);
    };

    let first = match evaluate(now) {
        Ok(evaluation) => evaluation,
        Err(_) => return Trust::unchecked(Doubt::Unavailable),
    };
    let validity = &signer.tbs_certificate.validity;
    let from = validity.not_before.to_unix_duration().as_secs();
    let until = validity.not_after.to_unix_duration().as_secs();

    let (standing, evaluation) = if first.passed {
        (Standing::Trusted, first)
    } else if now > until || now < from {
        // Asked again inside the signer's own dates: at its last moment if it
        // has expired, at its first if it has not begun. Only a chain that
        // passes there makes the dates the news.
        let at = now.clamp(from, until);
        let second = match evaluate(at) {
            Ok(evaluation) => evaluation,
            Err(_) => return Trust::unchecked(Doubt::Unavailable),
        };
        let standing = if now > until {
            Standing::Expired
        } else {
            Standing::NotYetValid
        };
        (standing, second)
    } else {
        (Standing::Untrusted, first)
    };

    if !evaluation.passed {
        return answer(Standing::Untrusted, Some(evaluation.failure));
    }
    if !serves {
        let doubt = match purpose {
            Purpose::Documents => Doubt::Purpose,
            Purpose::Timestamping => Doubt::Timestamping,
        };
        return answer(Standing::Untrusted, Some(doubt));
    }
    answer(standing, None)
}

/// The signer's standing **at `at`**, the time an intact timestamp from an
/// authority this computer trusts attests --- Phase 6's long-term validation,
/// since 2026-09-28.
///
/// Asked instead of the present moment, not as well: the point of an attested
/// time is that a certificate which has run out since, or been revoked since,
/// did not stop the signature being sound when it was made. The caller
/// decides when the time qualifies (`docinfo`: token `intact`, authority
/// `trusted`, not revoked, and `genTime` inside the authority's certificate's
/// dates); this only asks the store about that moment.
#[must_use]
pub fn of_blob_at(
    blob: &[u8],
    extra: &[Vec<u8>],
    purpose: Purpose,
    at: u64,
    anchors: Anchors<'_>,
) -> Trust {
    let Some((leaf, others)) = certificates_with(blob, extra) else {
        return Trust::unchecked(Doubt::Certificate);
    };
    let Ok(parsed) = Certificate::from_der(&leaf) else {
        return Trust::unchecked(Doubt::Certificate);
    };
    judge_at(&parsed, purpose, at, |moment| {
        platform::evaluate(&leaf, &others, anchors, moment)
    })
}

/// [`judge_for`] at an attested moment: one evaluation, at `at`, with the
/// signer's own dates asked first --- a certificate not in force when the
/// signature provably existed is [`Doubt::NotInForce`], whatever the chain.
pub fn judge_at(
    signer: &Certificate,
    purpose: Purpose,
    at: u64,
    mut evaluate: impl FnMut(u64) -> Result<Evaluation, String>,
) -> Trust {
    let answer = |standing: Standing, why: Option<Doubt>| Trust {
        standing,
        why,
        store: platform::STORE,
        attested_at: crate::certificate::format_time(at),
    };
    let Some(serves) = serves(signer, purpose) else {
        return Trust::unchecked(Doubt::Certificate);
    };
    let validity = &signer.tbs_certificate.validity;
    let from = validity.not_before.to_unix_duration().as_secs();
    let until = validity.not_after.to_unix_duration().as_secs();
    if at < from || at > until {
        return answer(Standing::Untrusted, Some(Doubt::NotInForce));
    }
    let evaluation = match evaluate(at) {
        Ok(evaluation) => evaluation,
        Err(_) => return Trust::unchecked(Doubt::Unavailable),
    };
    if !evaluation.passed {
        return answer(Standing::Untrusted, Some(evaluation.failure));
    }
    if !serves {
        let doubt = match purpose {
            Purpose::Documents => Doubt::Purpose,
            Purpose::Timestamping => Doubt::Timestamping,
        };
        return answer(Standing::Untrusted, Some(doubt));
    }
    answer(Standing::TrustedAtTimestamp, None)
}

/// Whether the certificate's extended key usage admits `purpose`.
///
/// For documents, `Some(true)` for a certificate stating none, which restricts
/// nothing, and for one naming a purpose in `sign_cms::DOCUMENT_PURPOSES`. For
/// timestamping, `Some(true)` only for an extension marked critical that names
/// `id-kp-timeStamping` and nothing else: RFC 3161 §2.3 says the authority's
/// certificate "MUST" carry exactly that, critical, and OpenSSL enforces it.
/// Stating none is `Some(false)`, because there the extension is a
/// requirement rather than a restriction. Every live authority measured on
/// 2026-09-28 (DigiCert, Sectigo, GlobalSign) meets the strict form, so it
/// refuses nothing real. `None` either way for an extension that is
/// present and will not decode: a usage nobody can read is neither a
/// restriction nor its absence.
fn serves(certificate: &Certificate, purpose: Purpose) -> Option<bool> {
    let extensions = certificate
        .tbs_certificate
        .extensions
        .as_deref()
        .unwrap_or(&[]);
    let Some(extension) = extensions
        .iter()
        .find(|extension| extension.extn_id.to_string() == "2.5.29.37")
    else {
        return Some(purpose == Purpose::Documents);
    };
    let usage =
        x509_cert::ext::pkix::ExtendedKeyUsage::from_der(extension.extn_value.as_bytes()).ok()?;
    Some(match purpose {
        Purpose::Documents => usage
            .0
            .iter()
            .any(|named| crate::sign_cms::DOCUMENT_PURPOSES.contains(&named.to_string().as_str())),
        Purpose::Timestamping => {
            extension.critical
                && matches!(usage.0.as_slice(), [only] if only.to_string() == TIMESTAMPING)
        }
    })
}

/// id-kp-timeStamping, RFC 5280 §4.2.1.12.
pub const TIMESTAMPING: &str = "1.3.6.1.5.5.7.3.8";

/// Whether a certificate's subject and issuer are the same name.
fn self_issued(der: &[u8]) -> bool {
    Certificate::from_der(der).is_ok_and(|certificate| {
        let tbs = &certificate.tbs_certificate;
        tbs.subject.to_der().ok() == tbs.issuer.to_der().ok()
    })
}

/// A failed chain whose last certificate is not self-issued stopped short of
/// any root: its issuer was nowhere to be found. That is a different sentence
/// from "ends at a root nobody trusts", and one both platforms can tell apart
/// by the chain's shape rather than by their own error vocabularies.
fn incomplete(chain: &[Vec<u8>]) -> bool {
    chain.last().is_some_and(|last| !self_issued(last))
}

#[cfg(target_os = "macos")]
pub(crate) mod platform {
    use super::{incomplete, Anchors, Doubt, Evaluation, Store};
    use core_foundation::date::CFDate;
    use security_framework::certificate::SecCertificate;
    use security_framework::policy::SecPolicy;
    use security_framework::trust::SecTrust;

    pub const STORE: Option<Store> = Some(Store::Mac);

    /// `errSecCertificateExpired` and `errSecCertificateNotValidYet`.
    const EXPIRED: isize = -67_818;
    const NOT_YET: isize = -67_819;
    /// `errSecNotTrusted`: the chain ends at a root no anchor names.
    const NOT_TRUSTED: isize = -67_843;
    /// The codes that say the evaluation itself did not happen:
    /// `errSecInternalError`, `errSecInternalComponent`, `errSecNotAvailable`
    /// and `errSecServiceNotAvailable`. The first is what `SecTrustEvaluateWithError`
    /// returned, measured 2026-09-27, when a sandbox denied the Mach lookup to
    /// `trustd` --- and the first version of this module read it as a chain
    /// the OS had refused, which is tpdf's failure told as the document's.
    const UNREACHED: [isize; 4] = [-26_276, -2_070, -25_291, -67_585];

    /// Seconds between the Unix epoch and Core Foundation's, 2001-01-01.
    const CF_EPOCH: u64 = 978_307_200;

    /// A `SecTrust` over `certificates`, basic X.509 policy, no network.
    ///
    /// **Shared with `keystore::platform::chain_of`**, which assembles the chain
    /// placed in a signature tpdf makes: one construction, so the chain a
    /// signature carries and the chain its verdict is read against are built
    /// the same way.
    pub fn trust_over(certificates: &[SecCertificate]) -> Option<SecTrust> {
        let mut trust =
            SecTrust::create_with_certificates(certificates, &[SecPolicy::create_x509()]).ok()?;
        trust.set_network_fetch_allowed(false).ok()?;
        Some(trust)
    }

    /// The chain `trust` assembled when it was last evaluated, signer first.
    #[allow(deprecated)] // `chain()` is macOS 12; the bundle targets 10.13.
    pub fn chain_of(trust: &SecTrust) -> Vec<Vec<u8>> {
        (0..trust.certificate_count())
            .filter_map(|at| trust.certificate_at_index(at))
            .map(|certificate| certificate.to_der())
            .collect()
    }

    /// Why `SecTrustEvaluateWithError` refused, from its code and the chain.
    ///
    /// `Err` when the code says the evaluation did not happen at all ---
    /// [`UNREACHED`] --- which is not a chain the OS refused: the answer is
    /// then tpdf's failure, and `judge` reads it as unchecked.
    pub fn failure_of(code: isize, chain: &[Vec<u8>]) -> Result<Doubt, String> {
        if UNREACHED.contains(&code) {
            return Err(format!("the trust service did not answer ({code})"));
        }
        Ok(if incomplete(chain) {
            Doubt::Incomplete
        } else {
            match code {
                EXPIRED | NOT_YET => Doubt::Dates,
                NOT_TRUSTED => Doubt::Root,
                _ => Doubt::Rejected,
            }
        })
    }

    pub fn evaluate(
        leaf: &[u8],
        others: &[Vec<u8>],
        anchors: Anchors<'_>,
        at: u64,
    ) -> Result<Evaluation, String> {
        let parse = |der: &Vec<u8>| {
            SecCertificate::from_der(der)
                .map_err(|e| format!("the OS would not read a certificate: {e}"))
        };
        let mut certificates = vec![parse(&leaf.to_vec())?];
        for other in others {
            // One the OS will not read is left out rather than refusing the
            // lot: it is a candidate issuer, and a chain that needed it reads
            // as incomplete, which is what it is.
            if let Ok(certificate) = parse(other) {
                certificates.push(certificate);
            }
        }
        let mut trust = trust_over(&certificates).ok_or("SecTrust could not be created")?;
        if let Anchors::Only(roots) = anchors {
            let roots = roots.iter().map(parse).collect::<Result<Vec<_>, _>>()?;
            trust
                .set_anchor_certificates(&roots)
                .map_err(|e| e.to_string())?;
            trust
                .set_trust_anchor_certificates_only(true)
                .map_err(|e| e.to_string())?;
        }
        let seconds = i64::try_from(at).unwrap_or(i64::MAX) - CF_EPOCH as i64;
        #[allow(clippy::cast_precision_loss)] // seconds since 2001 fit an f64 exactly
        let date = CFDate::new(seconds as f64);
        trust
            .set_trust_verify_date(&date)
            .map_err(|e| e.to_string())?;
        let outcome = trust.evaluate_with_error();
        let chain = chain_of(&trust);
        let failure = match &outcome {
            Ok(()) => Doubt::Rejected,
            Err(error) => failure_of(error.code(), &chain)?,
        };
        Ok(Evaluation {
            passed: outcome.is_ok(),
            failure,
            chain,
        })
    }
}

#[cfg(windows)]
pub(crate) mod platform {
    use super::{incomplete, Anchors, Doubt, Evaluation, Store};
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Security::Cryptography::{
        CertAddEncodedCertificateToStore, CertCloseStore, CertCreateCertificateChainEngine,
        CertCreateCertificateContext, CertFreeCertificateChain, CertFreeCertificateChainEngine,
        CertFreeCertificateContext, CertGetCertificateChain, CertOpenStore,
        CertVerifyCertificateChainPolicy, CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL, CERT_CHAIN_CONTEXT,
        CERT_CHAIN_DISABLE_AIA, CERT_CHAIN_DISABLE_AUTH_ROOT_AUTO_UPDATE, CERT_CHAIN_ENGINE_CONFIG,
        CERT_CHAIN_PARA, CERT_CHAIN_POLICY_BASE, CERT_CHAIN_POLICY_PARA, CERT_CHAIN_POLICY_STATUS,
        CERT_CONTEXT, CERT_STORE_ADD_ALWAYS, CERT_STORE_PROV_MEMORY, CERT_TRUST_IS_NOT_TIME_NESTED,
        CERT_TRUST_IS_NOT_TIME_VALID, CERT_TRUST_IS_UNTRUSTED_ROOT, HCERTCHAINENGINE, HCERTSTORE,
        PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
    };

    pub const STORE: Option<Store> = Some(Store::Windows);

    /// Cache only, no AIA, no root auto-update: nothing here goes online.
    ///
    /// **Shared with `keystore::platform::chain_of`**, so the chain a signature
    /// tpdf makes carries and the chain its verdict is read against are built
    /// under the same rules.
    pub const OFFLINE: u32 = CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL
        | CERT_CHAIN_DISABLE_AIA
        | CERT_CHAIN_DISABLE_AUTH_ROOT_AUTO_UPDATE;

    const ENCODING: u32 = X509_ASN_ENCODING | PKCS_7_ASN_ENCODING;

    /// A certificate's DER, from a context crypt32 handed back.
    pub fn encoded(context: *const CERT_CONTEXT) -> Vec<u8> {
        // A context crypt32 handed back is valid for as long as it is held,
        // and `pbCertEncoded` is `cbCertEncoded` bytes of it.
        unsafe {
            std::slice::from_raw_parts((*context).pbCertEncoded, (*context).cbCertEncoded as usize)
                .to_vec()
        }
    }

    /// The first simple chain's certificates, end certificate first.
    ///
    /// # Safety
    ///
    /// `chain` is a live context `CertGetCertificateChain` returned.
    pub unsafe fn elements(chain: *const CERT_CHAIN_CONTEXT) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        unsafe {
            if (*chain).cChain > 0 {
                let simple = *(*chain).rgpChain;
                for at in 0..(*simple).cElement as usize {
                    let element = *(*simple).rgpElement.add(at);
                    out.push(encoded((*element).pCertContext));
                }
            }
        }
        out
    }

    /// An in-memory store holding `certificates`, which the caller closes.
    fn memory(certificates: &[Vec<u8>]) -> Result<HCERTSTORE, String> {
        let store = unsafe { CertOpenStore(CERT_STORE_PROV_MEMORY, 0, 0, 0, std::ptr::null()) };
        if store.is_null() {
            return Err(format!(
                "an in-memory store could not be made: {}",
                std::io::Error::last_os_error()
            ));
        }
        for der in certificates {
            let Ok(len) = u32::try_from(der.len()) else {
                continue;
            };
            // One crypt32 will not read is left out: it is a candidate
            // issuer, and a chain that needed it reads as incomplete.
            unsafe {
                CertAddEncodedCertificateToStore(
                    store,
                    ENCODING,
                    der.as_ptr(),
                    len,
                    CERT_STORE_ADD_ALWAYS,
                    std::ptr::null_mut(),
                );
            }
        }
        Ok(store)
    }

    /// Seconds since the Unix epoch as a `FILETIME`: 100 ns since 1601.
    fn filetime(seconds: u64) -> FILETIME {
        let ticks = seconds
            .saturating_add(11_644_473_600)
            .saturating_mul(10_000_000);
        FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        }
    }

    pub fn evaluate(
        leaf: &[u8],
        others: &[Vec<u8>],
        anchors: Anchors<'_>,
        at: u64,
    ) -> Result<Evaluation, String> {
        let len = u32::try_from(leaf.len()).map_err(|_| "certificate too large")?;
        let context = unsafe { CertCreateCertificateContext(ENCODING, leaf.as_ptr(), len) };
        if context.is_null() {
            return Err("crypt32 would not read the signer's certificate".into());
        }
        let additional = match memory(others) {
            Ok(store) => store,
            Err(e) => {
                unsafe { CertFreeCertificateContext(context) };
                return Err(e);
            }
        };
        // The test seam: an engine whose only roots are `roots`, held in a
        // store that exists in this process and nowhere else.
        let mut roots_store: HCERTSTORE = std::ptr::null_mut();
        let mut engine: HCERTCHAINENGINE = std::ptr::null_mut();
        let mut result = Ok(());
        if let Anchors::Only(roots) = anchors {
            match memory(roots) {
                Ok(store) => {
                    roots_store = store;
                    let config = CERT_CHAIN_ENGINE_CONFIG {
                        cbSize: std::mem::size_of::<CERT_CHAIN_ENGINE_CONFIG>() as u32,
                        hExclusiveRoot: roots_store,
                        ..Default::default()
                    };
                    if unsafe { CertCreateCertificateChainEngine(&config, &mut engine) } == 0 {
                        result = Err(format!(
                            "an exclusive-root chain engine could not be made: {}",
                            std::io::Error::last_os_error()
                        ));
                    }
                }
                Err(e) => result = Err(e),
            }
        }
        let evaluation = result.and_then(|()| {
            let para = CERT_CHAIN_PARA {
                cbSize: std::mem::size_of::<CERT_CHAIN_PARA>() as u32,
                ..Default::default()
            };
            let time = filetime(at);
            let mut chain: *mut CERT_CHAIN_CONTEXT = std::ptr::null_mut();
            let built = unsafe {
                CertGetCertificateChain(
                    engine,
                    context,
                    &time,
                    additional,
                    &para,
                    OFFLINE,
                    std::ptr::null(),
                    &mut chain,
                )
            } != 0;
            if !built || chain.is_null() {
                return Err(format!(
                    "no chain could be built: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let policy = CERT_CHAIN_POLICY_PARA {
                cbSize: std::mem::size_of::<CERT_CHAIN_POLICY_PARA>() as u32,
                ..Default::default()
            };
            let mut status = CERT_CHAIN_POLICY_STATUS {
                cbSize: std::mem::size_of::<CERT_CHAIN_POLICY_STATUS>() as u32,
                ..Default::default()
            };
            let asked = unsafe {
                CertVerifyCertificateChainPolicy(
                    CERT_CHAIN_POLICY_BASE,
                    chain,
                    &policy,
                    &mut status,
                )
            } != 0;
            let errors = unsafe { (*chain).TrustStatus.dwErrorStatus };
            let certificates = unsafe { elements(chain) };
            unsafe { CertFreeCertificateChain(chain) };
            if !asked {
                return Err("the base chain policy could not be asked".into());
            }
            let failure = if incomplete(&certificates) {
                Doubt::Incomplete
            } else if errors & (CERT_TRUST_IS_NOT_TIME_VALID | CERT_TRUST_IS_NOT_TIME_NESTED) != 0 {
                Doubt::Dates
            } else if errors & CERT_TRUST_IS_UNTRUSTED_ROOT != 0 {
                Doubt::Root
            } else {
                Doubt::Rejected
            };
            Ok(Evaluation {
                passed: status.dwError == 0,
                failure,
                chain: certificates,
            })
        });
        unsafe {
            if !engine.is_null() {
                CertFreeCertificateChainEngine(engine);
            }
            if !roots_store.is_null() {
                CertCloseStore(roots_store, 0);
            }
            CertCloseStore(additional, 0);
            CertFreeCertificateContext(context);
        }
        evaluation
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) mod platform {
    use super::{Anchors, Evaluation, Store};

    pub const STORE: Option<Store> = None;

    pub fn evaluate(_: &[u8], _: &[Vec<u8>], _: Anchors<'_>, _: u64) -> Result<Evaluation, String> {
        Err("no trust store on this platform".into())
    }
}

#[cfg(test)]
mod tests;
