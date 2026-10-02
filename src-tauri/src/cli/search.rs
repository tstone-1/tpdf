//! `tpdf search <file.pdf>... (--text STR | --pattern REGEX)...`: where a
//! document says something.
//!
//! **The viewer's find, and `redact`'s.** The queries are compiled by
//! `search.rs` and walked by [`super::redact::search_pages`], so what this
//! prints for `--text X` is exactly what `redact --text X` would remove and
//! what the window's find bar would step through: case folded unless
//! `--case-sensitive`, a phrase found across a page break, one extraction per
//! page. The options are spelled as `redact` spells them on purpose, so a line
//! can be tried here and then run there.
//!
//! **Several documents, as `grep` takes several files.** "Which of these
//! mention it" is the question a script asks, and a document that cannot be
//! read is reported and the rest are still searched.
//!
//! **Exit 1 means nothing was found**, as `grep`'s does, so `tpdf search ... &&`
//! works in a shell. It is the code `verify --strict` uses for "read, and the
//! answer is no". A document that could not be read outranks it.
//!
//! **A page with no text is said, not passed over.** A scan has nothing to
//! search, and "no matches" for it reads as "it is not in there". Every page
//! that held no characters is listed.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::args::{unknown, value};
use super::redact::{search_pages, Found};
use super::report::{self, ErrorKind, FileError, SearchKind, SCHEMA};
use super::text::{page_list, password, selected, variable};
use super::{json, opened, say, Env, Exit, Failure, Registered, Subcommand};
use crate::save_outside::Declined;
use crate::search::{Options, Prepared};
use crate::text::PageText;
use crate::worker_proto::{Reply, Request};

/// `search`, registered.
pub const COMMAND: Registered = Registered {
    name: "search",
    usage: "search <file.pdf>... (--text STR | --pattern REGEX)...\n        [--case-sensitive] [--whole-word] [--pages 1-3,7] [--password-env VAR] [--json]",
    summary: "Prints every match of each --text and --pattern with the words\n            around it, one line per match, found as the viewer's find and\n            `redact` find it. Exit 1 when nothing matched. A page with no\n            text to search is named.",
    parse: boxed,
};

/// The most matches one document reports.
///
/// Refused past this rather than cut, as `text` refuses past its size: a list
/// that stops part-way reads as a document that stops matching there.
pub const MAX_MATCHES: usize = 10_000;

/// `tpdf search`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    /// The documents, in the order given.
    pub files: Vec<PathBuf>,
    /// `--text`, in the order given.
    pub texts: Vec<String>,
    /// `--pattern`, in the order given.
    pub patterns: Vec<String>,
    /// `--case-sensitive`, for every query.
    pub case_sensitive: bool,
    /// `--whole-word`, for every query.
    pub whole_word: bool,
    /// `--pages`, counted from 1; `None` for all.
    pub pages: Option<Vec<u32>>,
    /// The environment variable holding the password, when one was named.
    pub password_env: Option<String>,
    /// `--json`.
    pub json: bool,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `search`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Search, String> {
    let mut command = Search {
        files: Vec::new(),
        texts: Vec::new(),
        patterns: Vec::new(),
        case_sensitive: false,
        whole_word: false,
        pages: None,
        password_env: None,
        json: false,
    };
    let mut positional = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match (positional, arg.as_str()) {
            (false, "--") => positional = true,
            (false, "--text") => command.texts.push(value(arg, &mut rest)?.clone()),
            (false, "--pattern") => command.patterns.push(value(arg, &mut rest)?.clone()),
            (false, "--case-sensitive") => command.case_sensitive = true,
            (false, "--whole-word") => command.whole_word = true,
            (false, "--pages") => command.pages = Some(page_list(value(arg, &mut rest)?)?),
            (false, "--password-env") => {
                command.password_env = Some(variable(value(arg, &mut rest)?)?);
            }
            (false, "--json") => command.json = true,
            (false, flag) if flag.starts_with('-') => return Err(unknown("search", flag)),
            (_, path) => command.files.push(PathBuf::from(path)),
        }
    }
    if command.files.is_empty() {
        return Err("`search` needs a document to search".into());
    }
    if command.texts.is_empty() && command.patterns.is_empty() {
        return Err(
            "`search` needs something to find: `--text STR` or `--pattern REGEX`, after the \
             document --- `search report.pdf --text invoice`"
                .into(),
        );
    }
    // Compiled here as well as in `run`, so a pattern that does not compile is
    // exit 2 with no document opened.
    queries(&command)?;
    Ok(command)
}

/// The queries, compiled, in report order: every `--text`, then every `--pattern`.
fn queries(command: &Search) -> Result<Vec<(SearchKind, String, Prepared)>, String> {
    let texts = command
        .texts
        .iter()
        .map(|q| (SearchKind::Text, "--text", q));
    let patterns = command
        .patterns
        .iter()
        .map(|q| (SearchKind::Pattern, "--pattern", q));
    texts
        .chain(patterns)
        .map(|(kind, flag, query)| {
            let options = Options {
                match_case: command.case_sensitive,
                whole_word: command.whole_word,
                regex: kind == SearchKind::Pattern,
            };
            let compiled = Prepared::new(query, options)
                .map_err(|problem| format!("`{flag} {query}`: {problem}"))?;
            if compiled.matches_nothing() {
                return Err(format!(
                    "`{flag}` needs something to find, and `{query}` can match nothing"
                ));
            }
            Ok((kind, query.clone(), compiled))
        })
        .collect()
}

impl Subcommand for Search {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let password = password(self.password_env.as_deref())?;
        let queries = queries(self).map_err(|why| Failure::new(Exit::Usage, why))?;
        let report = report::Searched {
            schema: SCHEMA,
            command: "search".into(),
            queries: queries
                .iter()
                .map(|(kind, query, _)| report::Query {
                    kind: *kind,
                    query: query.clone(),
                })
                .collect(),
            files: self
                .files
                .iter()
                .map(|path| search_one(env, self, path, &queries, password.as_deref()))
                .collect(),
        };
        for file in &report.files {
            if let Some(error) = &file.error {
                say(err, &format!("{}: {}", env.program, error.message));
            }
            if let Some(note) = textless(file) {
                say(err, &format!("{}: {note}", env.program));
            }
        }
        if self.json {
            json(out, &report);
        } else {
            let text = plain(&report);
            if !text.is_empty() {
                let _ = out.write_all(text.as_bytes());
                let _ = out.flush();
            }
        }
        Ok(search_exit(&report))
    }
}

/// The exit code: a document that could not be read first, then whether
/// anything matched at all.
#[must_use]
pub fn search_exit(report: &report::Searched) -> Exit {
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
        None if report.files.iter().all(|file| file.matches.is_empty()) => Exit::Strict,
        None => Exit::Ok,
    }
}

/// The sentence for a document some of whose pages held nothing to search.
#[must_use]
pub fn textless(file: &report::SearchedFile) -> Option<String> {
    let empty = &file.pages_without_text;
    Some(match empty.as_slice() {
        [] => return None,
        all if all.len() == file.pages_searched as usize => format!(
            "{}: none of its pages has text to search --- a scan has to be read by OCR first",
            file.path
        ),
        [one] => format!("{}: page {one} has no text to search", file.path),
        many => {
            let shown: Vec<String> = many.iter().take(10).map(u32::to_string).collect();
            let more = if many.len() > 10 { ", ..." } else { "" };
            format!(
                "{}: {} pages have no text to search ({}{more})",
                file.path,
                many.len(),
                shown.join(", ")
            )
        }
    })
}

/// `search` without `--json`: one line per match, `grep`'s shape.
///
/// The path leads each line only when more than one document was given.
#[must_use]
pub fn plain(report: &report::Searched) -> String {
    let named = report.files.len() > 1;
    let mut out = String::new();
    for file in &report.files {
        for hit in &file.matches {
            if named {
                out.push_str(&file.path);
                out.push(':');
            }
            out.push_str(&format!(
                "{}: {}{}{}\n",
                hit.page, hit.before, hit.hit, hit.after
            ));
        }
    }
    out
}

/// What the walk found in one document, as the report carries it.
///
/// Matches are sorted by page and then by where they start, whichever query
/// found them: the walk returns them per query within a page.
#[must_use]
pub fn searched(path: &str, pages: usize, empty: Vec<u32>, found: &Found) -> report::SearchedFile {
    let mut hits: Vec<(u32, u32, report::Hit)> = found
        .matches
        .iter()
        .zip(&found.by)
        .map(|(hit, query)| {
            (
                hit.page,
                hit.start,
                report::Hit {
                    page: hit.page + 1,
                    end_page: hit.end_page.map(|page| page + 1),
                    query: *query,
                    before: hit.before.clone(),
                    hit: hit.hit.clone(),
                    after: hit.after.clone(),
                },
            )
        })
        .collect();
    hits.sort_by_key(|(page, start, hit)| (*page, *start, hit.query));
    report::SearchedFile {
        path: path.to_string(),
        error: None,
        pages_searched: u32::try_from(pages).unwrap_or(u32::MAX),
        pages_without_text: empty,
        matches: hits.into_iter().map(|(_, _, hit)| hit).collect(),
    }
}

/// One document, searched, or the reason it was not.
fn search_one(
    env: &Env<'_>,
    command: &Search,
    path: &Path,
    queries: &[(SearchKind, String, Prepared)],
    password: Option<&str>,
) -> report::SearchedFile {
    let shown = path.display().to_string();
    let failed = |kind: ErrorKind, message: String| report::SearchedFile {
        path: shown.clone(),
        error: Some(FileError { kind, message }),
        pages_searched: 0,
        pages_without_text: Vec::new(),
        matches: Vec::new(),
    };
    let declined = |why: Declined| match why {
        Declined::Locked(_) if password.is_some() => failed(
            ErrorKind::Locked,
            format!("{shown} is encrypted, and the password given did not open it"),
        ),
        Declined::Locked(_) => failed(
            ErrorKind::Locked,
            format!("{shown} is encrypted with a password --- give it with --password-env"),
        ),
        Declined::Refused(why) => failed(ErrorKind::Refused, format!("{shown}: {why}")),
        Declined::Failed(why) => failed(ErrorKind::Failed, format!("{shown}: {why}")),
    };
    let (file, len) = match opened(path) {
        Ok(opened) => opened,
        Err(why) => return failed(ErrorKind::Unreadable, why),
    };
    let mut session = match env.worker().session(&file, len, password) {
        Ok(session) => session,
        Err(why) => return declined(why),
    };
    let count = match session.ask(Request::Open {
        lazy_geometry: true,
    }) {
        Ok(Reply::Open { page_count, .. }) => u32::try_from(page_count).unwrap_or(u32::MAX),
        Ok(other) => {
            return failed(
                ErrorKind::Failed,
                format!("{shown}: the worker answered an open with {other:?}"),
            )
        }
        Err(why) => return declined(why),
    };
    let pages = match selected(command.pages.as_deref(), count) {
        Ok(pages) => pages,
        Err(why) => return failed(ErrorKind::Refused, format!("{shown}: {why}")),
    };
    let mut empty = Vec::new();
    let found = search_pages(queries, &pages, |page| -> Result<PageText, String> {
        match session.ask(Request::Text { page, crop: None }) {
            Ok(Reply::Text(text)) => {
                if text.codes.is_empty() {
                    empty.push(page + 1);
                }
                Ok(text)
            }
            Ok(other) => Err(format!("the worker answered a text request with {other:?}")),
            Err(Declined::Locked(why) | Declined::Refused(why) | Declined::Failed(why)) => Err(why),
        }
    });
    match found {
        Err(why) => failed(ErrorKind::Failed, format!("{shown}: {why}")),
        Ok(found) if found.matches.len() > MAX_MATCHES => failed(
            ErrorKind::Refused,
            format!(
                "{shown}: more than {MAX_MATCHES} matches --- narrow the search, or ask for \
                 fewer pages with --pages"
            ),
        ),
        Ok(found) => searched(&shown, pages.len(), empty, &found),
    }
}
