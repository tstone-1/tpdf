//! `tpdf protect` and `tpdf unprotect`: a copy with a password, or without one.
//!
//! Both write a copy and never touch the input. The new password is read from
//! an environment variable, as the one that opens the input is: a password on
//! the command line is readable by every process on the machine.
//!
//! The copy is staged beside its destination and opened in a fresh worker
//! before it is published. A protected copy must refuse to open without the
//! password and open with it; an unprotected one must open with none and say
//! it is not encrypted. Either way the pages must be the source's.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::args::{lexically_same, unknown, value};
use super::fill::SignedState;
use super::pages::{check_target, read_input, same_sizes, Temporary};
use super::report::{self, SCHEMA};
use super::text::{declined, password, variable};
use super::{json, opened, say, Env, Exit, Failure, Registered, Subcommand};
use crate::protect::{self, Protection};
use crate::save;
use crate::save_outside::Declined;
use crate::worker_proto::Request;

pub const PROTECT: Registered = Registered {
    name: "protect",
    usage: "protect <in.pdf> -o <out.pdf> --new-password-env VAR [--password-env VAR]\n        [--invalidate-signatures] [--force] [--json]",
    summary: "Writes a copy that needs a password to open, encrypted with AES-256.\n            The password is read from the environment variable --new-password-env\n            names. A document that already has one gets the new one instead.",
    parse: |args| parse(args, true).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

pub const UNPROTECT: Registered = Registered {
    name: "unprotect",
    usage: "unprotect <in.pdf> -o <out.pdf> --password-env VAR\n        [--invalidate-signatures] [--force] [--json]",
    summary: "Writes a copy that opens without a password, from a document that\n            needs one. The password is read from the environment variable\n            --password-env names.",
    parse: |args| parse(args, false).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

#[derive(Debug)]
struct Protect {
    /// `protect` rather than `unprotect`.
    set: bool,
    input: PathBuf,
    output: PathBuf,
    new_password_env: Option<String>,
    password_env: Option<String>,
    invalidate: bool,
    force: bool,
    json: bool,
}

impl Protect {
    fn name(&self) -> &'static str {
        if self.set {
            "protect"
        } else {
            "unprotect"
        }
    }
}

fn parse(args: &[String], set: bool) -> Result<Protect, String> {
    let name = if set { "protect" } else { "unprotect" };
    let mut command = Protect {
        set,
        input: PathBuf::new(),
        output: PathBuf::new(),
        new_password_env: None,
        password_env: None,
        invalidate: false,
        force: false,
        json: false,
    };
    let mut paths = Vec::new();
    let mut output = None;
    let mut rest = args.iter();
    let mut positional = false;
    while let Some(arg) = rest.next() {
        if positional {
            paths.push(PathBuf::from(arg));
            continue;
        }
        match arg.as_str() {
            "--" => positional = true,
            "-o" | "--output" => output = Some(PathBuf::from(value(arg, &mut rest)?)),
            "--new-password-env" if set => {
                command.new_password_env = Some(variable(value(arg, &mut rest)?)?);
            }
            "--password-env" => command.password_env = Some(variable(value(arg, &mut rest)?)?),
            "--invalidate-signatures" => command.invalidate = true,
            "--force" => command.force = true,
            "--json" => command.json = true,
            flag if flag.starts_with('-') => return Err(unknown(name, flag)),
            path => paths.push(path.into()),
        }
    }
    if paths.len() != 1 {
        return Err(format!("{name} needs exactly one input document"));
    }
    command.input = paths.remove(0);
    command.output = output.ok_or("-o <out.pdf> is required; the input is never overwritten")?;
    if lexically_same(&command.input, &command.output) {
        return Err("the output names the input; choose a different name".into());
    }
    if set && command.new_password_env.is_none() {
        return Err(
            "protect needs --new-password-env VAR, naming the environment variable that holds \
             the new password --- the password itself never goes on the command line"
                .into(),
        );
    }
    if !set && command.password_env.is_none() {
        return Err(
            "unprotect needs --password-env VAR, naming the environment variable that holds \
             the document's password"
                .into(),
        );
    }
    Ok(command)
}

/// Whether `path` refuses to open without a password, asked of a fresh worker.
fn locked(env: &Env<'_>, path: &Path) -> Result<bool, Failure> {
    let shown = path.display().to_string();
    let (file, len) = opened(path).map_err(|why| Failure::new(Exit::Internal, why))?;
    let asked = env
        .worker()
        .session(&file, len, None)
        .and_then(|mut session| session.ask(Request::Properties));
    match asked {
        Ok(_) => Ok(false),
        Err(Declined::Locked(_)) => Ok(true),
        Err(why) => Err(declined(&shown, why, false)),
    }
}

impl Protect {
    fn run_protect(&self, env: &Env<'_>, out: &mut dyn Write) -> Result<Exit, Failure> {
        let inputs = std::slice::from_ref(&self.input);
        check_target(inputs, &self.output, self.force)?;
        let key = password(self.password_env.as_deref())?;
        let new = password(self.new_password_env.as_deref())?;
        if let Some(new) = &new {
            protect::acceptable(new).map_err(|why| Failure::new(Exit::Usage, why))?;
        }
        let (mut input, session) = read_input(env, &self.input, key.as_deref())?;
        drop(session);
        if !self.set && !input.encrypted {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{} has no password, so there is none to remove",
                    self.input.display()
                ),
            ));
        }
        if input.signed.is_some() && !self.invalidate {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{} is signed or its signatures could not be fully read; changing its \
                     password rewrites the document and requires --invalidate-signatures",
                    self.input.display()
                ),
            ));
        }
        input.plan.protection = match &new {
            Some(new) => Protection::Set(new.clone()),
            None => Protection::Remove,
        };

        let agrees = |input: &super::pages::Input| {
            input
                .plan
                .opened_as
                .as_ref()
                .expect("read_input fingerprints every source")
                .agrees_with(&self.input)
                .map_err(|why| Failure::new(Exit::Refused, why))
        };
        agrees(&input)?;
        let staging = Temporary::beside(&self.output)?;
        let staged = staging.0.join("output.pdf");
        let result = save::write_copy(
            &self.input,
            &input.plan,
            &staged,
            key.as_deref(),
            &env.worker(),
        )
        .map_err(|why| Failure::new(Exit::Refused, why.message))?;
        if result.changed {
            return Err(Failure::new(
                Exit::Refused,
                "the source changed while writing; no output was published",
            ));
        }

        // The staged file, in fresh workers, which share no code with the
        // writer's own read-back: PDFium decides whether it opens.
        let unpublished = |what: &str| {
            Failure::new(
                Exit::Internal,
                format!("the staged file {what}; no output was published"),
            )
        };
        if locked(env, &staged)? != self.set {
            return Err(unpublished(if self.set {
                "opens without the password"
            } else {
                "still asks for a password"
            }));
        }
        let (after, session) = read_input(env, &staged, new.as_deref()).map_err(|why| {
            Failure::new(
                Exit::Internal,
                format!(
                    "the staged file could not be opened again: {}; no output was published",
                    why.message
                ),
            )
        })?;
        drop(session);
        if after.encrypted != self.set {
            return Err(unpublished("is not protected the way that was asked"));
        }
        if !same_sizes(&after.sizes, &input.sizes) {
            return Err(unpublished("does not have the source's pages"));
        }
        agrees(&input)?;
        check_target(inputs, &self.output, self.force)?;
        Temporary::publish(&staged, &self.output, self.force)?;

        let report = report::Protected {
            schema: SCHEMA,
            command: self.name().into(),
            input: self.input.display().to_string(),
            output: self.output.display().to_string(),
            pages: input.plan.baseline,
            protected: self.set,
            was_protected: input.encrypted,
            signatures_invalidated: match input.signed {
                Some(SignedState::Signed(n)) => n,
                _ => 0,
            },
            signatures_unknown: input.signatures_unknown,
        };
        if self.json {
            json(out, &report);
        } else {
            say(out, &plain(&report));
        }
        Ok(Exit::Ok)
    }
}

/// The report as sentences.
fn plain(report: &report::Protected) -> String {
    let mut lines = vec![match (report.protected, report.was_protected) {
        (true, false) => format!("{}: needs the new password to open, AES-256", report.output),
        (true, true) => format!(
            "{}: needs the new password to open, AES-256; the one it had no longer opens it",
            report.output
        ),
        (false, _) => format!("{}: opens without a password", report.output),
    }];
    if report.signatures_invalidated > 0 || report.signatures_unknown {
        lines.push("The rewrite invalidates existing signatures.".into());
    }
    lines.join("\n")
}

impl Subcommand for Protect {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        self.run_protect(env, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn protect_needs_a_variable_for_the_new_password() {
        for line in [
            "",
            "a.pdf",
            "a.pdf -o b.pdf",
            "a.pdf b.pdf -o c.pdf --new-password-env NEW",
            "a.pdf -o a.pdf --new-password-env NEW",
            "a.pdf -o b.pdf --new-password-env",
            "a.pdf -o b.pdf --new-password-env A=B",
            "a.pdf -o b.pdf --new-password-env NEW --unknown",
        ] {
            assert!(parse(&args(line), true).is_err(), "{line}");
        }
        let parsed = parse(
            &args("a.pdf -o b.pdf --new-password-env NEW --password-env OLD --force"),
            true,
        )
        .unwrap();
        assert_eq!(parsed.new_password_env.as_deref(), Some("NEW"));
        assert_eq!(parsed.password_env.as_deref(), Some("OLD"));
        assert!(parsed.set && parsed.force && !parsed.invalidate);
    }

    #[test]
    fn unprotect_needs_the_password_and_takes_no_new_one() {
        for line in [
            "a.pdf -o b.pdf",
            "a.pdf -o a.pdf --password-env OLD",
            "a.pdf -o b.pdf --password-env OLD --new-password-env NEW",
        ] {
            assert!(parse(&args(line), false).is_err(), "{line}");
        }
        let parsed = parse(&args("a.pdf -o b.pdf --password-env OLD --json"), false).unwrap();
        assert!(!parsed.set && parsed.json);
        assert_eq!(parsed.name(), "unprotect");
    }

    #[test]
    fn the_sentences_say_which_password_opens_the_copy() {
        let mut report = report::Protected {
            schema: SCHEMA,
            command: "protect".into(),
            input: "a.pdf".into(),
            output: "b.pdf".into(),
            pages: 2,
            protected: true,
            was_protected: false,
            signatures_invalidated: 0,
            signatures_unknown: false,
        };
        assert_eq!(
            plain(&report),
            "b.pdf: needs the new password to open, AES-256"
        );
        report.was_protected = true;
        assert!(plain(&report).ends_with("the one it had no longer opens it"));
        report.protected = false;
        report.signatures_invalidated = 1;
        assert_eq!(
            plain(&report),
            "b.pdf: opens without a password\nThe rewrite invalidates existing signatures."
        );
    }
}
