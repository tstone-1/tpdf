//! Adding long-term validation data, and an archive timestamp over it, to a
//! document that is **already signed**.
//!
//! ## What it does
//!
//! A document somebody signed --- with tpdf or anything else --- stays
//! checkable only while its certificates are in force and their authorities
//! still answer. This takes the file as it is, asks the certificate
//! authorities about every certificate on every signature's chain and every
//! timestamp's, has a worker append the answers as a `/DSS` revision, and
//! then has the timestamp authority the reader chose stamp the whole: PAdES
//! B-LTA, reached after the fact. Run again later on its own result, it adds
//! the data for the previous archive timestamp's authority and a further
//! archive timestamp, which is what keeps a document checkable past that
//! authority's own certificate --- **while every certificate involved is
//! still valid**: the gate below vouches for nothing out of its dates, so
//! the further archive has to be added before the earlier authority's
//! certificate expires, and a run after that day is refused.
//!
//! **Every step is the signing path's**: [`super::gather`] fetches and
//! judges, `sign_dss::append` writes the `/DSS`, [`super::archived`] the
//! document timestamp, the worker reads each result with `docinfo::scan`,
//! and the walk that decides what to ask about is [`super::walked`]. What is
//! this module's own is where the certificates come from, and what that
//! changes.
//!
//! ## What is different when somebody else made the signature
//!
//! While signing, the signer's certificate comes from the reader's own key
//! store and only the timestamp token comes from elsewhere. Here **everything
//! comes from the document**, which is anybody's:
//!
//! - **The values cross the boundary, and nothing else of the document
//!   does.** A worker parses the file and answers each signature's CMS as
//!   the DER it is (`sign_dss::survey`); this process, which holds the
//!   network, reads those with the readers and the bounds it already reads a
//!   token from the network with. It still never parses a PDF.
//! - **Nothing is fetched for a signer this computer does not trust.** The
//!   addresses asked are the ones certificates name, and a document can
//!   carry a certificate naming any address. So the rule [`super::vouched`]
//!   holds a timestamp authority to is held to **every** leaf here: its chain
//!   must end at a root the operating system trusts for that purpose, now
//!   and offline, and only certificates on the chain the store assembled may
//!   stand above it. A self-signed signature, or one whose authority this
//!   computer does not know, is refused before any request is made.
//! - **All of the document's signatures, or none.** The archive timestamp
//!   covers the whole file, and a reader takes a document with one as kept
//!   for the long term. If one signature cannot be covered --- it does not
//!   verify, its signer is not trusted, its authority publishes nothing, a
//!   responder does not answer --- nothing is written, and the refusal names
//!   the signature and the reason.
//! - **Judged now.** The data is fetched now and each answer is judged at the
//!   present, as while signing: a certificate its authority says is revoked
//!   is refused, also where the signature's timestamp shows it was made
//!   before the revocation. Data that says *revoked after* is data a reader
//!   has to weigh, and tpdf adds only data that reads `good`.
//! - **No earlier byte is written.** Both revisions are appended, and before
//!   anything is written the worker's reading of the result is held against
//!   its reading of the original ([`covered`]): every signature still intact,
//!   with no page touched after it that was not before, and every
//!   certificate asked about reading `good` from the document's own data.
//!   The same is asked again of the written file ([`read_back`]).

use der::{Decode as _, Encode as _};
use x509_cert::Certificate;

use super::{
    archived, chains_good, gather, leaves_good, signed_data, signer_of, token_of, unvouched,
    walked, Archive, Fetch, Leg, Subject, Vouched, Whose, MAX_SUBJECTS, TOTAL,
};
use crate::docinfo::Signature;
use crate::integrity::{Verdict, Why};
use crate::trust::{Purpose, Standing};

/// `/SubFilter` of a document timestamp, whose value is the token itself.
const DOCUMENT_TIMESTAMP: &str = "ETSI.RFC3161";

/// Who says whether a certificate's chain is trusted for a purpose, and by
/// which chain, given the CMS that names it as its signer and the present:
/// [`super::vouched_for`] over the system's store in the application and the
/// tool, the same rule over a test's own roots in a test.
pub type VouchFor<'a> = dyn Fn(&[u8], Purpose, u64) -> Vouched + 'a;

/// Why no long-term validation data was added. Nothing is written for any
/// of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The worker that reads the document died, did not answer, or answered
    /// something else: tpdf's own failure.
    Failed(String),
    /// The document cannot take the data at all --- encrypted, or not
    /// parseable as a file a revision can be appended to: the worker's
    /// sentence.
    Document(String),
    /// The document has no signature.
    Unsigned,
    /// Not every signature field, or not every signature's value, could be
    /// read.
    Incomplete,
    /// The document is certified with no changes permitted: by the signature
    /// in this field, or --- for `None` --- by a certification that is no
    /// field of the form.
    Certified(Option<String>),
    /// A signature, or its timestamp, does not verify: the field, and what
    /// it reads as.
    NotIntact {
        /// The field.
        field: String,
        /// Whether it is the signature's timestamp that does not, rather
        /// than the signature.
        timestamp: bool,
        /// How it reads, in the words the properties dialog's list uses.
        reads: String,
    },
    /// The signatures together cover more than tpdf hashes for one document,
    /// so the result could not be checked.
    Budget,
    /// A signer this computer does not trust, so nothing is fetched for it.
    Untrusted {
        /// The field.
        field: String,
        /// The signer, by its common name.
        name: String,
        /// Why not, as a clause.
        why: String,
    },
    /// The document's signatures name more certificates than one run asks
    /// about.
    TooMany,
    /// The document's `/DSS` holds as many entries as tpdf's reader takes, or
    /// more: what would be added could not be read back.
    Full,
    /// What the steps shared with signing refused: about one signature, when
    /// `field` names it, or about the whole.
    About {
        /// The signature it is about, when it is about one.
        field: Option<String>,
        /// The refusal.
        why: super::Refusal,
    },
    /// With the data added, a signature the document already held does not
    /// read as it did: tpdf's own check before writing, or of the written
    /// file.
    Changed {
        /// The field.
        field: String,
        /// What it reads as, as a clause after the field's name.
        what: String,
    },
    /// The archive timestamp is not in the result as it must be.
    Archive(String),
}

impl Refusal {
    /// The sentence the reader is told. No full stop: the caller adds that
    /// nothing was written.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Refusal::Failed(why) => {
                format!("tpdf could not read this document's signatures: {why}")
            }
            Refusal::Document(why) => why.clone(),
            Refusal::Unsigned => {
                "this document has no signature, so there is nothing to add long-term validation \
                 data for"
                    .into()
            }
            Refusal::Incomplete => {
                "tpdf could not read every signature in this document, so it cannot say that the \
                 data would cover them all"
                    .into()
            }
            Refusal::Certified(field) => format!(
                "{} with no changes permitted, and tpdf appends nothing to such a document: \
                 how readers judge validation data and a timestamp added after that \
                 certification has not been measured",
                match field {
                    Some(field) => format!("the signature {field} certifies this document"),
                    None => "this document is certified".to_string(),
                }
            ),
            Refusal::NotIntact {
                field,
                timestamp,
                reads,
            } => format!(
                "{} {field} does not verify (it reads {reads}), and long-term validation data \
                 cannot make it verify",
                if *timestamp {
                    "the timestamp of the signature"
                } else {
                    "the signature"
                }
            ),
            Refusal::Budget => {
                "this document's signatures together cover more data than tpdf checks at once, \
                 so it could not check the result before writing it"
                    .into()
            }
            Refusal::Untrusted { field, name, why } => format!(
                "the signer of {field} ({name}) is not trusted by this computer, so tpdf will \
                 not fetch revocation data for it: {why}"
            ),
            Refusal::TooMany => format!(
                "this document's signatures name more than {MAX_SUBJECTS} certificates to ask \
                 about, more than tpdf asks about in one run"
            ),
            Refusal::Full => {
                "this document already carries as much validation data as tpdf reads, so \
                 tpdf could not read back what it would add"
                    .into()
            }
            Refusal::About { field, why } => match field {
                Some(field) => format!("for the signature {field}: {}", said(why)),
                None => said(why),
            },
            Refusal::Changed { field, what } => format!(
                "tpdf's own check of the document with the data added did not pass: the \
                 signature {field} {what}"
            ),
            Refusal::Archive(why) => {
                format!("tpdf's own check of the document with the data added did not pass: {why}")
            }
        }
    }

    /// Whether this refusal is tpdf's own failure rather than the document's,
    /// an authority's or a certificate authority's: a worker that died, or a
    /// result its own check does not pass. The command line exits 4 for
    /// these and 3 for every other.
    #[must_use]
    pub fn tpdf_failed(&self) -> bool {
        match self {
            Refusal::Failed(_) | Refusal::Changed { .. } | Refusal::Archive(_) => true,
            Refusal::About { why, .. } => why.tpdf_failed(),
            _ => false,
        }
    }
}

/// A refusal of the shared steps, as it is said of a signature somebody else
/// made: [`super::Refusal::sentence`], but for the three whose words are
/// about a signing --- a signature *just made*, a certificate tpdf will not
/// *sign with*, and the advice that a timestamp alone would work.
fn said(why: &super::Refusal) -> String {
    match why {
        super::Refusal::Unreadable(why) => {
            format!("its value could not be read to find its certificates: {why}")
        }
        super::Refusal::NotPublished(name) => format!(
            "{name} does not say where its revocation data is published (an OCSP responder or \
             a revocation list over http or https), so there is none to add for it"
        ),
        super::Refusal::Revoked { name, at, by } => format!(
            "{name} has been revoked{} according to {by}, and tpdf adds long-term validation \
             data only for certificates that are not revoked",
            if at.is_empty() {
                String::new()
            } else {
                format!(" since {at}")
            }
        ),
        other => other.sentence(),
    }
}

/// The document with its validation data and archive timestamp appended, not
/// yet written, and what it held before.
#[derive(Clone, Debug, PartialEq)]
pub struct Added {
    /// The original bytes with both revisions after them.
    pub bytes: Vec<u8>,
    /// Every signature field of the original, as a worker read it before
    /// anything was added: what [`read_back`] holds the written file to.
    pub before: Vec<Signature>,
}

/// Refuses a survey holding more than a worker's may: the counts and sizes
/// `sign_dss::survey` and the scan hold the document to, asked again here.
///
/// **The worker's word is not taken for its own bounds.** The survey comes
/// from a process that parsed the document, and a document that had taken
/// that process over could answer thousands of values and a `/DSS` of any
/// size; this process then parses each value and asks the OS about it. So
/// the bounds are held where the work is done, before any of it.
///
/// # Errors
///
/// [`Refusal::Failed`]: a worker that answers past its bounds has failed,
/// whatever the document.
fn bounded(survey: &crate::sign_dss::Survey) -> Result<(), Refusal> {
    let over = survey.signatures.len() > crate::docinfo::MAX_SIGNATURES
        || survey.values.len() > crate::docinfo::MAX_SIGNATURES
        || survey
            .values
            .iter()
            .any(|(_, value)| value.len() > crate::docinfo::MAX_SIG_BLOB)
        || survey.store.len() > crate::revocation::MAX_DSS_CERTIFICATES
        || survey
            .store
            .iter()
            .any(|der| der.len() > crate::trust::MAX_CERTIFICATE_BYTES)
        || survey.bytes() > crate::sign_dss::MAX_BYTES;
    if over {
        return Err(Refusal::Failed(
            "the worker's answer holds more signatures or certificates than a worker's may".into(),
        ));
    }
    Ok(())
}

/// Refuses unless tpdf's reader will take everything this run adds beside
/// what the `/DSS` holds: it reads a bounded number of certificates,
/// responses and lists, and past the bound every answer reads as not
/// checked. Asked before anything is fetched, with the most a run can add:
/// the certificates the `/DSS` does not hold yet, and a response or a list
/// for every certificate asked about.
///
/// # Errors
///
/// [`Refusal::Full`].
fn room(
    survey: &crate::sign_dss::Survey,
    subjects: &[Subject],
    carried: &[Vec<u8>],
) -> Result<(), Refusal> {
    let [certificates, responses, lists] = survey.held;
    let new = carried
        .iter()
        .filter(|der| !survey.store.contains(der))
        .count();
    let fits = !survey.store_cut
        && certificates + new <= crate::revocation::MAX_DSS_CERTIFICATES
        && responses + subjects.len() <= crate::revocation::MAX_RESPONSES
        && lists + subjects.len() <= crate::revocation::MAX_LISTS;
    if fits {
        Ok(())
    } else {
        Err(Refusal::Full)
    }
}

/// The signed fields of `survey` beside their values, once the document is
/// one this can be done for.
///
/// # Errors
///
/// The worker's own refusal of the document; no signature; a field or value
/// that could not be read; a certification with no changes permitted; a
/// signature or a signature's timestamp that does not read as intact.
fn held(survey: &crate::sign_dss::Survey) -> Result<Vec<(&Signature, &[u8])>, Refusal> {
    let signed: Vec<&Signature> = survey.signatures.iter().filter(|s| s.signed).collect();
    if signed.is_empty() {
        return Err(Refusal::Unsigned);
    }
    // The values are listed by the walk the scan lists the fields by, so the
    // two agree name for name --- or one of them was cut short.
    let agree = signed.len() == survey.values.len()
        && signed
            .iter()
            .zip(&survey.values)
            .all(|(signature, (name, value))| &signature.field == name && !value.is_empty());
    if !survey.complete || !agree {
        return Err(Refusal::Incomplete);
    }
    // The catalog's own certification, which need not be one of the fields:
    // the level a further signature field is refused by when the document
    // timestamp's revision is built, asked here before anybody is.
    if survey.certified == 1 {
        return Err(Refusal::Certified(None));
    }
    for signature in &signed {
        let field = signature.field.clone();
        if signature.certification == 1 {
            return Err(Refusal::Certified(Some(field)));
        }
        match signature.integrity.as_ref() {
            Some(found) if found.verdict == Verdict::Intact => {}
            Some(found)
                if found.verdict == Verdict::Unchecked && found.why == Some(Why::Budget) =>
            {
                return Err(Refusal::Budget)
            }
            other => {
                return Err(Refusal::NotIntact {
                    field,
                    timestamp: false,
                    reads: crate::words::verdict_briefly(other),
                })
            }
        }
        // A signature's own token. (A document timestamp's is the field's own
        // verdict, read once and answered for above.)
        if let Some(stamp) = signature.timestamp.as_ref() {
            if stamp.integrity.as_ref().map(|i| i.verdict) != Some(Verdict::Intact) {
                return Err(Refusal::NotIntact {
                    field,
                    timestamp: true,
                    reads: crate::words::verdict_briefly(stamp.integrity.as_ref()),
                });
            }
        }
    }
    Ok(signed
        .into_iter()
        .zip(&survey.values)
        .map(|(signature, (_, value))| (signature, value.as_slice()))
        .collect())
}

/// The chain `vouch` vouches for the signer of `blob` by, for `purpose`, or
/// who it is and why not.
fn vouched_chain(
    blob: &[u8],
    purpose: Purpose,
    whose: &str,
    now: u64,
    vouch: &VouchFor<'_>,
) -> Result<Vec<Vec<u8>>, (String, String)> {
    let Vouched { trust, chain } = vouch(blob, purpose, now);
    // Trusted, and nothing less, as [`super::vouched`] says of an authority.
    if trust.standing == Standing::Trusted {
        Ok(chain)
    } else {
        Err(unvouched(&trust, blob, whose))
    }
}

/// What to ask about for one signature already in a document, and every
/// certificate its `/DSS` should carry: [`super::plan`], for a signature
/// whose every certificate came from the document.
///
/// `kind` is the field's `/SubFilter` and `value` its `/Contents`: a CMS, or
/// for a document timestamp the token, whose signer is the authority. `store`
/// is what the document's `/DSS` already carries, which the reader of the
/// result will have as candidates and so the walk has too.
///
/// # Errors
///
/// A value that cannot be read; a signer or an authority `vouch` does not
/// call trusted; and what [`super::walked`] refuses.
fn plan_one(
    field: &str,
    kind: &str,
    value: &[u8],
    store: &[(Vec<u8>, Certificate)],
    now: u64,
    vouch: &VouchFor<'_>,
) -> Result<(Vec<Subject>, Vec<Vec<u8>>), Refusal> {
    let about = |why: super::Refusal| Refusal::About {
        field: Some(field.to_string()),
        why,
    };
    let unreadable = |why: String| about(super::Refusal::Unreadable(why));
    let unidentified = || unreadable("its certificate is not identified".into());
    let encoded = |certificates: Vec<&Certificate>| -> Vec<Vec<u8>> {
        certificates
            .iter()
            .filter_map(|certificate| certificate.to_der().ok())
            .collect()
    };
    let authority_chain = |token: &[u8]| {
        vouched_chain(token, Purpose::Timestamping, "the authority's", now, vouch)
            .map_err(|(name, why)| about(super::Refusal::Untrusted { name, why }))
    };

    let signed = signed_data(value).map_err(unreadable)?;
    let leaf = signer_of(&signed).ok_or_else(unidentified)?;
    let in_value = encoded(crate::docinfo::certificates_of(&signed));

    // The signer's leg, and its token's when it carries one. A document
    // timestamp has one leg: its signer is the authority.
    let document_timestamp = kind == DOCUMENT_TIMESTAMP;
    let (own_chain, token) = if document_timestamp {
        (authority_chain(value)?, None)
    } else {
        let chain = vouched_chain(value, Purpose::Documents, "the signer's", now, vouch).map_err(
            |(name, why)| Refusal::Untrusted {
                field: field.to_string(),
                name,
                why,
            },
        )?;
        let token = match token_of(&signed) {
            None => None,
            Some(der) => {
                let parsed = signed_data(&der).map_err(unreadable)?;
                let authority = signer_of(&parsed).ok_or_else(|| {
                    unreadable("its timestamp's certificate is not identified".into())
                })?;
                let chain = authority_chain(&der)?;
                let set = encoded(crate::docinfo::certificates_of(&parsed));
                Some((authority, chain, set))
            }
        };
        (chain, token)
    };

    // Every candidate the reader of the result will have: the value's own
    // set, its token's, the `/DSS` as it is, and the chains vouched for,
    // which go into the `/DSS`. Which of them may be a link is the legs'.
    //
    // Each once, by a set and not by a search: a value's own set is as long
    // as the document made it. The `/DSS`'s are parsed already.
    let mut known: std::collections::HashSet<&[u8]> = std::collections::HashSet::new();
    let mut candidates: Vec<Certificate> = Vec::new();
    fn parsed(der: &Vec<u8>) -> (&[u8], Option<Certificate>) {
        (der.as_slice(), Certificate::from_der(der).ok())
    }
    for (der, certificate) in in_value
        .iter()
        .chain(token.iter().flat_map(|(_, _, set)| set))
        .map(parsed)
        .chain(
            store
                .iter()
                .map(|(der, certificate)| (der.as_slice(), Some(certificate.clone()))),
        )
        .chain(
            own_chain
                .iter()
                .chain(token.iter().flat_map(|(_, chain, _)| chain))
                .map(parsed),
        )
    {
        if known.insert(der) {
            candidates.extend(certificate);
        }
    }
    let everyone: Vec<&Certificate> = candidates.iter().collect();

    let (own, above) = if document_timestamp {
        (Whose::Authority, Whose::AuthorityIssuer)
    } else {
        (Whose::Signer, Whose::SignerIssuer)
    };
    let mut legs = vec![Leg {
        leaf: &leaf,
        own,
        above,
        admitted: &own_chain,
    }];
    if let Some((authority, chain, _)) = &token {
        legs.push(Leg {
            leaf: authority,
            own: Whose::Authority,
            above: Whose::AuthorityIssuer,
            admitted: chain,
        });
    }
    walked(&legs, &everyone).map_err(about)
}

/// What to ask about for every signature of the document, each certificate
/// once, and every certificate to carry.
///
/// # Errors
///
/// What [`plan_one`] refuses for any signature --- all of them are covered,
/// or none --- and more than [`MAX_SUBJECTS`] certificates together.
fn plan(
    held: &[(&Signature, &[u8])],
    store: &[Vec<u8>],
    now: u64,
    vouch: &VouchFor<'_>,
) -> Result<(Vec<Subject>, Vec<Vec<u8>>), Refusal> {
    plan_within(held, store, now, vouch, MAX_SUBJECTS)
}

/// [`plan`], with `most` the most certificates asked about: split so a test
/// reaches the bound with the chains a test PKI has.
fn plan_within(
    held: &[(&Signature, &[u8])],
    store: &[Vec<u8>],
    now: u64,
    vouch: &VouchFor<'_>,
    most: usize,
) -> Result<(Vec<Subject>, Vec<Vec<u8>>), Refusal> {
    let der = |certificate: &Certificate| certificate.to_der().unwrap_or_default();
    // The `/DSS`'s certificates, parsed once for every signature's walk.
    let store: Vec<(Vec<u8>, Certificate)> = store
        .iter()
        .filter_map(|der| Some((der.clone(), Certificate::from_der(der).ok()?)))
        .collect();
    let mut subjects: Vec<Subject> = Vec::new();
    let mut carried: Vec<Vec<u8>> = Vec::new();
    for (signature, value) in held {
        let (asked, carry) =
            plan_one(&signature.field, &signature.kind, value, &store, now, vouch)?;
        for subject in asked {
            let this = der(&subject.certificate);
            if !subjects.iter().any(|s| der(&s.certificate) == this) {
                subjects.push(subject);
            }
        }
        for certificate in carry {
            if !carried.contains(&certificate) {
                carried.push(certificate);
            }
        }
    }
    if subjects.len() > most {
        return Err(Refusal::TooMany);
    }
    Ok((subjects, carried))
}

/// Pages a signature's appendix rewrote other than to list a signature or
/// timestamp field, and objects it removed: what `verify --strict` judges
/// (`cli::verify::after_last_signature`).
fn disturbed(signature: &Signature) -> (usize, usize) {
    signature.appendix.as_ref().map_or((0, 0), |appendix| {
        (
            appendix
                .pages_touched
                .saturating_sub(appendix.pages_listing.len()),
            appendix.removed,
        )
    })
}

/// Refuses unless every signature the document held reads, in `after` ---
/// the worker's reading with `appended` bytes after the original --- as this
/// must leave it: there, by its name and by where its range ends; intact; no
/// page rewritten and no object removed after it that was not before; and
/// its certificate, its timestamp authority's, and every certificate above
/// either `good` from the document's own data.
///
/// **The rule about revocation is [`super::check`]'s**, through the two
/// functions it is made of, so a signature tpdf made yesterday and one
/// somebody else made are held to the same reading.
///
/// # Errors
///
/// [`Refusal::Changed`] for a signature that is missing, not intact or
/// followed by more than was appended; [`Refusal::Budget`] for one the
/// reading had no hashing budget left for; [`Refusal::About`] with
/// [`super::check`]'s refusal for the revocation answers.
pub fn covered(before: &[Signature], after: &[Signature], appended: u64) -> Result<(), Refusal> {
    let name = |certificate: Option<&crate::docinfo::Certificate>, whose: &str| {
        certificate.map_or_else(
            || whose.to_string(),
            |c| format!("{whose} ({})", c.subject_cn),
        )
    };
    for was in before.iter().filter(|s| s.signed) {
        let field = was.field.clone();
        let changed = |what: &str| Refusal::Changed {
            field: field.clone(),
            what: what.to_string(),
        };
        // By what was written, as `sign_cms::ours` finds a new signature:
        // the same name, and exactly the appended bytes more after it.
        let Some(now) = after.iter().find(|s| {
            s.signed
                && s.field == was.field
                && s.appended_bytes == was.appended_bytes.saturating_add(appended)
        }) else {
            return Err(changed("is not in it where it was"));
        };
        match now.integrity.as_ref() {
            Some(found) if found.verdict == Verdict::Intact => {}
            Some(found)
                if found.verdict == Verdict::Unchecked && found.why == Some(Why::Budget) =>
            {
                return Err(Refusal::Budget)
            }
            _ => return Err(changed("does not read as intact")),
        }
        // Bytes follow it now, so there is an appendix to have read.
        match now.appendix.as_ref() {
            Some(appendix) if !appendix.unread => {}
            _ => return Err(changed("is followed by a revision tpdf could not read")),
        }
        let (pages_were, removed_were) = disturbed(was);
        let (pages, removed) = disturbed(now);
        if pages > pages_were || removed > removed_were {
            return Err(changed("reads as followed by a change to a page"));
        }

        let about = |why: super::Refusal| Refusal::About {
            field: Some(field.clone()),
            why,
        };
        let document_timestamp = now.kind == DOCUMENT_TIMESTAMP;
        let (leaf, above) = if document_timestamp {
            (
                "the timestamp authority's certificate",
                "the timestamp authority's",
            )
        } else {
            ("the signer's certificate", "the signer's")
        };
        let mut leaves = vec![(
            now.revocation.as_ref(),
            name(now.certificate.as_ref(), leaf),
        )];
        let mut chains = vec![(now.revocation_chain.as_ref(), above)];
        // The signature's own token, when it carries one: its authority is
        // asked about too, and its verdict must not have moved. (A document
        // timestamp's is a copy of the field's own answers, so it is held to
        // them twice and to nothing else.)
        if let Some(stamp) = now.timestamp.as_ref() {
            if stamp.integrity.as_ref().map(|i| i.verdict) != Some(Verdict::Intact) {
                return Err(changed("has a timestamp that does not read as intact"));
            }
            leaves.push((
                stamp.revocation.as_ref(),
                name(
                    stamp.authority.as_ref(),
                    "the timestamp authority's certificate",
                ),
            ));
            chains.push((stamp.revocation_chain.as_ref(), "the timestamp authority's"));
        } else if was.timestamp.is_some() {
            return Err(changed("no longer carries its timestamp"));
        }
        leaves_good(&leaves).map_err(about)?;
        chains_good(&chains).map_err(about)?;
    }
    Ok(())
}

/// The archive timestamp this added, among `after`: the document timestamp
/// whose range ends at the file's last byte.
#[must_use]
pub fn archive_of(after: &[Signature]) -> Option<&Signature> {
    after
        .iter()
        .find(|s| s.signed && s.kind == DOCUMENT_TIMESTAMP && s.appended_bytes == 0)
}

/// What the written file reads back as, against what [`add`] built:
/// [`covered`] over every signature the original held, and the archive
/// timestamp intact over the whole file.
///
/// `appended` is how many bytes follow the original in the written file.
///
/// # Errors
///
/// What [`covered`] refuses, and [`Refusal::Archive`] when the archive
/// timestamp is not there, does not cover the whole file or is not intact.
pub fn read_back(before: &[Signature], after: &[Signature], appended: u64) -> Result<(), Refusal> {
    covered(before, after, appended)?;
    let Some(archive) = archive_of(after) else {
        return Err(Refusal::Archive(
            "the archive timestamp is not in it".into(),
        ));
    };
    if !archive.covers_whole_file {
        return Err(Refusal::Archive(
            "the archive timestamp does not cover the whole file".into(),
        ));
    }
    match archive.integrity.as_ref() {
        Some(found) if found.verdict == Verdict::Intact => Ok(()),
        Some(found) if found.verdict == Verdict::Unchecked && found.why == Some(Why::Budget) => {
            Err(Refusal::Budget)
        }
        _ => Err(Refusal::Archive(
            "the archive timestamp does not read as intact".into(),
        )),
    }
}

/// Whether the signatures `held` cover, with one more over `whole` bytes,
/// more than a reading of the result has the budget to hash
/// (`integrity::MAX_HASHED`): the archive timestamp is listed last and would
/// be the one left unchecked.
fn past_budget(held: &[(&Signature, &[u8])], whole: usize) -> bool {
    held.iter()
        .map(|(signature, _)| signature.covered_bytes)
        .fold(whole as u64, u64::saturating_add)
        > crate::integrity::MAX_HASHED
}

/// Long-term validation data for every signature `original` holds, and an
/// archive timestamp over the whole: the document with both revisions
/// appended, when every step passed, and nothing otherwise.
///
/// `original` is the file as it was read. `vouch` says whether a signer or
/// an authority is trusted, and is asked before anything is fetched for it;
/// `fetch` asks the certificate authorities; `archive` asks the timestamp
/// authority the reader chose for a token over the covered pieces.
///
/// # Errors
///
/// Every [`Refusal`].
pub fn add(
    original: &[u8],
    now: u64,
    worker: &dyn crate::save::Verifier,
    vouch: &VouchFor<'_>,
    fetch: &mut Fetch<'_>,
    archive: &mut Archive<'_>,
) -> Result<Added, Refusal> {
    let whole = |why: super::Refusal| Refusal::About { field: None, why };
    let survey = worker.survey(original).map_err(Refusal::Failed)?;
    if let Some(why) = &survey.refused {
        return Err(Refusal::Document(why.clone()));
    }
    bounded(&survey)?;
    let held = held(&survey)?;
    // Before anything is asked of anybody: a result tpdf could not check is
    // one it would not write.
    if past_budget(&held, original.len()) {
        return Err(Refusal::Budget);
    }
    let (subjects, certificates) = plan(&held, &survey.store, now, vouch)?;
    room(&survey, &subjects, &certificates)?;
    let gathered = gather(&subjects, certificates, now, TOTAL, fetch).map_err(whole)?;
    let extended = worker
        .validation(original, &gathered)
        .map_err(|why| whole(super::Refusal::Written(why)))?;
    if extended.built_against != original.len() {
        return Err(whole(super::Refusal::Written(format!(
            "the worker built its revision against {} bytes, and the document is {}",
            extended.built_against,
            original.len()
        ))));
    }
    covered(
        &survey.signatures,
        &extended.signatures,
        extended.update.len() as u64,
    )?;
    let mut with_data = Vec::with_capacity(original.len() + extended.update.len());
    with_data.extend_from_slice(original);
    with_data.extend_from_slice(&extended.update);
    // The archive timestamp, held to what makes it one: an authority this
    // computer trusts for timestamping. A token from any other would be
    // sealed intact and attest nothing to the reader of the file.
    let bytes = archived(with_data, worker, &mut |pieces| {
        let token = archive(pieces)?;
        match vouched_chain(&token, Purpose::Timestamping, "the authority's", now, vouch) {
            Ok(_) => Ok(token),
            Err((name, why)) => Err(format!(
                "the timestamp authority {name} is not trusted by this computer, so its \
                 timestamp would not keep the document checkable here: {why}"
            )),
        }
    })
    .map_err(whole)?;
    if past_budget(&held, bytes.len()) {
        return Err(Refusal::Budget);
    }
    Ok(Added {
        bytes,
        before: survey.signatures,
    })
}

#[cfg(test)]
mod tests;
