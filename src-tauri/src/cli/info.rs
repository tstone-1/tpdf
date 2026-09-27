//! `tpdf info <file.pdf>... [--password-env VAR] [--json]`: what the
//! properties dialog shows, as data.
//!
//! Four questions to one worker per document, every one of them a question
//! the application already asks: `Request::Properties` (the dialog itself ---
//! the same request `verify` makes, so the signatures are `verify`'s objects
//! built by `verify`'s function), `Request::Open` for the page sizes the
//! viewer lays out from, and `Request::Form` for the form the viewer fills.
//! Nothing was added to any of them.
//!
//! **A locked document is a finding, not a failure.** `info` reports it with
//! `error.kind` `locked` and still exits 0, because "this file needs a
//! password" is what the dialog would have said about it.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::args::unknown;
use super::report::{self, Described, ErrorKind, FileError, SCHEMA};
use super::text::{password, variable};
use super::verify::{signature_report, signature_text};
use super::{json, opened, say, Env, Exit, Failure, Registered, Subcommand};
use crate::docinfo::Properties;
use crate::save_outside::{Declined, Session};
use crate::worker_proto::{Reply, Request};

/// `info`, registered.
pub const COMMAND: Registered = Registered {
    name: "info",
    usage: "info <file.pdf>... [--password-env VAR] [--json]",
    summary: "Describes each document as the properties dialog does: pages,\n            metadata, encryption, tagging, claimed conformance, attachments,\n            its form, and its signatures as verify reports them.",
    parse: boxed,
};

/// `tpdf info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// The documents, in the order given.
    pub files: Vec<PathBuf>,
    /// The environment variable holding the password, when one was named.
    pub password_env: Option<String>,
    /// `--json`.
    pub json: bool,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `info`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Info, String> {
    let mut files = Vec::new();
    let mut password_env = None;
    let mut json = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--password-env" => {
                password_env = Some(variable(super::args::value(arg, &mut rest)?)?);
            }
            flag if flag.starts_with('-') && flag != "-" => return Err(unknown("info", flag)),
            path => files.push(PathBuf::from(path)),
        }
    }
    if files.is_empty() {
        return Err("`info` needs at least one document".into());
    }
    Ok(Info {
        files,
        password_env,
        json,
    })
}

impl Subcommand for Info {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let password = password(self.password_env.as_deref())?;
        let report = report::Info {
            schema: SCHEMA,
            command: "info".into(),
            files: self
                .files
                .iter()
                .map(|path| describe_one(env, path, password.as_deref()))
                .collect(),
        };
        for file in &report.files {
            if let Some(error) = &file.error {
                say(err, &format!("{}: {}", env.program, error.message));
            }
        }
        if self.json {
            json(out, &report);
        } else {
            say(out, &info_text(&report));
        }
        Ok(info_exit(&report))
    }
}

/// The exit code: 0 when every document was described or is merely locked.
#[must_use]
pub fn info_exit(report: &report::Info) -> Exit {
    report
        .files
        .iter()
        .filter_map(|file| file.error.as_ref())
        .map(|error| match error.kind {
            ErrorKind::Locked => Exit::Ok,
            ErrorKind::Unreadable | ErrorKind::Refused => Exit::Refused,
            ErrorKind::Failed => Exit::Internal,
        })
        .max()
        .unwrap_or(Exit::Ok)
}

fn describe_one(env: &Env<'_>, path: &Path, password: Option<&str>) -> Described {
    let shown = path.display().to_string();
    let failed = |kind: ErrorKind, message: String| Described {
        path: shown.clone(),
        error: Some(FileError { kind, message }),
        document: None,
    };
    let (file, len) = match opened(path) {
        Ok(opened) => opened,
        Err(why) => return failed(ErrorKind::Unreadable, why),
    };
    let asked = env
        .worker()
        .session(&file, len, password)
        .and_then(|mut session| ask_all(&mut session));
    match asked {
        Ok((properties, sizes, form)) => Described {
            path: shown,
            error: None,
            document: Some(document(&properties, &sizes, form)),
        },
        Err(Declined::Locked(_)) => failed(
            ErrorKind::Locked,
            if password.is_some() {
                format!("{shown} is encrypted, and the password given did not open it")
            } else {
                format!(
                    "{shown} is encrypted with a password --- give it with --password-env to \
                     describe it"
                )
            },
        ),
        Err(Declined::Refused(why)) => failed(ErrorKind::Refused, format!("{shown}: {why}")),
        Err(Declined::Failed(why)) => failed(ErrorKind::Failed, format!("{shown}: {why}")),
    }
}

type Answers = (
    Properties,
    Vec<crate::render::PageSize>,
    Result<crate::forms::Form, String>,
);

/// The three requests, in the order the dialog's data arrives.
fn ask_all(session: &mut Session) -> Result<Answers, Declined> {
    let unexpected = |what: &str, reply: &Reply| {
        Declined::Failed(format!("the worker answered {what} with {reply:?}"))
    };
    let properties = match session.ask(Request::Properties)? {
        Reply::Properties(properties) => *properties,
        other => return Err(unexpected("the properties request", &other)),
    };
    let sizes = match session.ask(Request::Open {
        lazy_geometry: false,
    })? {
        Reply::Open { pages, .. } => pages,
        other => return Err(unexpected("an open", &other)),
    };
    // A form the worker refuses --- XFA, or past a bound --- is a fact about
    // the form and not about the document, so it is kept rather than raised.
    let form = match session.ask(Request::Form) {
        Ok(Reply::Form(form)) => Ok(form),
        Ok(other) => return Err(unexpected("the form request", &other)),
        Err(Declined::Refused(why)) => Err(why),
        Err(other) => return Err(other),
    };
    Ok((properties, sizes, form))
}

/// The report for one document, from what its worker answered.
#[must_use]
pub fn document(
    properties: &Properties,
    sizes: &[crate::render::PageSize],
    form: Result<crate::forms::Form, String>,
) -> report::Document {
    report::Document {
        version: properties.version.clone(),
        bytes: properties.bytes,
        pages: properties.pages,
        page_sizes: page_sizes(sizes),
        revisions: properties.revisions,
        metadata: properties.fields.clone(),
        language: properties.language.clone(),
        encryption: properties.encryption.clone(),
        tagged: properties.tagged,
        conformance: properties.xmp.as_ref().map(|xmp| report::Conformance {
            claimed: xmp.conformance.clone(),
            unread: xmp.unread,
        }),
        attachments: properties.attachments,
        form: form_report(form),
        signatures: properties
            .signatures
            .iter()
            .filter(|s| s.signed)
            .map(signature_report)
            .collect(),
        unsigned_signature_fields: properties.signatures.iter().filter(|s| !s.signed).count(),
        limits: properties.limits.clone(),
    }
}

/// Points to two decimals, which is finer than any paper size differs by.
fn points(value: f32) -> f64 {
    (f64::from(value) * 100.0).round() / 100.0
}

/// Each distinct displayed size and how many pages have it, first seen first.
#[must_use]
pub fn page_sizes(sizes: &[crate::render::PageSize]) -> Vec<report::PageSize> {
    let mut out: Vec<report::PageSize> = Vec::new();
    for size in sizes {
        let (width_pt, height_pt) = (points(size.width_pt), points(size.height_pt));
        match out
            .iter_mut()
            .find(|s| s.width_pt == width_pt && s.height_pt == height_pt)
        {
            Some(seen) => seen.count += 1,
            None => out.push(report::PageSize {
                width_pt,
                height_pt,
                count: 1,
            }),
        }
    }
    out
}

/// The form as the viewer's form filling read it.
#[must_use]
pub fn form_report(form: Result<crate::forms::Form, String>) -> report::Form {
    match form {
        Ok(form) => report::Form {
            readable: true,
            fields: form
                .widgets
                .iter()
                .map(|w| w.object)
                .collect::<BTreeSet<_>>()
                .len(),
            widgets: form.widgets.len(),
            xfa: false,
            why: None,
        },
        Err(why) => report::Form {
            readable: false,
            fields: 0,
            widgets: 0,
            xfa: why == crate::forms::XFA_REFUSAL,
            why: Some(why),
        },
    }
}

/// `info` without `--json`.
#[must_use]
pub fn info_text(report: &report::Info) -> String {
    let mut lines = Vec::new();
    for file in &report.files {
        lines.push(file.path.clone());
        match (&file.error, &file.document) {
            (Some(error), _) if error.kind == ErrorKind::Locked => {
                lines.push("  Locked: it needs a password to be read.".into());
            }
            (Some(_), _) | (None, None) => lines.push("  Could not be read (see above).".into()),
            (None, Some(document)) => lines.extend(document_text(document)),
        }
    }
    lines.join("\n")
}

fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "could not be read",
    }
}

fn document_text(d: &report::Document) -> Vec<String> {
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    let sizes: Vec<String> = d
        .page_sizes
        .iter()
        .map(|s| format!("{} x {} pt ({})", s.width_pt, s.height_pt, s.count))
        .collect();
    let mut lines = vec![
        format!(
            "  PDF {}, {}, {}, {} bytes",
            d.version,
            plural(d.pages as usize, "page", "pages"),
            plural(d.revisions, "revision", "revisions"),
            d.bytes
        ),
        format!("  Page sizes: {}", sizes.join(", ")),
    ];
    for field in &d.metadata {
        lines.push(format!("  {}: {}", field.name, field.value));
    }
    if !d.language.is_empty() {
        lines.push(format!("  Language: {}", d.language));
    }
    lines.push(match &d.encryption {
        None => "  Encrypted: no".into(),
        Some(e) => {
            let denied: Vec<&str> = e
                .permissions
                .iter()
                .filter(|p| !p.allowed)
                .map(|p| p.what.as_str())
                .collect();
            let limits = if denied.is_empty() {
                "nothing restricted".to_string()
            } else {
                format!("not allowed: {}", denied.join(", "))
            };
            format!("  Encrypted: {}, {limits}", e.method)
        }
    });
    lines.push(format!("  Tagged: {}", yes_no(d.tagged)));
    if let Some(c) = &d.conformance {
        if !c.claimed.is_empty() {
            lines.push(format!(
                "  Claims to conform to: {} (claimed, not checked)",
                c.claimed.join(", ")
            ));
        }
        if c.unread {
            lines.push("  Its XMP metadata could not all be read.".into());
        }
    }
    lines.push(match d.attachments {
        Some(n) => format!("  Attachments: {n}"),
        None => "  Attachments: could not be counted".into(),
    });
    lines.push(match (&d.form.why, d.form.xfa, d.form.fields) {
        (Some(_), true, _) => "  Form: XFA, which tpdf does not read".into(),
        (Some(why), false, _) => format!("  Form: could not be read ({why})"),
        (None, _, 0) => "  Form: none".into(),
        (None, _, n) => format!(
            "  Form: {}, {}",
            plural(n, "field", "fields"),
            plural(d.form.widgets, "widget", "widgets")
        ),
    });
    match d.signatures.len() {
        0 => lines.push("  Signatures: none".into()),
        n => {
            lines.push(format!("  Signatures: {n}"));
            for signature in &d.signatures {
                lines.extend(signature_text(signature).lines().map(|l| format!("  {l}")));
            }
        }
    }
    if d.unsigned_signature_fields > 0 {
        lines.push(format!(
            "  Empty signature fields: {}",
            d.unsigned_signature_fields
        ));
    }
    if d.limits.any() {
        lines.push("  Some of it could not be read; --json lists what, under `limits`.".into());
    }
    lines
}
