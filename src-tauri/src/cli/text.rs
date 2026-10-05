//! `tpdf text <file.pdf> [--pages 1-3,7] [--json] [-o out.txt]`: a document's
//! text in the order it is read.
//!
//! **The viewer's extraction and the viewer's order.** Each page is the
//! worker's `Request::Text` --- the `PageText` selection, search and the
//! screen-reader layer are built on --- and its order is `reading.ts`'s, as
//! [`crate::reading`] restates it: the document's own tags where they claim
//! every visible character, and the geometry otherwise. `order` in the JSON
//! says which, per page, from the same decision the viewer makes
//! (`usableRuns`), not from whether the document happens to have a tree.
//!
//! **Lines, not the copy buffer.** The viewer's select-all copies the page's
//! characters in reading order with the line breaks PDFium synthesised left
//! wherever they fell. This prints `readingLines`: each line's characters with
//! those breaks taken out and its trailing whitespace trimmed, one line per
//! `\n` --- the lines the screen-reader layer is built from. Pages end with a
//! form feed, as `pdftotext` ends them.

use std::io::Write;
use std::path::PathBuf;

use super::args::{lexically_same, operand, unknown, value};
use super::pages::{check_target, publish_copy};
use super::report::{self, Encoding, Order, SCHEMA};
use super::{Env, Exit, Failure, Registered, Subcommand};
use crate::reading;
use crate::save_outside::{Declined, Session};
use crate::worker_proto::{Reply, Request};

/// `text`, registered.
pub const COMMAND: Registered = Registered {
    name: "text",
    usage: "text <file.pdf> [--pages 1-3,7] [--password-env VAR] [--json]\n        [-o <out.txt> [--force]]",
    summary: "Prints the document's text in the order it is read: the\n            document's own tags where they cover the page, and the order the\n            viewer recovers from the layout otherwise. Pages end with a form\n            feed. --pages counts from 1.",
    parse: boxed,
};

/// The most text one run writes, in bytes of UTF-8.
///
/// Refused past this rather than cut: text that stops part-way through a page
/// reads as a document that stops there. 64 MiB is a few thousand pages of
/// dense prose; a document with more is asked for a page at a time with
/// `--pages`. Each page is already bounded by the worker, whose reply may not
/// exceed `worker_proto::MAX_REPLY_BYTES`.
pub const MAX_TEXT_BYTES: usize = 64 * 1024 * 1024;

/// `tpdf text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    /// The document.
    pub input: PathBuf,
    /// The pages asked for, counted from 1, in document order; `None` for all.
    pub pages: Option<Vec<u32>>,
    /// The environment variable holding the password, when one was named.
    pub password_env: Option<String>,
    /// `--json`.
    pub json: bool,
    /// `-o`: where to write instead of stdout.
    pub output: Option<PathBuf>,
    /// `--force`: replace an existing `-o`.
    pub force: bool,
}

fn boxed(args: &[String]) -> Result<Box<dyn Subcommand>, String> {
    parse(args).map(|c| Box::new(c) as Box<dyn Subcommand>)
}

/// Reads the arguments after `text`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String]) -> Result<Text, String> {
    let mut input: Option<PathBuf> = None;
    let mut pages = None;
    let mut password_env = None;
    let mut json = false;
    let mut positional = false;
    let mut output: Option<PathBuf> = None;
    let mut force = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match (positional, arg.as_str()) {
            (false, "--") => positional = true,
            (false, "--pages") => pages = Some(page_list(value(arg, &mut rest)?)?),
            (false, "--password-env") => password_env = Some(variable(value(arg, &mut rest)?)?),
            (false, "--json") => json = true,
            (false, "-o" | "--output") => output = Some(PathBuf::from(value(arg, &mut rest)?)),
            (false, "--force") => force = true,
            (false, flag) if flag.starts_with('-') && flag != "-" => {
                return Err(unknown("text", flag))
            }
            (after, path) => {
                if input.is_some() {
                    return Err(format!(
                        "`text` takes one document, and `{path}` is a second"
                    ));
                }
                input = Some(operand(after, path));
            }
        }
    }
    let input = input.ok_or("`text` needs the document to read")?;
    if let Some(out) = &output {
        if lexically_same(&input, out) {
            return Err("`-o` names the document being read".into());
        }
    }
    if force && output.is_none() {
        return Err("`--force` replaces an existing `-o` file, and there is no `-o`".into());
    }
    Ok(Text {
        input,
        pages,
        password_env,
        json,
        output,
        force,
    })
}

/// `--password-env`'s value: the *name* of a variable, never the password.
///
/// # Errors
///
/// A name that is empty or holds `=`, which no variable can have.
pub(crate) fn variable(name: &str) -> Result<String, String> {
    if name.is_empty() || name.contains('=') || name.contains('\0') {
        return Err(format!(
            "`--password-env` takes the name of an environment variable, and `{name}` cannot be \
             one --- the password itself never goes on the command line"
        ));
    }
    Ok(name.to_string())
}

/// Reads `1-3,7` into pages counted from 1, sorted, each once.
///
/// The palette's rules (`src/lib/pageranges.ts`): a reversed range is refused
/// rather than turned round, an overlap is merged, and the result is in
/// document order whatever order it was typed in. Whether the document *has*
/// those pages is a question about the document, asked in [`Subcommand::run`].
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn page_list(raw: &str) -> Result<Vec<u32>, String> {
    let number = |part: &str| -> Result<u32, String> {
        match part.trim().parse::<u32>() {
            Ok(n) if n >= 1 => Ok(n),
            _ => Err(format!(
                "`--pages {raw}`: `{}` is not a page number --- pages count from 1",
                part.trim()
            )),
        }
    };
    let mut pages = std::collections::BTreeSet::new();
    for part in raw.split(',') {
        if part.trim().is_empty() {
            return Err(format!("`--pages {raw}` has an empty part"));
        }
        match part.split_once('-') {
            None => {
                pages.insert(number(part)?);
            }
            Some((from, to)) => {
                let (from, to) = (number(from)?, number(to)?);
                if from > to {
                    return Err(format!("`--pages {raw}`: {from}-{to} runs backwards"));
                }
                if to - from >= 100_000 {
                    return Err("--pages selects too many pages; at most 100000 per command".into());
                }
                pages.extend(from..=to);
            }
        }
        if pages.len() > 100_000 {
            return Err("--pages selects too many pages; at most 100000 per command".into());
        }
    }
    Ok(pages.into_iter().collect())
}

impl Subcommand for Text {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        if let Some(output) = &self.output {
            // As `sign` orders them: the second name first, which is a mistake
            // in the command line whatever else is true.
            if crate::save::same_file(&self.input, output) {
                return Err(Failure::new(
                    Exit::Usage,
                    "-o is the document being read, under another name",
                ));
            }
            if !self.force && output.exists() {
                return Err(Failure::new(
                    Exit::Refused,
                    format!("{} exists --- give --force to replace it", output.display()),
                ));
            }
            // What --force does not replace: a link, a directory.
            check_target(std::slice::from_ref(&self.input), output, self.force)?;
        }
        let password = password(self.password_env.as_deref())?;
        let report = read_text(env, self, password.as_deref())?;
        let written = if self.json {
            let mut text = super::ascii_json(&report)
                .map_err(|e| Failure::new(Exit::Internal, format!("could not encode: {e}")))?;
            text.push('\n');
            text
        } else {
            plain(&report)
        };
        match &self.output {
            None => {
                let _ = out.write_all(written.as_bytes());
                let _ = out.flush();
            }
            // Staged beside the output and put in place whole, as every
            // command that writes a file does: a write that fails part-way
            // leaves an existing file as it was. The text has no second reader
            // to be read back by, and nothing it was read from is compared,
            // since the document may have come from a pipe.
            Some(path) => publish_copy(
                std::slice::from_ref(&self.input),
                &[],
                path,
                self.force,
                |staged| {
                    std::fs::write(staged, written.as_bytes())
                        .map(|()| (false, ()))
                        .map_err(|e| {
                            Failure::new(
                                Exit::Refused,
                                format!("could not write {}: {e}", path.display()),
                            )
                        })
                },
                |_, ()| Ok(()),
            )?,
        }
        Ok(Exit::Ok)
    }
}

/// The password `--password-env` names, read from the environment.
///
/// # Errors
///
/// Exit 2: the variable is not set, or is not text.
pub(crate) fn password(name: Option<&str>) -> Result<Option<String>, Failure> {
    let Some(name) = name else {
        return Ok(None);
    };
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Err(Failure::new(
            Exit::Usage,
            format!("--password-env names {name}, and no such environment variable is set"),
        )),
        Err(std::env::VarError::NotUnicode(_)) => Err(Failure::new(
            Exit::Usage,
            format!("the environment variable {name} is not valid Unicode"),
        )),
    }
}

/// A refusal from the worker, as this command's failure. `fields` and
/// `fill` share it.
pub(crate) fn declined(shown: &str, why: Declined, password_given: bool) -> Failure {
    match why {
        Declined::Locked(_) if password_given => Failure::new(
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
}

/// Every page asked for, read and ordered.
fn read_text(
    env: &Env<'_>,
    command: &Text,
    password: Option<&str>,
) -> Result<report::Text, Failure> {
    let shown = command.input.display().to_string();
    let (file, len) =
        super::opened_or_stdin(&command.input).map_err(|why| Failure::new(Exit::Refused, why))?;
    let mut session = env
        .worker()
        .session(&file, len, password)
        .map_err(|why| declined(&shown, why, password.is_some()))?;
    let count = match session
        .ask(Request::Open {
            lazy_geometry: true,
        })
        .map_err(|why| declined(&shown, why, password.is_some()))?
    {
        Reply::Open { page_count, .. } => u32::try_from(page_count).unwrap_or(u32::MAX),
        other => {
            return Err(Failure::new(
                Exit::Internal,
                format!("the worker answered an open with {other:?}"),
            ))
        }
    };
    let pages = selected(command.pages.as_deref(), count)
        .map_err(|why| Failure::new(Exit::Refused, format!("{shown}: {why}")))?;
    let mapping = match session
        .ask(Request::Mapping)
        .map_err(|why| declined(&shown, why, password.is_some()))?
    {
        Reply::Mapping(mapping) => mapping,
        other => {
            return Err(Failure::new(
                Exit::Internal,
                format!("the worker answered a mapping request with {other:?}"),
            ))
        }
    };
    let mut out = Vec::with_capacity(pages.len());
    let mut total = 0usize;
    for page in pages {
        let one = page_text(&mut session, page, mapping.get(page as usize - 1))
            .map_err(|why| declined(&format!("{shown}, page {page}"), why, password.is_some()))?;
        total += one.text.len();
        if total > MAX_TEXT_BYTES {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{shown}: the text passes {} MiB by page {page} --- ask for fewer pages \
                     with --pages",
                    MAX_TEXT_BYTES / (1024 * 1024)
                ),
            ));
        }
        out.push(one);
    }
    Ok(report::Text {
        schema: SCHEMA,
        command: "text".into(),
        path: shown,
        pages: out,
    })
}

/// The pages to read: those asked for, or all of them.
///
/// # Errors
///
/// A page past the end, named.
pub fn selected(asked: Option<&[u32]>, count: u32) -> Result<Vec<u32>, String> {
    match asked {
        None => Ok((1..=count).collect()),
        Some(pages) => match pages.iter().find(|page| **page > count) {
            Some(page) => Err(format!(
                "there is no page {page}: the document has {count} page{}",
                if count == 1 { "" } else { "s" }
            )),
            None => Ok(pages.to_vec()),
        },
    }
}

/// One page, extracted by the worker and ordered here.
fn page_text(
    session: &mut Session,
    page: u32,
    mapping: Option<&crate::encoding::PageMapping>,
) -> Result<report::PageText, Declined> {
    let text = match session.ask(Request::Text {
        page: page - 1,
        crop: None,
    })? {
        Reply::Text(text) => text,
        other => {
            return Err(Declined::Failed(format!(
                "the worker answered a text request with {other:?}"
            )))
        }
    };
    Ok(ordered(page, &text, mapping))
}

/// A page's text as `text` reports it.
#[must_use]
pub fn ordered(
    page: u32,
    text: &crate::text::PageText,
    mapping: Option<&crate::encoding::PageMapping>,
) -> report::PageText {
    let reading = reading::read(text);
    let order = if text.codes.is_empty() {
        Order::None
    } else {
        match reading.route {
            reading::Route::Tagged => Order::Tagged,
            reading::Route::Geometric => Order::Geometric,
        }
    };
    let encoding = match mapping {
        Some(m) if m.unreadable() => Encoding::Guessed,
        Some(m) if m.certain() => Encoding::Stated,
        _ => Encoding::Unknown,
    };
    report::PageText {
        page,
        order,
        encoding,
        text: lines(text, &reading.lines).join("\n"),
    }
}

/// Each line's characters, the synthesised line breaks taken out and the
/// whitespace at its end trimmed.
///
/// Both are PDFium's, not the document's: it inserts a space or a `\r\n`
/// between two text objects, and when the reading order moves a column's line
/// away from the line it was drawn beside, the separator travels with it ---
/// `columns.pdf`'s interleaved page came back as `alpha one ` until this
/// trimmed it. Leading whitespace is kept, because an indent can be the
/// document's.
#[must_use]
pub fn lines(text: &crate::text::PageText, lines: &[Vec<reading::Range>]) -> Vec<String> {
    lines
        .iter()
        .map(|ranges| {
            let line: String = ranges
                .iter()
                .flat_map(|r| r.from..r.to)
                .map(|i| {
                    text.codes
                        .get(i)
                        .and_then(|code| char::from_u32(*code))
                        .unwrap_or('\u{FFFD}')
                })
                .filter(|c| *c != '\r' && *c != '\n')
                .collect();
            line.trim_end().to_string()
        })
        .collect()
}

/// `text` without `--json`: each page, then a form feed.
///
/// The page's text through [`super::printable`]: it is the document's, and
/// what it keeps of the control characters is the line breaks and the tabs.
/// The form feed is this function's own and is added after.
#[must_use]
pub fn plain(report: &report::Text) -> String {
    let mut out = String::new();
    for page in &report.pages {
        out.push_str(&super::printable(&page.text));
        if !page.text.is_empty() {
            out.push('\n');
        }
        out.push('\u{000C}');
    }
    out
}
