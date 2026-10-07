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
use crate::hidden;
use crate::render::PageSize;
use crate::worker_proto::{Reply, Request};

pub const COMMAND: Registered = Registered {
    name: "hidden",
    usage: "hidden <file.pdf> [--pages 1-3,7] [--password-env VAR] [--json]",
    summary: "Lists text that is in the document and not visible on its pages:\n            words under a black box, in the background's colour, or never painted.\n            Exits 1 when it finds any.",
    parse: |args| parse(args).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

/// Pixels per point a page is compared at: 144 DPI, `tpdf render`'s default.
const WANTED_SCALE: f32 = 2.0;

/// The lowest scale a page is compared at. Below it ordinary body text is too
/// small to judge, and the page is reported as not checked.
const MIN_SCALE: f32 = 1.0;

/// `tpdf render`'s bounds on one image.
const MAX_SIDE: f32 = 8192.0;
const MAX_PIXELS: f32 = 16_777_216.0;

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

/// The image a page is compared at: its size in pixels and the scale that
/// gives it. `None` for a page with no size, or one so large that it cannot be
/// rendered at [`MIN_SCALE`] within `tpdf render`'s bounds.
fn image_of(size: PageSize) -> Option<(u32, u32, f32)> {
    let (w, h) = (size.width_pt, size.height_pt);
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return None;
    }
    let scale = WANTED_SCALE
        .min(MAX_SIDE / w.max(h))
        .min((MAX_PIXELS / (w * h)).sqrt());
    if scale < MIN_SCALE {
        return None;
    }
    // Rounded as `tpdf render` and the viewer round a page, and floored on
    // the scale side so the product stays inside the bound.
    let width = (w * scale).round().max(1.0) as u32;
    let height = (h * scale).round().max(1.0) as u32;
    Some((width, height, scale))
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
        let reply = session.ask(Request::Open {
            lazy_geometry: false,
        })?;
        let Reply::Open {
            pages, page_count, ..
        } = reply
        else {
            return Err(Failure::new(
                Exit::Internal,
                "the worker did not return page sizes",
            ));
        };
        if pages.len() != page_count {
            return Err(Failure::new(
                Exit::Internal,
                "the worker returned incomplete page sizes",
            ));
        }
        let count = u32::try_from(page_count).unwrap_or(u32::MAX);
        let selection = self.pages.clone().unwrap_or_else(|| (1..=count).collect());
        if selection.iter().any(|n| *n > count) {
            return Err(Failure::new(
                Exit::Refused,
                "--pages names a page past the end of the document",
            ));
        }

        let mut report = report::Hidden {
            schema: SCHEMA,
            command: "hidden".into(),
            input: shown.clone(),
            pages: count,
            found: Vec::new(),
            compared: 0,
            unjudged: 0,
            without_text: Vec::new(),
            not_compared: Vec::new(),
        };
        for n in &selection {
            let page = n - 1;
            let reply = session
                .ask(Request::Text { page, crop: None })
                .map_err(|why| declined(&shown, why, key.is_some()))?;
            let Reply::Text(text) = reply else {
                return Err(Failure::new(
                    Exit::Internal,
                    format!("unexpected worker reply: {reply:?}"),
                ));
            };
            if !crate::ocr_layer::has_text(&text) {
                report.without_text.push(*n);
                continue;
            }
            let Some((width, height, scale)) = image_of(pages[page as usize]) else {
                report.not_compared.push(*n);
                continue;
            };
            let pixels = render(&mut session, page, width, height, scale)?;
            let judged = hidden::judge(&text, &pixels, width as usize, height as usize, scale);
            report.compared += judged.judged as u64;
            report.unjudged += judged.unjudged as u64;
            report
                .found
                .extend(judged.found.into_iter().map(|found| report::HiddenText {
                    page: *n,
                    text: found.text,
                    rect: found.rect,
                    characters: found.hidden as u32,
                    off_page: found.off_page,
                }));
        }
        drop(session);

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
            say(out, &summary(&report, selection.len()));
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

/// The sentence a run ends on. What was not checked is in it, so the result
/// is never read without its limits.
fn summary(report: &report::Hidden, selected: usize) -> String {
    let mut line = if report.found.is_empty() {
        format!(
            "No hidden text found: {} characters on {} compared with the rendered page",
            report.compared,
            pages_of(selected - report.without_text.len() - report.not_compared.len()),
        )
    } else {
        let on: std::collections::BTreeSet<u32> = report.found.iter().map(|f| f.page).collect();
        format!(
            "{} in the file and not visible on the page, on {}",
            if report.found.len() == 1 {
                "1 passage is".to_string()
            } else {
                format!("{} passages are", report.found.len())
            },
            pages_of(on.len()),
        )
    };
    if report.unjudged > 0 {
        line.push_str(&format!(
            "; {} characters could not be judged",
            report.unjudged
        ));
    }
    if !report.without_text.is_empty() {
        line.push_str(&format!(
            "; {} without text not checked ({})",
            pages_of(report.without_text.len()),
            numbers(&report.without_text)
        ));
    }
    if !report.not_compared.is_empty() {
        line.push_str(&format!(
            "; {} too large to compare ({})",
            pages_of(report.not_compared.len()),
            numbers(&report.not_compared)
        ));
    }
    line.push('.');
    line
}

fn pages_of(count: usize) -> String {
    if count == 1 {
        "1 page".into()
    } else {
        format!("{count} pages")
    }
}

fn numbers(pages: &[u32]) -> String {
    pages
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
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

    #[test]
    fn a_page_is_compared_at_144_dpi_or_as_large_as_the_bounds_allow() {
        let size = |width_pt, height_pt| PageSize {
            width_pt,
            height_pt,
        };
        assert_eq!(image_of(size(595.0, 842.0)), Some((1190, 1684, 2.0)));
        // The longer side decides: 6000 points at 8192 pixels.
        let (width, height, scale) = image_of(size(6000.0, 100.0)).expect("it fits");
        assert!((width, height) == (8192, 137) && scale < 2.0 && scale > 1.0);
        // The area decides: 4000 points square is 16 million pixels at 1.024.
        let (width, height, scale) = image_of(size(4000.0, 4000.0)).expect("it fits");
        assert!(scale >= 1.0 && u64::from(width) * u64::from(height) <= 16_777_216);
        for (w, h) in [
            (9000.0, 100.0),
            (4200.0, 4200.0),
            (0.0, 10.0),
            (-1.0, 10.0),
            (f32::NAN, 10.0),
            (10.0, f32::INFINITY),
        ] {
            assert_eq!(image_of(size(w, h)), None, "{w} by {h}");
        }
    }

    fn report(found: Vec<report::HiddenText>) -> report::Hidden {
        report::Hidden {
            schema: SCHEMA,
            command: "hidden".into(),
            input: "a.pdf".into(),
            pages: 9,
            found,
            compared: 1200,
            unjudged: 0,
            without_text: Vec::new(),
            not_compared: Vec::new(),
        }
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
    fn the_last_line_says_what_was_found_and_what_was_not_checked() {
        assert_eq!(
            summary(&report(Vec::new()), 9),
            "No hidden text found: 1200 characters on 9 pages compared with the rendered page."
        );
        assert_eq!(
            summary(&report(vec![passage(2, false)]), 9),
            "1 passage is in the file and not visible on the page, on 1 page."
        );
        assert_eq!(
            summary(
                &report(vec![passage(2, false), passage(2, true), passage(7, false)]),
                9
            ),
            "3 passages are in the file and not visible on the page, on 2 pages."
        );
        // Nothing found is never said without what was left out.
        let mut partial = report(Vec::new());
        partial.unjudged = 40;
        partial.without_text = vec![3, 4];
        partial.not_compared = vec![9];
        assert_eq!(
            summary(&partial, 9),
            "No hidden text found: 1200 characters on 6 pages compared with the rendered page; \
             40 characters could not be judged; 2 pages without text not checked (3, 4); \
             1 page too large to compare (9)."
        );
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
