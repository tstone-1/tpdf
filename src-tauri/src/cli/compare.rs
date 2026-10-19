//! `tpdf compare <old.pdf> <new.pdf>`: what changed in the words.
//!
//! **The window's *Compare with*, without the window.** Both documents are read
//! page by page through a sandboxed worker, and their words are compared by
//! [`crate::compare`], which is the comparison the window runs. So what this
//! prints is the list the window's sidebar shows.
//!
//! **Text only, and stderr says so on every run.** A changed picture, drawing,
//! colour or layout is not a changed word, and "no differences" is a statement
//! about the words.
//!
//! **Exit 1 means the words differ**, as `diff` and `cmp` use it, so `tpdf
//! compare a.pdf b.pdf && ...` works in a shell. A document that cannot be read
//! outranks it, and so does one with no text on any page (exit 3): two scans
//! have no words to differ in, and exit 0 would read as "the same". Words left
//! uncompared because they face a page with no text also exit 1: what changed
//! may be on that page.
//!
//! Plain output is one line per change, `wdiff`'s notation: the words the first
//! document has in `[-...-]`, the second's in `{+...+}`, with a few shared
//! words either side, after the page each is on, `left -> right`.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::args::{unknown, value};
use super::regions::{regions_on, Half};
use super::report::{self, HitRect, SCHEMA};
use super::text::{password, variable};
use super::{json, say, Env, Exit, Failure, Registered, Subcommand};
use crate::compare::{self, Change, Comparison, PageCodes, Span};
use crate::save_outside::Declined;
use crate::text::PageText;
use crate::worker_proto::{Reply, Request};

/// `compare`, registered.
pub const COMMAND: Registered = Registered {
    name: "compare",
    usage: "compare <old.pdf> <new.pdf> [--password-env VAR] [--json]",
    summary: "Prints every change in the words from the first document to the\n            second, one line per change, as the window's Compare with lists\n            them. Text only: pictures, drawings, formatting and layout are\n            not compared. Exit 1 when the words differ or some could not be\n            compared beside a page with no text, 3 when a document has no\n            text on any page.",
    parse: boxed,
};

/// `tpdf compare`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compare {
    /// The older document.
    pub old: PathBuf,
    /// The newer one.
    pub new: PathBuf,
    /// The environment variable holding the password, for either document.
    pub password_env: Option<String>,
    /// `--json`.
    pub json: bool,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `compare`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Compare, String> {
    let mut files = Vec::new();
    let mut password_env = None;
    let mut json = false;
    let mut positional = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match (positional, arg.as_str()) {
            (false, "--") => positional = true,
            (false, "--password-env") => password_env = Some(variable(value(arg, &mut rest)?)?),
            (false, "--json") => json = true,
            (false, flag) if flag.starts_with('-') && flag != "-" => {
                return Err(unknown("compare", flag))
            }
            (after, path) => files.push(super::args::operand(after, path)),
        }
    }
    let [old, new] = <[PathBuf; 2]>::try_from(files).map_err(|given: Vec<PathBuf>| {
        format!(
            "`compare` takes two documents, the older first --- `compare old.pdf new.pdf` --- \
             and was given {}",
            given.len()
        )
    })?;
    if old == Path::new("-") && new == Path::new("-") {
        return Err("only one of the two documents can come from standard input".into());
    }
    Ok(Compare {
        old,
        new,
        password_env,
        json,
    })
}

/// One document's pages, read.
struct Read {
    shown: String,
    /// Every page's text, counted from 0.
    texts: Vec<PageText>,
    /// Pages, counted from 1, that held no characters.
    without_text: Vec<u32>,
}

/// Reads every page of `path` through a worker.
fn read(env: &Env<'_>, path: &Path, password: Option<&str>) -> Result<Read, Failure> {
    let shown = path.display().to_string();
    let declined = |why: Declined| -> Failure {
        match why {
            Declined::Locked(_) if password.is_some() => Failure::new(
                Exit::Refused,
                format!("{shown} is encrypted, and the password given did not open it"),
            ),
            Declined::Locked(_) => Failure::new(
                Exit::Refused,
                format!("{shown} is encrypted with a password --- give it with --password-env"),
            ),
            Declined::Refused(why) => Failure::new(Exit::Refused, format!("{shown}: {why}")),
            Declined::Failed(why) => Failure::new(Exit::Internal, format!("{shown}: {why}")),
        }
    };
    let (file, len) =
        super::opened_or_stdin(path).map_err(|why| Failure::new(Exit::Refused, why))?;
    let mut session = env
        .worker()
        .session(&file, len, password)
        .map_err(declined)?;
    let count = match session
        .ask(Request::Open {
            lazy_geometry: true,
        })
        .map_err(declined)?
    {
        Reply::Open { page_count, .. } => u32::try_from(page_count).unwrap_or(u32::MAX),
        other => {
            return Err(Failure::new(
                Exit::Internal,
                format!("{shown}: the worker answered an open with {other:?}"),
            ))
        }
    };
    let mut texts = Vec::new();
    let mut without_text = Vec::new();
    for page in 0..count {
        match session
            .ask(Request::Text { page, crop: None })
            .map_err(declined)?
        {
            Reply::Text(text) => {
                if text.codes.is_empty() {
                    without_text.push(page + 1);
                }
                texts.push(text);
            }
            other => {
                return Err(Failure::new(
                    Exit::Internal,
                    format!(
                        "{shown}: page {} could not be read: the worker answered a text \
                         request with {other:?}",
                        page + 1
                    ),
                ))
            }
        }
    }
    Ok(Read {
        shown,
        texts,
        without_text,
    })
}

impl Subcommand for Compare {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let password = password(self.password_env.as_deref())?;
        let old = read(env, &self.old, password.as_deref())?;
        let new = read(env, &self.new, password.as_deref())?;
        for side in [&old, &new] {
            if side.without_text.len() == side.texts.len() {
                return Err(Failure::new(
                    Exit::Refused,
                    format!(
                        "{}: none of its pages has text, so there are no words to compare --- \
                         a scan has to be read by OCR first",
                        side.shown
                    ),
                ));
            }
        }
        let codes = |side: &Read| -> Vec<PageCodes> {
            side.texts
                .iter()
                .zip(0u32..)
                .map(|(text, page)| PageCodes {
                    page,
                    codes: text.codes.clone(),
                })
                .collect()
        };
        let comparison = compare::compare(&codes(&old), &codes(&new))
            .map_err(|why| Failure::new(Exit::Refused, why))?;
        let report = compared(&old, &new, &comparison);

        say(
            err,
            &format!(
                "{}: the words were compared; pictures, drawings, formatting and layout were not",
                env.program
            ),
        );
        for side in [&old, &new] {
            if let Some(note) = textless(side) {
                say(err, &format!("{}: {note}", env.program));
            }
        }
        if report.total_changes as usize > report.changes.len() {
            say(
                err,
                &format!(
                    "{}: {} changes, and the first {} are listed",
                    env.program,
                    report.total_changes,
                    report.changes.len()
                ),
            );
        }
        if report.unaligned > 0 {
            say(
                err,
                &format!(
                    "{}: {} stretches share no word that occurs once in each, and are each \
                     listed as one replacement",
                    env.program, report.unaligned
                ),
            );
        }
        if self.json {
            json(out, &report);
        } else {
            let text = plain(&report);
            if !text.is_empty() {
                let _ = out.write_all(super::printable(&text).as_bytes());
                let _ = out.flush();
            }
        }
        for (side, other) in [(&report.old, &report.new), (&report.new, &report.old)] {
            if side.words_not_compared > 0 {
                say(
                    err,
                    &format!(
                        "{}: {}: {} words stand where {} has a page with no text, and were not \
                         compared",
                        env.program, side.path, side.words_not_compared, other.path
                    ),
                );
            }
        }
        // 0 only when everything was read and nothing changed: words left
        // uncompared beside a scan may be exactly where the change is.
        Ok(
            if report.total_changes == 0
                && report.old.words_not_compared == 0
                && report.new.words_not_compared == 0
            {
                Exit::Ok
            } else {
                Exit::Strict
            },
        )
    }
}

/// The sentence for a document some of whose pages held no characters. One
/// with none on any page was refused in `run` before the comparison.
fn textless(side: &Read) -> Option<String> {
    match side.without_text.as_slice() {
        [] => None,
        [one] => Some(format!("{}: page {one} has no text", side.shown)),
        many => Some(format!("{}: {} pages have no text", side.shown, many.len())),
    }
}

/// The report for `comparison` of `old` against `new`.
fn compared(old: &Read, new: &Read, comparison: &Comparison) -> report::Compared {
    let file = |side: &Read, words: u32, unread: u32| report::ComparedFile {
        path: side.shown.clone(),
        pages: u32::try_from(side.texts.len()).unwrap_or(u32::MAX),
        pages_without_text: side.without_text.clone(),
        words,
        words_not_compared: unread,
    };
    report::Compared {
        schema: SCHEMA,
        command: "compare".into(),
        old: file(old, comparison.left_words, comparison.left_unread),
        new: file(new, comparison.right_words, comparison.right_unread),
        removed_words: comparison.removed_words,
        added_words: comparison.added_words,
        total_changes: comparison.total_changes,
        unaligned: comparison.unaligned,
        changes: comparison
            .changes
            .iter()
            .map(|change| changed(old, new, change))
            .collect(),
    }
}

fn changed(old: &Read, new: &Read, change: &Change) -> report::ComparedChange {
    report::ComparedChange {
        kind: change.kind,
        old_page: change.left_at.page + 1,
        new_page: change.right_at.page + 1,
        removed: change.removed.clone(),
        added: change.added.clone(),
        before: change.before.clone(),
        after: change.after.clone(),
        old_rects: rects(&old.texts, &change.left),
        new_rects: rects(&new.texts, &change.right),
    }
}

/// The rectangles a change's spans cover, as `search` reports a hit's.
fn rects(texts: &[PageText], spans: &[Span]) -> Vec<HitRect> {
    spans
        .iter()
        .flat_map(|span| {
            let half = Half {
                page: span.page,
                from: span.start,
                to: Some(span.end),
            };
            texts
                .get(span.page as usize)
                .map(|text| regions_on(text, span.page, &[half]))
                .unwrap_or_default()
                .into_iter()
                .map(move |[left, top, right, bottom]| HitRect {
                    page: span.page + 1,
                    rect: [left, top, right - left, bottom - top],
                })
        })
        .collect()
}

/// `compare` without `--json`: one line per change.
#[must_use]
pub fn plain(report: &report::Compared) -> String {
    let mut out = String::new();
    for change in &report.changes {
        out.push_str(&format!("{} -> {}: ", change.old_page, change.new_page));
        if !change.before.is_empty() {
            out.push_str(&change.before);
            out.push(' ');
        }
        if !change.removed.is_empty() {
            out.push_str(&format!("[-{}-]", change.removed));
        }
        if !change.added.is_empty() {
            out.push_str(&format!("{{+{}+}}", change.added));
        }
        if !change.after.is_empty() {
            out.push(' ');
            out.push_str(&change.after);
        }
        out.push('\n');
    }
    out
}
