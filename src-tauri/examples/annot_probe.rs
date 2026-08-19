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
use tpdf_lib::docmodel::{MarkKind, Quad};
use tpdf_lib::edits::{Edits, NewMark};
use tpdf_lib::progressive::{self, Placement, RawBitmap, RawDocument};
use tpdf_lib::save;
use tpdf_lib::save::OUTLINE_WIDTH;
use tpdf_lib::text;

/// The document handle every mode opens under. One document, so any number does.
const DOC: u32 = 1;

/// How much of the page's text to highlight, in characters.
const DEFAULT_CHARS: usize = 40;

/// What the probe types on the mark, read back out of the written file.
const NOTE: &str = "written by annot-probe";

/// The colour written, and the one the pixel counts look for.
const YELLOW: [f32; 3] = [1.0, 0.9, 0.2];

/// The colour a line kind is written in, mirroring `edits.ts`'s `MARK_COLORS`.
///
/// **Not the wash's yellow, and the reason is what `--mode rule` measures.** A
/// 0.9 pt yellow rule on white paper is close to invisible, which is why the
/// application does not draw one --- and a probe that sent yellow anyway would
/// be measuring a mark no reader will ever see. The first run of that mode did
/// exactly that and reported zero rule pixels, which reads like a renderer
/// ignoring our appearance stream rather than like a probe using the wrong
/// colour.
const RULE_RED: [f32; 3] = [0.85, 0.15, 0.15];

/// The colour the probe writes for a kind.
fn color_for(kind: MarkKind) -> [f32; 3] {
    match kind {
        MarkKind::Highlight => YELLOW,
        MarkKind::Underline | MarkKind::StrikeOut => RULE_RED,
        // The wash's yellow, matching `MARK_COLORS` in `edits.ts`: `/C` is what
        // a reader colours its own comment icon with, so this is the colour the
        // bubble comes out in everywhere else.
        MarkKind::Note => YELLOW,
        // The lines' red, matching `MARK_COLORS` in `edits.ts`. A box's ink is
        // a stroke, and `--mode outline` classifies pixels by the colour it
        // asked for --- so a yellow box on white paper would be measured as an
        // absence, which is the mistake `RULE_RED`'s own comment records.
        MarkKind::Square => RULE_RED,
    }
}

/// Smallest quad, in rendered pixels, whose coverage is worth a percentage.
///
/// Below this a box is mostly the antialiased edge of its own glyph, and the
/// figure says more about the renderer's smoothing than about where the mark
/// went. Quads under it are counted and named in the output.
const MEASURABLE_PX: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Roundtrip,
    /// Where a line kind's rule actually lands, in pixels.
    Rule,
    /// That a box is a frame and not a filled rectangle, in pixels.
    Outline,
    Ink,
    NoAp,
    Legible,
    Refuse,
}

struct Args {
    file: PathBuf,
    page: u32,
    /// Which of the three marks to write. Every mode that writes one uses it,
    /// so `--kind underline` re-runs the whole roundtrip against a line rather
    /// than a wash --- the subtype, the appearance geometry and the opacity all
    /// change, and nothing else does.
    kind: MarkKind,
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
        Mode::Rule => rule(args, &document, bindings),
        Mode::Outline => outline(args, &document, bindings),
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
    // **One rectangle for a box**, which is what the application sends: a box
    // comes from a drag, and a drag produces one. The other kinds come from a
    // text selection, which legitimately produces one quad per line, and
    // collapsing those would be measuring a mark tpdf never makes. Done here
    // rather than in the caller so that every mode sees the same shape --- the
    // round trip's quad count and the outline's emptiness band would otherwise
    // be reading two different marks.
    let quads = if matches!(args.kind, MarkKind::Square) {
        let box_ = union(&quads);
        vec![Quad {
            left: box_[0],
            top: box_[1],
            right: box_[2],
            bottom: box_[3],
        }]
    } else {
        quads
    };
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

    let made = edits
        .annotate(
            DOC,
            NewMark {
                kind: args.kind,
                page: id,
                quads: quads
                    .iter()
                    .flat_map(|q| [q.left, q.top, q.right, q.bottom])
                    .collect(),
                color: color_for(args.kind),
                author: "annot-probe".to_string(),
                note: String::new(),
            },
            save::pdf_date(std::time::SystemTime::now()),
        )
        .map_err(|e| format!("the model refused the mark: {e}"))?;

    // Typed afterwards rather than passed above, which is the route a reader
    // actually takes: a highlight is made from a selection with nothing to say,
    // and the note arrives as a separate command that undo can step over. The
    // two routes end in the same `/Contents`, so covering this one covers both,
    // and it is the only one where the text has to survive a journal.
    let mark = made
        .marks
        .first()
        .ok_or("the state carried no mark to note")?
        .id;
    edits
        .renote(DOC, mark, NOTE.to_string())
        .map_err(|e| format!("the model refused the note: {e}"))?;

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

    // The reader's vocabulary against the writer's, which are two enums in two
    // modules that meet only in the file. A wrong subtype draws correctly from
    // our own `/AP` and is reported as the wrong kind by every other program,
    // so this is the assertion no rendering check can stand in for.
    let expected = match args.kind {
        MarkKind::Highlight => Kind::Highlight,
        MarkKind::Underline => Kind::Underline,
        MarkKind::StrikeOut => Kind::StrikeOut,
        // The one pair whose two names differ. `MarkKind::Note` is what a
        // reader calls it and `Kind::Text` is what the file calls it, so this
        // arm is the round trip that says `save.rs` wrote `/Text` and
        // `annots.rs` read it back as the same thing --- the two enums meeting
        // in the file, which is exactly what this block exists to check.
        MarkKind::Note => Kind::Text,
        // The one pair whose two names agree, and it earns an arm by saying so:
        // `/Square` is what the writer emits and what the reader reads, and the
        // word "box" a reader actually sees is in neither enum.
        MarkKind::Square => Kind::Square,
    };
    ok &= check(
        &format!("kind read back as {expected:?}"),
        mark.kind == expected,
    );
    ok &= check("page is the one marked", mark.page == args.page);
    ok &= check("author survived", mark.author == "annot-probe");
    ok &= check("note survived", mark.body == NOTE);
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

    // **A comment expects none, and that is asserted rather than skipped.**
    // `/QuadPoints` is a text-markup key and `/Text` is not a markup subtype, so
    // the right number here is zero --- and passing zero puts it through the
    // same counting check the other kinds use rather than stepping around it.
    // Skipping would have been the reassuring branch: a writer that stopped
    // emitting quads for *everything* looks identical to one that correctly
    // omits them here, which is why the three markup kinds keep asserting a
    // real count in the same run.
    // A box expects none for the same reason by a different route: it is not a
    // markup subtype either, and `is_text_markup` in `save.rs` is the single
    // predicate both of them go through.
    let expected_quads = if matches!(args.kind, MarkKind::Note | MarkKind::Square) {
        0
    } else {
        quads.len()
    };
    ok &= quad_points_are_in_reading_order(&out, expected_quads);
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

/// Counts pixels of the line colour inside a display-space band.
///
/// A separate classifier from [`count`]'s, and it has to be: that one calls a
/// pixel "wash" when it is yellow-ish and "ink" when it is dark, and a red rule
/// at (217, 38, 38) is neither. Measured, not guessed --- the first draft reused
/// `count` and reported zero rule pixels everywhere, which reads exactly like an
/// appearance stream the renderer ignored.
fn rule_pixels(
    pixels: &[u8],
    width: u32,
    height: u32,
    band: [f32; 4],
    scale: f32,
    want: [f32; 3],
) -> usize {
    let target = want.map(|c| (c * 255.0) as i32);
    let x0 = (band[0] * scale).floor().max(0.0) as u32;
    let y0 = (band[1] * scale).floor().max(0.0) as u32;
    let x1 = ((band[2] * scale).ceil() as u32).min(width);
    let y1 = ((band[3] * scale).ceil() as u32).min(height);
    let mut found = 0usize;
    for y in y0..y1 {
        for x in x0..x1 {
            let at = ((y * width + x) * 4) as usize;
            // RGBA -- see `count`, which records why reading this the other way
            // round once reported no wash on a page that had one.
            let (r, g, b) = (
                pixels[at] as i32,
                pixels[at + 1] as i32,
                pixels[at + 2] as i32,
            );
            // Near the colour the probe asked for, rather than a hardcoded
            // red: the classifier and the mark then cannot disagree, which is
            // the whole of what went wrong the first time this ran.
            if (r - target[0]).abs() < 40
                && (g - target[1]).abs() < 40
                && (b - target[2]).abs() < 40
            {
                found += 1;
            }
        }
    }
    found
}

/// A line kind draws its rule in the half of the quad its kind names.
///
/// **The check no file-level assertion can make.** `save.rs`'s tests prove the
/// rectangle written into the appearance stream is inside the quad and at the
/// right height, and `--mode roundtrip` proves the subtype survives a save and a
/// reopen. Neither says the *renderer* honours our `/AP` rather than generating
/// its own --- PDFium does generate appearances for markup annotations that have
/// none, and a reader looking at the page sees whatever it decided.
///
/// So: render before and after, and ask where the new red pixels are. An
/// underline's belong in the bottom half of the quad and nowhere in the top; a
/// strikeout's cross the middle. The two assertions together are what tells the
/// kinds apart --- "some red appeared" is satisfied by either one drawn wrongly.
fn rule(
    args: &Args,
    document: &RawDocument,
    bindings: progressive::Bindings,
) -> Result<bool, String> {
    if !matches!(args.kind, MarkKind::Underline | MarkKind::StrikeOut) {
        return Err(
            "--mode rule is for a line kind: pass --kind underline or --kind strikeout.              A highlight fills its quad, which is what --mode legible measures; a note              draws no ink of ours at all, since the reader synthesises its icon; and a box              draws on all four edges, which is what --mode outline measures."
                .to_string(),
        );
    }
    let (out, quads) = mark_and_save(args, document)?;

    let (before, bw, bh) = render(bindings, &args.file, args.page, args.scale)?;
    let (after, aw, ah) = render(bindings, &out, args.page, args.scale)?;
    if args.keep.is_none() {
        let _ = std::fs::remove_file(&out);
    }
    if (bw, bh) != (aw, ah) {
        return Err(format!(
            "the copy renders {aw}x{ah} where the source renders {bw}x{bh}, so no              pixel comparison between them means anything"
        ));
    }

    let (mut top, mut bottom, mut middle, mut before_any) = (0usize, 0usize, 0usize, 0usize);
    for quad in &quads {
        let full = quad.bottom - quad.top;
        // Thirds of the quad, in display space. The middle band is where a
        // strikeout goes and the bottom where an underline does; the top is the
        // control, since neither may draw there.
        let bands = [
            [quad.left, quad.top, quad.right, quad.top + full / 3.0],
            [
                quad.left,
                quad.top + full / 3.0,
                quad.right,
                quad.top + 2.0 * full / 3.0,
            ],
            [
                quad.left,
                quad.top + 2.0 * full / 3.0,
                quad.right,
                quad.bottom,
            ],
        ];
        let want = color_for(args.kind);
        top += rule_pixels(&after, aw, ah, bands[0], args.scale, want);
        middle += rule_pixels(&after, aw, ah, bands[1], args.scale, want);
        bottom += rule_pixels(&after, aw, ah, bands[2], args.scale, want);
        before_any += rule_pixels(&before, bw, bh, bands[0], args.scale, want)
            + rule_pixels(&before, bw, bh, bands[1], args.scale, want)
            + rule_pixels(&before, bw, bh, bands[2], args.scale, want);
    }

    println!(
        "{:?}: {top} px in the top third, {middle} in the middle, {bottom} in the bottom",
        args.kind
    );

    let mut ok = true;
    ok &= check(
        "the source page has no rule where the mark went (the control)",
        before_any == 0,
    );
    ok &= check("the renderer drew a rule at all", top + middle + bottom > 0);
    let (wanted, forbidden, where_) = match args.kind {
        MarkKind::Underline => (bottom, top, "bottom"),
        MarkKind::StrikeOut => (middle, bottom, "middle"),
        MarkKind::Highlight => unreachable!("refused above"),
        // Refused above with the highlight, and for a stronger reason: a
        // highlight draws a rule nowhere because it is a wash, and a comment
        // draws no ink of ours at all --- `save.rs` writes it no appearance
        // stream, so what appears on the page is the reader's own icon. There
        // is nothing here for a band measurement to be about.
        MarkKind::Note => unreachable!("refused above"),
        // Refused above, and a third distinct reason: a box draws ink in all
        // three bands, because its ink is on its edges rather than in a band
        // inside it. Thirds of the quad cannot discriminate anything about it,
        // which is what `--mode outline` exists for.
        MarkKind::Square => unreachable!("refused above"),
    };
    ok &= check(
        &format!("most of the rule is in the {where_} third ({wanted} px)"),
        wanted * 2 > top + middle + bottom,
    );
    // The discrimination, and the reason this is two assertions rather than
    // one: an underline drawn across the middle satisfies "there is a rule" and
    // "it is inside the quad", and only a band that must be *empty* separates
    // the two kinds by pixels.
    ok &= check(
        &format!("nothing was drawn in the band this kind must leave alone ({forbidden} px)"),
        forbidden == 0,
    );
    Ok(ok)
}

/// A box is a frame: ink on its edges and nothing inside it.
///
/// **The one measurement that separates `re S` from `re f`.** Everything a file
/// assertion can say about a `/Square` --- the subtype, the rectangle, the
/// absence of `/QuadPoints`, that an `/AP` exists at all --- is satisfied
/// equally by a stroked box and a solid block of colour, and the solid block is
/// what a one-character slip in the content stream produces. It is also the
/// exact failure a reader would report, because a filled box hides the figure it
/// was drawn around.
///
/// Three readings, and the middle one is the assertion:
///
///  * the source page, in the same rectangle, as the control;
///  * the frame --- the whole quad --- which must have ink;
///  * the middle, inset well clear of the stroke, which must have none.
///
/// The inset is 25% of each side rather than a fixed number of points, so it
/// scales with whatever quad the fixture's text produced, and at any plausible
/// size it clears a 1.5 pt stroke by a wide margin.
fn outline(
    args: &Args,
    document: &RawDocument,
    bindings: progressive::Bindings,
) -> Result<bool, String> {
    if !matches!(args.kind, MarkKind::Square) {
        return Err(
            "--mode outline is for a box: pass --kind square. The other kinds fill              something, which is what --mode legible and --mode rule measure."
                .to_string(),
        );
    }
    let (out, quads) = mark_and_save(args, document)?;
    // One rectangle, which is what the application sends and what `mark_and_save`
    // collapses a multi-line run into for this kind. Asserted rather than
    // assumed: several boxes would put a stroke through the middle band and the
    // emptiness check below would fail for a reason that is not a defect.
    if quads.len() != 1 {
        return Err(format!(
            "a box is one rectangle and this run made {}; --mode outline cannot              read a mark with several",
            quads.len()
        ));
    }

    // **Raised rather than refused**, and it prints what it used. The thickness
    // reading below distinguishes a full stroke from a clipped one by a factor
    // of two, and at the default scale of 2 that is 3 px against 1.5 -- a
    // difference antialiasing swallows. At 4 it is 6 against 3. Refusing would
    // make the documented invocation red at its own default, which is the trap
    // about a control that cannot discriminate being reported as a failure.
    let scale = args.scale.max(4.0);
    if scale != args.scale {
        println!("     rendering at {scale}x rather than {}x: a stroke {OUTLINE_WIDTH} pt thick needs pixels to be measured in", args.scale);
    }
    let (before, bw, bh) = render(bindings, &args.file, args.page, scale)?;
    let (after, aw, ah) = render(bindings, &out, args.page, scale)?;
    if args.keep.is_none() {
        let _ = std::fs::remove_file(&out);
    }
    if (bw, bh) != (aw, ah) {
        return Err(format!(
            "the copy renders {aw}x{ah} where the source renders {bw}x{bh}, so no              pixel comparison between them means anything"
        ));
    }

    let quad = union(&quads);
    let (width, height) = (quad[2] - quad[0], quad[3] - quad[1]);
    let whole = [quad[0], quad[1], quad[2], quad[3]];
    let inside = [
        quad[0] + width / 4.0,
        quad[1] + height / 4.0,
        quad[2] - width / 4.0,
        quad[3] - height / 4.0,
    ];
    // One column, at the box's horizontal centre, over the top quarter and the
    // bottom quarter. `rule_pixels` floors and ceils its bounds, so a band this
    // narrow is exactly one pixel wide, and each edge's stroke is the only ink
    // in its quarter.
    //
    // **Both edges, and the reading is the thinner of the two.** One was not
    // enough, and finding that out is the only reason this is two: the mutation
    // written to prove the check --- dropping `outline_path`'s inset --- left
    // the path's *size* reduced while removing only its origin shift, so it
    // clipped the bottom and left edges and moved the top and right ones
    // *inward*. A probe reading the top alone saw no change at all and reported
    // a pass. A defect that clips one edge is not less of a defect than one
    // that clips four.
    let centre = (quad[0] + quad[2]) / 2.0;
    let edges = [
        [centre, quad[1], centre + 0.01, quad[1] + height / 4.0],
        [centre, quad[3] - height / 4.0, centre + 0.01, quad[3]],
    ];

    let want = color_for(args.kind);
    let frame = rule_pixels(&after, aw, ah, whole, scale, want);
    let middle = rule_pixels(&after, aw, ah, inside, scale, want);
    let thick = edges
        .iter()
        .map(|band| rule_pixels(&after, aw, ah, *band, scale, want))
        .min()
        .unwrap_or(0);
    let control = rule_pixels(&before, bw, bh, whole, scale, want);
    println!(
        "box {width:.1}x{height:.1} pt: {frame} px in the whole quad, {middle} inside it,          {thick} px on its thinner edge, {control} on the source page"
    );

    let mut ok = true;
    ok &= check(
        "the source page has no box where the mark went (the control)",
        control == 0,
    );
    ok &= check(
        &format!("the renderer drew the box at all ({frame} px)"),
        frame > 0,
    );
    // The discrimination. A filled box satisfies the check above exactly as a
    // stroked one does, and this is the only reading that tells them apart.
    ok &= check(
        &format!("the middle of the box is empty ({middle} px)"),
        middle == 0,
    );
    // **That the stroke is not clipped in half**, which is what `outline_path`'s
    // inset is for. Measured rather than reasoned about: this was first written
    // up as something pixels could not see, on the argument that a /BBox clip
    // leaves no ink outside the quad and so nothing to count. True, and beside
    // the point -- it removes ink from *inside*, and dropping the inset costs
    // about a fifth of the frame and half the top edge's thickness. The first
    // account was wrong, and one run said so.
    //
    // 0.7 rather than 1.0 because the classifier's tolerance drops the faintest
    // antialiased row at each end of the run. A clipped stroke is at half.
    let expected = OUTLINE_WIDTH as f32 * scale;
    ok &= check(
        &format!("every edge is its full {OUTLINE_WIDTH} pt, not clipped by the /BBox (thinnest {thick} px of an expected {expected:.1})"),
        thick as f32 >= expected * 0.7,
    );
    Ok(ok)
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
            kind: MarkKind::Highlight,
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
            kind: MarkKind::Highlight,
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
            kind: MarkKind::Highlight,
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
        kind: MarkKind::Highlight,
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
            "--kind" => {
                parsed.kind = match value.as_str() {
                    "highlight" => MarkKind::Highlight,
                    "underline" => MarkKind::Underline,
                    "strikeout" => MarkKind::StrikeOut,
                    // The serde name, which is what the frontend sends and what
                    // a saved session holds. `/Text` is the file's spelling and
                    // is deliberately not accepted here --- one name per thing
                    // at each boundary.
                    "note" => MarkKind::Note,
                    // The serde name again. `/Square` is the file's spelling and
                    // "box" is the reader's, and neither is accepted here.
                    "square" => MarkKind::Square,
                    other => return Err(format!("unknown kind {other}")),
                }
            }
            "--out" => parsed.keep = Some(PathBuf::from(value)),
            "--mode" => {
                parsed.mode = match value.as_str() {
                    "roundtrip" => Mode::Roundtrip,
                    "rule" => Mode::Rule,
                    "outline" => Mode::Outline,
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
