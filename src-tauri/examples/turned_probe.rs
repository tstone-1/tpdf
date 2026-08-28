//! Does a mark land where the reader put it, on a page the document turns?
//!
//! Every quad a mark carries is mapped out of the reader's frame and into the
//! page's own by `save::user_quads`, which is right for the rectangle and wrong
//! for anything drawn inside it that has a direction. This measures that:
//! one mark of each kind, the same box each time, on a document whose four
//! pages carry `/Rotate 0`, `90`, `180` and `270` and are otherwise identical.
//!
//! **`testdata/rotated.pdf` is the fixture and its own generator says why**:
//! *"the four pages carry identical content and differ only in /Rotate, so any
//! difference the probe reports is the rotation and nothing else."* That is the
//! whole design here --- the reading on page 0 is the reference and the other
//! three must match it, so nothing has to be predicted and no expected number
//! is written down.
//!
//! ## What is read
//!
//! Each page is rendered before the mark and after it, and the pixels that
//! moved inside the reader's box are reduced to four numbers, all of them
//! fractions of that box: how much of it is inked, and the ink's own bounding
//! box. Fractions rather than points, because the page's displayed size changes
//! with the turn and a rectangle in points would differ for that reason alone.
//!
//! ## What it caught, measured 2026-08-24 before the repair
//!
//! On `/Rotate 90`, against the same box on `/Rotate 0`:
//!
//! | kind | upright | turned |
//! |------|---------|--------|
//! | underline | a band at y 0.93..0.99 | a rule down the left edge, x 0.00..0.07 |
//! | strikeout | y 0.46..0.53 | a vertical line at x 0.46..0.53 |
//! | squiggly | y 0.81..0.99 | x 0.00..0.15 |
//! | text box | x 0.01..0.34 | a column at x 0.82..0.98, wrapped to the box's height |
//! | stamp | 25,011 px | 11,024 px, sideways |
//!
//! A highlight and a box came out right, and they are the two whose shape is
//! symmetric under a quarter turn. That symmetry is why nothing else caught
//! this: the window sweep's agreement check compares *coverage fractions*, and
//! a band turned through a right angle covers the same fraction of the same
//! rectangle. Only the text box tripped it, and the diagnosis recorded at the
//! time was that the box had been too short.
//!
//! **The squiggle is the reason this is a probe and not four more unit tests.**
//! It is a stroked path rather than a rectangle, so there is no operand a
//! source-level assertion can read; what it looks like is a question about
//! pixels.
//!
//! ## The control
//!
//! Page 0 against itself is not one --- it is the reference, and comparing it
//! with itself cannot fail. The control this run needs is that the reference
//! *discriminates*: a kind whose ink covers the whole box, or none of it, would
//! agree with anything. So each kind's reference reading is required to be a
//! proper fraction, and to differ from at least one other kind's, before any
//! comparison is made.
//!
//! Usage:
//!   turned-probe [file.pdf] [--lib DIR]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tpdf_lib::document::OpenDocument;

use pdfium_render::prelude::Pdfium;
use tpdf_lib::docmodel::{MarkKind, Quad, StampName};
use tpdf_lib::edits::{Edits, NewMark};
use tpdf_lib::progressive::{self, Placement, RawBitmap};
use tpdf_lib::save;

const DOC: u32 = 1;

/// How far two readings of one box may differ and still count as the same, as a
/// fraction of the box.
///
/// The two pages are rendered at different displayed sizes, so a shared edge
/// lands on a different pixel and antialiasing differs by a row. Measured after
/// the repair: the largest disagreement across all four turns and seven kinds
/// was 0.014 of the box, on the text box, whose glyphs are the finest thing
/// drawn here. Two hundredths is above that and an order of magnitude below
/// every defect in the table above, the smallest of which moved an edge by 0.9.
const SLACK: f64 = 0.02;

/// The pixel difference at which a pixel counts as having moved.
///
/// The marks are drawn in a strong blue over black type on white paper, so a
/// moved pixel moves a long way; this is set to clear JPEG-free rendering
/// noise, not to discriminate between colours.
const MOVED: i32 = 24;

fn main() {
    let mut file = PathBuf::from("testdata/rotated.pdf");
    let mut library = PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lib" => library = PathBuf::from(args.next().unwrap_or_default()),
            other => file = PathBuf::from(other),
        }
    }

    let path = Pdfium::pdfium_platform_library_name_at_path(&library);
    let bound = match Pdfium::bind_to_library(&path) {
        Ok(bound) => bound,
        Err(why) => {
            println!(
                "[FAIL] could not load pdfium from {}: {why}",
                library.display()
            );
            std::process::exit(1);
        }
    };
    let bindings = progressive::bindings_of(Box::leak(Box::new(Pdfium::new(bound))));

    let doc = match OpenDocument::open(bindings, &file, None) {
        Ok(doc) => doc,
        Err(why) => {
            println!("[FAIL] {}: {why}", file.display());
            std::process::exit(1);
        }
    };
    let pages = doc.page_count();
    // The reader's box, in the space the reader drags in, and the size every
    // page has to hold. Taken from the *smallest* displayed page, because a
    // turned page is the other way round and a box that fitted only the upright
    // one would make this a probe about clipping.
    let mut narrowest = f32::MAX;
    let mut shortest = f32::MAX;
    for index in 0..pages {
        let Ok(page) = doc.page(index) else {
            println!("[FAIL] page {index} would not load");
            std::process::exit(1);
        };
        narrowest = narrowest.min(page.width_pt());
        shortest = shortest.min(page.height_pt());
    }
    drop(doc);
    if pages < 2 {
        println!("[FAIL] {} has {pages} page(s); this needs a turned page and an upright one to compare it with", file.display());
        std::process::exit(1);
    }
    let quad = Quad {
        left: narrowest * 0.15,
        top: shortest * 0.25,
        right: narrowest * 0.15 + (narrowest * 0.5).min(300.0),
        bottom: shortest * 0.25 + (shortest * 0.1).min(40.0),
    };
    println!(
        "{} : {pages} pages, a box the reader sees as {:.0} x {:.0} pt",
        file.display(),
        quad.right - quad.left,
        quad.bottom - quad.top
    );

    const KINDS: [MarkKind; 7] = [
        MarkKind::Highlight,
        MarkKind::Underline,
        MarkKind::StrikeOut,
        MarkKind::Squiggly,
        MarkKind::Square,
        MarkKind::TextBox,
        MarkKind::Stamp,
    ];

    let mut references: Vec<(MarkKind, Reading)> = Vec::new();
    for kind in KINDS {
        let mut readings = Vec::new();
        for index in 0..pages {
            match reading(bindings, &file, index, pages, kind, quad) {
                Ok(read) => readings.push((index, read)),
                Err(why) => {
                    check(&format!("{kind:?}: page {index} was measured"), false, &why);
                    readings.clear();
                    break;
                }
            }
        }
        let Some((_, reference)) = readings.first().copied() else {
            continue;
        };
        // **A multiplied mark's coverage is a reading about the page**, so it
        // is left out of the comparison for the one kind that has one --- and
        // the predicate is `save`'s own rather than a copy, for the reason its
        // doc comment gives. Measured here, which is how it was found: a
        // highlight read 0.933 of the box upright and 1.000 at a half turn on a
        // fixture whose four pages are identical. Multiply leaves a pixel where
        // it is wherever the paper is already dark, and this fixture confines
        // its type to the upper part of the *page*, which is a different part of
        // the *display* at each turn. Nothing about the mark moved: its ink box
        // read 0.00..1.00 by 0.00..0.99 on all four pages.
        //
        // The exclusion cannot quietly grow: if another kind became a wash, its
        // coverage would still be compared here until this line was changed, and
        // the run would go red rather than pass in silence.
        let coverage = !save::is_wash(kind);
        // The reference has to be capable of disagreeing before it is compared
        // with anything: ink covering all of the box or none of it matches
        // every reading there is. For the wash, whose coverage is not compared,
        // the discriminating reading is the extent --- covering the whole box is
        // what tells it from every other kind.
        check(
            &format!("{kind:?}: the upright reading can discriminate"),
            if coverage {
                reference.covered > 0.005 && reference.covered < 0.995
            } else {
                reference.ink[2] - reference.ink[0] > 0.9
                    && reference.ink[3] - reference.ink[1] > 0.9
            },
            &format!(
                "{} upright{}",
                reference.tell(),
                if coverage {
                    ""
                } else {
                    ", read by extent because it multiplies"
                }
            ),
        );
        references.push((kind, reference));
        for (index, read) in readings.iter().skip(1) {
            let apart = reference.apart_from(read, coverage);
            check(
                &format!("{kind:?}: page {index} reads as page 0 does"),
                apart < SLACK,
                &format!(
                    "{} against {} --- {apart:.3} apart",
                    read.tell(),
                    reference.tell()
                ),
            );
        }
    }

    // The other half of the control, and it is about the *set* of readings:
    // every check above compares one kind with itself, so a run in which every
    // kind drew the same thing would be entirely green. Two kinds that are
    // drawn differently have to read differently.
    let far = references
        .iter()
        .flat_map(|(ka, a)| {
            references
                .iter()
                .filter(move |(kb, _)| kb != ka)
                .map(move |(kb, b)| (ka, kb, a.apart_from(b, true)))
        })
        .filter(|(_, _, apart)| *apart > SLACK)
        .count();
    check(
        "the kinds are told apart from one another",
        far > 0 && references.len() > 1,
        &format!("{far} pairs of {} kinds read differently", references.len()),
    );

    let (ran, passed) = (RAN.load(Ordering::Relaxed), PASSED.load(Ordering::Relaxed));
    println!("\n{passed}/{ran} checks passed");
    if passed != ran {
        std::process::exit(1);
    }
}

/// What one mark drew inside the reader's box, in fractions of that box.
#[derive(Clone, Copy)]
struct Reading {
    /// How much of the box moved between the two renders.
    covered: f64,
    /// The moved pixels' own bounding box: left, top, right, bottom.
    ink: [f64; 4],
}

impl Reading {
    /// The largest disagreement between two readings, in fractions of the box.
    ///
    /// A maximum rather than a mean, because the defects this exists for move
    /// one edge a long way and leave the others where they were --- a strikeout
    /// turned through a right angle keeps its coverage exactly and swaps two
    /// pairs of edges. A mean over five numbers would divide that by five.
    fn apart_from(&self, other: &Reading, coverage: bool) -> f64 {
        let mut worst = if coverage {
            (self.covered - other.covered).abs()
        } else {
            0.0
        };
        for (a, b) in self.ink.iter().zip(other.ink.iter()) {
            worst = worst.max((a - b).abs());
        }
        worst
    }

    fn tell(&self) -> String {
        format!(
            "{:.3} inked, x {:.2}..{:.2} y {:.2}..{:.2}",
            self.covered, self.ink[0], self.ink[2], self.ink[1], self.ink[3]
        )
    }
}

/// Places one mark on one page, saves, renders, and reads the box.
fn reading(
    bindings: progressive::Bindings,
    file: &Path,
    page: u32,
    pages: u32,
    kind: MarkKind,
    quad: Quad,
) -> Result<Reading, String> {
    // Rendered here rather than once per document: the mark is written by a
    // full copy, so the two renders have to be of the same page of two files,
    // and holding a page's "before" across every kind would keep seven copies
    // of a raster alive for no saving worth having.
    let scale = 3.0;
    let (before, width, height) = render(bindings, file, page, scale)?;

    let edits = Edits::default();
    edits.open(DOC, pages, None);
    let state = edits
        .state(DOC)
        .map_err(|e| format!("no edit state: {e}"))?;
    let id = state
        .pages
        .get(page as usize)
        .ok_or_else(|| format!("the model has no page {page}"))?
        .id;
    edits
        .annotate(
            DOC,
            NewMark {
                kind,
                // The biconditional the model enforces, restated at the one
                // place here that builds a mark.
                stamp: (kind == MarkKind::Stamp).then_some(StampName::Draft),
                page: id,
                quads: vec![quad.left, quad.top, quad.right, quad.bottom],
                strokes: Vec::new(),
                color: [0.15, 0.35, 0.9],
                author: String::new(),
                note: "the reader typed this".to_string(),
            },
            save::pdf_date(std::time::SystemTime::now()),
        )
        .map_err(|e| format!("the model refused the mark: {e}"))?;
    let plan = edits.plan(DOC).map_err(|e| format!("no plan: {e}"))?;

    let out = std::env::temp_dir().join(format!(
        "tpdf-turned-probe-{}-{page}-{kind:?}.pdf",
        std::process::id()
    ));
    save::write_copy(file, &plan, &out, None).map_err(|why| why.message)?;
    let rendered = render(bindings, &out, page, scale);
    let _ = std::fs::remove_file(&out);
    let (after, w2, h2) = rendered?;
    if (w2, h2) != (width, height) {
        return Err(format!(
            "the saved copy renders {w2}x{h2} where the source rendered {width}x{height}"
        ));
    }

    let x0 = ((quad.left * scale).floor().max(0.0) as u32).min(width);
    let y0 = ((quad.top * scale).floor().max(0.0) as u32).min(height);
    let x1 = ((quad.right * scale).ceil().max(0.0) as u32).min(width);
    let y1 = ((quad.bottom * scale).ceil().max(0.0) as u32).min(height);
    if x1 <= x0 || y1 <= y0 {
        return Err("the reader's box is off this page".to_string());
    }
    let (span_x, span_y) = ((x1 - x0) as f64, (y1 - y0) as f64);
    let mut moved = 0usize;
    let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in y0..y1 {
        for x in x0..x1 {
            let at = ((y * width + x) * 4) as usize;
            let apart = (i32::from(before[at]) - i32::from(after[at])).abs()
                + (i32::from(before[at + 1]) - i32::from(after[at + 1])).abs()
                + (i32::from(before[at + 2]) - i32::from(after[at + 2])).abs();
            if apart <= MOVED {
                continue;
            }
            moved += 1;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    if moved == 0 {
        return Err("nothing moved inside the box, so there is no mark to read".to_string());
    }
    Ok(Reading {
        covered: moved as f64 / (span_x * span_y),
        ink: [
            f64::from(left - x0) / span_x,
            f64::from(top - y0) / span_y,
            f64::from(right - x0) / span_x,
            f64::from(bottom - y0) / span_y,
        ],
    })
}

/// Renders a page of a file and returns its pixels and size.
///
/// Takes the bindings rather than loading the library itself: PDFium refuses a
/// second `bind_to_library` in one process, which is the trap
/// `PdfiumLibraryBindingsAlreadyInitialized` names.
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
    Ok((
        bitmap.pixels().to_vec(),
        u32::from(width),
        u32::from(height),
    ))
}

/// Checks run and checks passed, for the summary line at the end.
///
/// A probe that died halfway prints exactly the same green lines as one that
/// finished, so the count is the only thing that distinguishes them.
static RAN: AtomicUsize = AtomicUsize::new(0);
static PASSED: AtomicUsize = AtomicUsize::new(0);

fn check(name: &str, ok: bool, detail: &str) {
    RAN.fetch_add(1, Ordering::Relaxed);
    if ok {
        PASSED.fetch_add(1, Ordering::Relaxed);
    }
    // Brackets tight in the literal: `mutate_viewer.py` matches
    // `^\[(OK|FAIL|SKIP)\]\s+`, and padding the label puts the spaces *inside*
    // the brackets, which stops every line being a check line at all.
    println!("[{}] {name}  {detail}", if ok { "OK" } else { "FAIL" });
}
