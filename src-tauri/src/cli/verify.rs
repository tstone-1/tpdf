//! `tpdf verify <file.pdf>... [--strict] [--json]`: every signature's
//! integrity verdict and trust standing, in the properties dialog's words.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::args::unknown;
use super::report::{self, ErrorKind, FileError, IntegrityReport, TrustReport, SCHEMA};
use super::{json, opened, say, words, Env, Exit, Failure, Registered, Subcommand};
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
    /// `--strict`: exit 1 unless every document has a signature and every
    /// signature is intact and trusted.
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
    let mut strict = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "--strict" => strict = true,
            flag if flag.starts_with('-') && flag != "-" => return Err(unknown("verify", flag)),
            path => files.push(PathBuf::from(path)),
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
    let trust = signature.trust.as_ref().map(|trust| TrustReport {
        standing: trust.standing,
        why: trust.why,
        store: trust.store,
        sentence: words::trust_sentence(trust, &from, &until),
    });
    let name = |cn: &str, whole: &str| {
        if cn.is_empty() {
            whole.to_string()
        } else {
            cn.to_string()
        }
    };
    report::Signature {
        field: signature.field.clone(),
        signer: certificate.map_or_else(String::new, |c| name(&c.subject_cn, &c.subject)),
        issuer: certificate.map_or_else(String::new, |c| name(&c.issuer_cn, &c.issuer)),
        claimed_time: signature.when.clone(),
        covers_whole_file: signature.covers_whole_file,
        appended_bytes: signature.appended_bytes,
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
    }
}

/// Whether a signature passes `--strict`: intact, and trusted.
#[must_use]
pub fn passes_strict(signature: &report::Signature) -> bool {
    signature.integrity.verdict == crate::integrity::Verdict::Intact
        && signature
            .trust
            .as_ref()
            .is_some_and(|t| t.standing == crate::trust::Standing::Trusted)
}

/// Reads one document's signatures through a worker.
fn verify_one(env: &Env<'_>, path: &Path) -> report::File {
    let shown = path.display().to_string();
    let failed = |kind: ErrorKind, message: String| report::File {
        path: shown.clone(),
        error: Some(FileError { kind, message }),
        signatures: Vec::new(),
    };
    let (file, len) = match opened(path) {
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
    report::File {
        path: shown,
        error: None,
        signatures: properties
            .signatures
            .iter()
            .filter(|s| s.signed)
            .map(signature_report)
            .collect(),
    }
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

/// Builds the report for files already read.
#[must_use]
pub fn verified(files: Vec<report::File>) -> report::Verified {
    let strict_passed = files.iter().all(|file| {
        file.error.is_none()
            && !file.signatures.is_empty()
            && file.signatures.iter().all(passes_strict)
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
    let report = verified(verify.files.iter().map(|p| verify_one(env, p)).collect());
    for file in &report.files {
        if let Some(error) = &file.error {
            say(err, &format!("{}: {}", env.program, error.message));
        }
    }
    if verify.json {
        json(out, &report);
    } else {
        say(out, &verify_text(&report));
    }
    Ok(verify_exit(&report, verify.strict))
}

/// `verify` without `--json`.
#[must_use]
pub fn verify_text(report: &report::Verified) -> String {
    let mut lines = Vec::new();
    for file in &report.files {
        if file.error.is_some() {
            lines.push(format!("{}: could not be read (see above)", file.path));
            continue;
        }
        match file.signatures.len() {
            0 => lines.push(format!("{}: no signatures", file.path)),
            1 => lines.push(format!("{}: 1 signature", file.path)),
            n => lines.push(format!("{}: {n} signatures", file.path)),
        }
        for signature in &file.signatures {
            lines.push(signature_text(signature));
        }
    }
    lines.join("\n")
}

pub(crate) fn signature_text(signature: &report::Signature) -> String {
    let who = if signature.signer.is_empty() {
        "a certificate that could not be read".to_string()
    } else {
        format!(
            "certificate issued to {} by {}",
            signature.signer, signature.issuer
        )
    };
    let mut lines = vec![format!("  {} --- {who}", signature.field)];
    if !signature.claimed_time.is_empty() {
        lines.push(format!("    Date given: {}", signature.claimed_time));
    }
    lines.push(format!("    Integrity: {}", signature.integrity.sentence));
    if let Some(trust) = &signature.trust {
        lines.push(format!("    Trust: {}", trust.sentence));
    }
    lines.join("\n")
}
