//! Headless visual assertions use the viewer's contained renderer. The caller
//! assembles bounded raw tiles and encodes PNG; it never parses PDF or decodes
//! worker-provided image files. Publication uses the same staging as PDF edits.
//!
//! `render - -o - --fit WxH` reads the document from standard input and writes
//! only the PNG to standard output. That is the Explorer handler's whole route
//! to a page (`shellext/`, `docs/THREAT-MODEL.md` §T6.47): it gets a stream
//! from Explorer and must not parse it, so it hands the bytes here.

use std::io::Write;
use std::path::PathBuf;

use super::args::{unknown, value};
use super::pages::{check_target, unchanged, Temporary};
use super::report::{self, SCHEMA};
use super::text::{declined, password, variable};
use super::{json, opened, say, Env, Exit, Failure, Registered, Subcommand};
use crate::fingerprint::Fingerprint;
use crate::render::{PageSize, TileFormat, TileRequest};
use crate::worker_proto::{Reply, Request};

pub const COMMAND: Registered = Registered {
    name: "render",
    usage: "render <in.pdf | -> -o <page.png | -> [--page N] [--dpi N | --fit WxH]\n        [--password-env VAR] [--force] [--json]",
    summary: "Renders one page to PNG through the viewer's sandboxed renderer.\n            Defaults to page 1 at 144 DPI; respects saved crops and rotations.\n            --fit makes the page as large as fits in W x H pixels; -o - writes\n            the PNG to standard output.",
    parse: |args| parse(args).map(|p| Box::new(p) as Box<dyn Subcommand>),
};

#[derive(Debug)]
struct Render {
    input: PathBuf,
    output: PathBuf,
    page: u32,
    dpi: u32,
    /// `--fit`: the box the page is fitted into, in pixels, instead of `dpi`.
    fit: Option<(u32, u32)>,
    password_env: Option<String>,
    force: bool,
    json: bool,
}

/// `-o -`: the PNG goes to standard output and nothing else does.
fn to_stdout(output: &std::path::Path) -> bool {
    output == std::path::Path::new("-")
}

/// `--fit`'s value: `WxH`, each side 1 to 4096 pixels, so that any box is
/// inside the 16777216-pixel bound [`dimensions`] holds every render to.
fn fit_box(text: &str) -> Result<(u32, u32), String> {
    let side = |part: &str| part.parse::<u32>().ok().filter(|n| (1..=4096).contains(n));
    text.split_once(['x', 'X'])
        .and_then(|(w, h)| Some((side(w)?, side(h)?)))
        .ok_or_else(|| "--fit needs WxH, each side from 1 to 4096 pixels, as in 256x256".into())
}

/// The scale at which a page of `size` points is as large as fits in `fit`.
fn fit_scale(size: PageSize, fit: (u32, u32)) -> f32 {
    (fit.0 as f32 / size.width_pt).min(fit.1 as f32 / size.height_pt)
}

fn parse(args: &[String]) -> Result<Render, String> {
    let mut command = Render {
        input: PathBuf::new(),
        output: PathBuf::new(),
        page: 1,
        dpi: 144,
        fit: None,
        password_env: None,
        force: false,
        json: false,
    };
    let mut paths = Vec::new();
    let mut rest = args.iter();
    let mut positional = false;
    let mut dpi_given = false;
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
            "--fit" => command.fit = Some(fit_box(value(arg, &mut rest)?)?),
            "--dpi" => {
                dpi_given = true;
                command.dpi = value(arg, &mut rest)?
                    .parse::<u32>()
                    .ok()
                    .filter(|n| (1..=600).contains(n))
                    .ok_or("--dpi needs an integer from 1 to 600")?
            }
            "--password-env" => command.password_env = Some(variable(value(arg, &mut rest)?)?),
            "--force" => command.force = true,
            "--json" => command.json = true,
            "-" => paths.push(arg.into()),
            flag if flag.starts_with('-') => return Err(unknown("render", flag)),
            path => paths.push(path.into()),
        }
    }
    if paths.len() != 1 || command.output.as_os_str().is_empty() {
        return Err("render needs exactly one input document and -o <page.png>".into());
    }
    command.input = paths.remove(0);
    if dpi_given && command.fit.is_some() {
        return Err("render takes --dpi or --fit, not both".into());
    }
    if command.json && to_stdout(&command.output) {
        return Err("render cannot write --json and the PNG to standard output at once".into());
    }
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

impl Render {
    /// `--fit`'s scale for a page of `size`, else `--dpi`'s.
    fn scale(&self, size: PageSize) -> f32 {
        match self.fit {
            Some(fit) => fit_scale(size, fit),
            None => self.dpi as f32 / 72.0,
        }
    }
}

impl Subcommand for Render {
    fn run(
        &self,
        env: &Env<'_>,
        out: &mut dyn Write,
        _err: &mut dyn Write,
    ) -> Result<Exit, Failure> {
        let inputs = std::slice::from_ref(&self.input);
        let stdout = to_stdout(&self.output);
        let stdin = self.input == std::path::Path::new("-");
        if !stdout {
            check_target(inputs, &self.output, self.force)?;
        }
        let key = password(self.password_env.as_deref())?;
        let (file, len) = if stdin {
            super::opened_or_stdin(&self.input)
        } else {
            opened(&self.input)
        }
        .map_err(|e| Failure::new(Exit::Refused, e))?;
        // A document read from standard input is a file only this process can
        // name, so there is nothing for it to have changed under.
        let fingerprint = if stdin {
            None
        } else {
            Some(
                Fingerprint::of_open(&file, &self.input)
                    .map_err(|e| Failure::new(Exit::Refused, e))?,
            )
        };
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
        let scale = self.scale(*size);
        let (width, height) = dimensions(*size, scale)?;
        if let Some((w, h)) = self.fit {
            // Rounding the other side may not overshoot the box; the page's
            // own aspect makes one side exact and the other smaller.
            if width > w || height > h {
                return Err(Failure::new(
                    Exit::Internal,
                    format!("--fit {w}x{h} came out {width} x {height} pixels"),
                ));
            }
        }
        let mut pixels = vec![0; width as usize * height as usize * 4];
        for y in (0..height).step_by(1024) {
            for x in (0..width).step_by(1024) {
                let tw = (width - x).min(1024) as u16;
                let th = (height - y).min(1024) as u16;
                let tile = session.tile(TileRequest {
                    annotations: true,
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
        if let Some(fingerprint) = &fingerprint {
            unchanged(&[(self.input.as_path(), fingerprint)])?;
        }
        let png = crate::render::encode_png(&pixels, width, height)
            .map_err(|e| Failure::new(Exit::Internal, e))?;
        if stdout {
            out.write_all(&png)
                .and_then(|()| out.flush())
                .map_err(|e| {
                    Failure::new(Exit::Refused, format!("could not write the PNG: {e}"))
                })?;
            return Ok(Exit::Ok);
        }
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
            dpi: (scale * 72.0).round() as u32,
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
                    (scale * 72.0).round()
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
            "a.pdf -o a.png --fit 256",
            "a.pdf -o a.png --fit 0x256",
            "a.pdf -o a.png --fit 256x4097",
            "a.pdf -o a.png --fit 256x256 --dpi 72",
            "a.pdf -o a.png --dpi 72 --fit 256x256",
            "a.pdf -o - --json",
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
        let command = parse(&["-", "-o", "-", "--fit", "256X128"].map(str::to_owned)).unwrap();
        assert_eq!(command.input, PathBuf::from("-"));
        assert!(to_stdout(&command.output));
        assert_eq!(command.fit, Some((256, 128)));
        let letter = PageSize {
            width_pt: 612.0,
            height_pt: 792.0,
        };
        assert_eq!(
            dimensions(letter, command.scale(letter)).unwrap(),
            (99, 128)
        );
        let at_dpi = parse(&["a.pdf", "-o", "a.png", "--dpi", "36"].map(str::to_owned)).unwrap();
        assert_eq!(
            dimensions(letter, at_dpi.scale(letter)).unwrap(),
            (306, 396)
        );
    }
    #[test]
    fn a_fitted_page_fills_one_side_of_the_box_and_stays_inside_the_other() {
        let size = |width_pt, height_pt| PageSize {
            width_pt,
            height_pt,
        };
        for (page, fit, expect) in [
            (size(612.0, 792.0), (256, 256), (198, 256)),
            (size(792.0, 612.0), (256, 256), (256, 198)),
            (size(595.0, 842.0), (400, 300), (212, 300)),
            (size(100.0, 100.0), (1, 1), (1, 1)),
            (size(612.0, 792.0), (4096, 4096), (3165, 4096)),
        ] {
            assert_eq!(
                dimensions(page, fit_scale(page, fit)).unwrap(),
                expect,
                "{fit:?}"
            );
        }
    }
}
