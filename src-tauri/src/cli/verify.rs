//! `tpdf verify <file.pdf>... [--strict] [--json]`: every signature's
//! integrity verdict and trust standing, and its timestamp's, in the
//! properties dialog's words.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::args::unknown;
use super::report::{
    self, AppendixReport, ChainCertificate, ChainReport, ErrorKind, FileError, IntegrityReport,
    ListedPage, RevocationReport, TrustReport, SCHEMA,
};
use super::{json, say, words, Env, Exit, Failure, Registered, Subcommand};
use crate::docinfo;
use crate::save_outside::Declined;

/// `verify`, registered.
pub const COMMAND: Registered = Registered {
    name: "verify",
    usage: "verify <file.pdf>... [--strict] [--json]",
    summary: "Says whether each signature is intact and whether this computer\n            trusts its signer, in the words the application's properties\n            dialog uses.",
    parse: boxed,
};

/// `tpdf verify <file.pdf>... [--json] [--strict]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verify {
    /// The documents, in the order given.
    pub files: Vec<PathBuf>,
    /// `--json`.
    pub json: bool,
    /// `--strict`: exit 1 unless every document has a signature, every
    /// signature is intact and trusted, and nothing appended after the last
    /// of them touches a page ([`After`]).
    pub strict: bool,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `verify`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Verify, String> {
    let mut files = Vec::new();
    let mut json = false;
    let mut positional = false;
    let mut strict = false;
    for arg in args {
        match (positional, arg.as_str()) {
            (false, "--") => positional = true,
            (false, "--json") => json = true,
            (false, "--strict") => strict = true,
            (false, flag) if flag.starts_with('-') && flag != "-" => {
                return Err(unknown("verify", flag))
            }
            (after, path) => files.push(super::args::operand(after, path)),
        }
    }
    if files.is_empty() {
        return Err("`verify` needs at least one document".into());
    }
    Ok(Verify {
        files,
        json,
        strict,
    })
}

impl Subcommand for Verify {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        run_verify(env, self, out, err)
    }
}

/// One signature, as the report shows it, from what a worker read.
#[must_use]
pub fn signature_report(signature: &docinfo::Signature) -> report::Signature {
    let integrity = signature.integrity.clone().unwrap_or_default();
    let certificate = signature.certificate.as_ref();
    let (from, until) = certificate.map_or((String::new(), String::new()), |c| {
        (c.from.clone(), c.until.clone())
    });
    let document_timestamp = signature.kind == "ETSI.RFC3161";
    let trust = signature.trust.as_ref().map(|trust| TrustReport {
        standing: trust.standing,
        why: trust.why,
        store: trust.store,
        attested_at: trust.attested_at.clone(),
        // A document timestamp's certificate is its authority's and names no
        // person, so the signer's sentence would be wrong about it twice.
        sentence: if document_timestamp {
            words::authority_sentence(trust, &from, &until)
        } else {
            words::trust_sentence(trust, &from, &until)
        },
    });
    let name = |cn: &str, whole: &str| {
        if cn.is_empty() {
            whole.to_string()
        } else {
            cn.to_string()
        }
    };
    report::Signature {
        document_timestamp,
        field: signature.field.clone(),
        signer: certificate.map_or_else(String::new, |c| name(&c.subject_cn, &c.subject)),
        issuer: certificate.map_or_else(String::new, |c| name(&c.issuer_cn, &c.issuer)),
        claimed_time: signature.when.clone(),
        covers_whole_file: signature.covers_whole_file,
        appended_bytes: signature.appended_bytes,
        appendix: signature.appendix.as_ref().map(appendix_report),
        integrity: IntegrityReport {
            verdict: integrity.verdict,
            why: integrity.why,
            digest: integrity.digest.clone(),
            method: integrity.method.clone(),
            sentence: words::integrity_sentence(
                &integrity,
                signature.appended_bytes,
                trust.is_some(),
            ),
        },
        trust,
        timestamp: signature
            .timestamp
            .as_ref()
            .map(|stamp| timestamp_report(stamp, document_timestamp)),
        // A document timestamp's signer is its authority, so its revocation
        // is the authority's and is worded so.
        revocation: signature
            .revocation
            .as_ref()
            .map(|r| revocation_report(r, document_timestamp)),
        revocation_chain: signature
            .revocation_chain
            .as_ref()
            .map(|c| chain_report(c, document_timestamp)),
        pades_level: signature.pades,
        pades: signature.pades.map(words::pades_sentence),
    }
}

/// What was appended after one signature, as the report shows it: every
/// count and name the worker read, and the dialog's sentence over them.
#[must_use]
pub fn appendix_report(appendix: &docinfo::Appendix) -> AppendixReport {
    AppendixReport {
        unread: appendix.unread,
        added: appendix.added,
        replaced: appendix.replaced,
        removed: appendix.removed,
        kinds: appendix.kinds.clone(),
        catalog_gained: appendix.catalog_gained.clone(),
        pages_touched: appendix.pages_touched,
        pages_listing: appendix
            .pages_listing
            .iter()
            .map(|listed| ListedPage {
                page: listed.page,
                timestamp: listed.timestamp,
            })
            .collect(),
        sentence: appendix_sentence(appendix),
    }
}

/// The Appended row's value: `properties.ts`'s `appendixRow(signature).value`,
/// restated as `words.rs` restates `integrity.ts` and held to it the same
/// way --- `wording.json`'s `appended` cases, which `cliwording.test.ts` asks
/// the original.
///
/// What arrived comes first, most specific reading first: validation data
/// when the catalog gained `/DSS`, another signature when a `/Sig` is among
/// the objects, otherwise the file's own names for them. Then what was
/// removed, when anything was, and then the pages.
#[must_use]
pub fn appendix_sentence(appendix: &docinfo::Appendix) -> String {
    if appendix.unread {
        return "something, but its contents could not be read".into();
    }
    let arrived = appendix.added + appendix.replaced;
    let what = if appendix.catalog_gained.iter().any(|key| key == "DSS") {
        "the certificates and revocation records a signature needs to be checked later".to_string()
    } else if appendix.kinds.iter().any(|kind| kind == "Sig") {
        "another signature".to_string()
    } else if arrived == 0 && appendix.removed > 0 {
        // Nothing arrived and something went: the removal is the whole of
        // it, and "0 objects" in front of it would be noise.
        let removed = match appendix.removed {
            1 => "1 object was removed".to_string(),
            n => format!("{n} objects were removed"),
        };
        return format!("{removed}, and {}", appendix_pages(appendix));
    } else {
        let objects = match arrived {
            1 => "1 object".to_string(),
            n => format!("{n} objects"),
        };
        if appendix.kinds.is_empty() {
            objects
        } else {
            format!("{objects}: {}", appendix.kinds.join(", "))
        }
    };
    match appendix.removed {
        0 => format!("{what}, and {}", appendix_pages(appendix)),
        1 => format!(
            "{what}, with 1 object removed, and {}",
            appendix_pages(appendix)
        ),
        n => format!(
            "{what}, with {n} objects removed, and {}",
            appendix_pages(appendix)
        ),
    }
}

/// What the append did to pages: `properties.ts`'s `describePages`.
///
/// Only when every touched page was rewritten to list a field does the
/// sentence say so and name the pages. A mix keeps the bare count, so "pages
/// were rewritten" still means a page changed and tpdf does not know why.
fn appendix_pages(appendix: &docinfo::Appendix) -> String {
    let touched = appendix.pages_touched;
    let listing = &appendix.pages_listing;
    if touched == 0 {
        return "no page was rewritten".into();
    }
    let numbers: Vec<String> = listing.iter().map(|l| l.page.to_string()).collect();
    let Some((last, before)) = numbers.split_last().filter(|_| listing.len() == touched) else {
        return match touched {
            1 => "1 page was rewritten".into(),
            n => format!("{n} pages were rewritten"),
        };
    };
    let field = if listing.iter().all(|listed| listed.timestamp) {
        "timestamp field"
    } else if listing.iter().any(|listed| listed.timestamp) {
        "signature or timestamp field"
    } else {
        "signature field"
    };
    if before.is_empty() {
        format!(
            "a {field} was added to page {last}'s annotations (the page's content is unchanged)"
        )
    } else {
        format!(
            "a {field} was added to the annotations of pages {} and {last} \
             (their content is unchanged)",
            before.join(", ")
        )
    }
}

/// One chain's answer, as the report shows it. `authority` says the chain is
/// a timestamp authority's.
#[must_use]
pub fn chain_report(chain: &crate::revocation::chain::Chain, authority: bool) -> ChainReport {
    ChainReport {
        standing: chain.standing,
        after_moment: chain.after_moment,
        decided_by: chain.decided_by,
        dropped: chain.dropped,
        end: chain.end,
        certificates: chain
            .certificates
            .iter()
            .enumerate()
            .map(|(at, judged)| ChainCertificate {
                subject: if judged.subject_cn.is_empty() {
                    judged.subject.clone()
                } else {
                    judged.subject_cn.clone()
                },
                serial: judged.serial.clone(),
                revocation: RevocationReport {
                    sentence: if at == 0 {
                        words::revocation_sentence(&judged.revocation, authority)
                    } else {
                        words::revocation_sentence_about(
                            &judged.revocation,
                            authority,
                            &words::issuing_certificate(judged),
                        )
                    },
                    ..revocation_report(&judged.revocation, authority)
                },
            })
            .collect(),
        sentence: words::chain_sentence(chain, authority),
    }
}

/// Whether a chain's own row is shown: it holds a certificate above the leaf,
/// or one past the bound. Otherwise the leaf's row already says all of it.
/// `integrity.ts`'s `chainRow` returns nothing on the same rule.
#[must_use]
pub fn chain_shown(chain: &ChainReport) -> bool {
    chain.certificates.len() > 1 || chain.dropped > 0
}

/// One revocation answer, as the report shows it. `authority` says the
/// certificate is a timestamp authority's.
#[must_use]
pub fn revocation_report(
    revocation: &crate::revocation::Revocation,
    authority: bool,
) -> RevocationReport {
    RevocationReport {
        standing: revocation.standing,
        why: revocation.why,
        source: revocation.source,
        issued: revocation.issued.clone(),
        next: revocation.next.clone(),
        revoked: revocation.revoked.clone(),
        reason: revocation.reason,
        basis: revocation.basis,
        moment: revocation.moment.clone(),
        after_moment: revocation.after_moment,
        sentence: words::revocation_sentence(revocation, authority),
    }
}

/// One timestamp, as the report shows it. `document` says it is a document
/// timestamp, whose imprint covers the signed bytes rather than a signature.
#[must_use]
pub fn timestamp_report(stamp: &docinfo::Timestamp, document: bool) -> report::TimestampReport {
    let authority = stamp.authority.as_ref();
    let by = authority.map_or_else(String::new, |c| {
        if c.subject_cn.is_empty() {
            c.subject.clone()
        } else {
            c.subject_cn.clone()
        }
    });
    let (from, until) = authority.map_or((String::new(), String::new()), |c| {
        (c.from.clone(), c.until.clone())
    });
    let integrity = stamp.integrity.clone().unwrap_or_default();
    report::TimestampReport {
        time: stamp.when.clone(),
        authority: by.clone(),
        attested: stamp.attested,
        integrity: IntegrityReport {
            verdict: integrity.verdict,
            why: integrity.why,
            digest: integrity.digest.clone(),
            method: integrity.method.clone(),
            sentence: words::timestamp_sentence(
                &stamp.when,
                &by,
                stamp.integrity.as_ref(),
                document,
            ),
        },
        trust: stamp.trust.as_ref().map(|trust| TrustReport {
            standing: trust.standing,
            why: trust.why,
            store: trust.store,
            attested_at: trust.attested_at.clone(),
            sentence: words::authority_sentence(trust, &from, &until),
        }),
        revocation: stamp
            .revocation
            .as_ref()
            .map(|r| revocation_report(r, true)),
        revocation_chain: stamp
            .revocation_chain
            .as_ref()
            .map(|c| chain_report(c, true)),
    }
}

/// Whether a signature passes `--strict`: intact; trusted, now or at the
/// time a trusted timestamp attests; and neither its certificate nor any above
/// it shown revoked by the document's own data, unless after that attested
/// time.
///
/// **`none` passes, and so do `unknown` and `unchecked`**: a document
/// carrying no revocation data is the ordinary case, and failing it would
/// fail nearly every signed document there is. `--strict` asks whether
/// anything tpdf checked speaks against the signature, not whether
/// everything was checked --- `docs/PLAN.md` records the decision.
///
/// This is the signature's half. What was appended after the last signature
/// is the document's, and [`after_last_signature`] answers it.
#[must_use]
pub fn passes_strict(signature: &report::Signature) -> bool {
    use crate::trust::Standing;
    signature.integrity.verdict == crate::integrity::Verdict::Intact
        && signature
            .trust
            .as_ref()
            .is_some_and(|t| matches!(t.standing, Standing::Trusted | Standing::TrustedAtTimestamp))
        && !signature
            .revocation
            .as_ref()
            .is_some_and(|r| r.standing == crate::revocation::Status::Revoked && !r.after_moment)
        // A certificate above the signer's, revoked before the moment, undoes
        // the signature as the signer's own would: nothing it issued stands.
        // Above only: the chain's first certificate is the signer's, which the
        // clause before this one has answered for --- two clauses asking the
        // same question would leave either one's deletion unnoticed.
        && !signature.revocation_chain.as_ref().is_some_and(|c| {
            c.certificates.iter().skip(1).any(|above| {
                above.revocation.standing == crate::revocation::Status::Revoked
                    && !above.revocation.after_moment
            })
        })
}

/// What a document holds after its last signature that still stands, which no
/// signature covers.
///
/// **Why `--strict` asks.** A signature answers for the bytes in its range and
/// for nothing after them: a revision appended later can change every page a
/// reader sees while each signature stays `intact`, trusted and unrevoked. The
/// properties dialog states the appendix beside the verdict for that reason;
/// an exit code has nowhere to state it, so `--strict` judges it.
///
/// **What it lets pass** is what signing itself appends: validation data
/// (`/DSS`), which reaches no page; a signature or timestamp field listed
/// among a page's annotations with the page otherwise as it was
/// (`docinfo::Appendix::pages_listing`); and a signature put in an empty
/// signature field the signed revision already had, which is not counted as
/// a page touched at all (`docinfo::Appendix::pages_touched`). tpdf's own
/// revisions after a signature are those three and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    /// Nothing, or nothing that touches a page other than to list a field.
    Unchanged,
    /// This many pages were touched, other than to list a signature or
    /// timestamp field.
    Pages(usize),
    /// Something tpdf could not read, which is never read as nothing.
    Unread,
}

/// [`After`] for a document's signatures, as a worker read them.
///
/// The last signature that stands is the `intact` one whose range reaches
/// furthest. One that is not intact answers for nothing, so what follows it is
/// still judged against the intact one before it --- and `--strict` fails on
/// the signature that is not intact whatever this says.
#[must_use]
pub fn after_last_signature(signatures: &[docinfo::Signature]) -> After {
    let last = signatures
        .iter()
        .filter(|s| {
            s.signed
                && s.integrity
                    .as_ref()
                    .is_some_and(|i| i.verdict == crate::integrity::Verdict::Intact)
        })
        .min_by_key(|s| s.appended_bytes);
    match last.and_then(|s| s.appendix.as_ref()) {
        None => After::Unchanged,
        Some(appendix) if appendix.unread => After::Unread,
        Some(appendix) => {
            match appendix
                .pages_touched
                .saturating_sub(appendix.pages_listing.len())
            {
                0 => After::Unchanged,
                pages => After::Pages(pages),
            }
        }
    }
}

/// [`After`] as `verify` says it under a document's signatures, or nothing
/// when there is nothing to say.
#[must_use]
pub fn after_text(after: After) -> Option<String> {
    let what = match after {
        After::Unchanged => return None,
        After::Pages(1) => "1 page was rewritten, which no signature covers".to_string(),
        After::Pages(pages) => format!("{pages} pages were rewritten, which no signature covers"),
        After::Unread => {
            "something was appended that tpdf could not read, and no signature covers it".into()
        }
    };
    Some(format!("  After the last signature: {what}"))
}

/// Reads one document's signatures through a worker, and what follows the
/// last of them. A document that could not be read has nothing known to
/// follow: its error decides the exit code on its own.
fn verify_one(env: &Env<'_>, path: &Path) -> (report::File, After) {
    let shown = path.display().to_string();
    let failed = |kind: ErrorKind, message: String| {
        (
            report::File {
                path: shown.clone(),
                error: Some(FileError { kind, message }),
                signatures: Vec::new(),
            },
            After::Unchanged,
        )
    };
    let (file, len) = match super::opened_or_stdin(path) {
        Ok(opened) => opened,
        Err(why) => return failed(ErrorKind::Unreadable, why),
    };
    let properties = match env.worker().properties(&file, len) {
        Ok(properties) => properties,
        Err(Declined::Refused(why)) => {
            return failed(ErrorKind::Refused, format!("{shown}: {why}"));
        }
        // PDFium could not open it without a password: the worker answers
        // every question with that, and the sentence is the one below.
        Err(Declined::Locked(_)) => return failed(ErrorKind::Locked, locked_sentence(&shown)),
        Err(Declined::Failed(why)) => return failed(ErrorKind::Failed, format!("{shown}: {why}")),
    };
    if properties.limits.locked {
        return failed(ErrorKind::Locked, locked_sentence(&shown));
    }
    let after = after_last_signature(&properties.signatures);
    (
        report::File {
            path: shown,
            error: None,
            signatures: properties
                .signatures
                .iter()
                .filter(|s| s.signed)
                .map(signature_report)
                .collect(),
        },
        after,
    )
}

/// Why a document's signatures were not read: it needs a password.
///
/// One sentence for both ways of finding out --- PDFium refusing to open the
/// document at all, and `lopdf` opening it and finding every object ciphertext
/// --- because to the reader they are the same fact.
fn locked_sentence(shown: &str) -> String {
    format!(
        "{shown} is encrypted with a password, and its signatures cannot be read without it \
         --- open it in tpdf to check them"
    )
}

/// The exit code a verified report earns.
#[must_use]
pub fn verify_exit(report: &report::Verified, strict: bool) -> Exit {
    let worst = report
        .files
        .iter()
        .filter_map(|file| file.error.as_ref())
        .map(|error| match error.kind {
            ErrorKind::Failed => Exit::Internal,
            ErrorKind::Unreadable | ErrorKind::Refused | ErrorKind::Locked => Exit::Refused,
        })
        .max();
    match worst {
        Some(exit) => exit,
        None if strict && !report.strict_passed => Exit::Strict,
        None => Exit::Ok,
    }
}

/// Builds the report for files already read, with nothing known to follow
/// any document's last signature.
#[must_use]
pub fn verified(files: Vec<report::File>) -> report::Verified {
    let after = vec![After::Unchanged; files.len()];
    verified_after(files, &after)
}

/// [`verified`], with what follows each document's last signature: `after`
/// answers for `files` in order, and a document it does not reach has nothing
/// known to follow.
#[must_use]
pub fn verified_after(files: Vec<report::File>, after: &[After]) -> report::Verified {
    let strict_passed = files.iter().enumerate().all(|(at, file)| {
        file.error.is_none()
            && !file.signatures.is_empty()
            && file.signatures.iter().all(passes_strict)
            && after.get(at).is_none_or(|after| *after == After::Unchanged)
    });
    report::Verified {
        schema: SCHEMA,
        command: "verify".into(),
        strict_passed,
        files,
    }
}

fn run_verify(
    env: &Env<'_>,
    verify: &Verify,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Exit, Failure> {
    let (files, after): (Vec<report::File>, Vec<After>) =
        verify.files.iter().map(|p| verify_one(env, p)).unzip();
    let report = verified_after(files, &after);
    for file in &report.files {
        if let Some(error) = &file.error {
            say(err, &format!("{}: {}", env.program, error.message));
        }
    }
    if verify.json {
        json(out, &report);
    } else {
        say(out, &verify_text_after(&report, &after));
    }
    Ok(verify_exit(&report, verify.strict))
}

/// `verify` without `--json`, with nothing known to follow any document's
/// last signature.
#[must_use]
pub fn verify_text(report: &report::Verified) -> String {
    verify_text_after(report, &[])
}

/// `verify` without `--json`: each document's signatures, and under them what
/// follows the last one when that is more than signing appends
/// ([`after_text`]). `after` answers for the report's files in order.
#[must_use]
pub fn verify_text_after(report: &report::Verified, after: &[After]) -> String {
    let mut lines = Vec::new();
    for (at, file) in report.files.iter().enumerate() {
        if file.error.is_some() {
            lines.push(format!("{}: could not be read (see above)", file.path));
            continue;
        }
        lines.push(format!("{}: {}", file.path, counted(&file.signatures)));
        for signature in &file.signatures {
            lines.push(signature_text(signature));
        }
        lines.extend(after.get(at).copied().and_then(after_text));
    }
    lines.join("\n")
}

/// How many signatures and how many document timestamps, counted apart: a
/// document timestamp is a signature field in the file and nobody's signature,
/// so "2 signatures" over one of each says a second party signed.
/// `properties.ts`'s `countedSignatures` is the dialog's half.
#[must_use]
pub(crate) fn counted(signatures: &[report::Signature]) -> String {
    let stamps = signatures.iter().filter(|s| s.document_timestamp).count();
    let signed = match signatures.len() - stamps {
        0 => "no signatures".to_string(),
        1 => "1 signature".to_string(),
        n => format!("{n} signatures"),
    };
    match stamps {
        0 => signed,
        1 => format!("{signed} and 1 document timestamp"),
        n => format!("{signed} and {n} document timestamps"),
    }
}

pub(crate) fn signature_text(signature: &report::Signature) -> String {
    // A document timestamp's own certificate is its authority's, so its rows
    // are the authority's rows and the token's copies below are not repeated.
    let document = signature.document_timestamp;
    let authority = |name: &str| {
        if document {
            format!("Authority {}", name.to_lowercase())
        } else {
            name.to_string()
        }
    };
    let who = if signature.signer.is_empty() {
        "a certificate that could not be read".to_string()
    } else {
        format!(
            "certificate issued to {} by {}",
            signature.signer, signature.issuer
        )
    };
    let mut lines = vec![if document {
        format!("  Document timestamp {} --- {who}", signature.field)
    } else {
        format!("  {} --- {who}", signature.field)
    }];
    if !signature.claimed_time.is_empty() {
        lines.push(format!("    Date given: {}", signature.claimed_time));
    }
    lines.push(format!("    Integrity: {}", signature.integrity.sentence));
    // Under Integrity, whose sentence says something was appended: this row
    // says what.
    if let Some(appendix) = &signature.appendix {
        lines.push(format!("    Appended: {}", appendix.sentence));
    }
    if let Some(trust) = &signature.trust {
        let name = if document {
            "Timestamp authority"
        } else {
            "Trust"
        };
        lines.push(format!("    {name}: {}", trust.sentence));
    }
    if let Some(revocation) = &signature.revocation {
        lines.push(format!(
            "    {}: {}",
            authority("Revocation"),
            revocation.sentence
        ));
    }
    if let Some(chain) = signature
        .revocation_chain
        .as_ref()
        .filter(|c| chain_shown(c))
    {
        lines.push(format!(
            "    {}: {}",
            authority("Chain revocation"),
            chain.sentence
        ));
    }
    if let Some(stamp) = &signature.timestamp {
        lines.push(format!("    Timestamped: {}", stamp.integrity.sentence));
        if document {
            return lines.join("\n");
        }
        if let Some(trust) = &stamp.trust {
            lines.push(format!("    Timestamp authority: {}", trust.sentence));
        }
        if let Some(revocation) = &stamp.revocation {
            lines.push(format!("    Authority revocation: {}", revocation.sentence));
        }
        if let Some(chain) = stamp.revocation_chain.as_ref().filter(|c| chain_shown(c)) {
            lines.push(format!(
                "    Authority chain revocation: {}",
                chain.sentence
            ));
        }
    }
    if let Some(level) = &signature.pades {
        lines.push(format!("    PAdES level: {level}"));
    }
    lines.join("\n")
}
