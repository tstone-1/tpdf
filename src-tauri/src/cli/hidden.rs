//! `tpdf hidden <file.pdf> [--pages 1-3,7] [--json]`: text that is in a
//! document and not on its pages.
//!
//! The check for a document somebody else redacted. For each page the
//! sandboxed worker is asked two things, where the characters are and what the
//! page looks like, and `hidden.rs` compares the answers: a character whose
//! place on the rendered page shows no trace of it is in the file and not
//! visible. Black rectangles drawn over words, annotations lying over them,
//! words in the background's colour and words in a text mode that paints
//! nothing all come out the same way.
//!
//! **The answer is one-sided, and the wording keeps it so.** What is listed is
//! in the file. When nothing is listed, nothing was *found*: a page without
//! text was not compared at all, and `hidden.rs` names what else the
//! comparison cannot see. The report carries the pages that were not checked
//! and the characters that could not be judged, and the plain output says them
//! in the same sentence as the result.
//!
//! Exit code 1 when anything was found, so a script can refuse to publish.

use std::io::Write;
use std::path::PathBuf;

use super::args::{unknown, value};
use super::ocr::render;
use super::report::{self, SCHEMA};
use super::text::{declined, page_list, password, variable};
use super::{json, opened, say, Env, Exit, Failure, Registered, Subcommand};
use crate::hidden::survey::{self, Pages};
use crate::save_outside::Session;
use crate::text::PageText;
use crate::worker_proto::{Reply, Request};

pub const COMMAND: Registered = Registered {
    name: "hidden",
    usage: "hidden <file.pdf> [--pages 1-3,7] [--password-env VAR] [--json]",
    summary: "Lists text that is in the document and not visible on its pages:\n            words under a black box, in the background's colour, or never painted.\n            Exits 1 when it finds any.",
    parse: |args| parse(args).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

#[derive(Debug)]
struct Hidden {
    input: PathBuf,
    pages: Option<Vec<u32>>,
    password_env: Option<String>,
    json: bool,
}

fn parse(args: &[String]) -> Result<Hidden, String> {
    let mut command = Hidden {
        input: PathBuf::new(),
        pages: None,
        password_env: None,
        json: false,
    };
    let mut paths = Vec::new();
    let mut rest = args.iter();
    let mut positional = false;
    while let Some(arg) = rest.next() {
        if positional {
            paths.push(PathBuf::from(arg));
            continue;
        }
        match arg.as_str() {
            "--" => positional = true,
            "--pages" => command.pages = Some(page_list(value(arg, &mut rest)?)?),
            "--password-env" => command.password_env = Some(variable(value(arg, &mut rest)?)?),
            "--json" => command.json = true,
            flag if flag.starts_with('-') => return Err(unknown("hidden", flag)),
            path => paths.push(path.into()),
        }
    }
    if paths.len() != 1 {
        return Err("hidden needs exactly one document".into());
    }
    command.input = paths.remove(0);
    Ok(command)
}

/// The pages of the document the tool opened, answered by its worker.
struct Opened<'a> {
    session: &'a mut Session,
    /// The document as it was named, for a refusal.
    shown: &'a str,
    has_password: bool,
}

impl Pages for Opened<'_> {
    type Error = Failure;

    fn text(&mut self, page: u32) -> Result<PageText, Failure> {
        let reply = self
            .session
            .ask(Request::Text { page, crop: None })
            .map_err(|why| declined(self.shown, why, self.has_password))?;
        let Reply::Text(text) = reply else {
            return Err(Failure::new(
                Exit::Internal,
                format!("unexpected worker reply: {reply:?}"),
            ));
        };
        Ok(text)
    }

    fn pixels(
        &mut self,
        page: u32,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<Vec<u8>, Failure> {
        render(self.session, page, width, height, scale)
    }
}

impl Subcommand for Hidden {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let shown = self.input.display().to_string();
        let key = password(self.password_env.as_deref())?;
        let (file, len) = opened(&self.input).map_err(|e| Failure::new(Exit::Refused, e))?;
        let mut session = env
            .worker()
            .session(&file, len, key.as_deref())
            .map_err(|e| declined(&shown, e, key.is_some()))?;
        // Without every page's size: a page's text says how large the page is.
        let reply = session.ask(Request::Open {
            lazy_geometry: true,
        })?;
        let Reply::Open { page_count, .. } = reply else {
            return Err(Failure::new(
                Exit::Internal,
                "the worker did not say how many pages the document has",
            ));
        };
        let count = u32::try_from(page_count).unwrap_or(u32::MAX);
        let selection = self.pages.clone().unwrap_or_else(|| (1..=count).collect());
        if selection.iter().any(|n| *n > count) {
            return Err(Failure::new(
                Exit::Refused,
                "--pages names a page past the end of the document",
            ));
        }

        let walked = survey::survey(
            &mut Opened {
                session: &mut session,
                shown: &shown,
                has_password: key.is_some(),
            },
            &selection,
            &mut |_, _| Ok(()),
        )?;
        drop(session);

        let last = survey::summary(&walked);
        let report = report::Hidden {
            schema: SCHEMA,
            command: "hidden".into(),
            input: shown,
            pages: count,
            found: walked
                .found
                .into_iter()
                .map(|found| report::HiddenText {
                    page: found.page,
                    text: found.text,
                    rect: found.rect,
                    characters: found.characters,
                    off_page: found.off_page,
                })
                .collect(),
            compared: walked.compared,
            unjudged: walked.unjudged,
            without_text: walked.without_text,
            not_compared: walked.not_compared,
        };
        let exit = if report.found.is_empty() {
            Exit::Ok
        } else {
            Exit::Strict
        };
        if self.json {
            json(out, &report);
        } else {
            for found in &report.found {
                say(out, &line_of(found));
            }
            say(out, &last);
            say(out, survey::NOT_LOOKED_AT);
        }
        Ok(exit)
    }
}

/// One finding, as plain output lists it.
fn line_of(found: &report::HiddenText) -> String {
    if found.off_page {
        format!("page {}, outside the page: {}", found.page, found.text)
    } else {
        format!("page {}: {}", found.page, found.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn the_parser_takes_one_document_and_a_page_list() {
        for line in [
            "",
            "a.pdf b.pdf",
            "a.pdf --pages 0",
            "a.pdf --pages 3-1",
            "a.pdf --unknown",
            "a.pdf -o b.pdf",
        ] {
            assert!(parse(&argv(line)).is_err(), "{line}");
        }
        let command = parse(&argv("a.pdf --pages 2,5-6 --json --password-env KEY")).unwrap();
        assert_eq!(command.input, PathBuf::from("a.pdf"));
        assert_eq!(command.pages, Some(vec![2, 5, 6]));
        assert!(command.json);
        assert_eq!(command.password_env.as_deref(), Some("KEY"));
        let dashed = parse(&argv("-- -odd.pdf")).unwrap();
        assert_eq!(dashed.input, PathBuf::from("-odd.pdf"));
        assert_eq!((dashed.pages, dashed.json), (None, false));
    }

    fn passage(page: u32, off_page: bool) -> report::HiddenText {
        report::HiddenText {
            page,
            text: "Jane Example".into(),
            rect: [1.0, 2.0, 3.0, 4.0],
            characters: 11,
            off_page,
        }
    }

    #[test]
    fn a_passage_outside_the_page_is_listed_as_such() {
        assert_eq!(line_of(&passage(4, false)), "page 4: Jane Example");
        assert_eq!(
            line_of(&passage(4, true)),
            "page 4, outside the page: Jane Example"
        );
    }
}
