//! `tpdf page-numbers` and `tpdf watermark`: a copy with text on every page.
//!
//! Both are one writer, [`crate::addedtext`], with different defaults: a page
//! number goes at the bottom centre in 10 pt black as `Page {page} of {pages}`,
//! a watermark across the middle in 60 pt grey at 45 degrees, a third opaque.
//! Every option changes either. `--text` with fields of its own makes a header
//! or a footer: `--text "Confidential - {page}" --position top-right`.
//!
//! The input is never touched. The copy is staged beside its destination and
//! read back in a fresh worker before it is published, and the read-back is
//! the text extraction everything else reads: each page written to must hold
//! its own line, so text a reader's PDF viewer would not find is a refusal
//! rather than a file.

use std::io::Write;
use std::path::PathBuf;

use super::args::{lexically_same, unknown, value};
use super::fill::SignedState;
use super::pages::{check_target, copy_of, publish_copy, read_input, same_sizes};
use super::report::{self, SCHEMA};
use super::text::{page_list, password, variable};
use super::{json, say, Env, Exit, Failure, Registered, Subcommand};
use crate::addedtext::{self, AddedText, Position};
use crate::worker_proto::{Reply, Request};

pub const PAGE_NUMBERS: Registered = Registered {
    name: "page-numbers",
    usage: "page-numbers <in.pdf> -o <out.pdf> [--text TEXT] [--position POS] [--size PT]\n        [--start N] [--pages 1-3,7] [--margin PT] [--color r,g,b] [--password-env VAR]\n        [--invalidate-signatures] [--force] [--json]",
    summary: "Writes a copy with a page number on every page, `Page 1 of 3` at\n            the bottom centre unless --text and --position say otherwise.\n            {page} and {pages} in --text are the number and the count.",
    parse: |args| parse(args, false).map(|c| Box::new(c) as Box<dyn Subcommand>),
};

pub const WATERMARK: Registered = Registered {
    name: "watermark",
    usage: "watermark <in.pdf> -o <out.pdf> --text TEXT [--position POS] [--size PT]\n        [--angle DEG] [--opacity 0-1] [--color r,g,b] [--pages 1-3,7] [--password-env VAR]\n        [--invalidate-signatures] [--force] [--json]",
    summary: "Writes a copy with a line of text across every page, grey, at 45\n            degrees and a third opaque unless the options say otherwise.",
    parse: |args| parse(args, true).map(|c| Box::new(c) as Box<dyn Subcommand>),
};

/// `tpdf page-numbers` or `tpdf watermark`.
#[derive(Debug, Clone, PartialEq)]
pub struct AddText {
    /// `watermark` rather than `page-numbers`.
    pub watermark: bool,
    pub input: PathBuf,
    pub output: PathBuf,
    /// The text and how it is placed, with `pages` zero-based.
    pub added: AddedText,
    pub password_env: Option<String>,
    pub invalidate: bool,
    pub force: bool,
    pub json: bool,
}

impl AddText {
    fn name(&self) -> &'static str {
        if self.watermark {
            "watermark"
        } else {
            "page-numbers"
        }
    }
}

/// A number option's value, in `range`.
fn number(flag: &str, raw: &str) -> Result<f32, String> {
    raw.trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("`{flag} {raw}` is not a number"))
}

/// `--color`'s value: `r,g,b`, each from 0 to 1.
fn color(raw: &str) -> Result<[f32; 3], String> {
    let parts: Vec<f32> = raw
        .split(',')
        .filter_map(|part| part.trim().parse::<f32>().ok())
        .filter(|channel| (0. ..=1.).contains(channel))
        .collect();
    <[f32; 3]>::try_from(parts).map_err(|_| {
        format!("`--color {raw}` is not red, green and blue from 0 to 1 --- grey is `--color 0.5,0.5,0.5`")
    })
}

/// Reads the arguments after `page-numbers` or `watermark`.
///
/// # Errors
///
/// The sentence for exit code 2.
pub fn parse(args: &[String], watermark: bool) -> Result<AddText, String> {
    let name = if watermark {
        "watermark"
    } else {
        "page-numbers"
    };
    let mut added = if watermark {
        AddedText::watermark("")
    } else {
        AddedText::page_numbers()
    };
    let mut text = None;
    let mut paths = Vec::new();
    let mut output = None;
    let (mut password_env, mut invalidate, mut force, mut json) = (None, false, false, false);
    let mut positional = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if positional {
            paths.push(PathBuf::from(arg));
            continue;
        }
        match arg.as_str() {
            "--" => positional = true,
            "-o" | "--output" => output = Some(PathBuf::from(value(arg, &mut rest)?)),
            "--text" => text = Some(value(arg, &mut rest)?.clone()),
            "--position" => {
                let raw = value(arg, &mut rest)?;
                added.position = Position::named(raw).ok_or_else(|| {
                    let names: Vec<&str> = Position::ALL.iter().map(|p| p.name()).collect();
                    format!("`--position {raw}`: the positions are {}", names.join(", "))
                })?;
            }
            "--size" => added.size = number(arg, value(arg, &mut rest)?)?,
            "--margin" => added.margin = number(arg, value(arg, &mut rest)?)?,
            "--angle" if watermark => added.angle = number(arg, value(arg, &mut rest)?)?,
            "--opacity" if watermark => added.opacity = number(arg, value(arg, &mut rest)?)?,
            "--start" if !watermark => {
                let raw = value(arg, &mut rest)?;
                added.first_number = raw
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| format!("`--start {raw}` is not a whole number from 0"))?;
            }
            "--color" => added.color = color(value(arg, &mut rest)?)?,
            "--pages" => {
                let pages = page_list(value(arg, &mut rest)?)?;
                added.pages = Some(pages.into_iter().map(|p| p - 1).collect());
            }
            "--password-env" => password_env = Some(variable(value(arg, &mut rest)?)?),
            "--invalidate-signatures" => invalidate = true,
            "--force" => force = true,
            "--json" => json = true,
            flag if flag.starts_with('-') && flag != "-" => return Err(unknown(name, flag)),
            path => paths.push(path.into()),
        }
    }
    if paths.len() != 1 {
        return Err(format!("{name} needs exactly one input document"));
    }
    let input = paths.remove(0);
    let output = output.ok_or("-o <out.pdf> is required; the input is never overwritten")?;
    if lexically_same(&input, &output) {
        return Err("the output names the input; choose a different name".into());
    }
    match text {
        Some(text) => added.text = text,
        None if watermark => {
            return Err("watermark needs --text, the words to write across the pages".into())
        }
        None => {}
    }
    addedtext::check(&added)?;
    Ok(AddText {
        watermark,
        input,
        output,
        added,
        password_env,
        invalidate,
        force,
        json,
    })
}

/// Characters as a reader's search sees them: whitespace collapsed to one space.
fn collapsed(text: impl Iterator<Item = char>) -> String {
    let mut out = String::new();
    for ch in text {
        if ch.is_whitespace() {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }
    out.trim_end().to_string()
}

impl AddText {
    fn run_add(&self, env: &Env<'_>, out: &mut dyn Write) -> Result<Exit, Failure> {
        let inputs = std::slice::from_ref(&self.input);
        check_target(inputs, &self.output, self.force)?;
        let key = password(self.password_env.as_deref())?;
        let (mut input, session) = read_input(env, &self.input, key.as_deref())?;
        drop(session);
        if input.signed.is_some() && !self.invalidate {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{} is signed or its signatures could not be fully read; writing text on its \
                     pages rewrites the document and requires --invalidate-signatures",
                    self.input.display()
                ),
            ));
        }
        let count = input.plan.baseline;
        if let Some(past) = self
            .added
            .pages
            .as_ref()
            .and_then(|p| p.iter().find(|p| **p >= count))
        {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{}: --pages names page {}, and the document has {count}",
                    self.input.display(),
                    past + 1
                ),
            ));
        }
        // What each page written to must read back as, page by page.
        let chosen: Vec<u32> = match &self.added.pages {
            Some(pages) => {
                let mut pages = pages.clone();
                pages.sort_unstable();
                pages.dedup();
                pages
            }
            None => (0..count).collect(),
        };
        let total =
            self.added.first_number.saturating_sub(1) + u32::try_from(chosen.len()).unwrap_or(0);
        let expected: Vec<(u32, String)> = chosen
            .iter()
            .zip(self.added.first_number..)
            .map(|(page, n)| {
                let line = addedtext::filled(&self.added.text, n, total);
                (*page, collapsed(line.chars()))
            })
            .collect();
        input.plan.added_text = vec![self.added.clone()];

        let unpublished = |what: String| {
            Failure::new(
                Exit::Internal,
                format!("the staged file {what}; no output was published"),
            )
        };
        publish_copy(
            inputs,
            &[(self.input.as_path(), input.opened_as())],
            &self.output,
            self.force,
            copy_of(env, &self.input, &input.plan, key.as_deref()),
            |staged, ()| {
                let (after, mut session) =
                    read_input(env, staged, key.as_deref()).map_err(|why| {
                        unpublished(format!("could not be opened again: {}", why.message))
                    })?;
                if !same_sizes(&after.sizes, &input.sizes) {
                    return Err(unpublished("does not have the source's pages".into()));
                }
                for (page, line) in &expected {
                    let read = match session.ask(Request::Text {
                        page: *page,
                        crop: None,
                    }) {
                        Ok(Reply::Text(text)) => {
                            collapsed(text.codes.iter().filter_map(|c| char::from_u32(*c)))
                        }
                        _ => {
                            return Err(unpublished(format!("page {} could not be read", page + 1)))
                        }
                    };
                    if !read.contains(line.as_str()) {
                        return Err(unpublished(format!(
                            "does not read {line:?} on page {}",
                            page + 1
                        )));
                    }
                }
                Ok(())
            },
        )?;

        let report = report::TextAdded {
            schema: SCHEMA,
            command: self.name().into(),
            input: self.input.display().to_string(),
            output: self.output.display().to_string(),
            pages: count,
            written: expected.iter().map(|(page, _)| page + 1).collect(),
            text: self.added.text.clone(),
            position: self.added.position,
            first: expected
                .first()
                .map(|(_, line)| line.clone())
                .unwrap_or_default(),
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
#[must_use]
pub fn plain(report: &report::TextAdded) -> String {
    let pages = report.written.len();
    let mut lines = vec![format!(
        "{}: {:?} at the {} of {} page{}, the first reading {:?}",
        report.output,
        report.text,
        report.position.name().replace('-', " "),
        pages,
        if pages == 1 { "" } else { "s" },
        report.first
    )];
    if report.signatures_invalidated > 0 || report.signatures_unknown {
        lines.push("The rewrite invalidates existing signatures.".into());
    }
    lines.join("\n")
}

impl Subcommand for AddText {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        self.run_add(env, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn page_numbers_take_the_defaults_and_any_option() {
        let parsed = parse(&args("a.pdf -o b.pdf"), false).expect("defaults");
        assert_eq!(parsed.added, AddedText::page_numbers());
        let parsed = parse(
            &args("a.pdf -o b.pdf --position top-right --size 9 --start 3 --pages 2-4 --margin 20 --color 0,0,1 --json"),
            false,
        )
        .expect("options");
        assert_eq!(parsed.added.position, Position::TopRight);
        assert_eq!(
            (
                parsed.added.size,
                parsed.added.first_number,
                parsed.added.margin
            ),
            (9.0, 3, 20.0)
        );
        assert_eq!(parsed.added.pages, Some(vec![1, 2, 3]));
        assert_eq!(parsed.added.color, [0.0, 0.0, 1.0]);
        assert!(parsed.json && !parsed.watermark);
    }

    #[test]
    fn a_watermark_needs_its_text_and_takes_an_angle_and_an_opacity() {
        let why = parse(&args("a.pdf -o b.pdf"), true).expect_err("no text");
        assert!(why.contains("needs --text"), "{why}");
        let parsed = parse(
            &args("a.pdf -o b.pdf --text DRAFT --angle 30 --opacity 0.5"),
            true,
        )
        .expect("ok");
        assert_eq!((parsed.added.angle, parsed.added.opacity), (30.0, 0.5));
        assert_eq!(parsed.added.text, "DRAFT");
        assert_eq!(parsed.name(), "watermark");
    }

    #[test]
    fn what_cannot_be_written_is_a_usage_error() {
        for (line, watermark, said) in [
            ("a.pdf -o a.pdf", false, "names the input"),
            ("a.pdf", false, "-o <out.pdf> is required"),
            (
                "a.pdf -o b.pdf --position middle",
                false,
                "the positions are top-left",
            ),
            ("a.pdf -o b.pdf --size 1", false, "font size"),
            ("a.pdf -o b.pdf --angle 30", false, "--angle"),
            ("a.pdf -o b.pdf --text DRAFT --start 2", true, "--start"),
            (
                "a.pdf -o b.pdf --text {seite}",
                false,
                "names the field {seite}",
            ),
            ("a.pdf -o b.pdf --color 2,0,0", false, "--color 2,0,0"),
        ] {
            let why = parse(&args(line), watermark).expect_err(line);
            assert!(why.contains(said), "{line}: {why}");
        }
    }

    #[test]
    fn collapsing_matches_what_a_search_reads() {
        assert_eq!(collapsed("Page 1\r\n of  3 ".chars()), "Page 1 of 3");
    }
}
