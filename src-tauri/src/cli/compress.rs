//! `tpdf compress`: a smaller copy, and what one would come to.
//!
//! Writes a copy and never touches the input. On its own it changes nothing a
//! reader sees: streams are deflated and the copy is written with object
//! streams. `--pictures` names a preset that also shrinks pictures, and
//! `--dpi`, `--quality` and `--no-jpeg` set each number behind a preset
//! directly (`crate::compress`).
//!
//! `--dry-run` writes nothing and reports the size the copy would have.
//! `--preview` writes a PNG of the picture that loses the most, before on the
//! left and after on the right, each pixel of the original one pixel wide: the
//! answer to how it will look, which a percentage is not.
//!
//! The copy is staged beside its destination and opened in a fresh worker
//! before it is published. It must have the source's pages, be encrypted
//! exactly when the source was, and be smaller than the source: a copy that is
//! not smaller is not written.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::args::{lexically_same, unknown, value};
use super::fill::SignedState;
use super::pages::{
    check_target, copy_of, publish_copy, read_input, same_sizes, unchanged, Temporary,
};
use super::report::{self, SCHEMA};
use super::text::{declined, password, variable};
use super::{json, say, Env, Exit, Failure, Registered, Subcommand};
use crate::compress::{Compress as Way, Estimate, Pictures, Preset};
use crate::fingerprint::Fingerprint;
use crate::save;
use crate::worker_proto::{Reply, Request};

pub const COMPRESS: Registered = Registered {
    name: "compress",
    usage: "compress <in.pdf> -o <out.pdf> [--pictures screen|balanced|print]\n        [--dpi N] [--quality N] [--no-jpeg] [--dry-run] [--preview <file.png>]\n        [--password-env VAR] [--invalidate-signatures] [--force] [--json]",
    summary: "Writes a smaller copy. On its own it changes nothing a reader sees.\n            --pictures also scales pictures down, to 110, 150 or 300 pixels an\n            inch, and stores photographs as JPEG; --dpi and --quality set those\n            numbers directly and --no-jpeg keeps lossless pictures lossless.\n            --dry-run writes nothing and says what the size would be, and\n            --preview writes one picture before and after as a PNG. A copy\n            that would not be smaller is not written.",
    parse: |args| parse(args).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

#[derive(Debug)]
struct Compress {
    input: PathBuf,
    output: Option<PathBuf>,
    pictures: Option<Pictures>,
    dry_run: bool,
    preview: Option<PathBuf>,
    password_env: Option<String>,
    invalidate: bool,
    force: bool,
    json: bool,
}

fn whole<T: std::str::FromStr>(flag: &str, word: &str) -> Result<T, String> {
    word.parse()
        .map_err(|_| format!("{flag} takes a whole number, not {word:?}"))
}

fn parse(args: &[String]) -> Result<Compress, String> {
    let mut command = Compress {
        input: PathBuf::new(),
        output: None,
        pictures: None,
        dry_run: false,
        preview: None,
        password_env: None,
        invalidate: false,
        force: false,
        json: false,
    };
    let mut paths = Vec::new();
    let mut preset = None;
    let (mut dpi, mut quality, mut no_jpeg) = (None, None, false);
    let mut rest = args.iter();
    let mut positional = false;
    while let Some(arg) = rest.next() {
        if positional {
            paths.push(PathBuf::from(arg));
            continue;
        }
        match arg.as_str() {
            "--" => positional = true,
            "-o" | "--output" => command.output = Some(PathBuf::from(value(arg, &mut rest)?)),
            "--pictures" => {
                let word = value(arg, &mut rest)?;
                preset = Some(Preset::named(word).ok_or_else(|| {
                    format!("--pictures takes screen, balanced or print, not {word:?}")
                })?);
            }
            "--dpi" => dpi = Some(whole::<u32>(arg, value(arg, &mut rest)?)?),
            "--quality" => quality = Some(whole::<u8>(arg, value(arg, &mut rest)?)?),
            "--no-jpeg" => no_jpeg = true,
            "--dry-run" => command.dry_run = true,
            "--preview" => command.preview = Some(PathBuf::from(value(arg, &mut rest)?)),
            "--password-env" => command.password_env = Some(variable(value(arg, &mut rest)?)?),
            "--invalidate-signatures" => command.invalidate = true,
            "--force" => command.force = true,
            "--json" => command.json = true,
            flag if flag.starts_with('-') => return Err(unknown("compress", flag)),
            path => paths.push(path.into()),
        }
    }
    if paths.len() != 1 {
        return Err("compress needs exactly one input document".into());
    }
    command.input = paths.remove(0);
    // A number given without a preset starts from the middle one.
    if preset.is_some() || dpi.is_some() || quality.is_some() || no_jpeg {
        let mut pictures = Pictures::from(preset.unwrap_or(Preset::Balanced));
        pictures.dpi = dpi.unwrap_or(pictures.dpi);
        pictures.quality = quality.unwrap_or(pictures.quality);
        pictures.jpeg = !no_jpeg;
        command.pictures = Some(pictures.checked()?);
    }
    match (&command.output, command.dry_run) {
        (None, false) => {
            return Err(
                "-o <out.pdf> is required; the input is never overwritten. --dry-run \
                 says what the copy would come to without writing one"
                    .into(),
            )
        }
        (Some(_), true) => return Err("--dry-run writes nothing, so it takes no -o".into()),
        (Some(output), false) if lexically_same(&command.input, output) => {
            return Err("the output names the input; choose a different name".into());
        }
        _ => {}
    }
    if let Some(preview) = &command.preview {
        if command.pictures.is_none() {
            return Err(
                "--preview shows a picture before and after, and without --pictures, --dpi \
                 or --quality no picture changes"
                    .into(),
            );
        }
        if lexically_same(&command.input, preview) {
            return Err("the preview names the input; choose a different name".into());
        }
        // One name for both would have the copy replace the picture, or
        // refuse it, after the picture was already written.
        if command
            .output
            .as_ref()
            .is_some_and(|output| lexically_same(output, preview))
        {
            return Err("the preview names the output; choose a different name".into());
        }
    }
    Ok(command)
}

/// The preset's name, as `--pictures` takes it.
fn word(preset: Preset) -> &'static str {
    match preset {
        Preset::Screen => "screen",
        Preset::Balanced => "balanced",
        Preset::Print => "print",
    }
}

/// The share of `before` that `after` saves, in whole percent, rounded down so
/// the sentence never claims more than was saved.
#[must_use]
pub fn saved_percent(before: u64, after: u64) -> u64 {
    if before == 0 || after >= before {
        return 0;
    }
    (before - after) * 100 / before
}

/// Compare new names using the destination filesystem's case and normalization rules.
fn same_destination(output: &Path, preview: &Path) -> Result<bool, Failure> {
    if save::same_file(output, preview) {
        return Ok(true);
    }
    let parent = |path: &Path| {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf()
    };
    if output.exists() || preview.exists() || !save::same_file(&parent(output), &parent(preview)) {
        return Ok(false);
    }
    let (Some(left), Some(right)) = (output.file_name(), preview.file_name()) else {
        return Err(Failure::new(
            Exit::Usage,
            "choose filenames for the PDF and preview",
        ));
    };
    let temp = Temporary::beside(output)?;
    std::fs::File::create_new(temp.0.join(left))
        .map_err(|e| Failure::new(Exit::Refused, e.to_string()))?;
    temp.0
        .join(right)
        .try_exists()
        .map_err(|e| Failure::new(Exit::Refused, e.to_string()))
}

impl Compress {
    fn way(&self) -> Way {
        self.pictures.map_or(Way::Lossless, Way::Pictures)
    }

    fn source_size(&self, opened_as: &Fingerprint) -> Result<u64, Failure> {
        if self.dry_run {
            // Publishing a PDF checks the revision in publish_copy. An
            // estimate must also describe the revision still at this path.
            unchanged(&[(self.input.as_path(), opened_as)])?;
        }
        Ok(opened_as.len)
    }

    fn run_compress(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let inputs = std::slice::from_ref(&self.input);
        if let Some(output) = &self.output {
            check_target(inputs, output, self.force)?;
        }
        if let Some(preview) = &self.preview {
            check_target(inputs, preview, self.force)?;
            // The same file under two names, which the parser cannot see.
            if self
                .output
                .as_ref()
                .map(|output| same_destination(output, preview))
                .transpose()?
                .unwrap_or(false)
            {
                return Err(Failure::new(
                    Exit::Usage,
                    "the preview is the output under another name",
                ));
            }
        }
        let key = password(self.password_env.as_deref())?;
        let (mut input, mut session) = read_input(env, &self.input, key.as_deref())?;
        if input.signed.is_some() && !self.invalidate && !self.dry_run {
            return Err(Failure::new(
                Exit::Refused,
                format!(
                    "{} is signed or its signatures could not be fully read; a smaller copy \
                     rewrites the document and requires --invalidate-signatures",
                    self.input.display()
                ),
            ));
        }
        let shown = self.input.display().to_string();

        // The estimate, when it is the answer or the preview needs it.
        let estimate: Option<Estimate> = if self.dry_run || self.preview.is_some() {
            let asked = session.ask(Request::Shrink {
                compress: self.way(),
            });
            match asked {
                Ok(Reply::Shrunk(estimate)) => Some(*estimate),
                Ok(_) => {
                    return Err(Failure::new(
                        Exit::Internal,
                        "the worker answered another question",
                    ))
                }
                Err(why) => return Err(declined(&shown, why, false)),
            }
        } else {
            None
        };
        drop(session);

        // The preview is staged now and published only once the copy is: a
        // copy refused as not smaller leaves no picture of it behind, and
        // does not replace one that was there.
        let mut staged_preview = None;
        if let (Some(path), Some(estimate)) = (&self.preview, &estimate) {
            if let Some(sample) = &estimate.sample {
                let (width, height, pixels) = sample
                    .side_by_side()
                    .map_err(|why| Failure::new(Exit::Internal, why))?;
                let png = crate::render::encode_png(&pixels, width, height)
                    .map_err(|why| Failure::new(Exit::Internal, why))?;
                let staging = Temporary::beside(path)?;
                let staged = staging.0.join("preview.png");
                std::fs::write(&staged, png).map_err(|why| {
                    Failure::new(Exit::Internal, format!("{}: {why}", path.display()))
                })?;
                staged_preview = Some((staging, staged, path));
            }
        }

        let before = self.source_size(input.opened_as())?;
        let mut after = estimate.as_ref().map_or(0, |estimate| estimate.bytes_after);
        if let Some(output) = &self.output {
            input.plan.compress = self.way();
            // The staged file, in a fresh worker, which shares no code with the
            // writer's own read-back: PDFium decides whether it opens.
            let unpublished = |what: &str| {
                Failure::new(
                    Exit::Internal,
                    format!("the staged file {what}; no output was published"),
                )
            };
            after = publish_copy(
                inputs,
                &[(self.input.as_path(), input.opened_as())],
                output,
                self.force,
                copy_of(env, &self.input, &input.plan, key.as_deref()),
                |staged, ()| {
                    let after =
                        std::fs::metadata(staged)
                            .map(|data| data.len())
                            .map_err(|why| {
                                Failure::new(Exit::Internal, format!("the staged file: {why}"))
                            })?;
                    if after >= before {
                        return Err(Failure::new(
                            Exit::Refused,
                            format!(
                                "{shown} cannot be made smaller this way: the copy would be \
                                 {after} bytes and the document is {before}; no output was \
                                 published"
                            ),
                        ));
                    }
                    let (reread, session) =
                        read_input(env, staged, key.as_deref()).map_err(|why| {
                            Failure::new(
                                Exit::Internal,
                                format!(
                                    "the staged file could not be opened again: {}; no output \
                                     was published",
                                    why.message
                                ),
                            )
                        })?;
                    drop(session);
                    if reread.encrypted != input.encrypted {
                        return Err(unpublished("is not protected the way the source is"));
                    }
                    if !same_sizes(&reread.sizes, &input.sizes) {
                        return Err(unpublished("does not have the source's pages"));
                    }
                    Ok(after)
                },
            )?;
        }

        let mut preview = None;
        if let Some((_staging, staged, path)) = &staged_preview {
            let published = if self
                .output
                .as_ref()
                .is_some_and(|pdf| save::same_file(pdf, path))
            {
                Err(Failure::new(
                    Exit::Refused,
                    "the preview now names the published PDF",
                ))
            } else {
                Temporary::publish(inputs, staged, path, self.force)
            };
            if let Err(failure) = published {
                let Some(pdf) = &self.output else {
                    return Err(failure);
                };
                let failure = Failure::new(
                    failure.exit,
                    format!(
                        "{} was written. Preview {} was not written: {}",
                        pdf.display(),
                        path.display(),
                        failure.message
                    ),
                );
                say(err, &failure.message);
                if self.json {
                    json(
                        out,
                        &serde_json::json!({
                            "schema": SCHEMA, "command": "compress",
                            "output": pdf.display().to_string(), "written": true,
                            "preview_requested": path.display().to_string(), "preview_written": false,
                            "error": report::CommandError::from_failure(&failure),
                        }),
                    );
                }
                return Ok(failure.exit);
            }
            preview = Some(path.display().to_string());
        }

        let report = report::Compressed {
            schema: SCHEMA,
            command: "compress".into(),
            input: shown,
            output: self.output.as_ref().map(|path| path.display().to_string()),
            written: self.output.is_some(),
            pages: input.plan.baseline,
            preset: self
                .pictures
                .map(|pictures| pictures.preset().map_or("custom", word).to_string()),
            dpi: self.pictures.map(|pictures| pictures.dpi),
            quality: self.pictures.map(|pictures| pictures.quality),
            jpeg: self.pictures.map(|pictures| pictures.jpeg),
            bytes_before: before,
            bytes_after: after,
            saved_percent: saved_percent(before, after),
            pictures_total: estimate.as_ref().map(|estimate| estimate.done.pictures),
            pictures_changed: estimate
                .as_ref()
                .map(|estimate| estimate.done.pictures_changed),
            preview,
            signatures_invalidated: match (input.signed, self.output.is_some()) {
                (Some(SignedState::Signed(n)), true) => n,
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
fn plain(report: &report::Compressed) -> String {
    let how = match (&report.preset, report.dpi, report.quality, report.jpeg) {
        (Some(preset), Some(dpi), Some(quality), Some(jpeg)) => format!(
            "target picture resolution {dpi} pixels an inch, JPEG quality {quality}{}{}",
            if jpeg {
                ""
            } else {
                ", lossless pictures kept lossless"
            },
            if preset == "custom" {
                String::new()
            } else {
                format!(" ({preset})")
            },
        ),
        _ => "nothing a reader sees changed".to_string(),
    };
    let size = format!(
        "{} bytes, {}% smaller than the {} it {}",
        report.bytes_after,
        report.saved_percent,
        report.bytes_before,
        if report.written { "was" } else { "is" },
    );
    let mut lines = vec![match &report.output {
        Some(output) => format!("{output}: {size}; {how}"),
        None if report.bytes_after >= report.bytes_before => format!(
            "{}: a copy would be {} bytes, which is not smaller than the {} it is; {how}",
            report.input, report.bytes_after, report.bytes_before
        ),
        None => format!("{}: a copy would be {size}; {how}", report.input),
    }];
    if let (Some(total), Some(changed)) = (report.pictures_total, report.pictures_changed) {
        if report.preset.is_some() {
            lines.push(format!("{changed} of {total} pictures stored smaller."));
        }
    }
    if let Some(preview) = &report.preview {
        lines.push(format!(
            "{preview}: the picture that loses the most, before on the left and after on the right."
        ));
    }
    if report.signatures_invalidated > 0 || (report.signatures_unknown && report.written) {
        lines.push("The rewrite invalidates existing signatures.".into());
    }
    lines.join("\n")
}

impl Subcommand for Compress {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        self.run_compress(env, out, err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn absent_output_names_follow_the_volume_case_rule() {
        let dir = tempfile::tempdir().unwrap();
        let control = dir.path().join("Case");
        std::fs::write(&control, []).unwrap();
        let insensitive = dir.path().join("CASE").exists();
        let pdf = dir.path().join("Report.pdf");
        assert_eq!(
            same_destination(&pdf, &dir.path().join("REPORT.pdf")).unwrap(),
            insensitive
        );
        assert!(!same_destination(&pdf, &dir.path().join("preview.png")).unwrap());
        assert!(!pdf.exists());
    }

    #[test]
    fn compress_takes_one_input_and_an_output_or_a_dry_run() {
        for line in [
            "",
            "a.pdf",
            "a.pdf b.pdf -o c.pdf",
            "a.pdf -o a.pdf",
            "a.pdf -o b.pdf --dry-run",
            "a.pdf -o b.pdf --pictures",
            "a.pdf -o b.pdf --pictures tiny",
            "a.pdf -o b.pdf --dpi fine",
            "a.pdf -o b.pdf --dpi 19",
            "a.pdf -o b.pdf --dpi 1201",
            "a.pdf -o b.pdf --quality 0",
            "a.pdf -o b.pdf --quality 101",
            "a.pdf -o b.pdf --quality 300",
            "a.pdf -o b.pdf --preview p.png",
            "a.pdf -o b.pdf --pictures screen --preview a.pdf",
            "a.pdf -o b.pdf --unknown",
        ] {
            assert!(parse(&args(line)).is_err(), "{line}");
        }
        let plainly = parse(&args("a.pdf -o b.pdf")).unwrap();
        assert!(plainly.pictures.is_none() && !plainly.force && !plainly.dry_run);
        assert_eq!(plainly.way(), Way::Lossless);
        let dry = parse(&args("a.pdf --dry-run --json")).unwrap();
        assert!(dry.dry_run && dry.output.is_none() && dry.json);
    }

    #[test]
    fn a_dry_run_reports_the_revision_used_for_its_estimate() {
        let dir = tempfile::tempdir().unwrap();
        let mut command = parse(&args("a.pdf --dry-run")).unwrap();
        command.input = dir.path().join("source.pdf");
        std::fs::write(&command.input, b"first").unwrap();
        let opened = Fingerprint::of(&command.input).unwrap();
        assert_eq!(command.source_size(&opened).unwrap(), 5);
        std::fs::write(&command.input, b"a different revision").unwrap();
        assert!(command.source_size(&opened).is_err());
    }

    #[test]
    fn a_preset_names_three_numbers_and_each_can_be_given_alone() {
        for (name, preset) in [
            ("screen", Preset::Screen),
            ("balanced", Preset::Balanced),
            ("print", Preset::Print),
        ] {
            let parsed = parse(&args(&format!(
                "a.pdf -o b.pdf --pictures {name} --force --password-env OLD"
            )))
            .unwrap();
            assert_eq!(parsed.pictures, Some(Pictures::from(preset)));
            assert_eq!(parsed.way(), Way::Pictures(preset.into()));
            assert_eq!(word(preset), name);
            assert!(parsed.force);
            assert_eq!(parsed.password_env.as_deref(), Some("OLD"));
        }
        let balanced = Pictures::from(Preset::Balanced);
        let only = |line: &str| parse(&args(line)).unwrap().pictures.unwrap();
        // A number alone starts from the middle preset.
        assert_eq!(
            only("a.pdf -o b.pdf --dpi 96"),
            Pictures {
                dpi: 96,
                ..balanced
            }
        );
        assert_eq!(
            only("a.pdf -o b.pdf --quality 40"),
            Pictures {
                quality: 40,
                ..balanced
            }
        );
        assert_eq!(
            only("a.pdf -o b.pdf --no-jpeg"),
            Pictures {
                jpeg: false,
                ..balanced
            }
        );
        // And overrides the preset it is given with.
        assert_eq!(
            only("a.pdf -o b.pdf --pictures print --quality 95 --preview p.png"),
            Pictures {
                quality: 95,
                ..Pictures::from(Preset::Print)
            }
        );
    }

    #[test]
    fn the_share_saved_is_rounded_down_and_never_negative() {
        assert_eq!(saved_percent(1000, 400), 60);
        assert_eq!(saved_percent(1000, 999), 0);
        assert_eq!(saved_percent(1000, 989), 1);
        assert_eq!(saved_percent(1000, 1000), 0);
        assert_eq!(saved_percent(1000, 2000), 0);
        assert_eq!(saved_percent(0, 0), 0);
    }

    fn report() -> report::Compressed {
        report::Compressed {
            schema: SCHEMA,
            command: "compress".into(),
            input: "a.pdf".into(),
            output: Some("b.pdf".into()),
            written: true,
            pages: 2,
            preset: None,
            dpi: None,
            quality: None,
            jpeg: None,
            bytes_before: 1000,
            bytes_after: 400,
            saved_percent: 60,
            pictures_total: None,
            pictures_changed: None,
            preview: None,
            signatures_invalidated: 0,
            signatures_unknown: false,
        }
    }

    #[test]
    fn the_sentence_says_what_was_done_to_the_pictures() {
        let mut said = report();
        assert_eq!(
            plain(&said),
            "b.pdf: 400 bytes, 60% smaller than the 1000 it was; nothing a reader sees changed"
        );
        said.preset = Some("screen".into());
        said.dpi = Some(110);
        said.quality = Some(60);
        said.jpeg = Some(true);
        said.signatures_invalidated = 1;
        assert_eq!(
            plain(&said),
            "b.pdf: 400 bytes, 60% smaller than the 1000 it was; target picture resolution 110 pixels \
             an inch, JPEG quality 60 (screen)\nThe rewrite invalidates existing signatures."
        );
        said.preset = Some("custom".into());
        said.jpeg = Some(false);
        said.signatures_invalidated = 0;
        assert!(plain(&said).ends_with(
            "target picture resolution 110 pixels an inch, JPEG quality 60, lossless pictures kept lossless"
        ));
    }

    #[test]
    fn a_dry_run_says_would_and_counts_the_pictures() {
        let mut said = report();
        said.output = None;
        said.written = false;
        said.preset = Some("balanced".into());
        said.dpi = Some(150);
        said.quality = Some(75);
        said.jpeg = Some(true);
        said.pictures_total = Some(7);
        said.pictures_changed = Some(3);
        said.preview = Some("p.png".into());
        // Signed, and a dry run invalidates nothing.
        said.signatures_unknown = true;
        assert_eq!(
            plain(&said),
            "a.pdf: a copy would be 400 bytes, 60% smaller than the 1000 it is; target picture \
             resolution 150 pixels an inch, JPEG quality 75 (balanced)\n\
             3 of 7 pictures stored smaller.\n\
             p.png: the picture that loses the most, before on the left and after on the right."
        );
        said.bytes_after = 1200;
        said.saved_percent = 0;
        said.preview = None;
        assert!(plain(&said).starts_with(
            "a.pdf: a copy would be 1200 bytes, which is not smaller than the 1000 it is;"
        ));
    }
}
