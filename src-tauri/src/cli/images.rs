//! `tpdf images`: a document made from pictures, one page each.
//!
//! The pictures are read here and decoded in a sandboxed worker
//! (`imagepages.rs`). The document is staged beside its destination and opened
//! in a fresh worker before it is published: it must have one page for each
//! picture.

use std::io::Write;
use std::path::PathBuf;

use super::args::{lexically_same, unknown, value};
use super::pages::{check_target, read_input, Temporary};
use super::report::{self, SCHEMA};
use super::{json, say, Env, Exit, Failure, Registered, Subcommand};
use crate::imagepages::{Options, Paper};
use crate::save;

pub const COMMAND: Registered = Registered {
    name: "images",
    usage: "images <picture>... -o <out.pdf> [--paper own|a4|letter] [--dpi N]\n        [--force] [--json]",
    summary: "Writes a document with one page for each PNG or JPEG picture, in the\n            order given. A page is the picture's own size unless --paper names\n            one; --dpi sets the resolution in place of the one each file states.\n            A photograph is turned the way its EXIF orientation says.",
    parse: |args| parse(args).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

#[derive(Debug)]
struct Images {
    inputs: Vec<PathBuf>,
    output: PathBuf,
    options: Options,
    force: bool,
    json: bool,
}

fn paper(raw: &str) -> Result<Paper, String> {
    match raw {
        "own" => Ok(Paper::Own),
        "a4" | "A4" => Ok(Paper::A4),
        "letter" | "Letter" => Ok(Paper::Letter),
        other => Err(format!(
            "`--paper {other}` is not a page tpdf knows --- give own, a4 or letter"
        )),
    }
}

fn parse(args: &[String]) -> Result<Images, String> {
    let mut command = Images {
        inputs: Vec::new(),
        output: PathBuf::new(),
        options: Options::default(),
        force: false,
        json: false,
    };
    let mut output = None;
    let mut rest = args.iter();
    let mut positional = false;
    while let Some(arg) = rest.next() {
        if positional {
            command.inputs.push(PathBuf::from(arg));
            continue;
        }
        match arg.as_str() {
            "--" => positional = true,
            "-o" | "--output" => output = Some(PathBuf::from(value(arg, &mut rest)?)),
            "--paper" => command.options.paper = paper(value(arg, &mut rest)?)?,
            "--dpi" => {
                let raw = value(arg, &mut rest)?;
                let dpi = raw
                    .parse::<u32>()
                    .map_err(|_| format!("`--dpi {raw}` is not a whole number"))?;
                command.options.dpi = Some(dpi);
            }
            "--force" => command.force = true,
            "--json" => command.json = true,
            flag if flag.starts_with('-') => return Err(unknown("images", flag)),
            path => command.inputs.push(path.into()),
        }
    }
    if command.inputs.is_empty() {
        return Err("images needs at least one picture".into());
    }
    command.output = output.ok_or("-o <out.pdf> is required")?;
    if command
        .inputs
        .iter()
        .any(|input| lexically_same(input, &command.output))
    {
        return Err("the output names one of the pictures; choose a different name".into());
    }
    command.options = command
        .options
        .checked()
        .map_err(|why| format!("--dpi: {why}"))?;
    Ok(command)
}

impl Images {
    fn run_images(&self, env: &Env<'_>, out: &mut dyn Write) -> Result<Exit, Failure> {
        check_target(&self.inputs, &self.output, self.force)?;
        let staging = Temporary::beside(&self.output)?;
        let staged = staging.0.join("output.pdf");
        let made = save::write_images(&self.inputs, &staged, self.options, &env.worker())
            .map_err(|why| Failure::new(Exit::Refused, why.message))?;

        // The staged file, in a fresh worker that did not write it.
        let (after, session) = read_input(env, &staged, None).map_err(|why| {
            Failure::new(
                Exit::Internal,
                format!(
                    "the staged file could not be opened again: {}; no output was published",
                    why.message
                ),
            )
        })?;
        drop(session);
        if after.sizes.len() != self.inputs.len() || made.pages as usize != self.inputs.len() {
            return Err(Failure::new(
                Exit::Internal,
                format!(
                    "the staged file has {} pages for {} pictures; no output was published",
                    after.sizes.len(),
                    self.inputs.len()
                ),
            ));
        }
        check_target(&self.inputs, &self.output, self.force)?;
        Temporary::publish(&staged, &self.output, self.force)?;

        let report = report::ImagesMade {
            schema: SCHEMA,
            command: "images".into(),
            output: self.output.display().to_string(),
            pages: self
                .inputs
                .iter()
                .zip(&after.sizes)
                .enumerate()
                .map(|(at, (input, size))| report::ImagePage {
                    page: at as u32 + 1,
                    source: input.display().to_string(),
                    width_pt: size.width_pt,
                    height_pt: size.height_pt,
                })
                .collect(),
        };
        if self.json {
            json(out, &report);
        } else {
            say(
                out,
                &format!(
                    "{}: {} {} written",
                    report.output,
                    report.pages.len(),
                    if report.pages.len() == 1 {
                        "page"
                    } else {
                        "pages"
                    }
                ),
            );
        }
        Ok(Exit::Ok)
    }
}

impl Subcommand for Images {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        self.run_images(env, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn the_parser_takes_pictures_in_order_and_an_output_that_is_none_of_them() {
        for line in [
            "",
            "-o out.pdf",
            "a.png b.jpg",
            "a.png -o a.png",
            "a.png b.jpg -o ./b.jpg",
            "a.png -o out.pdf --paper a3",
            "a.png -o out.pdf --dpi fine",
            "a.png -o out.pdf --dpi 5",
            "a.png -o out.pdf --dpi",
            "a.png -o out.pdf --unknown",
        ] {
            assert!(parse(&args(line)).is_err(), "{line}");
        }
        let parsed = parse(&args("b.jpg a.png -o out.pdf --paper a4 --dpi 150 --force")).unwrap();
        assert_eq!(
            parsed.inputs,
            [PathBuf::from("b.jpg"), PathBuf::from("a.png")]
        );
        assert_eq!(parsed.options.paper, Paper::A4);
        assert_eq!(parsed.options.dpi, Some(150));
        assert!(parsed.force && !parsed.json);
        let plain = parse(&args("a.png -o out.pdf")).unwrap();
        assert_eq!(plain.options, Options::default());
        assert_eq!(
            parse(&args("a.png -o out.pdf --paper letter"))
                .unwrap()
                .options
                .paper,
            Paper::Letter
        );
    }

    #[test]
    fn a_name_that_looks_like_an_option_is_a_picture_after_two_dashes() {
        let parsed = parse(&args("-o out.pdf -- --force.png")).unwrap();
        assert_eq!(parsed.inputs, [PathBuf::from("--force.png")]);
        assert!(!parsed.force);
    }
}
