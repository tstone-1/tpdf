//! `tpdf ocr`: a copy of a scanned document that can be searched and selected.
//!
//! Each selected page that has no text of its own is rendered by the sandboxed
//! worker, read by the operating system's recogniser in the OCR worker, and
//! the words are written into the copy as an invisible layer
//! (`textlayer.rs`). A page that already has text is left as it is.
//!
//! The copy is staged beside its destination and read back before it is
//! published: the page sizes and the encryption must be the source's, and
//! every page given a layer must read back with the characters the layer was
//! written from. A failure at any point publishes nothing.

use std::io::Write;
use std::path::PathBuf;

use super::args::{lexically_same, unknown, value};
use super::fill::SignedState;
use super::pages::{check_target, copy_of, publish_copy, read_input, same_sizes};
use super::report::{self, SCHEMA};
use super::text::{declined, page_list, password, variable};
use super::{json, say, Env, Exit, Failure, Registered, Subcommand};
use crate::ocr::Pixels;
use crate::ocr_layer;
use crate::ocr_worker::{OcrWorker, PIXELS_CAPACITY};
use crate::render::{PageSize, TileFormat, TileRequest};
use crate::save_outside::Session;
use crate::textlayer::Layer;
use crate::worker_proto::{Reply, Request};

pub const COMMAND: Registered = Registered {
    name: "ocr",
    usage: "ocr <in.pdf> -o <out.pdf> [--pages 1-3,7] [--language TAG]...\n        [--password-env VAR] [--invalidate-signatures] [--force] [--json]",
    summary: "Writes a copy in which scanned pages can be searched and selected:\n            pages without text are read by the system's text recogniser and\n            the words are added as an invisible layer. Pages that already\n            have text are left as they are. --language takes a BCP-47 tag such\n            as de-DE, most preferred first.",
    parse: |args| parse(args).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

/// The side of one render tile, in pixels. `tpdf render`'s.
const TILE: u32 = 1024;

#[derive(Debug)]
struct Ocr {
    input: PathBuf,
    output: PathBuf,
    pages: Option<Vec<u32>>,
    languages: Vec<String>,
    password_env: Option<String>,
    invalidate: bool,
    force: bool,
    json: bool,
}

fn parse(args: &[String]) -> Result<Ocr, String> {
    let mut command = Ocr {
        input: PathBuf::new(),
        output: PathBuf::new(),
        pages: None,
        languages: Vec::new(),
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
            "--pages" => command.pages = Some(page_list(value(arg, &mut rest)?)?),
            "--language" => command.languages.push(language(value(arg, &mut rest)?)?),
            "--password-env" => command.password_env = Some(variable(value(arg, &mut rest)?)?),
            "--invalidate-signatures" => command.invalidate = true,
            "--force" => command.force = true,
            "--json" => command.json = true,
            flag if flag.starts_with('-') => return Err(unknown("ocr", flag)),
            path => paths.push(path.into()),
        }
    }
    if paths.len() != 1 {
        return Err("ocr needs exactly one input document".into());
    }
    command.input = paths.remove(0);
    command.output = output.ok_or("-o <out.pdf> is required; the input is never overwritten")?;
    if lexically_same(&command.input, &command.output) {
        return Err("the output names the input; choose a different name".into());
    }
    Ok(command)
}

/// A language tag as the engines take it: letters, digits and hyphens.
fn language(raw: &str) -> Result<String, String> {
    let plausible = (2..=35).contains(&raw.len())
        && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && raw.starts_with(|c: char| c.is_ascii_alphabetic());
    if plausible {
        Ok(raw.to_string())
    } else {
        Err(format!(
            "`--language {raw}` is not a language tag --- give one such as en-US or de-DE"
        ))
    }
}

/// One page as raw RGBA, rendered by the worker in tiles.
fn render(
    session: &mut Session,
    page: u32,
    width: u32,
    height: u32,
    scale: f32,
) -> Result<Vec<u8>, Failure> {
    let mut pixels = vec![0; width as usize * height as usize * 4];
    for y in (0..height).step_by(TILE as usize) {
        for x in (0..width).step_by(TILE as usize) {
            let tw = (width - x).min(TILE) as u16;
            let th = (height - y).min(TILE) as u16;
            let tile = session.tile(TileRequest {
                rid: 0,
                doc: 0,
                page,
                scale,
                turns: 0,
                invert: false,
                crop: None,
                x: x as i32,
                y: y as i32,
                width: tw,
                height: th,
                format: TileFormat::Raw,
            })?;
            let stride = usize::from(tw) * 4;
            if tile.len() != stride * usize::from(th) {
                return Err(Failure::new(
                    Exit::Internal,
                    format!(
                        "the worker returned a tile of the wrong size for page {}",
                        page + 1
                    ),
                ));
            }
            for row in 0..usize::from(th) {
                let start = ((y as usize + row) * width as usize + x as usize) * 4;
                pixels[start..start + stride]
                    .copy_from_slice(&tile[row * stride..(row + 1) * stride]);
            }
        }
    }
    Ok(pixels)
}

/// What became of the selected pages.
#[derive(Default)]
struct Read {
    layers: Vec<Layer>,
    already_text: Vec<u32>,
    nothing_read: Vec<u32>,
    engine: Option<String>,
}

impl Ocr {
    /// Recognises every selected page that has no text of its own.
    fn read(
        &self,
        session: &mut Session,
        sizes: &[PageSize],
        selection: &[u32],
        has_password: bool,
    ) -> Result<Read, Failure> {
        let shown = self.input.display().to_string();
        let options = ocr_layer::options(self.languages.clone());
        let mut engine: Option<OcrWorker> = None;
        let mut read = Read::default();
        for n in selection {
            let page = n - 1;
            let reply = session
                .ask(Request::Text { page, crop: None })
                .map_err(|why| declined(&shown, why, has_password))?;
            let Reply::Text(text) = reply else {
                return Err(Failure::new(
                    Exit::Internal,
                    format!("unexpected worker reply: {reply:?}"),
                ));
            };
            if ocr_layer::has_text(&text) {
                read.already_text.push(*n);
                continue;
            }
            let size = sizes[page as usize];
            let (width, height, scale) =
                ocr_layer::render_size(size.width_pt, size.height_pt, PIXELS_CAPACITY).ok_or_else(
                    || {
                        Failure::new(
                            Exit::Refused,
                            format!(
                                "page {n} is {:.0} x {:.0} pt, too large to read at a resolution \
                                 text can be recognised at; use --pages to leave it out",
                                size.width_pt, size.height_pt
                            ),
                        )
                    },
                )?;
            let rgba = render(session, page, width, height, scale)?;
            // Spawned at the first page that needs it, so a document whose
            // pages all have text never starts an engine.
            let worker = match &mut engine {
                Some(worker) => worker,
                None => engine.insert(OcrWorker::spawn().map_err(|why| {
                    Failure::new(Exit::Refused, format!("the text could not be read. {why}"))
                })?),
            };
            let pixels = Pixels {
                rgba: &rgba,
                width,
                height,
                scale,
            };
            let (id, items) = worker.recognise(pixels, &options).map_err(|why| {
                Failure::new(Exit::Internal, format!("page {n} could not be read: {why}"))
            })?;
            read.engine.get_or_insert_with(|| id.to_string());
            match ocr_layer::layer_of(page, items) {
                Some(layer) => read.layers.push(layer),
                None => read.nothing_read.push(*n),
            }
        }
        Ok(read)
    }

    fn run_ocr(&self, env: &Env<'_>, out: &mut dyn Write) -> Result<Exit, Failure> {
        let inputs = std::slice::from_ref(&self.input);
        check_target(inputs, &self.output, self.force)?;
        let key = password(self.password_env.as_deref())?;
        let (mut input, mut session) = read_input(env, &self.input, key.as_deref())?;
        if input.signed.is_some() && !self.invalidate {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{} is signed or its signatures could not be fully read; adding a text layer \
                     rewrites the document and requires --invalidate-signatures",
                    self.input.display()
                ),
            ));
        }
        let selection = self
            .pages
            .clone()
            .unwrap_or_else(|| (1..=input.plan.baseline).collect());
        if selection.iter().any(|n| *n > input.plan.baseline) {
            return Err(Failure::new(
                Exit::Refused,
                "--pages names a page past the end of the document",
            ));
        }

        let read = self.read(&mut session, &input.sizes, &selection, key.is_some())?;
        drop(session);
        if read.layers.is_empty() {
            return Err(Failure::new(
                Exit::Refused,
                match (read.already_text.is_empty(), read.nothing_read.is_empty()) {
                    (false, true) => "every selected page already has text, so there is nothing \
                                      to add; no copy was written"
                        .to_string(),
                    (true, false) => "no text was recognised on any selected page; no copy was \
                                      written"
                        .to_string(),
                    _ => "the selected pages either have text already or none was recognised on \
                          them; no copy was written"
                        .to_string(),
                },
            ));
        }
        input.plan.text_layers = read.layers.clone();

        // The staged file, in a fresh worker: the same pages, the same
        // encryption, and on every page given a layer the characters the layer
        // was written from.
        let checking = |why: Failure| {
            Failure::new(
                Exit::Internal,
                format!("the staged file could not be checked: {}", why.message),
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
                    read_input(env, staged, key.as_deref()).map_err(checking)?;
                if after.encrypted != input.encrypted || !same_sizes(&after.sizes, &input.sizes) {
                    return Err(Failure::new(
                        Exit::Internal,
                        "the staged file's pages or encryption did not match the source; no \
                         output was published",
                    ));
                }
                for layer in &read.layers {
                    let reply = session
                        .ask(Request::Text {
                            page: layer.page,
                            crop: None,
                        })
                        .map_err(|why| checking(why.into()))?;
                    let back =
                        matches!(&reply, Reply::Text(text) if ocr_layer::reads_back(layer, text));
                    if !back {
                        return Err(Failure::new(
                            Exit::Internal,
                            format!(
                                "page {} of the staged file did not read back with the text \
                                 that was recognised; no output was published",
                                layer.page + 1
                            ),
                        ));
                    }
                }
                Ok(())
            },
        )?;

        let report = report::Ocr {
            schema: SCHEMA,
            command: "ocr".into(),
            input: self.input.display().to_string(),
            output: self.output.display().to_string(),
            engine: read.engine.unwrap_or_default(),
            pages: read
                .layers
                .iter()
                .map(|layer| report::OcrPage {
                    page: layer.page + 1,
                    words: layer.words.len(),
                })
                .collect(),
            already_text: read.already_text,
            nothing_read: read.nothing_read,
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
fn plain(report: &report::Ocr) -> String {
    let words: usize = report.pages.iter().map(|page| page.words).sum();
    let mut lines = vec![format!(
        "{}: text added to {} of {} pages, {words} words",
        report.output,
        report.pages.len(),
        report.pages.len() + report.already_text.len() + report.nothing_read.len(),
    )];
    if !report.already_text.is_empty() {
        lines.push(format!(
            "Already had text and were left as they are: {}",
            numbers(&report.already_text)
        ));
    }
    if !report.nothing_read.is_empty() {
        lines.push(format!(
            "No text was recognised on: {}",
            numbers(&report.nothing_read)
        ));
    }
    if report.signatures_invalidated > 0 || report.signatures_unknown {
        lines.push("The rewrite invalidates existing signatures.".into());
    }
    lines.join("\n")
}

fn numbers(pages: &[u32]) -> String {
    let shown: Vec<String> = pages.iter().map(u32::to_string).collect();
    format!(
        "{} {}",
        if pages.len() == 1 { "page" } else { "pages" },
        shown.join(", ")
    )
}

impl Subcommand for Ocr {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        self.run_ocr(env, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn the_parser_needs_one_input_and_an_output_that_is_not_it() {
        for line in [
            "",
            "a.pdf",
            "a.pdf b.pdf -o c.pdf",
            "a.pdf -o a.pdf",
            "a.pdf -o b.pdf --pages 0",
            "a.pdf -o b.pdf --language",
            "a.pdf -o b.pdf --language x",
            "a.pdf -o b.pdf --language de_DE",
            "a.pdf -o b.pdf --language -de",
            "a.pdf -o b.pdf --unknown",
        ] {
            assert!(parse(&args(line)).is_err(), "{line}");
        }
    }

    #[test]
    fn languages_are_kept_in_the_order_given() {
        let command = parse(&args(
            "a.pdf -o b.pdf --language de-DE --language en-US --pages 2-3",
        ))
        .unwrap();
        assert_eq!(command.languages, ["de-DE", "en-US"]);
        assert_eq!(command.pages, Some(vec![2, 3]));
        assert!(!command.invalidate && !command.force && !command.json);
    }

    #[test]
    fn a_name_starting_with_a_hyphen_is_an_input_after_two_hyphens() {
        let command = parse(&["-o", "b.pdf", "--", "-in.pdf"].map(str::to_owned)).unwrap();
        assert_eq!(command.input, PathBuf::from("-in.pdf"));
    }

    #[test]
    fn the_sentences_name_what_was_left_alone_and_why() {
        let report = report::Ocr {
            schema: SCHEMA,
            command: "ocr".into(),
            input: "in.pdf".into(),
            output: "out.pdf".into(),
            engine: "vision (26A428)".into(),
            pages: vec![
                report::OcrPage { page: 1, words: 40 },
                report::OcrPage { page: 4, words: 2 },
            ],
            already_text: vec![2],
            nothing_read: vec![3, 5],
            signatures_invalidated: 1,
            signatures_unknown: false,
        };
        assert_eq!(
            plain(&report),
            "out.pdf: text added to 2 of 5 pages, 42 words\n\
             Already had text and were left as they are: page 2\n\
             No text was recognised on: pages 3, 5\n\
             The rewrite invalidates existing signatures."
        );
    }
}
