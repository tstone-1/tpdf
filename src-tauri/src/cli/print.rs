//! `print`: a document, or some of its pages, to a printer named on the
//! command line, with no dialog.
//!
//! The job is the window's: the whole file byte for byte, or the pages listed,
//! written by a sandboxed worker (`save::print_range_bytes`), as *Print* with a
//! page range builds it. It is read back by the platform's own PDF parser, or
//! by PDFium in a worker on Linux, and its pages counted before anything goes
//! to the printer, as the window does before it shows the dialog. Then
//! `printer::send` hands it to the named printer.

use std::io::Write;
use std::path::PathBuf;

use super::args::{unknown, value};
use super::report::{self, SCHEMA};
use super::text::page_range;
use super::{json, say, Env, Exit, Failure, Registered, Subcommand};
use crate::save_outside::Declined;
use crate::{print, printer, save};

pub const COMMAND: Registered = Registered {
    name: "print",
    usage: "print <in.pdf> --printer NAME [--pages LIST] [--json]",
    summary: "Prints a document, or the pages listed, to the named printer, with\n            no dialog. The job is read back before it is sent.",
    parse: |args| parse(args).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

#[derive(Debug)]
struct Print {
    input: PathBuf,
    printer: String,
    pages: Option<Vec<u32>>,
    json: bool,
}

fn ordered_pages(raw: &str) -> Result<Vec<u32>, String> {
    let mut pages = Vec::new();
    for part in raw.split(',') {
        let range = page_range(raw, part)?;
        if pages.len() + (*range.end() - *range.start()) as usize + 1 > 100_000 {
            return Err("--pages selects too many pages; at most 100000 per command".into());
        }
        pages.extend(range);
    }
    Ok(pages)
}

fn parse(args: &[String]) -> Result<Print, String> {
    let mut command = Print {
        input: PathBuf::new(),
        printer: String::new(),
        pages: None,
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
            "--printer" => command.printer = value(arg, &mut rest)?.to_string(),
            "--pages" => command.pages = Some(ordered_pages(value(arg, &mut rest)?)?),
            "--json" => command.json = true,
            flag if flag.starts_with('-') => return Err(unknown("print", flag)),
            path => paths.push(path.into()),
        }
    }
    if paths.len() != 1 {
        return Err("print needs exactly one document".into());
    }
    if command.printer.trim().is_empty() {
        return Err("print needs --printer NAME; there is no dialog to choose one in".into());
    }
    command.input = paths.remove(0);
    Ok(command)
}

fn range_job(pages: &[u32]) -> print::Job {
    print::Job {
        pages: print::select(None, Some(pages.to_vec())),
        turns: 0,
    }
}

impl Subcommand for Print {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let refused = |e: String| Failure::new(Exit::Refused, e);
        let bytes = match &self.pages {
            // The file as it is, as the window prints a document it has not
            // changed: rewriting it to change nothing is only risk.
            None => std::fs::read(&self.input)
                .map_err(|e| refused(format!("could not read {}: {e}", self.input.display())))?,
            Some(pages) => {
                let job = range_job(pages);
                save::print_range_bytes(&self.input, &job, &env.worker())
                    .map_err(super::pages::refused)?
            }
        };
        let found = checked_pages(read_back(env, &bytes), self.pages.as_ref().map(Vec::len))?;
        let title = self.input.file_name().map_or_else(
            || "Document".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let pages = u32::try_from(found).unwrap_or(u32::MAX);
        let sent = printer::send(&self.printer, &bytes, &title, pages, &env.library_dir)
            .map_err(refused)?;
        let report = report::Printed {
            schema: SCHEMA,
            command: "print".into(),
            input: self.input.display().to_string(),
            printer: self.printer.clone(),
            pages,
            job: sent.job,
        };
        if self.json {
            json(out, &report);
        } else {
            let job = report
                .job
                .map_or_else(String::new, |job| format!(" as job {job}"));
            say(
                out,
                &format!(
                    "Sent {} page{} of {} to {}{job}",
                    report.pages,
                    if report.pages == 1 { "" } else { "s" },
                    report.input,
                    report.printer
                ),
            );
        }
        Ok(Exit::Ok)
    }
}

/// An invalid input is a refusal. Once the writer has built a range, a
/// refusal by the independent reader means the generated output failed.
fn checked_pages(
    reading: Result<usize, Declined>,
    expected: Option<usize>,
) -> Result<usize, Failure> {
    let exit = if expected.is_some() {
        Exit::Internal
    } else {
        Exit::Refused
    };
    let found = reading.map_err(|why| match why {
        Declined::Failed(_) => Failure::from(why),
        _ => Failure::new(exit, why.message()),
    })?;
    print::expect_pages(found, expected).map_err(|why| Failure::new(exit, why))?;
    Ok(found)
}

/// How many pages the platform's reader finds in the job: PDFKit on macOS,
/// `Windows.Data.Pdf` on Windows, PDFium in a worker on Linux.
fn read_back(env: &Env<'_>, bytes: &[u8]) -> Result<usize, Declined> {
    #[cfg(any(windows, target_os = "macos"))]
    let unread = || {
        Declined::Refused("the print job could not be read back, so it will not be printed".into())
    };
    #[cfg(target_os = "macos")]
    {
        let _ = env;
        crate::print_macos::read(bytes)
            .map(|reading| reading.pages.len())
            .ok_or_else(unread)
    }
    #[cfg(windows)]
    {
        let _ = env;
        crate::print_win::read(bytes)
            .map(|reading| reading.pages.len())
            .ok_or_else(unread)
    }
    #[cfg(target_os = "linux")]
    {
        let (path, file) = crate::print_linux::stage(bytes).map_err(Declined::Failed)?;
        let counted = env.worker().printed_pages(&file, bytes.len());
        drop(file);
        let _ = std::fs::remove_file(&path);
        counted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn print_refuses_unreadable_input_and_reports_worker_or_writer_failure() {
        assert_eq!(
            checked_pages(Err(Declined::Refused("unreadable".into())), None)
                .unwrap_err()
                .exit,
            Exit::Refused
        );
        assert_eq!(
            checked_pages(Err(Declined::Refused("unreadable".into())), Some(2))
                .unwrap_err()
                .exit,
            Exit::Internal
        );
        assert_eq!(
            checked_pages(Err(Declined::Failed("timeout".into())), None)
                .unwrap_err()
                .exit,
            Exit::Internal
        );
        assert_eq!(
            checked_pages(Ok(1), Some(2)).unwrap_err().exit,
            Exit::Internal
        );
        assert_eq!(checked_pages(Ok(2), Some(2)).unwrap(), 2);
    }

    #[test]
    fn print_range_keeps_page_identity_order_and_repetitions() {
        let args = ["doc.pdf", "--printer", "Office", "--pages", "2,1-2"].map(String::from);
        let command = parse(&args).unwrap();
        let (mut doc, _, _) = crate::forms::tests::fixture();
        for (page, id) in doc.get_pages() {
            doc.get_dictionary_mut(id)
                .unwrap()
                .set("TestPageNumber", i64::from(page));
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let job = range_job(command.pages.as_ref().unwrap());
        let output = print::build_update(&bytes, &job).unwrap();
        let written = lopdf::Document::load_mem(&output).unwrap();
        let labels: Vec<_> = written
            .get_pages()
            .values()
            .map(|id| {
                written
                    .get_dictionary(*id)
                    .unwrap()
                    .get(b"TestPageNumber")
                    .unwrap()
                    .as_i64()
                    .unwrap()
            })
            .collect();
        assert_eq!(labels, [2, 1, 2]);
        assert!(ordered_pages("1-100000,1").is_err());
    }
}
