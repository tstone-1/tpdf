//! `tpdf long-term <signed.pdf> -o <out.pdf> --timestamp <authority>`: the
//! application's *Add long-term validation data* from a terminal --- a
//! worker's reading of the signatures, the certificate authorities' answers,
//! a worker's two revisions, the application's writer, a worker's read-back.
//!
//! **A command of its own, not a mode of `sign`.** `sign` makes a signature
//! and needs `--identity`, a key and the OS's consent; this makes none, uses
//! no key, and works on a document somebody else signed. The word is `sign
//! --long-term`'s, because it is the same data, added later.

use std::io::Write;
use std::path::PathBuf;

use super::args::{lexically_same, unknown, value};
use super::report::{self, SCHEMA};
use super::verify::signature_report;
use super::{json, say, Env, Exit, Failure, Registered, Subcommand};
use crate::commands::validation::{asking_the_world, finish, name_of, refuse_too_large, Stopped};
use crate::save;

/// `long-term`, registered.
pub const COMMAND: Registered = Registered {
    name: "long-term",
    usage: "long-term <signed.pdf> -o <out.pdf>\n        --timestamp digicert|sectigo|globalsign|<url> [--force] [--json]",
    summary: "Adds what a verifier needs after the certificates expire to a\n            document that is already signed, by anybody: the certificate\n            authorities' answers about every signature's and timestamp's\n            certificates, and an archive timestamp over the whole from the\n            authority --timestamp names. No key is used. The original is never\n            changed; the copy is written to -o. All of the document's\n            signatures are covered, or nothing is written: a signature that\n            does not verify, a signer or authority this computer does not\n            trust, or revocation data that cannot be had is a refusal that\n            names the signature and the reason. Run again on its own result\n            it adds a further archive timestamp, while the certificates\n            involved are still valid: once one has expired it is refused.",
    parse: boxed,
};

/// `tpdf long-term <signed.pdf> -o <out.pdf> --timestamp <authority>`.
#[derive(Debug, Clone, PartialEq)]
pub struct LongTerm {
    /// The signed document. It is never written.
    pub input: PathBuf,
    /// Where the copy goes.
    pub output: PathBuf,
    /// The timestamp authority `--timestamp` names, already judged by
    /// `tsa::authority`: whom the archive timestamp is asked of.
    pub timestamp: url::Url,
    /// `--json`.
    pub json: bool,
    /// `--force`: replace an existing output file.
    pub force: bool,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `long-term`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<LongTerm, String> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut timestamp: Option<url::Url> = None;
    let mut json = false;
    let mut force = false;

    let mut rest = args.iter();
    let mut positional = false;
    while let Some(arg) = rest.next() {
        match (positional, arg.as_str()) {
            (false, "--") => positional = true,
            (false, "-o" | "--output") => output = Some(PathBuf::from(value(arg, &mut rest)?)),
            // Judged here, as `sign --timestamp` is: an address tpdf will not
            // ask is a malformed line and exit 2, before any worker or socket.
            (false, "--timestamp") => {
                timestamp = Some(
                    crate::tsa::authority(value(arg, &mut rest)?)
                        .map_err(|why| why.sentence(""))?,
                );
            }
            (false, "--json") => json = true,
            (false, "--force") => force = true,
            (false, flag) if flag.starts_with('-') && flag != "-" => {
                return Err(unknown("long-term", flag))
            }
            (_, path) => {
                if input.is_some() {
                    return Err(format!(
                        "`long-term` takes one document, and `{path}` is a second --- give \
                         them one at a time"
                    ));
                }
                input = Some(PathBuf::from(path));
            }
        }
    }

    let input = input.ok_or("`long-term` needs the signed document to add to")?;
    let output = output.ok_or(
        "`long-term` needs `-o <out.pdf>`: the document with the data added is written as a \
         new file, and the original is never changed",
    )?;
    if lexically_same(&input, &output) {
        return Err(
            "the output names the input --- the document with the data added is written as a \
             new file, so choose another name for it"
                .into(),
        );
    }
    // The archive timestamp is half of what this adds, and nobody is asked
    // for one without being named.
    let timestamp = timestamp.ok_or(
        "`long-term` needs `--timestamp`: the validation data is sealed with an archive \
         timestamp, and tpdf asks no authority you did not name",
    )?;
    Ok(LongTerm {
        input,
        output,
        timestamp,
        json,
        force,
    })
}

impl Subcommand for LongTerm {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        // What can be refused without a worker or a socket, first.
        if save::same_file(&self.input, &self.output) {
            return Err(Failure::new(
                Exit::Usage,
                "the output is the input under another name --- the document with the data \
                 added is written as a new file, so choose another name for it",
            ));
        }
        if !self.force && self.output.exists() {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{} already exists --- choose another name, or give --force to replace it",
                    self.output.display()
                ),
            ));
        }
        // Read once: every revision is built against exactly these bytes,
        // and they are what a worker is handed to read the signatures in.
        let original = std::fs::read(&self.input).map_err(|e| {
            Failure::new(
                Exit::Refused,
                format!("could not read {}: {e}", self.input.display()),
            )
        })?;
        if original.is_empty() {
            return Err(Failure::new(
                Exit::Refused,
                format!("{} is empty", self.input.display()),
            ));
        }
        refuse_too_large(original.len() as u64).map_err(|why| Failure::new(Exit::Refused, why))?;

        let worker = env.worker();
        let finished = asking_the_world(env.anchors, &self.timestamp, |asking| {
            finish(
                &original,
                &self.output,
                env.now,
                &worker,
                asking,
                &|bytes| {
                    save::write_signed(&self.input, &self.output, bytes).map_err(|why| why.message)
                },
            )
        })
        .map_err(|stopped| stopped_failure(&stopped, &name_of(&self.output)))?;

        let report = report::LongTerm {
            schema: SCHEMA,
            command: "long-term".into(),
            input: self.input.display().to_string(),
            output: self.output.display().to_string(),
            covered: finished.covered,
            archive: finished.archive,
            signatures: finished
                .signatures
                .iter()
                .filter(|signature| signature.signed)
                .map(signature_report)
                .collect(),
            summary: finished.summary.clone(),
        };
        if self.json {
            json(out, &report);
        } else {
            say(out, &finished.summary);
        }
        Ok(Exit::Ok)
    }
}

/// How a run that stopped ends: 3 for the document's, an authority's or a
/// certificate authority's refusal and for a copy that could not be written,
/// 4 when the failure is tpdf's own (`Stopped::tpdf_failed`).
pub(crate) fn stopped_failure(stopped: &Stopped, name: &str) -> Failure {
    let exit = if stopped.tpdf_failed() {
        Exit::Internal
    } else {
        Exit::Refused
    };
    Failure::new(exit, stopped.sentence(name))
}
