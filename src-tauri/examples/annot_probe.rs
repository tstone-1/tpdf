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
//! Eight modes. The first five write a mark and read it back a different way;
//! the last three measure what was drawn:
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
//!   quad's corners survived every other mode in this file. It is the only
//!   thing here that reads those numbers at all, which is reason enough; what
//!   it is **not** is a stand-in for some particular reader.
//!
//!   > This bullet used to end *"and PDFKit --- which is Preview --- is
//!   > measured to be one [that ignores `/AP`]"*, and that is false on macOS 26.
//!   > Blanking the `/AP` key of a saved highlight with spaces --- same file
//!   > length, so every xref offset still holds --- changed what PDFKit draws:
//!   > **43634 px over a 13.2 pt band with the appearance present, 33680 over
//!   > 10.8 pt without it**. It reads ours when it is there and synthesises its
//!   > own when it is not. `docs/TRAPS.md` carries the same claim under *"A
//!   > mutation that survives every check because nothing reads the field"* and
//!   > it is corrected there too.
//!
//! * `--mode refuse` --- the two refusals that are not defensive: a mark whose
//!   page is shared by two page numbers, and one covering no area.
//!
//! * `--mode rule` --- an underline or a strikeout puts its line in one band of
//!   the quad and leaves another empty. **Which band is "under" depends on the
//!   page's turn**, and the four answers are tabled in [`rule`].
//!
//! * `--mode outline` --- a box is a frame: ink on all four edges and nothing
//!   inside. The one measurement that separates `re S` from `re f`.
//!
//! * `--mode strokes` --- freehand ink: the two strokes were drawn, they were
//!   drawn apart, and **each one runs the length of the mark**. The last of
//!   those was added after the other four passed on a drawing whose strokes came
//!   out at a nineteenth of their length.
//!
//! Usage:
//!   annot-probe <file.pdf> [--page N]
//!               [--mode roundtrip|ink|legible|noap|refuse|rule|outline|strokes]
//!               [--kind highlight|underline|strikeout|note|square|ink]
//!               [--chars N] [--scale F] [--out PATH] [--lib DIR]

use std::path::{Path, PathBuf};
use tpdf_lib::document::OpenDocument;

use tpdf_lib::annots::{self, Kind};
use tpdf_lib::docmodel::INK_WIDTH;
use tpdf_lib::docmodel::{MarkKind, Quad, StampName};
use tpdf_lib::edits::{Edits, NewMark};
use tpdf_lib::progressive::{self, Placement, RawBitmap};
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
        // The rules' red, and it is the same ink: a squiggle is a line under
        // words, drawn opaque, and `--mode wave` classifies by the colour it
        // asked for exactly as the modes above do.
        MarkKind::Squiggly => RULE_RED,
        // The wash's yellow, matching `MARK_COLORS` in `edits.ts`: `/C` is what
        // a reader colours its own comment icon with, so this is the colour the
        // bubble comes out in everywhere else.
        MarkKind::Note => YELLOW,
        // The lines' red, with the box: a stamp's ink is a stroked border and a
        // filled word, both opaque, and `--mode outline` classifies pixels by
        // the colour it asked for exactly as it does for a box.
        MarkKind::Stamp => RULE_RED,
        // The lines' red, matching `MARK_COLORS` in `edits.ts`. A box's ink is
        // a stroke, and `--mode outline` classifies pixels by the colour it
        // asked for --- so a yellow box on white paper would be measured as an
        // absence, which is the mistake `RULE_RED`'s own comment records.
        MarkKind::Square => RULE_RED,
        // The lines' red again, and for the box's reason exactly: ink is a
        // stroke, so `--mode outline` classifies its pixels by the colour it
        // asked for and yellow ink on white paper would measure as an absence.
        MarkKind::Ink => RULE_RED,
        // The box's red, for the box's reason: an ellipse's ink is a stroke and
        // `--mode outline` classifies pixels by the colour it asked for.
        MarkKind::Ellipse => RULE_RED,
        // The rules' red, and here it is the colour of the *words* rather than
        // of a stroke -- `/DA` carries it as a fill. Black would read better on
        // a page and would make every ink measurement in these modes unable to
        // tell the mark's pixels from the document's own text.
        MarkKind::TextBox => RULE_RED,
    }
}

/// Smallest quad, in rendered pixels, whose coverage is worth a percentage.
///
/// Below this a box is mostly the antialiased edge of its own glyph, and the
/// figure says more about the renderer's smoothing than about where the mark
/// went. Quads under it are counted and named in the output.
/// The size `save.rs` sets a text box's words at.
///
/// A second copy of that constant, which is what the probe's own note about
/// `OUTLINE_WIDTH` argues against -- but this one cannot be shared: it is
/// private to `save.rs`, and making it public so a probe can predict a width
/// would let the probe agree with a wrong value as readily as with a right one.
/// The width prediction is checked against PDFKit's ink either way, so a drift
/// between the two shows up as a failed comparison rather than as agreement.
///
/// **Carries `preview_pdfkit`'s own `cfg`**, because that is its only reader and
/// PDFKit is macOS only. Without it the constant is dead code on Windows and
/// clippy's `-D warnings` refuses the build --- which is invisible from a Mac,
/// where the compiler never parses the other platform's arms at all. It cost a
/// rehearsal tag: `v26.8.6-rc1` was 16/16 here and 15/16 on `windows-2025`,
/// clippy the only red one.
#[cfg(target_os = "macos")]
const TEXT_SIZE: f64 = 11.0;

const MEASURABLE_PX: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Roundtrip,
    /// Where a line kind's rule actually lands, in pixels.
    Rule,
    /// That a box is a frame and not a filled rectangle, in pixels.
    Outline,
    /// That a stamp is a border **and** a word, which is neither of its
    /// neighbours.
    ///
    /// **The mode that exists because [`Mode::Outline`] cannot fail for this
    /// kind**, in the shape [`Mode::Wave`] records one line below: a stamp is a
    /// box with something inside it, so every reading `Outline` takes of a box
    /// is satisfied by a stamp except the one it gets backwards --- it requires
    /// an empty middle and a stamp's middle carries its word. Giving the new
    /// kind the old one's expectations would have produced a check that reports
    /// green for a stamp drawn as a plain rectangle.
    Stamp,
    /// That a squiggle rises above where an underline's rule stops.
    ///
    /// **The mode that exists because [`Mode::Rule`] cannot fail for this
    /// kind.** Thirds of a quad put an underline and a squiggle in the same one,
    /// so every reading that mode takes is satisfied by either -- and the
    /// obvious move, giving the new kind the old one's expectations, produces a
    /// check that reports green for the whole life of the defect. This reads the
    /// strip between them.
    Wave,
    /// That freehand ink is drawn where it was drawn, and only there.
    ///
    /// **Named `Strokes`, not `Ink`**, because [`Mode::Ink`] below is nine
    /// months older and means something else entirely: how much ink of *any*
    /// kind lands in a mark's band. The collision is with `MarkKind::Ink`, which
    /// is a kind of mark rather than a measurement, and renaming the older mode
    /// would break a documented invocation for a name nobody is confused by
    /// until they read both.
    Strokes,
    Ink,
    NoAp,
    Legible,
    Refuse,
    /// What PDFKit --- which is Preview --- makes of the saved file.
    Preview,
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
    /// Which standard stamp `--kind stamp` writes. Ignored by every other kind.
    stamp: StampName,
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
    let document = OpenDocument::open(bindings, &args.file, None)?;

    match args.mode {
        Mode::Roundtrip => roundtrip(args, &document),
        Mode::Rule => rule(args, &document, bindings),
        Mode::Outline => outline(args, &document, bindings),
        Mode::Stamp => stamp(args, &document, bindings),
        Mode::Wave => wave(args, &document, bindings),
        Mode::Strokes => strokes(args, &document, bindings),
        Mode::Ink | Mode::NoAp => ink(args, &document, bindings),
        Mode::Legible => legible(args, &document, bindings),
        Mode::Refuse => refuse(args, &document),
        Mode::Preview => preview(args, &document),
    }
}

/// That freehand ink is drawn where it was drawn, and only there.
///
/// **The measurement is a band that must be empty, and it is aimed at one
/// specific way of getting `/InkList` wrong.** The mark `mark_and_save` writes
/// for this mode is two horizontal strokes near the top and bottom of the text,
/// both running left to right, with a wide gap between them. A writer that
/// flattened the list into one path would join the end of the upper stroke to
/// the start of the lower one, and that join is a diagonal crossing the gap
/// across its full width --- so the gap is empty when the strokes are kept apart
/// and inked when they are not.
///
/// The two outer bands are read as well, and neither is redundant. Ink in the
/// gap alone would not say *which* stroke was drawn, and a writer that emitted
/// only the first stroke leaves the gap perfectly empty --- so "the upper band
/// is inked" and "the lower band is inked" are the two readings that make the
/// empty gap mean what it appears to mean.
///
/// Rendered at 4x for [`outline`]'s reason: at the default 2x a
/// [`INK_WIDTH`] stroke is 5 px and its antialiased edges are a large
/// fraction of that, which is a poor base for a count.
fn strokes(
    args: &Args,
    document: &OpenDocument,
    bindings: progressive::Bindings,
) -> Result<bool, String> {
    if !matches!(args.kind, MarkKind::Ink) {
        return Err(
            "--mode strokes is for freehand ink: pass --kind ink. The other kinds take              their shape from a rectangle, which is what --mode rule and --mode outline              measure."
                .to_string(),
        );
    }
    let (out, quads) = mark_and_save(args, document)?;
    // One rectangle, and for ink it is not sent at all: the model derives it
    // from the strokes. Asserted because everything below reads bands of it.
    if quads.len() != 1 {
        return Err(format!(
            "ink is one derived rectangle and this run made {}; --mode strokes cannot              read that",
            quads.len()
        ));
    }

    let scale = args.scale.max(4.0);
    if scale != args.scale {
        println!("     rendering at {scale}x rather than {}x: a stroke {INK_WIDTH} pt thick needs pixels to be measured in", args.scale);
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
    // **The strokes are offset across the rectangle's short side and run the
    // length of its long one**, whichever those turn out to be. Reading the
    // bands down the page regardless is the assumption `mark_and_save` used to
    // make, and it is the reason this mode certified a transposed drawing ---
    // see the comment there. Decided from the geometry rather than from the
    // page's `/Rotate`, so that it is an independent determination: if the two
    // ever disagree, the span checks below are what says so.
    let sideways = width < height;
    let across = if sideways { width } else { height };
    // **The bands only separate on a tall enough line, and that is checked
    // rather than assumed.** A stroke drawn at 5% of the text box occupies
    // `[0.05h, 0.05h + INK_WIDTH]` from the derived top and the boundary is
    // `(h + INK_WIDTH)/3`, so the ink clears the band when
    // `h > INK_WIDTH * 2 / 0.85`. Below that the mark's own ink lands in the
    // band this mode asserts is empty, and the run would report a defect that is
    // not there --- a control that cannot pass, which this repository has
    // recorded from several directions. Refused with the number, so the reason
    // is the fixture rather than the code.
    let text_across = across - INK_WIDTH as f32;
    let shortest = INK_WIDTH as f32 * 2.0 / 0.85;
    if text_across <= shortest {
        return Err(format!(
            "the marked text is {text_across:.1} pt across and --mode strokes needs more              than {shortest:.1} pt for its bands to separate a {INK_WIDTH} pt stroke;              point it at a document with larger type"
        ));
    }
    let whole = [quad[0], quad[1], quad[2], quad[3]];
    // Thirds of the derived rectangle, taken across its **short** side. The
    // strokes sit at 5% and 95% of the *text* box, and the derived rectangle is
    // that padded by half a line width, so both land inside the outer thirds and
    // the middle third holds neither.
    let (upper, gap, lower) = if sideways {
        (
            [quad[0], quad[1], quad[0] + across / 3.0, quad[3]],
            [
                quad[0] + across / 3.0,
                quad[1],
                quad[2] - across / 3.0,
                quad[3],
            ],
            [quad[2] - across / 3.0, quad[1], quad[2], quad[3]],
        )
    } else {
        (
            [quad[0], quad[1], quad[2], quad[1] + across / 3.0],
            [
                quad[0],
                quad[1] + across / 3.0,
                quad[2],
                quad[3] - across / 3.0,
            ],
            [quad[0], quad[3] - across / 3.0, quad[2], quad[3]],
        )
    };

    let want = color_for(args.kind);
    let all = rule_pixels(&after, aw, ah, whole, scale, want);
    let top = rule_pixels(&after, aw, ah, upper, scale, want);
    let middle = rule_pixels(&after, aw, ah, gap, scale, want);
    let bottom = rule_pixels(&after, aw, ah, lower, scale, want);
    let control = rule_pixels(&before, bw, bh, whole, scale, want);
    println!(
        "ink {width:.1}x{height:.1} pt: {all} px in the whole rectangle, {top} upper,          {middle} in the gap, {bottom} lower, {control} on the source page"
    );

    let mut ok = true;
    ok &= check(
        "the source page has no ink where the mark went (the control)",
        control == 0,
    );
    ok &= check(&format!("the renderer drew ink at all ({all} px)"), all > 0);
    ok &= check(&format!("the upper stroke was drawn ({top} px)"), top > 0);
    // Second, and not a duplicate of the one above: a writer that emitted only
    // the first entry of `/InkList` passes that check and leaves the gap empty,
    // so this is what separates "kept apart" from "only one of them exists".
    ok &= check(
        &format!("the lower stroke was drawn ({bottom} px)"),
        bottom > 0,
    );
    // The discrimination. A flattened `/InkList` draws a diagonal across this
    // band; two separate strokes leave it untouched.
    ok &= check(
        &format!("the gap between the strokes is empty ({middle} px)"),
        middle == 0,
    );
    // **How long each stroke is, which none of the four checks above can see.**
    // Each one runs the whole length of the text box, and the derived rectangle
    // is that box padded by half a line width at either end, so the expected
    // span is `long_side - INK_WIDTH` --- about 99% of it on every fixture
    // measured. The bound is 80%, so the margin is roughly a fifth of the
    // rectangle rather than the two hundredths of a point this mode's band
    // arithmetic once stood on. A stroke drawn across the run instead of along
    // it spans one line width, which is 1%.
    //
    // **The longer of the two axes, and not the one `sideways` picked.** The
    // first version of this check asked along `sideways`'s long axis, which
    // makes it a second reader of the decision it exists to police: reverting
    // both halves of the fix left it reporting *"14.2 pt of 14.4, needs 11.5"*
    // and passing, because a wrong `sideways` shrinks the expectation by
    // exactly as much as it shrinks the measurement. Taking the maximum over
    // both axes and comparing against the rectangle's own longer side shares
    // nothing with the band split, so the two can disagree --- which is the
    // whole point of having both.
    let long_side = width.max(height);
    let first = rule_span(&after, aw, ah, upper, scale, want, true)
        .max(rule_span(&after, aw, ah, upper, scale, want, false));
    let second = rule_span(&after, aw, ah, lower, scale, want, true)
        .max(rule_span(&after, aw, ah, lower, scale, want, false));
    let wanted = long_side * 0.8;
    ok &= check(
        &format!(
            "the first stroke runs the length of the mark ({first:.1} pt of {long_side:.1}, needs {wanted:.1})"
        ),
        first >= wanted,
    );
    ok &= check(
        &format!(
            "the second stroke runs the length of the mark ({second:.1} pt of {long_side:.1}, needs {wanted:.1})"
        ),
        second >= wanted,
    );
    Ok(ok)
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
fn mark_and_save(args: &Args, document: &OpenDocument) -> Result<(PathBuf, Vec<Quad>), String> {
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

    // **Ink sends strokes and no quads**, because its rectangle is derived from
    // what was drawn rather than sent --- see `NewMark::quads`. Synthetic rather
    // than anything a hand made, and that is the right shape for a probe: what
    // is being measured is that the points reach the page in the right place,
    // and a path whose turns are known is one whose pixels can be predicted.
    //
    // **Two horizontal strokes with a wide empty band between them, and the
    // emptiness is the whole design.** A writer that flattened `/InkList` into
    // one path would join the end of the upper stroke to the start of the lower
    // one, and because the upper runs left-to-right and so does the lower, that
    // join is a diagonal crossing the middle band across its full width. So the
    // middle band is empty for a correct writer and inked for a flattening one
    // --- which is the discrimination `--mode strokes` reads, and the same shape
    // `--mode rule` uses for a band that must be empty.
    let strokes: Vec<Vec<f32>> = if matches!(args.kind, MarkKind::Ink) {
        let box_ = union(&quads);
        let (left, top, right, bottom) = (box_[0], box_[1], box_[2], box_[3]);
        // **Along the run, not along the screen's horizontal.** This is the same
        // rule [`quads_for`] states below and the same trap `docs/TRAPS.md`
        // records for the line grouping: on a page displayed sideways the lines
        // advance across the screen and the characters run down it. A stroke
        // drawn from `left` to `right` then crosses the text instead of
        // following it.
        //
        // Measured on `rotated-90`, which `BUILD.md` has recommended running
        // since the day this mode landed: each stroke came out **11.9 pt where
        // it should be 246.7**, and every check below stayed green, because two
        // stubs at the ends of the rectangle satisfy "ink in the outer thirds,
        // nothing in the middle" exactly as well as two full-length strokes do.
        // The writer was not involved --- `save::user_strokes` mapped what it
        // was handed --- so this is the probe's own input, and the assertion
        // that now catches it is in [`strokes`].
        let sideways = extracted.quarter_turns % 2 == 1;
        let (long_start, long_end) = if sideways {
            (top, bottom)
        } else {
            (left, right)
        };
        let across = if sideways { right - left } else { bottom - top };
        // **5% rather than something more central, and the margin is arithmetic
        // rather than taste.** `--mode strokes` reads thirds of the *derived*
        // rectangle, which is this box grown by half a line width at each edge.
        // A stroke centred at `f` of the text box's **short** side occupies
        // `[f*h, f*h + INK_WIDTH]` measured from that side's near edge, and the
        // band boundary is `(h + INK_WIDTH)/3`. At f = 0.15 on this corpus that is
        // 3.88 pt against 3.90 --- it passes, by two hundredths of a point, and
        // a fixture with slightly shorter lines would put ink in the band that
        // is asserted empty and report a defect that is not there. At 0.05 the
        // margin is about a point. The mode refuses a rectangle too short for
        // the bands to separate at all rather than reading one.
        let (near, far) = if sideways {
            (left + across * 0.05, right - across * 0.05)
        } else {
            (top + across * 0.05, bottom - across * 0.05)
        };
        if sideways {
            vec![
                vec![near, long_start, near, long_end],
                vec![far, long_start, far, long_end],
            ]
        } else {
            vec![
                vec![long_start, near, long_end, near],
                vec![long_start, far, long_end, far],
            ]
        }
    } else {
        Vec::new()
    };
    let quads = if strokes.is_empty() {
        quads
    } else {
        Vec::new()
    };

    let edits = Edits::default();
    edits.open(DOC, document.page_count(), None);
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
                // The model refuses a name on any other kind and a stamp with
                // none, so this is the biconditional restated at the one place
                // that builds a mark here rather than a default that would be
                // wrong for eight kinds out of nine.
                stamp: (args.kind == MarkKind::Stamp).then_some(args.stamp),
                reply_to: None,
                page: id,
                quads: quads
                    .iter()
                    .flat_map(|q| [q.left, q.top, q.right, q.bottom])
                    .collect(),
                strokes,
                color: color_for(args.kind),
                width: INK_WIDTH,
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
    save::write_copy(&args.file, &plan, &out, None).map_err(|why| why.message)?;
    if args.mode == Mode::NoAp {
        strip_appearances(&out)?;
    }
    // **The plan's quads, not the ones sent**, and for ink they are the only
    // ones there are: its rectangle is derived by the model from the strokes, so
    // this end sends none. Read back rather than recomputed here, because a
    // second copy of `Stroke::bounds` in the probe would agree with a wrong one
    // in the model as readily as with a right one --- and every band `--mode
    // strokes` measures is a fraction of this rectangle.
    //
    // Identical to `quads` for every other kind, which is what makes this safe
    // to do for all of them rather than only for ink: one mark was written, and
    // the plan holds what the model made of what was sent.
    let quads = plan
        .marks
        .first()
        .map(|mark| mark.quads.clone())
        .unwrap_or(quads);
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
fn roundtrip(args: &Args, document: &OpenDocument) -> Result<bool, String> {
    let (out, quads) = mark_and_save(args, document)?;
    let bytes = std::fs::read(&out).map_err(|e| format!("could not read {out:?}: {e}"))?;
    let found = annots::scan(&bytes, document.page_count() as usize, None)?;

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
        // Names that agree, and it still earns an arm: `/Squiggly` is the one
        // markup subtype tpdf could not write until this kind existed, so this
        // is the round trip saying `save.rs` emits it and `annots.rs` reads it
        // back rather than falling through to "some other annotation".
        MarkKind::Squiggly => Kind::Squiggly,
        // Names that agree again, and it earns its arm for the squiggle's
        // reason: `/Stamp` is a subtype tpdf could not write until this kind
        // existed, so the round trip is what says `save.rs` emits it and
        // `annots.rs` reads it back rather than reporting some other annotation.
        MarkKind::Stamp => Kind::Stamp,
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
        // The second pair whose names agree, and it is worth an arm rather than
        // being folded in with the box above: what a reader sees is "Draw",
        // which is in neither enum, so both of the two spellings this file can
        // see happen to be the third one.
        MarkKind::Ink => Kind::Ink,
        // **The pair whose names differ in the direction that matters most
        // here.** `MarkKind::Ellipse` is written as `/Circle` and read back as
        // `Kind::Circle`, so this arm is the one that would catch `subtype`
        // emitting `/Square` for it --- which is a defect our own `/AP` hides
        // completely, because the appearance stream draws the right ellipse
        // whatever the subtype says. Every other program would call it a
        // rectangle.
        MarkKind::Ellipse => Kind::Circle,
        // Two names that differ, and the reason to spell it out is that the
        // reader's word is a third: `/FreeText` in the file, `textbox` on the
        // wire, "Text box" in the note header.
        MarkKind::TextBox => Kind::FreeText,
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
    // predicate both of them go through. Ink is the third and the clearest of
    // them: its shape is a list of paths, and a rectangle per line of text would
    // be a claim about words it does not mark.
    // The ellipse is the fourth, and it is the box's route exactly: not a markup
    // subtype, so `is_text_markup` answers no for it and quads on it would be an
    // unlisted key claiming a run of words the mark does not cover.
    // The stamp is the sixth and last, by the same route as the box and for the
    // same reason: PDF 32000-1 lists `/QuadPoints` on the four markup subtypes
    // and on nothing else, and a stamp's rectangle is the reader's rather than a
    // run of words.
    let expected_quads = if matches!(
        args.kind,
        MarkKind::Note
            | MarkKind::Square
            | MarkKind::Ink
            | MarkKind::Ellipse
            | MarkKind::TextBox
            | MarkKind::Stamp
    ) {
        0
    } else {
        quads.len()
    };
    ok &= quad_points_are_in_reading_order(&out, expected_quads);
    ok &= ink_list_matches_what_was_drawn(&out, args.kind);
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

/// Reads `/InkList` off the written file and checks it against the appearance.
///
/// **The `/AP` is what every reader draws, and the list is what an editor
/// reads.** `--mode strokes` measures the first in pixels and would pass on a
/// file whose `/InkList` was absent, wrong, or flattened, because nothing
/// renders it. So this is the other half, and the two together are what make a
/// file *ink* rather than a picture of ink.
///
/// What it can say is structural: the key exists on an `/Ink` and on nothing
/// else, it holds one array per stroke, each holds an even number of numbers,
/// and every point is inside the annotation's own `/Rect`. That last one is the
/// assertion with teeth --- a wrong turn in [`user_strokes`] puts the points on
/// the page in a plausible place, and `/Rect` is computed from the quads by a
/// different route, so a mapping that disagreed with it would land outside.
///
/// What it deliberately does not do is compare the numbers against what the
/// probe sent. Those are display-space and these are page-space, so the
/// comparison would need the mapping --- and a check that reimplements the
/// mapping it is checking agrees with a wrong one exactly as readily.
fn ink_list_matches_what_was_drawn(file: &Path, kind: MarkKind) -> bool {
    let Ok(doc) = lopdf::Document::load(file) else {
        return check("the written file reopens", false);
    };
    let mut found = 0usize;
    let mut ok = true;
    // **Separate from `ok`, and the reason is a control that misreported.**
    // Written as `check("every point is inside /Rect", ok)` this read back the
    // *accumulated* flag, so a mutation that stopped writing the key at all made
    // it go red too --- naming a geometry defect for a file with no geometry in
    // it. One predicate answering two questions, caught by its own control.
    let mut inside = true;
    for object in doc.objects.values() {
        let Ok(dictionary) = object.as_dict() else {
            continue;
        };
        let Ok(list) = dictionary.get(b"InkList").and_then(lopdf::Object::as_array) else {
            continue;
        };
        found += 1;
        let rect: Vec<f32> = dictionary
            .get(b"Rect")
            .and_then(lopdf::Object::as_array)
            .map(|r| r.iter().filter_map(|v| v.as_float().ok()).collect())
            .unwrap_or_default();
        if rect.len() != 4 {
            return check("an /Ink annotation carries a four-number /Rect", false);
        }
        for stroke in list {
            let Ok(points) = stroke.as_array() else {
                return check("every /InkList entry is an array", false);
            };
            let values: Vec<f32> = points.iter().filter_map(|v| v.as_float().ok()).collect();
            if values.len() != points.len() || values.len() % 2 != 0 {
                return check("every stroke is an even count of numbers", false);
            }
            // Half a line width of slack, because `/Rect` is grown by exactly
            // that and floats do not round trip through a decimal literal.
            let slack = (INK_WIDTH / 2.0) as f32 + 0.01;
            for pair in values.chunks_exact(2) {
                inside &= pair[0] >= rect[0] - slack
                    && pair[0] <= rect[2] + slack
                    && pair[1] >= rect[1] - slack
                    && pair[1] <= rect[3] + slack;
            }
        }
        ok &= check(
            &format!("the /InkList holds {} stroke(s)", list.len()),
            list.len() == 2,
        );
    }
    // **Both directions**, and the second is what a `matches!` on the kind buys:
    // an `/InkList` on a highlight would be as wrong as its absence on ink, and
    // a writer that emitted the key unconditionally passes every assertion above.
    let wanted = usize::from(kind == MarkKind::Ink);
    ok &= check(
        &format!("{found} annotation(s) carry an /InkList, and {wanted} should"),
        found == wanted,
    );
    // Only where there is a list to be inside anything: with none, this would
    // be an assertion about no points, which passes by having nothing to check.
    if found > 0 {
        ok &= check(
            "every /InkList point is inside the annotation's /Rect",
            inside,
        );
    }
    ok
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
/// What a parser that did not write the file makes of the mark.
///
/// **Phase 2's exit criterion, which every other mode in this file is
/// structurally unable to reach.** `--mode roundtrip` reads the saved file with
/// `annots.rs`; the pixel modes render it with PDFium. Both of those are ours,
/// and a writer and its own reader agree about a document that is wrong ---
/// which is the failure `docs/TRAPS.md` records from four directions. The
/// criterion is that the document "look right reopened in Acrobat and Preview",
/// and only a foreign parser can say so.
///
/// PDFKit is what Preview is. It is already a dependency, because
/// `print_macos.rs` reads every print job back with it for this same reason.
///
/// **Every check here is between two *readers*, and that bounds what the mode
/// can ever catch.** A writer that moves something legally moves it for both of
/// them, so they agree and the mode passes. Measured rather than reasoned about,
/// by mutating `save.rs` and watching:
///
/// | mutation | preview | what does catch it |
/// |----------|---------|--------------------|
/// | no `/Contents` | **red** | --- |
/// | no `/T` | **red** | --- |
/// | appearance `/BBox` shrunk to a 1x1 corner | **red** on 5 of 6 kinds | nothing else |
/// | `/Subtype` written as `/Underline` for a strikeout | green | `save::tests::each_kind_writes_its_own_subtype`, and `--mode roundtrip` |
/// | `/Rect` shifted three points sideways | green | `--mode roundtrip`, which compares against where the characters were |
/// | no `/AP` at all | green | `save.rs`'s own test that the key is written |
///
/// The `/BBox` row is the one that justifies the mode's existence beside the
/// PDFium ones: PDFKit drew **196 px into a 14 pt corner** where a correct box
/// draws 1306 across 254, while PDFium scaled the same form up until the frame
/// filled the rectangle solid. Two renderers, two different wrong pictures, and
/// only one of them is Preview. The kind that survives it is [`MarkKind::Note`],
/// correctly: `save.rs` writes a comment no appearance stream at all, because
/// every reader draws its own icon, so there is no `/BBox` to shrink.
///
/// **The rectangle comparison converts, and says so.** `annots.rs` answers in
/// display space, after `/Rotate`; `PDFAnnotation.bounds` answers with the raw
/// `/Rect`. Reading one against the other directly is the trap
/// `docs/TRAPS.md` names under *"PDFKit reports an annotation's bounds rotated
/// and renders the page unrotated"*, and it produced two rounds of a confident
/// wrong conclusion the first time. `text::from_device` plus the crop origin is
/// the conversion, which is `save::user_quads`'s, so what survives the
/// cancellation is whether PDFKit found the same four numbers.
///
/// **The pixel half is whole-page on a turned page and cannot be otherwise.**
/// Measured 2026-08-20: PDFKit draws a `/Rotate` page's content *rotated* into
/// an *unrotated* frame --- `boundsForBox` answers 612x792 for a page poppler
/// renders at 792x612, and six of `rotated-90`'s twelve lines are clipped away
/// --- while `bounds` stays unrotated. So the annotation layer and the content
/// layer are in different frames and "coverage inside its own rectangle" reads
/// 0.0% for a mark that is drawn perfectly. The containment check is therefore
/// skipped on a turned page, out loud, rather than reported as a failure.
#[cfg(target_os = "macos")]
fn preview(args: &Args, document: &OpenDocument) -> Result<bool, String> {
    let (out, _) = mark_and_save(args, document)?;
    let outcome = preview_pdfkit(args, document, &out);
    if args.keep.is_none() {
        let _ = std::fs::remove_file(&out);
    }
    outcome
}

/// The platform refusal, which is a gap rather than a guarantee.
///
/// Windows has an independent PDF parser and the print path already reads jobs
/// back with it --- but `Windows.Data.Pdf` renders and exposes **no annotation
/// object model at all**, so the metadata half of this mode has no counterpart
/// there and the pixel half would be a second implementation rather than a
/// port. Said here rather than left to fail obscurely, because a mode that is
/// absent on a platform looks exactly like one that passes.
#[cfg(not(target_os = "macos"))]
fn preview(_args: &Args, _document: &OpenDocument) -> Result<bool, String> {
    Err(
        "--mode preview reads the saved file with PDFKit, which is macOS only. \
         Windows.Data.Pdf is the platform's own parser and the print path \
         already uses it, but it exposes no annotation object model, so there \
         is nothing there to ask the questions this mode asks."
            .to_string(),
    )
}

#[cfg(target_os = "macos")]
fn preview_pdfkit(args: &Args, document: &OpenDocument, out: &Path) -> Result<bool, String> {
    let page = document.page(args.page)?;
    let turns = page.quarter_turns();
    let (width_pt, height_pt) = (page.width_pt(), page.height_pt());
    let (origin_x, origin_y) = page.origin_pt();

    // Our own reader over the same bytes, for the two checks that are
    // differentials rather than assertions against what was asked for.
    let bytes = std::fs::read(out).map_err(|e| format!("cannot re-read {}: {e}", out.display()))?;
    let ours = annots::scan(&bytes, document.page_count() as usize, None)?;
    let mine: Vec<&annots::Comment> = ours
        .items
        .iter()
        .filter(|c| c.page == args.page && c.author == "annot-probe")
        .collect();

    let theirs = open_pdfkit(out)?;
    let source = open_pdfkit(&args.file)?;
    let listed = pdfkit_annotations(&theirs, args.page)?;
    let already = pdfkit_annotations(&source, args.page)?;
    // The source page's own annotations are not ours --- `links-cropped.pdf`
    // carries a `/Link` --- so the count that has to agree is the difference.
    let added = listed.len() as i64 - already.len() as i64;

    println!(
        "PDFKit on page {}: {} annotation(s), {} before the mark; annots.rs reports {} of ours",
        args.page,
        listed.len(),
        already.len(),
        mine.len()
    );

    let mut ok = true;
    ok &= check(
        &format!("annots.rs found exactly one mark of ours ({})", mine.len()),
        mine.len() == 1,
    );
    ok &= check(
        &format!("PDFKit found one annotation more than the source page had ({added})"),
        added == 1,
    );
    let Some(mark) = mine.first() else {
        return Ok(false);
    };
    let Some(theirs_mark) = listed
        .iter()
        .find(|a| a.author.as_deref() == Some("annot-probe"))
    else {
        // Not folded into the count check above: a mark PDFKit lists but cannot
        // attribute is a different defect from one it does not list at all.
        check("PDFKit attributes the mark to its author", false);
        return Ok(false);
    };

    // **Two readers of one file, compared through a third thing that is neither
    // of them.** PDFKit's `/Subtype` string goes through `annots::Kind::of`, so
    // what is asserted is that the two agree about what the mark *is* rather
    // than about a spelling.
    //
    // This deliberately does **not** compare against the table `save.rs` writes
    // from. That was the first version, and mutating `save::subtype` to write
    // `/Underline` for a strikeout left it green: the check read the same table
    // the writer did, so both moved together. A legal-but-wrong subtype is
    // caught by `save::tests::each_kind_writes_its_own_subtype` and by
    // `--mode roundtrip`'s kind assertion, both measured red under that
    // mutation; what this one is for is a foreign parser disagreeing, which
    // nothing else here can see.
    let theirs_kind = theirs_mark
        .subtype
        .as_deref()
        .and_then(|name| annots::Kind::of(name.as_bytes()));
    ok &= check(
        &format!(
            "PDFKit and annots.rs agree what the mark is ({} against {:?})",
            theirs_mark.subtype.as_deref().unwrap_or("(none)"),
            mark.kind
        ),
        theirs_kind == Some(mark.kind),
    );
    ok &= check(
        &format!(
            "the note survives to a foreign reader ({:?})",
            theirs_mark.note.as_deref().unwrap_or("")
        ),
        theirs_mark.note.as_deref() == Some("written by annot-probe"),
    );

    // The conversion, stated: display space -> the page's own, exactly as
    // `save::user_quads` does it.
    let us = to_page_space(turns, width_pt, height_pt, mark.rect, (origin_x, origin_y));
    let them = theirs_mark.bounds;
    if args.kind == MarkKind::Note {
        // **A `/Text` annotation's rectangle is advisory and PDFKit replaces
        // it**, which this mode found on its first run and which is the whole
        // point of asking a foreign reader. Measured: `/Rect` written as
        // `[60.322 717.074 313.652 730.192]`, 253.3 x 13.1, and PDFKit reports
        // `(60.322, 706.192) 24 x 24` --- the standard icon, anchored at the
        // rectangle's top-left corner, since `730.192 - 24 = 706.192`. The
        // specification allows exactly that: a reader draws the icon at a size
        // of its own choosing.
        //
        // So the equality below would be a defect report about a mark that is
        // correct. What is asserted instead is the anchor and the size, which
        // still proves PDFKit found our rectangle --- it cannot place the icon
        // at our top-left corner without having read it.
        let corner = (them[0] - us[0]).abs().max((them[3] - us[3]).abs());
        ok &= check(
            &format!(
                "PDFKit hangs the note's icon off the corner annots.rs reports                  ({corner:.2} pt away)"
            ),
            corner < 0.5,
        );
        ok &= check(
            &format!(
                "and draws it at the standard size ({:.1} x {:.1})",
                them[2] - them[0],
                them[3] - them[1]
            ),
            (them[2] - them[0] - 24.0).abs() < 0.5 && (them[3] - them[1] - 24.0).abs() < 0.5,
        );
    } else {
        let apart = (0..4)
            .map(|i| (them[i] - us[i]).abs())
            .fold(0.0f64, f64::max);
        ok &= check(
            &format!(
                "PDFKit and annots.rs agree about the rectangle (worst corner {apart:.2} pt apart)"
            ),
            apart < 0.5,
        );
    }

    let before = pdfkit_render(&source, args.page)?;
    let after = pdfkit_render(&theirs, args.page)?;
    if before.1 != after.1 || before.2 != after.2 {
        return Err(format!(
            "PDFKit renders the copy {}x{} and the source {}x{}, so no comparison \
             between them means anything",
            after.1, after.2, before.1, before.2
        ));
    }
    let (changed, box_) = differing(&before.0, &after.0, before.1, before.2, after.3);
    println!(
        "PDFKit's own render: {changed} px changed of {}, within ({:.1}, {:.1}) {:.1} x {:.1}",
        before.1 * before.2,
        box_[0],
        box_[1],
        box_[2] - box_[0],
        box_[3] - box_[1]
    );
    ok &= check(
        &format!("PDFKit draws something the source page does not ({changed} px)"),
        changed > 0,
    );
    // **How much of the rectangle the drawing reaches, which "something was
    // drawn" cannot see.** Kind-independent, and it has to be: a highlight
    // fills its box, an underline is a rule two points tall inside a box of
    // thirteen, and ink is two thin strokes. What every one of them does is
    // span the rectangle's *longer* side, so that is the only dimension asked
    // about.
    //
    // Written because a mutation shrinking the appearance stream's `/BBox` to a
    // 1x1 corner survived everything above: PDFKit drew 196 px in a 14 pt
    // square where a correct box draws 1306 across 254 pt, and both "something
    // was drawn" and "it stayed inside the rectangle" are satisfied by a mark
    // that has collapsed. PDFium does not fail the same way --- it scales the
    // form up until the frame fills the rectangle solid --- which is the whole
    // argument for asking a second renderer.
    let (drew, spans) = (
        (box_[2] - box_[0]).max(box_[3] - box_[1]),
        (them[2] - them[0]).max(them[3] - them[1]),
    );
    if args.kind == MarkKind::TextBox {
        // **A text box is the one kind whose ink is not supposed to span its
        // rectangle**, and the check above reported a failure on a mark that was
        // drawn perfectly: one short line of type is as wide as its words and no
        // wider. Every other kind's ink is derived from the rectangle, so
        // "reaches most of it" is the right question for all of them and the
        // wrong one for this.
        //
        // What replaces it is a stronger reading, not a weaker one. The width of
        // the drawn text is predictable -- `textbox::advance` is what the wrap
        // arithmetic uses -- so this asserts PDFKit laid the line down at the
        // width our own metrics said it would. **That is the Helvetica widths
        // table checked through a second, independent renderer**: `helvetica-probe`
        // measures it through PDFium, and the two engines share no code with each
        // other or with the table.
        //
        // One-sided and generous for `helvetica-probe`'s reason: measured ink runs
        // from the first glyph's left edge to the last one's right, and an
        // advance includes the trailing side bearing, so ink is expected to come
        // in under. The 15% floor is what separates "the line was drawn" from
        // "half of it was clipped".
        // `box_` is the ink PDFKit actually laid down; `them` is the rectangle
        // it reports for the annotation, which is ours and says nothing about
        // the words. Written with `them` first, which compared a 253 pt
        // rectangle against a 109 pt line and failed -- a check reading the
        // wrong one of two rectangles that are both right there.
        let predicted = tpdf_lib::textbox::advance(NOTE, TEXT_SIZE);
        let across = box_[2] - box_[0];
        ok &= check(
            &format!(
                "the line is as wide as its own words say it should be              ({across:.1} pt drawn, {predicted:.1} pt predicted)"
            ),
            // The upper slack is rasterisation, not metrics: the ink box is
            // measured in whole pixels and antialiasing spills a fraction of a
            // point past the outermost glyph, so a correct line reads about half
            // a point over. Measured at 110.0 against 109.4.
            across <= predicted + 2.0 && across >= predicted * 0.85,
        );
    } else {
        ok &= check(
            &format!(
                "and draws across the rectangle rather than into a corner of it              ({drew:.1} pt of {spans:.1})"
            ),
            drew >= spans * 0.8,
        );
    }

    if turns % 4 == 0 {
        // **Against the rectangle PDFKit reports, not the one we wrote**, and
        // the two are tied together by the agreement check above --- which is
        // what makes this more than PDFKit agreeing with itself. For a note
        // they genuinely differ, since the icon hangs *below* our rectangle's
        // bottom edge, so measuring containment against ours would report a
        // defect in a mark the same run has just shown to be right.
        //
        // Two points of slack at each edge: the comparison is at one pixel per
        // point and a stroke straddles its path.
        let inside = box_[0] >= them[0] - 2.0
            && box_[1] >= them[1] - 2.0
            && box_[2] <= them[2] + 2.0
            && box_[3] <= them[3] + 2.0;
        ok &= check(
            &format!(
                "everything PDFKit drew is inside the rectangle it reports                  (({:.1}, {:.1}) {:.1} x {:.1})",
                them[0],
                them[1],
                them[2] - them[0],
                them[3] - them[1]
            ),
            inside,
        );
    } else {
        println!(
            "[SKIP] everything PDFKit drew is inside the mark's own rectangle: this page is \
             /Rotate {}, and PDFKit draws the content turned while reporting bounds \
             unturned -- the two layers are in different frames, so the containment \
             figure would be about PDFKit rather than about us",
            turns as u32 * 90
        );
    }
    Ok(ok)
}

/// One annotation as PDFKit reports it.
#[cfg(target_os = "macos")]
struct Listed {
    subtype: Option<String>,
    author: Option<String>,
    note: Option<String>,
    /// `PDFAnnotation.bounds`, which is the raw `/Rect` --- **not** turned by
    /// `/Rotate`, measured 2026-08-20 against the bytes of a marked
    /// `rotated-90`. See [`preview_pdfkit`] for why that matters.
    bounds: [f64; 4],
}

#[cfg(target_os = "macos")]
fn open_pdfkit(path: &Path) -> Result<objc2::rc::Retained<objc2_pdf_kit::PDFDocument>, String> {
    use objc2::AnyThread;
    use objc2_foundation::{NSString, NSURL};
    use objc2_pdf_kit::PDFDocument;

    let text = NSString::from_str(&path.to_string_lossy());
    let url = NSURL::fileURLWithPath(&text);
    unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url) }.ok_or_else(|| {
        format!(
            "PDFKit will not open {} at all. That is a finding rather than a harness \
             fault: it is the parser Preview uses, and a file it refuses is a file a \
             reader cannot open.",
            path.display()
        )
    })
}

#[cfg(target_os = "macos")]
fn pdfkit_annotations(doc: &objc2_pdf_kit::PDFDocument, page: u32) -> Result<Vec<Listed>, String> {
    let page = unsafe { doc.pageAtIndex(page as usize) }
        .ok_or_else(|| format!("PDFKit reports no page {page}"))?;
    let list = unsafe { page.annotations() };
    let mut out = Vec::new();
    for index in 0..list.count() {
        let annot = list.objectAtIndex(index);
        let bounds = unsafe { annot.bounds() };
        out.push(Listed {
            subtype: unsafe { annot.r#type() }.map(|s| s.to_string()),
            author: unsafe { annot.userName() }.map(|s| s.to_string()),
            note: unsafe { annot.contents() }.map(|s| s.to_string()),
            bounds: [
                bounds.origin.x,
                bounds.origin.y,
                bounds.origin.x + bounds.size.width,
                bounds.origin.y + bounds.size.height,
            ],
        });
    }
    Ok(out)
}

/// PDFKit's own render of one page, at one pixel per point, RGBA.
///
/// One pixel per point rather than the 4x the ink modes use, because nothing
/// here measures a coverage fraction --- the questions are "did anything change"
/// and "where", and a 2.5 pt stroke is 2.5 px, which is plenty for both. Row 0
/// of the buffer is the **top** of the page, which is a fact about
/// `CGBitmapContextCreate` measured rather than reasoned about: reading it the
/// other way round put every changed box at the wrong end of the page.
#[cfg(target_os = "macos")]
fn pdfkit_render(
    doc: &objc2_pdf_kit::PDFDocument,
    page: u32,
) -> Result<(Vec<u8>, usize, usize, [f64; 2]), String> {
    use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGImageAlphaInfo};
    use objc2_pdf_kit::PDFDisplayBox;

    let page = unsafe { doc.pageAtIndex(page as usize) }
        .ok_or_else(|| format!("PDFKit reports no page {page}"))?;
    let box_ = unsafe { page.boundsForBox(PDFDisplayBox::MediaBox) };
    let (w, h) = (box_.size.width as usize, box_.size.height as usize);
    if w == 0 || h == 0 {
        return Err(format!(
            "PDFKit reports a {w}x{h} media box, which cannot be rendered"
        ));
    }
    // White, opaque, before anything is drawn --- the page's own background is
    // not painted by `drawWithBox:`, so an unfilled buffer would make every
    // blank pixel differ from itself run to run.
    let mut pixels = vec![0xFFu8; w * h * 4];
    let space = CGColorSpace::new_device_rgb().ok_or("no device RGB colour space")?;
    let ctx = unsafe {
        CGBitmapContextCreate(
            pixels.as_mut_ptr().cast(),
            w,
            h,
            8,
            w * 4,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }
    .ok_or("CGBitmapContextCreate returned nothing")?;
    unsafe { page.drawWithBox_toContext(PDFDisplayBox::MediaBox, &ctx) };
    drop(ctx);
    Ok((pixels, w, h, [box_.origin.x, box_.origin.y]))
}

/// How many pixels differ, and the box in page points that holds all of them.
///
/// The box is in the page's own space, bottom-left origin, so it can be compared
/// with a `/Rect` directly.
#[cfg(target_os = "macos")]
fn differing(a: &[u8], b: &[u8], w: usize, h: usize, origin: [f64; 2]) -> (usize, [f64; 4]) {
    let (mut lo_x, mut lo_y, mut hi_x, mut hi_y) = (w, h, 0usize, 0usize);
    let mut count = 0usize;
    for y in 0..h {
        for x in 0..w {
            let at = (y * w + x) * 4;
            if a[at] != b[at] || a[at + 1] != b[at + 1] || a[at + 2] != b[at + 2] {
                count += 1;
                lo_x = lo_x.min(x);
                hi_x = hi_x.max(x);
                lo_y = lo_y.min(y);
                hi_y = hi_y.max(y);
            }
        }
    }
    if count == 0 {
        return (0, [0.0; 4]);
    }
    // Row 0 is the top, so the bottom edge is the *largest* row index.
    (
        count,
        [
            lo_x as f64 + origin[0],
            (h - hi_y - 1) as f64 + origin[1],
            (hi_x + 1) as f64 + origin[0],
            (h - lo_y) as f64 + origin[1],
        ],
    )
}

/// A mark's rectangle out of display space and into the page's own.
///
/// `save::user_quads`' mapping, and deliberately the same one: what the
/// comparison in [`preview_pdfkit`] is for is whether PDFKit found the same
/// four numbers, so the conversion cancelling on both sides is the point rather
/// than a weakness. Written here rather than called because `user_quads` is
/// private to the writer and takes a `PlannedMark`.
#[cfg(target_os = "macos")]
fn to_page_space(
    turns: u8,
    width_pt: f32,
    height_pt: f32,
    rect: [f32; 4],
    origin: (f32, f32),
) -> [f64; 4] {
    let page = text::from_device(turns, width_pt, height_pt, rect);
    [
        page[0] + f64::from(origin.0),
        page[1] + f64::from(origin.1),
        page[2] + f64::from(origin.0),
        page[3] + f64::from(origin.1),
    ]
}

/// How far the mark's own ink reaches along one axis inside `band`, in points.
///
/// **The one question `rule_pixels` cannot answer, and the reason a transposed
/// drawing passed every check in `--mode strokes` for a day.** Counting pixels
/// in a band says *whether* ink is there; two 11.9 pt stubs at the ends of a
/// 224.5 pt rectangle put ink in both outer thirds and none in the middle,
/// which is exactly what two full-length strokes do. The extent along the
/// rectangle's long side is what separates them: 1% of it against 99%.
///
/// Returns 0.0 when the band holds none of the colour, which the caller reads
/// as a stroke that was not drawn --- the same reading the count checks give.
fn rule_span(
    pixels: &[u8],
    width: u32,
    height: u32,
    band: [f32; 4],
    scale: f32,
    want: [f32; 3],
    along_x: bool,
) -> f32 {
    let target = want.map(|c| (c * 255.0) as i32);
    let x0 = (band[0] * scale).floor().max(0.0) as u32;
    let y0 = (band[1] * scale).floor().max(0.0) as u32;
    let x1 = ((band[2] * scale).ceil() as u32).min(width);
    let y1 = ((band[3] * scale).ceil() as u32).min(height);
    let (mut low, mut high) = (u32::MAX, 0u32);
    for y in y0..y1 {
        for x in x0..x1 {
            let at = ((y * width + x) * 4) as usize;
            let (r, g, b) = (
                pixels[at] as i32,
                pixels[at + 1] as i32,
                pixels[at + 2] as i32,
            );
            if (r - target[0]).abs() < 40
                && (g - target[1]).abs() < 40
                && (b - target[2]).abs() < 40
            {
                let at = if along_x { x } else { y };
                low = low.min(at);
                high = high.max(at);
            }
        }
    }
    if low == u32::MAX {
        return 0.0;
    }
    (high + 1 - low) as f32 / scale
}

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
    document: &OpenDocument,
    bindings: progressive::Bindings,
) -> Result<bool, String> {
    if !matches!(
        args.kind,
        MarkKind::Underline | MarkKind::Squiggly | MarkKind::StrikeOut
    ) {
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

    // **Which edge of the quad is "under" is decided by the page's turn, and
    // there are four answers.** An underline sits below the baseline in the
    // page's own space; `text::from_device` maps the device box there, and
    // reading its four arms backwards says which device edge that is:
    //
    // | turn | page-space bottom is the device box's |
    // |------|---------------------------------------|
    // |   0  | bottom                                |
    // |  90  | left                                  |
    // | 180  | top                                   |
    // | 270  | right                                 |
    //
    // Splitting the quad into thirds down the page regardless is what this mode
    // used to do, and on `rotated-90` it reported 330/330/332 --- an underline
    // drawn correctly along a run that goes *down* the screen crosses all three
    // horizontal bands, so both assertions failed on a mark that was right. That
    // is the loud direction of the same defect `--mode strokes` had quietly, and
    // the same rule `quads_for` states below.
    let turns = document.page(args.page)?.quarter_turns() % 4;
    let (mut under, mut opposite, mut middle, mut before_any) = (0usize, 0usize, 0usize, 0usize);
    for quad in &quads {
        // Thirds along the axis the turn puts the baseline on: down the page for
        // an upright or upside-down page, across it for a sideways one.
        let sideways = turns % 2 == 1;
        let full = if sideways {
            quad.right - quad.left
        } else {
            quad.bottom - quad.top
        };
        let band = |from: f32, to: f32| {
            if sideways {
                [quad.left + from, quad.top, quad.left + to, quad.bottom]
            } else {
                [quad.left, quad.top + from, quad.right, quad.top + to]
            }
        };
        let near = band(0.0, full / 3.0);
        let centre = band(full / 3.0, 2.0 * full / 3.0);
        let far = band(2.0 * full / 3.0, full);
        // `near` is the low edge of the axis --- top for a vertical split, left
        // for a horizontal one --- so the table above chooses between them.
        let (under_band, opposite_band) = match turns {
            0 | 3 => (far, near),
            _ => (near, far),
        };
        let want = color_for(args.kind);
        under += rule_pixels(&after, aw, ah, under_band, args.scale, want);
        middle += rule_pixels(&after, aw, ah, centre, args.scale, want);
        opposite += rule_pixels(&after, aw, ah, opposite_band, args.scale, want);
        before_any += rule_pixels(&before, bw, bh, near, args.scale, want)
            + rule_pixels(&before, bw, bh, centre, args.scale, want)
            + rule_pixels(&before, bw, bh, far, args.scale, want);
    }

    println!(
        "{:?} on a /Rotate {}: {under} px under the baseline, {middle} in the middle, \
         {opposite} on the far side",
        args.kind,
        turns as u32 * 90
    );

    let mut ok = true;
    ok &= check(
        "the source page has no rule where the mark went (the control)",
        before_any == 0,
    );
    ok &= check(
        "the renderer drew a rule at all",
        under + middle + opposite > 0,
    );
    let (wanted, forbidden, where_) = match args.kind {
        MarkKind::Underline => (under, opposite, "under-the-baseline"),
        MarkKind::StrikeOut => (middle, under, "middle"),
        // **The underline's row exactly, and that is the point rather than an
        // oversight.** A squiggle is a bottom-of-the-quad mark, so this mode
        // says the true and useful thing that its ink is under the baseline and
        // not through the words. What it cannot say is that the mark is a
        // squiggle rather than an underline: thirds are far too coarse, and both
        // kinds put all their ink in the same one. `--mode wave` is where that
        // is measured, and it is a separate mode because it needs a band a third
        // of a quad tall cannot express.
        MarkKind::Squiggly => (under, opposite, "under-the-baseline"),
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
        // Refused above with the box, and for the same reason one step further:
        // ink is not in a band *or* on an edge, it is wherever the reader's hand
        // went, so thirds of its bounding rectangle discriminate nothing at all.
        // `--mode outline` measures it, and the inset it uses for a box does not
        // transfer --- see that mode.
        MarkKind::Ink => unreachable!("refused above"),
        // Refused above with the box, whose reason it shares exactly: an
        // ellipse's ink is on a curve through all three bands rather than in
        // one of them, so thirds discriminate nothing. `--mode outline` is
        // where it is measured, and it takes this kind.
        MarkKind::Ellipse => unreachable!("refused above"),
        // Refused above with the box, whose reason it shares and then some: a
        // stamp's ink is a border round all three bands *and* a word across the
        // middle one, so thirds discriminate nothing about it either.
        // `--mode outline` measures the border, which is the part this mode's
        // question is nearest to.
        MarkKind::Stamp => unreachable!("refused above"),
        // Refused above, and for a reason none of the others has: a text box's
        // ink is wherever its words fall, which depends on how many there are.
        // A one-line box puts everything in the top third and a four-line box
        // fills all three -- so thirds do not describe this kind at all, rather
        // than describing it too coarsely. `--mode text` measures it.
        MarkKind::TextBox => unreachable!("refused above"),
    };
    ok &= check(
        &format!("most of the rule is in the {where_} third ({wanted} px)"),
        wanted * 2 > under + middle + opposite,
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

/// A squiggle rises above where an underline's rule stops; an underline does not.
///
/// **Why this is a mode of its own.** `--mode rule` splits a quad into thirds and
/// asks which one holds the ink. Both an underline and a squiggle put all of
/// theirs in the bottom third, so every assertion that mode makes is satisfied
/// by either kind, and a `Paint` that drew a flat rule for a `/Squiggly` would
/// pass it. The kinds differ in *height*, and a third of a quad is far too
/// coarse to see it.
///
/// The reading is the strip between the two: above where the rule stops, below
/// where the wave's peaks reach. An underline must leave it empty and a squiggle
/// must put ink in it.
///
/// **The two kinds are a pair and both should be run.** Asserting emptiness for
/// the underline alone would be an assertion with no control -- "the strip is
/// clear" and "the renderer drew nothing at all" are the same reading -- and
/// asserting ink for the squiggle alone would not say the strip is anywhere an
/// underline avoids.
fn wave(
    args: &Args,
    document: &OpenDocument,
    bindings: progressive::Bindings,
) -> Result<bool, String> {
    if !matches!(args.kind, MarkKind::Squiggly | MarkKind::Underline) {
        return Err(
            "--mode wave is for a squiggle and its control: pass --kind squiggly or              --kind underline. The other kinds are not lines under words."
                .to_string(),
        );
    }
    let wavy = matches!(args.kind, MarkKind::Squiggly);
    let (out, quads) = mark_and_save(args, document)?;
    if quads.len() != 1 {
        return Err(format!(
            "--mode wave reads one quad and this run made {}; a multi-line run              would put one kind's ink at several heights",
            quads.len()
        ));
    }

    // The same raise `--mode outline` makes and for the same reason: the strip
    // this reads is about a tenth of a quad's height, and on body text that is
    // barely a point. Printed, so a run says what it measured in.
    let scale = args.scale.max(4.0);
    if scale != args.scale {
        println!("     rendering at {scale}x rather than {}x: the strip this reads is a tenth of a quad tall", args.scale);
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

    // **Upright pages only, and refused rather than assumed.** This mode reads
    // two horizontal bands, and which edge of a quad is "under the baseline"
    // depends on the page's turn -- `--mode rule` above has the four-row table
    // and the account of reporting two failures on a sideways underline that was
    // drawn correctly. Rather than repeat that arithmetic for a second mode, a
    // turned page is refused: the discrimination this mode makes is about
    // heights and is the same on every turn, so measuring it upright loses
    // nothing.
    let turns = document.page(args.page)?.quarter_turns() % 4;
    if turns != 0 {
        return Err(format!(
            "--mode wave reads bands down the page and this one is /Rotate {}; the              strip it measures is not horizontal there",
            turns * 90
        ));
    }

    let quad = union(&quads);
    let height = quad[3] - quad[1];
    // **`union` is in display space, where y grows DOWNWARD**, so the foot of
    // the quad is `quad[3]` and not `quad[1]`. Written the other way round
    // first, which put both bands at the top of the quad and read 0 px for an
    // underline that was drawn perfectly -- the control failing is what caught
    // it, one run in, which is the whole argument for the control existing.
    //
    // **Fixed fractions, not derived from `SQUIGGLE_HEIGHT` or
    // `LINE_FRACTION`.** A band computed from the constants it is policing moves
    // with them and stops being able to fail -- the trap about a check that
    // measures along the axis it polices. These sit inside the gap the two
    // constants leave: the rule ends at 7% of the height and the wave's peaks
    // reach 18%, so 10% to 16% is clear of both edges by three points of margin
    // at either end.
    let strip = [
        quad[0],
        quad[3] - height * 0.16,
        quad[2],
        quad[3] - height * 0.10,
    ];
    // The bottom sliver, where BOTH kinds have ink. Without it a squiggle that
    // failed to draw at all would be indistinguishable from an underline that
    // correctly left the strip empty, and the emptiness assertion would be the
    // reassuring branch.
    let foot = [quad[0], quad[3] - height * 0.05, quad[2], quad[3]];

    let want = color_for(args.kind);
    let in_strip = rule_pixels(&after, aw, ah, strip, scale, want);
    let in_foot = rule_pixels(&after, aw, ah, foot, scale, want);
    let control = rule_pixels(&before, bw, bh, strip, scale, want);
    let shape = if wavy { "squiggle" } else { "rule" };
    println!(
        "{shape} over {:.1} pt: {in_strip} px in the strip above the rule, {in_foot} px at the              foot of the quad, {control} on the source page",
        quad[2] - quad[0]
    );

    let mut ok = true;
    ok &= check(
        "the source page has no line where the mark went (the control)",
        control == 0,
    );
    ok &= check(
        &format!("the renderer drew a line at the foot of the quad at all ({in_foot} px)"),
        in_foot > 0,
    );
    if wavy {
        ok &= check(
            &format!("the squiggle rises above where a rule stops ({in_strip} px)"),
            in_strip > 0,
        );
    } else {
        ok &= check(
            &format!("a rule leaves that strip empty (the squiggle's control, {in_strip} px)"),
            in_strip == 0,
        );
    }
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
/// That a stamp is a border **and** a word, which is what makes it a stamp.
///
/// **`--mode outline` cannot do this, and reusing it would have been the
/// mistake.** A stamp is a box with something in it, so every reading that mode
/// takes of a box is also true of a stamp except one --- the middle, which that
/// mode requires to be *empty* and this one requires to be inked. A stamp drawn
/// as a plain rectangle would satisfy `outline` completely, and a stamp drawn as
/// a bare word with no border would satisfy `legible`. Neither says what this
/// kind is. The trap about a near-twin inheriting a predicate, avoided by
/// reading the one thing that differs.
///
/// Three bands, and each of the three is needed:
///
/// - **the whole quad**, so a stamp that drew nothing is not reported as one
///   with an empty middle;
/// - **the middle**, which a box leaves empty and a stamp fills with its word;
/// - **the top edge column**, which a text box leaves empty and a stamp strokes.
///
/// The two failure modes it is aimed at are therefore each other's opposite, and
/// a single reading cannot catch both.
fn stamp(
    args: &Args,
    document: &OpenDocument,
    bindings: progressive::Bindings,
) -> Result<bool, String> {
    if !matches!(args.kind, MarkKind::Stamp) {
        return Err(
            "--mode stamp is for a stamp: pass --kind stamp. Every other kind draws \
             either a border or a filling and not both, which is what --mode outline \
             and --mode legible measure."
                .to_string(),
        );
    }
    let (out, quads) = mark_and_save(args, document)?;
    if quads.len() != 1 {
        return Err(format!(
            "a stamp is one rectangle and this run made {}; --mode stamp cannot read \
             a mark with several",
            quads.len()
        ));
    }

    // `outline`'s reason exactly: a border is OUTLINE_WIDTH thick, and at the
    // default scale its antialiased edges are most of it.
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
            "the copy renders {aw}x{ah} where the source renders {bw}x{bh}, so no \
             pixel comparison between them means anything"
        ));
    }

    let quad = union(&quads);
    let (width, height) = (quad[2] - quad[0], quad[3] - quad[1]);
    let whole = [quad[0], quad[1], quad[2], quad[3]];
    // The middle third both ways, which is where the word is and where a box's
    // stroke is not. A quarter in from each edge would also clear the border;
    // a third is chosen so the band is comfortably inside the inset the word
    // itself sits in, rather than a hair from it.
    let middle = [
        quad[0] + width / 3.0,
        quad[1] + height / 3.0,
        quad[2] - width / 3.0,
        quad[3] - height / 3.0,
    ];
    // A column a tenth of the width, at the horizontal centre, over the top
    // eighth. The border crosses it; the word does not reach it, because the
    // word is centred on the middle and `STAMP_INSET` keeps it clear of the
    // edge.
    //
    // **A tenth rather than one pixel, and the width is the reading.** The first
    // version used `centre + 0.01`, which `rule_pixels` rounds to a single pixel
    // column: a correct border then measured **5 px**, five above a bound of
    // zero, and a check whose passing reading is five pixels is one antialiasing
    // change away from flaking. A tenth of the width is the same question asked
    // where the answer has room --- and it is still comfortably clear of the
    // word, whose cap height is centred on the middle third.
    let centre = (quad[0] + quad[2]) / 2.0;
    let top = [
        centre - width / 20.0,
        quad[3] - height / 8.0,
        centre + width / 20.0,
        quad[3],
    ];

    let want = color_for(args.kind);
    let frame = rule_pixels(&after, aw, ah, whole, scale, want);
    let word = rule_pixels(&after, aw, ah, middle, scale, want);
    let border = rule_pixels(&after, aw, ah, top, scale, want);
    let control = rule_pixels(&before, bw, bh, whole, scale, want);
    println!(
        "stamp {width:.1}x{height:.1} pt: {frame} px in the whole quad, {word} in its \
         middle third, {border} on its top edge, {control} on the source page"
    );

    let mut ok = true;
    ok &= check(
        "the source page has no stamp where the mark went (the control)",
        control == 0,
    );
    ok &= check(
        &format!("the renderer drew the stamp at all ({frame} px)"),
        frame > 0,
    );
    // The discrimination against a box, which strokes an edge and fills nothing.
    ok &= check(
        &format!("the stamp says something in its middle ({word} px)"),
        word > 0,
    );
    // And against a text box, which sets type and draws no border. Read at the
    // top edge rather than anywhere on the frame, because the word could
    // otherwise be mistaken for the border on a stamp whose type is large.
    ok &= check(
        &format!("the stamp has a border round it ({border} px)"),
        border > 0,
    );
    Ok(ok)
}

fn outline(
    args: &Args,
    document: &OpenDocument,
    bindings: progressive::Bindings,
) -> Result<bool, String> {
    if !matches!(args.kind, MarkKind::Square | MarkKind::Ellipse) {
        return Err(
            "--mode outline is for the two shapes: pass --kind square or --kind ellipse.              The other kinds fill something, which is what --mode legible and --mode rule measure."
                .to_string(),
        );
    }
    let round = matches!(args.kind, MarkKind::Ellipse);
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
    // **The one reading that tells the two shapes apart**, and the reason the
    // ellipse needed more than a new `--kind` here. The three readings above are
    // satisfied by a rectangle and by an ellipse alike: both put ink in the
    // quad, both leave the inner half empty, and both cross the centre column at
    // full thickness -- an ellipse touches its bounding box exactly at the top
    // and bottom centres, which is where `edges` reads. So a `subtype` or a
    // `Paint` that drew `re` for an ellipse would pass every one of them.
    //
    // A corner separates them: a rectangle's two edges meet there and an
    // ellipse's curve is nowhere near it. An eighth of each side, which is
    // comfortably clear of the curve -- at an eighth in from the left the
    // ellipse is still within the middle 66% of the height, so the band is empty
    // by a margin rather than by a hair.
    let corner = [
        quad[0],
        quad[1],
        quad[0] + width / 8.0,
        quad[1] + height / 8.0,
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
    let corner_px = rule_pixels(&after, aw, ah, corner, scale, want);
    let shape = if round { "ellipse" } else { "box" };
    println!(
        "{shape} {width:.1}x{height:.1} pt: {frame} px in the whole quad, {middle} inside it,          {thick} px on its thinner edge, {corner_px} in its top-left corner, {control} on the source page"
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
    // The second discrimination, and it runs in **both** directions rather than
    // only for the ellipse. Asserting emptiness for the ellipse alone would be
    // an assertion with no control: a reading of zero would mean "the corner is
    // clear" and "the renderer drew nothing at all" identically, and the box is
    // the case that proves the band is somewhere ink can land.
    if round {
        ok &= check(
            &format!("the ellipse leaves its bounding corner empty ({corner_px} px)"),
            corner_px == 0,
        );
    } else {
        ok &= check(
            &format!("the box draws into its corner (the ellipse's control, {corner_px} px)"),
            corner_px > 0,
        );
    }
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
    document: &OpenDocument,
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
    document: &OpenDocument,
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
fn refuse(_args: &Args, document: &OpenDocument) -> Result<bool, String> {
    let edits = Edits::default();
    edits.open(DOC, document.page_count(), None);
    let state = edits.state(DOC).map_err(|e| format!("no state: {e}"))?;
    let id = state.pages.first().ok_or("the document has no pages")?.id;

    let mut ok = true;

    let empty = edits.annotate(
        DOC,
        NewMark {
            kind: MarkKind::Highlight,
            stamp: None,
            reply_to: None,
            page: id,
            quads: vec![10.0, 10.0, 10.0, 40.0],
            strokes: Vec::new(),
            color: YELLOW,
            width: INK_WIDTH,
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
            stamp: None,
            reply_to: None,
            page: id,
            quads: vec![10.0, 10.0, 40.0],
            strokes: Vec::new(),
            color: YELLOW,
            width: INK_WIDTH,
            author: String::new(),
            note: String::new(),
        },
        save::pdf_date(std::time::SystemTime::now()),
    );
    ok &= check(
        &format!("quads that are not a multiple of four are refused: {ragged:?}"),
        ragged.is_err(),
    );

    // **The biconditional `Mark::strokes` states, both ways round.** Neither is
    // reachable from the window --- the frontend sends strokes for ink and quads
    // for everything else --- so these are the only place the rule can be shown
    // to hold, and a rule with no failing case is a comment.
    let strokes_on_a_highlight = edits.annotate(
        DOC,
        NewMark {
            kind: MarkKind::Highlight,
            stamp: None,
            reply_to: None,
            page: id,
            quads: vec![10.0, 10.0, 200.0, 40.0],
            strokes: vec![vec![10.0, 10.0, 200.0, 40.0]],
            color: YELLOW,
            width: INK_WIDTH,
            author: String::new(),
            note: String::new(),
        },
        save::pdf_date(std::time::SystemTime::now()),
    );
    ok &= check(
        &format!("strokes on a kind that is not ink are refused: {strokes_on_a_highlight:?}"),
        strokes_on_a_highlight.is_err(),
    );

    let ink_with_nothing_drawn = edits.annotate(
        DOC,
        NewMark {
            kind: MarkKind::Ink,
            stamp: None,
            reply_to: None,
            page: id,
            quads: Vec::new(),
            strokes: Vec::new(),
            color: RULE_RED,
            width: INK_WIDTH,
            author: String::new(),
            note: String::new(),
        },
        save::pdf_date(std::time::SystemTime::now()),
    );
    ok &= check(
        &format!("ink with no strokes at all is refused: {ink_with_nothing_drawn:?}"),
        ink_with_nothing_drawn.is_err(),
    );

    // **Not the same refusal as the one above, and this is the one a padded
    // rectangle would let through.** `Stroke::bounds` grows the quad by half the
    // line width, so a stroke standing still still covers area --- which is why
    // the model asks `is_drawable` for ink rather than `covers_area`. Without
    // that branch this passes and a reader gets an invisible mark they cannot
    // find again to remove.
    let ink_that_never_moved = edits.annotate(
        DOC,
        NewMark {
            kind: MarkKind::Ink,
            stamp: None,
            reply_to: None,
            page: id,
            quads: Vec::new(),
            strokes: vec![vec![50.0, 50.0, 50.0, 50.0, 50.0, 50.0]],
            color: RULE_RED,
            width: INK_WIDTH,
            author: String::new(),
            note: String::new(),
        },
        save::pdf_date(std::time::SystemTime::now()),
    );
    ok &= check(
        &format!("ink that never moved is refused: {ink_that_never_moved:?}"),
        ink_that_never_moved.is_err(),
    );

    let odd_stroke = edits.annotate(
        DOC,
        NewMark {
            kind: MarkKind::Ink,
            stamp: None,
            reply_to: None,
            page: id,
            quads: Vec::new(),
            strokes: vec![vec![10.0, 10.0, 40.0]],
            color: RULE_RED,
            width: INK_WIDTH,
            author: String::new(),
            note: String::new(),
        },
        save::pdf_date(std::time::SystemTime::now()),
    );
    ok &= check(
        &format!("a stroke that is not pairs of numbers is refused: {odd_stroke:?}"),
        odd_stroke.is_err(),
    );

    let gone = edits.unannotate(DOC, 4242, 0);
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
            stamp: None,
            reply_to: None,
            page: id,
            quads: vec![10.0, 10.0, 200.0, 40.0],
            strokes: Vec::new(),
            color: YELLOW,
            width: INK_WIDTH,
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
        stamp: StampName::Approved,
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
                    // The serde name once more. `/Ink` is the file's spelling
                    // and "draw" is the reader's; neither is accepted here.
                    "ink" => MarkKind::Ink,
                    // The serde name, the PDF name and the reader's word, all
                    // three the same for once.
                    "squiggly" => MarkKind::Squiggly,
                    // The serde name. `/FreeText` is the file's and "text box"
                    // is the reader's; neither is accepted here.
                    "textbox" => MarkKind::TextBox,
                    // The serde name a fourth time. `/Circle` is the file's
                    // spelling and "ellipse" is both the reader's word and the
                    // serde name, which makes this the one kind where the name
                    // accepted here is also the one a reader would say.
                    "ellipse" => MarkKind::Ellipse,
                    // The serde name, the PDF name and the reader's word, all
                    // three the same for the third time. Which stamp it says is
                    // `--stamp`, and defaults to `approved`.
                    "stamp" => MarkKind::Stamp,
                    other => return Err(format!("unknown kind {other}")),
                }
            }
            "--stamp" => {
                parsed.stamp = match value.as_str() {
                    "approved" => StampName::Approved,
                    "confidential" => StampName::Confidential,
                    "draft" => StampName::Draft,
                    "final" => StampName::Final,
                    other => return Err(format!("unknown stamp {other}")),
                }
            }
            "--out" => parsed.keep = Some(PathBuf::from(value)),
            "--mode" => {
                parsed.mode = match value.as_str() {
                    "roundtrip" => Mode::Roundtrip,
                    "rule" => Mode::Rule,
                    "outline" => Mode::Outline,
                    "wave" => Mode::Wave,
                    "strokes" => Mode::Strokes,
                    "ink" => Mode::Ink,
                    "noap" => Mode::NoAp,
                    "stamp" => Mode::Stamp,
                    "legible" => Mode::Legible,
                    "refuse" => Mode::Refuse,
                    "preview" => Mode::Preview,
                    other => return Err(format!("unknown mode {other}")),
                }
            }
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(parsed)
}
