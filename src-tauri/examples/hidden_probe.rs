//! Does PDFium's renderer honour an annotation's Hidden flag, per annotation?
//!
//! `docs/PLAN.md` §10 q8 ranks one piece of work: draw a comment's icon in
//! tpdf's own overlay from `/C`, because PDFium draws it in a fixed colour while
//! Preview and Acrobat both read `/C` (measured 2026-08-31). Every design for
//! that begins by stopping PDFium drawing *that* icon without stopping it
//! drawing every other mark on the page --- `FPDF_ANNOT` is a per-render flag, so
//! the obvious lever is the wrong shape. The per-annotation lever is `/F` bit 2,
//! Hidden, and whether PDFium's renderer obeys it is the question the whole
//! increment turns on. It is cheaper to ask than to assume.
//!
//! **The fixture carries two marks and only one is hidden**, because what has to
//! be established is that the flag is read *per annotation*. A page with one
//! annotation cannot tell "PDFium honours Hidden" from "PDFium stopped drawing
//! annotations", and those have opposite consequences for the design.
//!
//! Four readings, and three of them are controls:
//!
//! * **both vs source** --- the fixture really does draw two marks. Without it a
//!   run over a page where nothing was drawn reports the icon as successfully
//!   hidden.
//! * **hidden vs both** --- something changed. Zero here is the flag being
//!   ignored, which is the answer that kills the design.
//! * **hidden vs source, inside the highlight's quad** --- the highlight is still
//!   drawn. Zero here means Hidden suppressed everything, not one annotation.
//! * **hidden vs source, inside the note's rectangle** --- the icon is gone.
//!   Non-zero here means it is still being drawn somewhere in that box.
//!
//! The two files differ in **one byte**: `/F 4` against `/F 6`, which is
//! Print against Print|Hidden. `docs/TRAPS.md` records `/F` as a bit field whose
//! flags are easy to confuse, so the value is built from named constants below
//! rather than written as a number.
//!
//! Usage:
//!   hidden-probe <both.pdf> <hidden.pdf> [--page N] [--scale F] [--lib DIR]
//!
//! Both files are produced by `annot-probe --mode iconhide`, which is the only
//! caller; see `BUILD.md`.

use std::path::{Path, PathBuf};
use tpdf_lib::document::OpenDocument;
use tpdf_lib::progressive::{self, Placement, RawBitmap};

/// `/F` bit 3, value 4: print the annotation. Every mark tpdf writes sets it.
const ANNOT_PRINT: u32 = 4;
/// `/F` bit 2, value 2: do not display the annotation on screen.
const ANNOT_HIDDEN: u32 = 2;

fn render(
    bindings: progressive::Bindings,
    file: &Path,
    number: u32,
    scale: f32,
) -> Result<(Vec<u8>, u32, u32), String> {
    let document = OpenDocument::open(bindings, file, None)?;
    let page = document.page(number)?;
    let width = (page.width_pt() * scale).round() as u16;
    let height = (page.height_pt() * scale).round() as u16;
    let mut buffer = vec![0u8; width as usize * height as usize * 4];
    let mut bitmap = RawBitmap::borrowed(bindings, &mut buffer, width, height)?;
    let placement = Placement::tile(&page, scale, 0, 0, 0);
    let progress = progressive::render(
        &mut bitmap,
        &page,
        placement,
        None,
        &progressive::CancelToken::new(),
    );
    if !progress.outcome.is_done() {
        return Err(format!("render did not complete: {:?}", progress.outcome));
    }
    Ok((bitmap.pixels().to_vec(), width as u32, height as u32))
}

/// Differing pixels between two renders, optionally inside one rectangle.
///
/// The rectangle is in the render's own pixel space. `None` means the whole
/// page, and the two are not interchangeable: a whole-page count answers "did
/// anything change" and a windowed one answers "did *this* change", and this
/// probe needs both about the same pair.
fn differing(a: &[u8], b: &[u8], w: u32, h: u32, window: Option<[u32; 4]>) -> usize {
    let [x0, y0, x1, y1] = window.unwrap_or([0, 0, w, h]);
    let mut n = 0;
    for y in y0..y1.min(h) {
        for x in x0..x1.min(w) {
            let i = ((y * w + x) * 4) as usize;
            if a[i..i + 4] != b[i..i + 4] {
                n += 1;
            }
        }
    }
    n
}

/// The same loader every other probe uses; `vendor/pdfium/<subdir>` by default.
fn bind(library: &Path) -> Result<progressive::Bindings, String> {
    use pdfium_render::prelude::Pdfium;
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = Pdfium::bind_to_library(&path)
        .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;
    Ok(progressive::bindings_of(Box::leak(Box::new(Pdfium::new(
        bound,
    )))))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut files: Vec<PathBuf> = Vec::new();
    let mut page = 0u32;
    let mut scale = 1.0f32;
    let mut library: Option<PathBuf> = None;
    let mut source: Option<PathBuf> = None;
    let mut note: Option<[u32; 4]> = None;
    let mut quad: Option<[u32; 4]> = None;
    while let Some(arg) = args.next() {
        let rect = |v: Option<String>| -> [u32; 4] {
            let v = v.unwrap_or_default();
            let parts: Vec<u32> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
            if parts.len() != 4 {
                eprintln!("[FAIL] a rectangle wants four comma-separated pixel values");
                std::process::exit(2);
            }
            [parts[0], parts[1], parts[2], parts[3]]
        };
        match arg.as_str() {
            "--page" => page = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--scale" => scale = args.next().and_then(|v| v.parse().ok()).unwrap_or(1.0),
            "--lib" => library = args.next().map(PathBuf::from),
            "--source" => source = args.next().map(PathBuf::from),
            "--note-rect" => note = Some(rect(args.next())),
            "--quad-rect" => quad = Some(rect(args.next())),
            other => files.push(PathBuf::from(other)),
        }
    }
    if files.len() != 2 || source.is_none() {
        eprintln!("[FAIL] wants <both.pdf> <hidden.pdf> --source <source.pdf>");
        std::process::exit(2);
    }
    println!(
        "[INFO] Hidden is /F bit 2 (value {ANNOT_HIDDEN}); the marks also set Print \
         (value {ANNOT_PRINT}), so the two files read /F {} and /F {}",
        ANNOT_PRINT,
        ANNOT_PRINT | ANNOT_HIDDEN
    );

    let dir =
        library.unwrap_or_else(|| PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR));
    let bindings = match bind(&dir) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(1);
        }
    };
    let load = |p: &Path| match render(bindings, p, page, scale) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[FAIL] {}: {e}", p.display());
            std::process::exit(1);
        }
    };
    let (src, w, h) = load(source.as_deref().unwrap());
    let (both, _, _) = load(&files[0]);
    let (hidden, _, _) = load(&files[1]);

    let mut bad = 0;
    let mut say = |ok: bool, line: String| {
        println!("{} {line}", if ok { "[OK]  " } else { "[FAIL]" });
        if !ok {
            bad += 1;
        }
    };

    let drawn = differing(&src, &both, w, h, None);
    say(
        drawn > 0,
        format!("control: the fixture draws its two marks ({drawn} px against the source)"),
    );
    let moved = differing(&both, &hidden, w, h, None);
    say(
        moved > 0,
        format!("the Hidden flag changes the render ({moved} px moved)"),
    );
    if let Some(q) = quad {
        let still = differing(&src, &hidden, w, h, Some(q));
        say(
            still > 0,
            format!("control: the highlight is still drawn ({still} px in its quad)"),
        );
    }
    if let Some(r) = note {
        let left = differing(&src, &hidden, w, h, Some(r));
        say(
            left == 0,
            format!("the comment's icon is gone ({left} px left in its rectangle)"),
        );
    }
    println!(
        "\n{}",
        if bad == 0 {
            "[OK] PDFium honours the Hidden flag per annotation"
        } else {
            "[FAIL] see above"
        }
    );
    std::process::exit(if bad == 0 { 0 } else { 1 });
}
