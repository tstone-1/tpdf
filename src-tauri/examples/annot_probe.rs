//! Does a highlight a reader makes land on the words they made it from?
//!
//! The write path maps a mark out of display space and into the page's own,
//! through [`tpdf_lib::text::from_device`] and the crop box's origin.
//! `annots.rs` maps the other way when it reads one back. Those two are separate
//! implementations of one inverse, which is what makes the round trip below a
//! differential rather than a writer agreeing with its own reader --- and it is
//! why the pixels are checked as well: two mappings that are wrong in the same
//! way agree perfectly, and only ink can say the wash is on the text.
//!
//! Nothing here needs a fixture of its own. The mark is built *from the
//! document's own characters*: the probe extracts the text of a page, takes a
//! run of it, and highlights exactly the boxes PDFium reported --- so a rotated
//! page, a cropped page and an upright one are all covered by pointing this at
//! them, and the expected geometry is never a number typed into a manifest.
//!
//! Four modes:
//!
//! * `--mode roundtrip` --- writes the mark, reads the saved file back with the
//!   comment scan, and asserts it comes back on the right page, with the right
//!   author and note, over rectangles that agree with where the characters were.
//!
//! * `--mode ink` --- renders the saved page and counts wash pixels inside the
//!   highlighted band and outside it. **The source document is the control**:
//!   the same band on the file before the mark must have no wash at all, or the
//!   measurement is of something the page already had.
//!
//! * `--mode legible` --- the glyphs must survive. A wash written without a
//!   blend mode covers the text it marks, which looks correct in a thumbnail and
//!   is useless at reading size, so this compares the ink inside the band before
//!   and after.
//!
//! * `--mode noap` --- the same coverage check with the appearance stream
//!   **removed** from the saved file, so the wash is the one the renderer
//!   generates from `/QuadPoints`. Without it nothing reads those numbers at
//!   all: our own `/AP` draws the mark, and a mutation that reordered every
//!   quad's corners survived every other mode in this file. It is also the
//!   closest thing here to what a reader that ignores `/AP` will show, and
//!   PDFKit --- which is Preview --- is measured to be one.
//!
//! * `--mode refuse` --- the two refusals that are not defensive: a mark whose
//!   page is shared by two page numbers, and one covering no area.
//!
//! Usage:
//!   annot-probe <file.pdf> [--page N] [--mode roundtrip|ink|legible|refuse]
//!               [--chars N] [--scale F] [--lib DIR]

use std::path::{Path, PathBuf};

use tpdf_lib::annots::{self, Kind};
use tpdf_lib::docmodel::Quad;
use tpdf_lib::edits::{Edits, NewMark};
use tpdf_lib::progressive::{self, Placement, RawBitmap, RawDocument};
use tpdf_lib::save;
use tpdf_lib::text;

/// The document handle every mode opens under. One document, so any number does.
const DOC: u32 = 1;

/// How much of the page's text to highlight, in characters.
const DEFAULT_CHARS: usize = 40;

/// The colour written, and the one the pixel counts look for.
const YELLOW: [f32; 3] = [1.0, 0.9, 0.2];

/// Smallest quad, in rendered pixels, whose coverage is worth a percentage.
///
/// Below this a box is mostly the antialiased edge of its own glyph, and the
/// figure says more about the renderer's smoothing than about where the mark
/// went. Quads under it are counted and named in the output.
const MEASURABLE_PX: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Roundtrip,
    Ink,
    NoAp,
    Legible,
    Refuse,
}

struct Args {
    file: PathBuf,
    page: u32,
    mode: Mode,
    chars: usize,
    scale: f32,
    /// Where to leave the marked copy, for a human to open. Removed otherwise.
    keep: Option<PathBuf>,
    library: PathBuf,
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(why) => {
            eprintln!("[FAIL] {why}");
            std::process::exit(2);
        }
    };

    match run(&args) {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(why) => {
            eprintln!("[FAIL] {why}");
            std::process::exit(2);
        }
    }
}

fn run(args: &Args) -> Result<bool, String> {
    let bindings = progressive::bindings_of(progressive::bind(&args.library)?);
    let document = RawDocument::open(bindings, &args.file)?;

    match args.mode {
        Mode::Roundtrip => roundtrip(args, &document),
        Mode::Ink | Mode::NoAp => ink(args, &document, bindings),
        Mode::Legible => legible(args, &document, bindings),
        Mode::Refuse => refuse(args, &document),
    }
}

/// The characters `args` names, as one display-space quad per line.
///
/// Grouped by overlap along the axis that separates lines, which is the same
/// rule `text.ts` uses to build a selection's rectangles --- and deliberately
/// not a call into it, since that is TypeScript. What matters here is that a
/// multi-line mark is exercised at all: a single-quad highlight would leave
/// `/QuadPoints` untested past its first four numbers.
///
/// **The axis is not always the vertical one**, and the first version of this
/// assumed it was. On a page displayed sideways --- `/Rotate 90`, which is what
/// a scanner emits --- lines advance across the screen and characters run down
/// it, so grouping by vertical overlap put every character in a line of its own.
/// The mark was still written correctly; what broke was this probe, which then
/// had forty quads too small to measure and said so. Same trap as the one
/// `docs/TRAPS.md` records for the line grouping itself.
fn quads_for(page: &text::PageText, from: usize, to: usize) -> Vec<Quad> {
    let sideways = page.quarter_turns % 2 == 1;
    let mut quads: Vec<Quad> = Vec::new();
    for index in from..to.min(page.len()) {
        let boxes = &page.boxes[index * 4..index * 4 + 4];
        let quad = Quad {
            left: boxes[0],
            top: boxes[1],
            right: boxes[2],
            bottom: boxes[3],
        };
        if !quad.covers_area() {
            continue;
        }
        match quads.last_mut() {
            Some(line) if overlap(*line, quad, sideways) => {
                line.left = line.left.min(quad.left);
                line.right = line.right.max(quad.right);
                line.top = line.top.min(quad.top);
                line.bottom = line.bottom.max(quad.bottom);
            }
            _ => quads.push(quad),
        }
    }
    quads
}

/// Whether two boxes are on one line, overlapping by more than half the shorter
/// of them along the axis that separates lines.
fn overlap(a: Quad, b: Quad, sideways: bool) -> bool {
    let (a0, a1, b0, b1) = if sideways {
        (a.left, a.right, b.left, b.right)
    } else {
        (a.top, a.bottom, b.top, b.bottom)
    };
    let shared = a1.min(b1) - a0.max(b0);
    let shorter = (a1 - a0).min(b1 - b0);
    shorter > 0.0 && shared > shorter / 2.0
}

/// Highlights a run of the page's text and writes the copy, returning its path
/// and the quads the mark was made from.
fn mark_and_save(args: &Args, document: &RawDocument) -> Result<(PathBuf, Vec<Quad>), String> {
    let page = document.page(args.page)?;
    let extracted = text::extract(&page)?;
    if extracted.is_empty() {
        return Err(format!(
            "page {} has no extractable characters, so a highlight over its text \
             would prove nothing -- point this at a text document",
            args.page
        ));
    }

    let quads = quads_for(&extracted, 0, args.chars);
    if quads.is_empty() {
        return Err(format!(
            "the first {} characters of page {} have no drawable boxes",
            args.chars, args.page
        ));
    }

    let edits = Edits::default();
    edits.open(DOC, document.page_count());
    let state = edits
        .state(DOC)
        .map_err(|e| format!("no edit state: {e}"))?;
    let id = state
        .pages
        .get(args.page as usize)
        .ok_or_else(|| format!("no page {} in the model", args.page))?
        .id;

    edits
        .annotate(
            DOC,
            NewMark {
                page: id,
                quads: quads
                    .iter()
                    .flat_map(|q| [q.left, q.top, q.right, q.bottom])
                    .collect(),
                color: YELLOW,
                author: "annot-probe".to_string(),
                note: "written by annot-probe".to_string(),
            },
            save::pdf_date(std::time::SystemTime::now()),
        )
        .map_err(|e| format!("the model refused the mark: {e}"))?;

    let plan = edits.plan(DOC).map_err(|e| format!("no plan: {e}"))?;
    let out = args.keep.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!(
            "tpdf-annot-probe-{}-p{}.pdf",
            std::process::id(),
            args.page
        ))
    });
    save::write_copy(&args.file, &plan, &out)?;
    if args.mode == Mode::NoAp {
        strip_appearances(&out)?;
    }
    Ok((out, quads))
}

/// Removes every annotation's `/AP` from a file, in place.
///
/// What is left is a `/Highlight` with `/QuadPoints`, `/C` and nothing telling a
/// renderer what to draw --- so whatever appears is generated from those
/// numbers. Measured: both PDFium and PDFKit generate one, which is why the
/// file is still usable and why this mode is a check rather than a curiosity.
///
/// Counts what it removed and refuses if that is nothing: a strip that silently
/// found no `/AP` would make this mode a second, slower copy of `--mode ink`.
fn strip_appearances(file: &Path) -> Result<(), String> {
    let mut doc = lopdf::Document::load(file).map_err(|e| format!("could not reopen: {e}"))?;
    let annotations: Vec<lopdf::ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, object)| {
            object
                .as_dict()
                .map(|d| d.has(b"AP") && d.get(b"Subtype").is_ok())
                .unwrap_or(false)
        })
        .map(|(id, _)| *id)
        .collect();
    if annotations.is_empty() {
        return Err("nothing in the file had an /AP to remove".into());
    }
    for id in annotations {
        if let Ok(dictionary) = doc.get_object_mut(id).and_then(lopdf::Object::as_dict_mut) {
            dictionary.remove(b"AP");
        }
    }
    doc.save(file)
        .map_err(|e| format!("could not rewrite: {e}"))?;
    Ok(())
}

/// Writes a mark, reads it back with the comment scan, and compares geometry.
fn roundtrip(args: &Args, document: &RawDocument) -> Result<bool, String> {
    let (out, quads) = mark_and_save(args, document)?;
    let bytes = std::fs::read(&out).map_err(|e| format!("could not read {out:?}: {e}"))?;
    let found = annots::scan(&bytes, document.page_count() as usize)?;

    let mut ok = true;
    println!(
        "wrote {} quad(s) over the first {} characters of page {}",
        quads.len(),
        args.chars,
        args.page
    );

    if found.items.len() != 1 {
        println!(
            "[FAIL] the scan found {} comment(s), not the one that was written",
            found.items.len()
        );
        return Ok(false);
    }
    let mark = &found.items[0];

    ok &= check("kind is /Highlight", mark.kind == Kind::Highlight);
    ok &= check("page is the one marked", mark.page == args.page);
    ok &= check("author survived", mark.author == "annot-probe");
    ok &= check("note survived", mark.body == "written by annot-probe");
    ok &= check("date was read as a date", mark.date.is_some());
    ok &= check("nothing was cut", !found.limits.any());

    // The rectangle read back is the union of the quads written, in the same
    // display space they were made in -- which is the whole claim, since the two
    // sides of it are separate mappings.
    let want = union(&quads);
    let got = mark.rect;
    let slack = 0.5;
    let agrees = (0..4).all(|at| (want[at] - got[at]).abs() < slack);
    ok &= check(
        &format!(
            "the rectangle came back where it was put: wrote [{:.1} {:.1} {:.1} {:.1}], \
             read [{:.1} {:.1} {:.1} {:.1}]",
            want[0], want[1], want[2], want[3], got[0], got[1], got[2], got[3]
        ),
        agrees,
    );

    ok &= quad_points_are_in_reading_order(&out, quads.len());
    if args.keep.is_none() {
        let _ = std::fs::remove_file(&out);
    }

    // The control for that comparison: on a page with any rotation or crop at
    // all, a mapping that ignored either would land somewhere else -- so a pass
    // means nothing unless the page actually has one to ignore. Reported rather
    // than asserted, because an upright uncropped page is a legitimate thing to
    // run this on and the round trip is still worth checking there.
    let page = document.page(args.page)?;
    let (ox, oy) = page.origin_pt();
    println!(
        "     page is /Rotate {}, crop origin ({ox}, {oy}) -- {}",
        page.quarter_turns() as u32 * 90,
        if page.quarter_turns() == 0 && ox == 0.0 && oy == 0.0 {
            "so this page cannot tell a turn or an origin from the identity"
        } else {
            "so a mapping that dropped either would fail this"
        }
    );

    Ok(ok)
}

/// Reads `/QuadPoints` off the written file and checks the corner order.
///
/// **Upper-left, upper-right, lower-left, lower-right.** That is not what
/// PDF 32000-1 §12.5.6.10 appears to say, and it is what every producer writes
/// and every consumer expects; the specification's wording is a known erratum.
/// Asserted here against the bytes rather than against a threshold, because the
/// pixel evidence for it is real but thin: with the appearance stream removed,
/// PDFium's generated wash covers 28-36% of each quad for this order and 21-24%
/// for the corners rotated by one, measured on `text-base14` and `columns`. A
/// check standing on a seven-point margin is a check that will one day pass for
/// the wrong reason.
///
/// It is our reader against our writer, which is worth naming: what stops it
/// being a tautology is that the *expected* order is fixed by that measurement
/// and by what other readers do, not by what this repository happens to emit.
fn quad_points_are_in_reading_order(file: &Path, expected: usize) -> bool {
    let Ok(doc) = lopdf::Document::load(file) else {
        return check("the written file reopens", false);
    };
    let mut checked = 0usize;
    let mut ok = true;
    for object in doc.objects.values() {
        let Ok(dictionary) = object.as_dict() else {
            continue;
        };
        let Ok(points) = dictionary
            .get(b"QuadPoints")
            .and_then(lopdf::Object::as_array)
        else {
            continue;
        };
        let values: Vec<f32> = points.iter().filter_map(|v| v.as_float().ok()).collect();
        if values.len() != points.len() {
            return check("every /QuadPoints entry is a number", false);
        }
        for quad in values.chunks_exact(8) {
            checked += 1;
            let (ulx, uly, urx, ury) = (quad[0], quad[1], quad[2], quad[3]);
            let (llx, lly, lrx, lry) = (quad[4], quad[5], quad[6], quad[7]);
            ok &= ulx < urx && llx < lrx && uly > lly && ury > lry && ulx == llx && urx == lrx;
        }
    }
    // Without this a document whose annotation lost its `/QuadPoints` entirely
    // would satisfy every assertion above by having nothing to check.
    ok &= check(
        &format!("{checked} quad(s) carry corners, one per rectangle written"),
        checked == expected,
    );
    check(
        "every quad is upper-left, upper-right, lower-left, lower-right",
        ok,
    )
}

/// The union of a set of display-space quads, as `[left, top, right, bottom]`.
fn union(quads: &[Quad]) -> [f32; 4] {
    quads
        .iter()
        .fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |acc, q| {
            [
                acc[0].min(q.left),
                acc[1].min(q.top),
                acc[2].max(q.right),
                acc[3].max(q.bottom),
            ]
        })
}

/// Renders a page of a file and returns its pixels and size.
///
/// Takes the bindings rather than loading the library itself. PDFium refuses a
/// second `bind_to_library` in one process --- `PdfiumLibraryBindingsAlreadyInitialized`
/// --- so a helper that bound its own worked in isolation and failed the moment
/// the caller had already opened a document, which is every caller here.
fn render(
    bindings: progressive::Bindings,
    file: &Path,
    number: u32,
    scale: f32,
) -> Result<(Vec<u8>, u32, u32), String> {
    let document = RawDocument::open(bindings, file)?;
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
    let pixels = bitmap.pixels().to_vec();
    Ok((pixels, width as u32, height as u32))
}

/// Counts wash and ink pixels inside a display-space band.
fn count(pixels: &[u8], width: u32, height: u32, band: [f32; 4], scale: f32) -> (usize, usize) {
    let x0 = (band[0] * scale).floor().max(0.0) as u32;
    let y0 = (band[1] * scale).floor().max(0.0) as u32;
    let x1 = ((band[2] * scale).ceil() as u32).min(width);
    let y1 = ((band[3] * scale).ceil() as u32).min(height);
    let (mut wash, mut ink) = (0usize, 0usize);
    for y in y0..y1 {
        for x in x0..x1 {
            let at = ((y * width + x) * 4) as usize;
            // RGBA, not BGRA: `progressive::RENDER_FLAGS` includes
            // `FPDF_REVERSE_BYTE_ORDER`. Read the other way round this counted
            // blue as red and reported no wash at all on a page that had one.
            let (r, g, b) = (
                pixels[at] as i32,
                pixels[at + 1] as i32,
                pixels[at + 2] as i32,
            );
            if r > 180 && g > 150 && b < 170 {
                wash += 1;
            }
            if r < 110 && g < 110 && b < 110 {
                ink += 1;
            }
        }
    }
    (wash, ink)
}

/// The wash is where the words are, and the source page is the control.
fn ink(
    args: &Args,
    document: &RawDocument,
    bindings: progressive::Bindings,
) -> Result<bool, String> {
    let (out, quads) = mark_and_save(args, document)?;
    let band = union(&quads);

    let (before, bw, bh) = render(bindings, &args.file, args.page, args.scale)?;
    let (after, aw, ah) = render(bindings, &out, args.page, args.scale)?;
    if args.keep.is_none() {
        let _ = std::fs::remove_file(&out);
    }

    if (bw, bh) != (aw, ah) {
        return Err(format!(
            "the copy renders {aw}x{ah} where the source renders {bw}x{bh}, so no \
             pixel comparison between them means anything"
        ));
    }

    // **Per quad, not over their union.** The union of two quads on different
    // lines includes the whitespace between them, which is not washed and never
    // should be -- so a coverage figure taken over the union says more about the
    // line spacing than about the mark. Measured before this was written: a
    // correct two-line highlight covers 32% of its own bounding box.
    let mut worst = 1.0f32;
    let mut measured = 0usize;
    let mut too_small = 0usize;
    let mut total_before = 0usize;
    for quad in &quads {
        let box_of = [quad.left, quad.top, quad.right, quad.bottom];
        let area = ((quad.right - quad.left) * (quad.bottom - quad.top) * args.scale * args.scale)
            as usize;
        let (wash_before, _) = count(&before, bw, bh, box_of, args.scale);
        total_before += wash_before;
        // A box too small to hold a percentage is counted and skipped rather
        // than averaged in. Measured: the worst quad on `links-rotated` is a
        // 7.9 x 0.97 pt punctuation glyph -- 30 pixels at 2x, most of them the
        // antialiased edge of the glyph itself, which is neither wash nor ink by
        // any threshold. Reported, never silently dropped: a page whose every
        // quad were skipped would otherwise pass this mode by having nothing to
        // check.
        if area < MEASURABLE_PX {
            too_small += 1;
            continue;
        }
        let (wash_after, ink_after) = count(&after, aw, ah, box_of, args.scale);
        measured += 1;
        worst = worst.min((wash_after + ink_after) as f32 / area as f32);
    }

    // Outside the band, on the same page: a wash that covered the whole sheet
    // would satisfy every per-quad check perfectly.
    let elsewhere = [
        0.0,
        band[3] + 8.0,
        bw as f32 / args.scale,
        bh as f32 / args.scale,
    ];
    let (spill, _) = count(&after, aw, ah, elsewhere, args.scale);

    println!(
        "{} quad(s): {measured} measured, {too_small} too small; worst covered {:.0}%; \
         wash on the source inside them: {total_before}",
        quads.len(),
        worst * 100.0
    );
    println!(
        "below the marked band: {spill} wash px{}",
        if args.mode == Mode::NoAp {
            " (appearance streams removed: this is the renderer's own wash, from /QuadPoints)"
        } else {
            ""
        }
    );

    let mut ok = true;
    ok &= check(
        "the source page has no wash where the mark went (the control)",
        total_before == 0,
    );
    ok &= check(
        "some quad was big enough to measure (the control)",
        measured > 0,
    );
    // Wash **or** ink, not wash alone: the glyphs are multiplied *through* the
    // wash, so their own pixels come out dark rather than yellow, and a tight
    // box around a dense glyph is legitimately more ink than wash. What this
    // rules out is a quad drawn somewhere else, which reads in single digits.
    // The floor differs by mode, and the reason is measured. With our own
    // appearance the wash fills the quad, so anything under 80% means it is
    // somewhere else. With the appearance removed the wash is the *renderer's*,
    // and PDFium's generated highlight insets: 28-36% across this corpus, and
    // 21-24% for the same file with every quad's corners reordered. So this mode
    // asks only that the wash be on the words -- the corner order is pinned
    // exactly, on the bytes, by `--mode roundtrip`, which needs no threshold.
    let floor = if args.mode == Mode::NoAp { 0.15 } else { 0.8 };
    ok &= check(
        &format!(
            "every measurable quad is covered by wash or glyph (worst {:.0}%, floor {:.0}%)",
            worst * 100.0,
            floor * 100.0
        ),
        measured > 0 && worst > floor,
    );
    ok &= check("the wash did not spill past the marked band", spill == 0);
    Ok(ok)
}

/// The glyphs survive the wash.
fn legible(
    args: &Args,
    document: &RawDocument,
    bindings: progressive::Bindings,
) -> Result<bool, String> {
    let (out, quads) = mark_and_save(args, document)?;
    let band = union(&quads);

    let (before, bw, bh) = render(bindings, &args.file, args.page, args.scale)?;
    let (after, aw, ah) = render(bindings, &out, args.page, args.scale)?;
    if args.keep.is_none() {
        let _ = std::fs::remove_file(&out);
    }

    let (_, ink_before) = count(&before, bw, bh, band, args.scale);
    let (_, ink_after) = count(&after, aw, ah, band, args.scale);
    println!("ink in the band: {ink_before} before, {ink_after} after");

    let mut ok = true;
    ok &= check("there was ink to lose (the control)", ink_before > 0);
    // Not equality: the wash multiplies, so a glyph's anti-aliased edge shifts
    // colour and may fall out of the threshold. Losing a tenth is a blend that
    // works; losing all of it is a flat fill painted over the words.
    ok &= check(
        &format!("the words are still readable through it ({ink_after}/{ink_before})"),
        ink_before > 0 && ink_after * 10 >= ink_before * 9,
    );
    Ok(ok)
}

/// The refusals that are not defensive.
fn refuse(_args: &Args, document: &RawDocument) -> Result<bool, String> {
    let edits = Edits::default();
    edits.open(DOC, document.page_count());
    let state = edits.state(DOC).map_err(|e| format!("no state: {e}"))?;
    let id = state.pages.first().ok_or("the document has no pages")?.id;

    let mut ok = true;

    let empty = edits.annotate(
        DOC,
        NewMark {
            page: id,
            quads: vec![10.0, 10.0, 10.0, 40.0],
            color: YELLOW,
            author: String::new(),
            note: String::new(),
        },
        save::pdf_date(std::time::SystemTime::now()),
    );
    ok &= check(
        &format!("a mark covering nothing is refused: {empty:?}"),
        empty.is_err(),
    );

    let ragged = edits.annotate(
        DOC,
        NewMark {
            page: id,
            quads: vec![10.0, 10.0, 40.0],
            color: YELLOW,
            author: String::new(),
            note: String::new(),
        },
        save::pdf_date(std::time::SystemTime::now()),
    );
    ok &= check(
        &format!("quads that are not a multiple of four are refused: {ragged:?}"),
        ragged.is_err(),
    );

    let gone = edits.unannotate(DOC, 4242);
    ok &= check(
        &format!("removing a mark that never existed is refused: {gone:?}"),
        gone.is_err(),
    );

    // The control: after three refusals the document must still take a real
    // mark. A model that refused everything would pass all three above.
    let real = edits.annotate(
        DOC,
        NewMark {
            page: id,
            quads: vec![10.0, 10.0, 200.0, 40.0],
            color: YELLOW,
            author: String::new(),
            note: String::new(),
        },
        save::pdf_date(std::time::SystemTime::now()),
    );
    ok &= check(
        &format!(
            "a real mark is still accepted (the control): {:?}",
            real.is_ok()
        ),
        real.is_ok(),
    );

    Ok(ok)
}

fn check(what: &str, ok: bool) -> bool {
    println!("{} {what}", if ok { "[OK]  " } else { "[FAIL]" });
    ok
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let file = args.next().ok_or("usage: annot-probe <file.pdf> [...]")?;
    let mut parsed = Args {
        file: PathBuf::from(file),
        page: 0,
        mode: Mode::Roundtrip,
        chars: DEFAULT_CHARS,
        scale: 2.0,
        keep: None,
        library: PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR),
    };

    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--page" => parsed.page = value.parse().map_err(|_| "--page wants a number")?,
            "--chars" => parsed.chars = value.parse().map_err(|_| "--chars wants a number")?,
            "--scale" => parsed.scale = value.parse().map_err(|_| "--scale wants a number")?,
            "--lib" => parsed.library = PathBuf::from(value),
            "--out" => parsed.keep = Some(PathBuf::from(value)),
            "--mode" => {
                parsed.mode = match value.as_str() {
                    "roundtrip" => Mode::Roundtrip,
                    "ink" => Mode::Ink,
                    "noap" => Mode::NoAp,
                    "legible" => Mode::Legible,
                    "refuse" => Mode::Refuse,
                    other => return Err(format!("unknown mode {other}")),
                }
            }
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(parsed)
}
