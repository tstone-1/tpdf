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
use super::text::page_list;
use super::{json, say, Env, Exit, Failure, Registered, Subcommand};
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
            "--pages" => command.pages = Some(page_list(value(arg, &mut rest)?)?),
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
                let job = print::Job {
                    pages: print::Pages::Only(
                        pages
                            .iter()
                            .map(|&number| print::PagePlan { number, turns: 0 })
                            .collect(),
                    ),
                    turns: 0,
                };
                save::print_range_bytes(&self.input, &job, &env.worker())
                    .map_err(|e| refused(e.message))?
            }
        };
        let found = read_back(env, &bytes)?;
        print::expect_pages(found, self.pages.as_ref().map(Vec::len)).map_err(refused)?;
        let title = self.input.file_name().map_or_else(
            || "Document".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let pages = u32::try_from(found).unwrap_or(u32::MAX);
        let sent = printer::send(&self.printer, &bytes, &title, pages).map_err(refused)?;
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

/// How many pages the platform's reader finds in the job: PDFKit on macOS,
/// `Windows.Data.Pdf` on Windows, PDFium in a worker on Linux.
fn read_back(env: &Env<'_>, bytes: &[u8]) -> Result<usize, Failure> {
    let unread = || {
        Failure::new(
            Exit::Refused,
            "the print job could not be read back, so it will not be printed",
        )
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
        let (path, file) =
            crate::print_linux::stage(bytes).map_err(|e| Failure::new(Exit::Internal, e))?;
        let counted = env.worker().printed_pages(&file, bytes.len());
        drop(file);
        let _ = std::fs::remove_file(&path);
        counted.map_err(|_| unread())
    }
}
