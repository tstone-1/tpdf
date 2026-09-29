//! Headless visual assertions use the viewer's contained renderer. The caller
//! assembles bounded raw tiles and encodes PNG; it never parses PDF or decodes
//! worker-provided image files. Publication uses the same staging as PDF edits.

use std::io::Write;
use std::path::PathBuf;

use super::args::{unknown, value};
use super::pages::{check_target, Temporary};
use super::report::{self, SCHEMA};
use super::text::{declined, password, variable};
use super::{json, opened, say, Env, Exit, Failure, Registered, Subcommand};
use crate::fingerprint::Fingerprint;
use crate::render::{PageSize, TileFormat, TileRequest};
use crate::worker_proto::{Reply, Request};

pub const COMMAND: Registered = Registered {
    name: "render",
    usage: "render <in.pdf> -o <page.png> [--page N] [--dpi N]\n        [--password-env VAR] [--force] [--json]",
    summary: "Renders one page to PNG through the viewer's sandboxed renderer.\n            Defaults to page 1 at 144 DPI; respects saved crops and rotations.",
    parse: |args| parse(args).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

#[derive(Debug)]
struct Render {
    input: PathBuf,
    output: PathBuf,
    page: u32,
    dpi: u32,
    password_env: Option<String>,
    force: bool,
    json: bool,
}

fn parse(args: &[String]) -> Result<Render, String> {
    let mut command = Render {
        input: PathBuf::new(),
        output: PathBuf::new(),
        page: 1,
        dpi: 144,
        password_env: None,
        force: false,
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
            "-o" | "--output" => command.output = value(arg, &mut rest)?.into(),
            "--page" => {
                command.page = value(arg, &mut rest)?
                    .parse::<u32>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("--page needs a positive integer")?
            }
            "--dpi" => {
                command.dpi = value(arg, &mut rest)?
                    .parse::<u32>()
                    .ok()
                    .filter(|n| (1..=600).contains(n))
                    .ok_or("--dpi needs an integer from 1 to 600")?
            }
            "--password-env" => command.password_env = Some(variable(value(arg, &mut rest)?)?),
            "--force" => command.force = true,
            "--json" => command.json = true,
            flag if flag.starts_with('-') => return Err(unknown("render", flag)),
            path => paths.push(path.into()),
        }
    }
    if paths.len() != 1 || command.output.as_os_str().is_empty() {
        return Err("render needs exactly one input document and -o <page.png>".into());
    }
    command.input = paths.remove(0);
    Ok(command)
}

// Bound allocation and work before rendering. Match the viewer's rounded page
// placement exactly; rounding up instead adds an unpainted edge at some DPIs.
fn dimensions(size: PageSize, scale: f32) -> Result<(u32, u32), Failure> {
    let width = (size.width_pt * scale).round();
    let height = (size.height_pt * scale).round();
    if !width.is_finite()
        || !height.is_finite()
        || width < 1.0
        || height < 1.0
        || width > 8192.0
        || height > 8192.0
        || f64::from(width) * f64::from(height) > 16_777_216.0
    {
        return Err(Failure::new(
            Exit::Refused,
            "render requires 1..8192 pixels per side and at most 16777216 pixels total; adjust --dpi",
        ));
    }
    Ok((width as u32, height as u32))
}

impl Subcommand for Render {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let inputs = std::slice::from_ref(&self.input);
        check_target(inputs, &self.output, self.force)?;
        let key = password(self.password_env.as_deref())?;
        let (file, len) = opened(&self.input).map_err(|e| Failure::new(Exit::Refused, e))?;
        let fingerprint =
            Fingerprint::of_open(&file, &self.input).map_err(|e| Failure::new(Exit::Refused, e))?;
        let mut session = env
            .worker()
            .session(&file, len, key.as_deref())
            .map_err(|e| declined(&self.input.display().to_string(), e, key.is_some()))?;
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
        let size = pages.get(self.page as usize - 1).ok_or_else(|| {
            Failure::new(
                Exit::Refused,
                "--page names a page past the end of the document",
            )
        })?;
        let scale = self.dpi as f32 / 72.0;
        let (width, height) = dimensions(*size, scale)?;
        let mut pixels = vec![0; width as usize * height as usize * 4];
        for y in (0..height).step_by(1024) {
            for x in (0..width).step_by(1024) {
                let tw = (width - x).min(1024) as u16;
                let th = (height - y).min(1024) as u16;
                let tile = session.tile(TileRequest {
                    rid: 0,
                    doc: 0,
                    page: self.page - 1,
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
                for row in 0..usize::from(th) {
                    let start = ((y as usize + row) * width as usize + x as usize) * 4;
                    let stride = usize::from(tw) * 4;
                    pixels[start..start + stride]
                        .copy_from_slice(&tile[row * stride..(row + 1) * stride]);
                }
            }
        }
        drop(session);
        drop(file);
        fingerprint
            .agrees_with(&self.input)
            .map_err(|e| Failure::new(Exit::Refused, e))?;
        let png = crate::render::encode_png(&pixels, width, height)
            .map_err(|e| Failure::new(Exit::Internal, e))?;
        let staging = Temporary::beside(&self.output)?;
        let staged = staging.0.join("page.png");
        std::fs::write(&staged, &png).map_err(|e| Failure::new(Exit::Refused, e.to_string()))?;
        check_target(inputs, &self.output, self.force)?;
        Temporary::publish(&staged, &self.output, self.force)?;
        let report = report::Rendered {
            schema: SCHEMA,
            command: "render".into(),
            input: self.input.display().to_string(),
            output: self.output.display().to_string(),
            page: self.page,
            dpi: self.dpi,
            width_px: width,
            height_px: height,
        };
        if self.json {
            json(out, &report);
        } else {
            say(
                out,
                &format!(
                    "Rendered page {} to {} ({} x {} pixels at {} DPI)",
                    self.page,
                    self.output.display(),
                    width,
                    height,
                    self.dpi
                ),
            );
        }
        Ok(Exit::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rendering_bounds_are_checked_before_allocation() {
        let size = |width_pt, height_pt| PageSize {
            width_pt,
            height_pt,
        };
        assert_eq!(dimensions(size(200.3, 300.4), 2.0).unwrap(), (401, 601));
        assert_eq!(dimensions(size(4096.0, 4096.0), 1.0).unwrap(), (4096, 4096));
        for (w, h) in [
            (8193.0, 1.0),
            (1.0, 8193.0),
            (4097.0, 4096.0),
            (0.0, 10.0),
            (-1.0, 10.0),
            (f32::NAN, 10.0),
            (10.0, f32::INFINITY),
        ] {
            assert_eq!(dimensions(size(w, h), 1.0).unwrap_err().exit, Exit::Refused);
        }
    }
    #[test]
    fn render_parser_requires_bounded_resolution_and_one_page() {
        for line in [
            "a.pdf",
            "a.pdf b.pdf -o a.png",
            "a.pdf -o a.png --page 0",
            "a.pdf -o a.png --dpi 0",
            "a.pdf -o a.png --dpi 601",
            "a.pdf -o a.png --dpi NaN",
            "a.pdf -o a.png --unknown",
        ] {
            assert!(
                parse(
                    &line
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                )
                .is_err(),
                "{line}"
            );
        }
        let command = parse(&["-o", "a.png", "--", "-input.pdf"].map(str::to_owned)).unwrap();
        assert_eq!(command.input, PathBuf::from("-input.pdf"));
        assert_eq!((command.page, command.dpi), (1, 144));
    }
}
