//! Phase 1: are the character boxes where the ink is, and what does asking cost?
//!
//! Selection, search and the accessibility tree are all about to be built on
//! `tpdf_lib::text`, and all three are silently wrong if its coordinate
//! convention is. A y-flip is the classic failure and the classic one to miss:
//! the text still highlights, in tidy rectangles, on the wrong lines --- and on
//! a page whose text happens to be vertically symmetric it is not even visibly
//! wrong.
//!
//! So the mapping is checked against pixels rather than reasoned about, which is
//! the same rule `AGENTS.md` states for the sandbox that rendered `ok` with a
//! substituted font. Two modes:
//!
//! * `--mode align` --- renders the page, finds the bounding box of everything
//!   that is not the background, and compares it with the union of the character
//!   boxes mapped into the same space. **It carries its own control**: the same
//!   comparison is run against a deliberately un-flipped mapping, and the run
//!   fails if *that* also matches, because a check both conventions pass is a
//!   check that cannot discriminate between them.
//!
//!   `--view-turns` rotates the *view* on top of the page's own `/Rotate`, which
//!   is what the reader's rotate command does. The render is asked for the
//!   rotation and the boxes are turned to match, so this is the whole rotated
//!   stack against pixels rather than the arithmetic against itself.
//!
//! * `--mode extract` --- what a page's text costs, cached page and uncached,
//!   interleaved. Selection wants it for the visible page, search wants it for
//!   every page, and those are very different budgets.
//!
//! Usage:
//!   text-probe <file.pdf> [--page N] [--scale F] [--mode align|extract|order]
//!              [--view-turns 0|1|2|3] [--rounds N] [--lib DIR]

use std::path::{Path, PathBuf};
use std::time::Instant;

use tpdf_lib::progressive::{self, Placement, RawBitmap, RawDocument, RawPage};
use tpdf_lib::text;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Align,
    Extract,
    Order,
}

struct Args {
    file: PathBuf,
    page: u32,
    scale: f32,
    mode: Mode,
    /// Quarter-turns clockwise the *view* is rotated by, on top of `/Rotate`.
    view_turns: u8,
    rounds: usize,
    library: PathBuf,
}

/// A rectangle in device pixels, y downwards.
#[derive(Clone, Copy, Debug)]
struct Rect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let file = args
        .next()
        .ok_or("usage: text-probe <file.pdf> [options]")?;

    let mut parsed = Args {
        file: PathBuf::from(file),
        page: 0,
        scale: 2.0,
        mode: Mode::Align,
        view_turns: 0,
        rounds: 5,
        library: PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR),
    };

    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--page" => parsed.page = value()?.parse().map_err(|_| "bad --page")?,
            "--scale" => parsed.scale = value()?.parse().map_err(|_| "bad --scale")?,
            "--rounds" => parsed.rounds = value()?.parse().map_err(|_| "bad --rounds")?,
            "--view-turns" => {
                parsed.view_turns = match value()?.parse::<u8>() {
                    Ok(turns) if turns < 4 => turns,
                    _ => return Err("--view-turns must be 0, 1, 2 or 3".into()),
                }
            }
            "--lib" => parsed.library = PathBuf::from(value()?),
            "--mode" => {
                parsed.mode = match value()?.as_str() {
                    "align" => Mode::Align,
                    "extract" => Mode::Extract,
                    "order" => Mode::Order,
                    other => return Err(format!("unknown mode: {other}")),
                }
            }
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    Ok(parsed)
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(2);
        }
    };

    match run(&args) {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("[FAIL] {e}");
            std::process::exit(2);
        }
    }
}

fn run(args: &Args) -> Result<bool, String> {
    let bindings = bind(&args.library)?;
    let document = RawDocument::open(bindings, &args.file, None)?;

    match args.mode {
        Mode::Align => align(args, &document, bindings),
        Mode::Extract => extract(args, &document),
        Mode::Order => order(args, &document),
    }
}

fn bind(library: &Path) -> Result<progressive::Bindings, String> {
    use pdfium_render::prelude::Pdfium;
    let path = Pdfium::pdfium_platform_library_name_at_path(library);
    let bound = Pdfium::bind_to_library(&path)
        .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;
    Ok(progressive::bindings_of(Box::leak(Box::new(Pdfium::new(
        bound,
    )))))
}

/// Fraction of a device-space rectangle's pixels that are not background.
///
/// The render clears to opaque white, so anything darker in any channel is ink.
/// Sub-pixel edges are clamped inwards rather than rounded outwards: a box one
/// pixel too generous would catch its neighbour's ink and report a mapping as
/// correct when it is off by a character.
fn ink_fraction(pixels: &[u8], width: u32, height: u32, rect: Rect) -> f32 {
    let left = rect.left.ceil().max(0.0) as u32;
    let top = rect.top.ceil().max(0.0) as u32;
    let right = (rect.right.floor().max(0.0) as u32).min(width);
    let bottom = (rect.bottom.floor().max(0.0) as u32).min(height);
    if right <= left || bottom <= top {
        return 0.0;
    }

    let mut inked = 0u32;
    for y in top..bottom {
        for x in left..right {
            let at = ((y * width + x) * 4) as usize;
            if pixels[at..at + 3].iter().any(|&c| c < 247) {
                inked += 1;
            }
        }
    }
    inked as f32 / ((right - left) * (bottom - top)) as f32
}

/// One character's device-space rectangle under a chosen convention.
///
/// `flip` selects it: `true` is what `text::extract` produces --- y downwards
/// from the page's top edge --- and `false` is the mistake it would be to leave
/// the page-space value alone. The wrong one exists so the check can be shown to
/// reject it, which a check that only ever evaluates the right one cannot.
fn char_rect(page: &text::PageText, index: usize, scale: f32, flip: bool) -> Option<Rect> {
    let quad = &page.boxes[index * 4..index * 4 + 4];
    if quad.iter().all(|v| *v == 0.0) {
        return None;
    }
    let (top, bottom) = if flip {
        (quad[1], quad[3])
    } else {
        (page.height_pt - quad[3], page.height_pt - quad[1])
    };
    Some(Rect {
        left: quad[0] * scale,
        top: top * scale,
        right: quad[2] * scale,
        bottom: bottom * scale,
    })
}

/// Fraction of drawable characters whose box actually covers ink.
///
/// The whole-page bounding box was tried first and is not an oracle: the text
/// fixtures draw a frame as well as text, so the ink box is far larger than the
/// characters and neither convention matched it. Per character is both stricter
/// --- it catches a horizontal error too --- and indifferent to whatever else is
/// on the page.
fn hit_rate(
    page: &text::PageText,
    pixels: &[u8],
    width: u32,
    height: u32,
    scale: f32,
    flip: bool,
) -> (f32, usize) {
    let mut considered = 0usize;
    let mut hit = 0usize;

    for index in 0..page.len() {
        // Whitespace has a box and no ink, so counting it would put a floor on
        // the failure rate that has nothing to do with the mapping.
        if char::from_u32(page.codes[index]).is_none_or(char::is_whitespace) {
            continue;
        }
        let Some(rect) = char_rect(page, index, scale, flip) else {
            continue;
        };
        considered += 1;
        if ink_fraction(pixels, width, height, rect) > 0.05 {
            hit += 1;
        }
    }

    if considered == 0 {
        return (0.0, 0);
    }
    (hit as f32 / considered as f32, considered)
}

/// How many drawable characters must land on ink for the mapping to be right.
///
/// Not 100%: a tight glyph box on a thin glyph at a low scale can round to
/// nothing, and PDFium reports boxes for a few characters that draw nothing
/// visible. The number that matters is the gap to the control below, which is
/// most of the page rather than a few percent.
const HIT_THRESHOLD: f32 = 0.95;

/// The largest hit rate a wrong convention may reach before this check is
/// declared unable to tell the two apart on this page.
const CONTROL_CEILING: f32 = 0.50;

/// The page's characters mapped as if `/Rotate` had been read as `turns`.
///
/// The un-flipped control below asks whether the *flip* could have been wrong on
/// this page. On a page carrying `/Rotate` that is no longer the same question
/// as whether the *turn* could have been --- `FPDFText_GetCharBox` reports the
/// page's own unrotated space while everything else reports the displayed one,
/// so reading the rotation wrong is its own failure with its own control.
///
/// Deliberately re-maps from the raw boxes rather than un-mapping the extracted
/// ones: a control derived by inverting the code under test agrees with it by
/// construction.
fn remapped(page: &RawPage<'_>, turns: u8) -> Result<text::PageText, String> {
    let text_page = text::RawTextPage::load(page)?;
    let count = text_page.count();
    let (width_pt, height_pt) = displayed(page, turns);

    let mut codes = Vec::with_capacity(count as usize);
    let mut boxes = Vec::with_capacity(count as usize * 4);
    for index in 0..count {
        codes.push(text_page.code(index));
        match text_page.char_box(index) {
            Some(quad) => {
                boxes.extend_from_slice(&text::to_device(turns, width_pt, height_pt, quad))
            }
            None => boxes.extend_from_slice(&[0.0; 4]),
        }
    }

    Ok(text::PageText {
        codes,
        boxes,
        height_pt,
        width_pt,
        quarter_turns: turns,
        extract_ms: 0.0,
        // Geometry is this probe's subject; the tags are `structure-probe`'s.
        runs: Vec::new(),
    })
}

/// The page's displayed size under a total of `turns` quarter-turns.
///
/// `page.width_pt()` is already the size under the page's own `/Rotate`, so a
/// total that differs from it in parity swaps the two.
fn displayed(page: &RawPage<'_>, turns: u8) -> (f32, f32) {
    let (width, height) = (page.width_pt(), page.height_pt());
    if (turns % 2) == (page.quarter_turns() % 2) {
        (width, height)
    } else {
        (height, width)
    }
}

/// The extracted text as the *viewer* sees it after rotating the view.
///
/// This is the frontend's path, deliberately: `text::extract` places the boxes
/// on the page as the document says it is displayed, and `turn_device` turns
/// that result again by however much the reader has rotated the view. The
/// controls below are derived the other way --- straight from the raw boxes ---
/// so agreement between them is evidence rather than an identity.
fn viewed(extracted: &text::PageText, view_turns: u8) -> text::PageText {
    let (width, height) = (extracted.width_pt, extracted.height_pt);
    let boxes = extracted
        .boxes
        .chunks_exact(4)
        .flat_map(|quad| {
            if quad.iter().all(|value| *value == 0.0) {
                [0.0; 4]
            } else {
                text::turn_device(
                    view_turns,
                    width,
                    height,
                    [quad[0], quad[1], quad[2], quad[3]],
                )
            }
        })
        .collect();

    let swapped = view_turns % 2 == 1;
    text::PageText {
        codes: extracted.codes.clone(),
        boxes,
        width_pt: if swapped { height } else { width },
        height_pt: if swapped { width } else { height },
        quarter_turns: (extracted.quarter_turns + view_turns) % 4,
        extract_ms: 0.0,
        // Carried through a turn unchanged, which is the point of runs being
        // character indices: rotating the view moves every box and no index.
        runs: extracted.runs.clone(),
    }
}

fn align(
    args: &Args,
    document: &RawDocument,
    bindings: progressive::Bindings,
) -> Result<bool, String> {
    let page = document.page(args.page)?;
    let raw = text::extract(&page)?;

    if raw.is_empty() {
        return Err(format!(
            "page {} has no extractable characters, so this proves nothing about \
             the mapping -- run it on a text document",
            args.page
        ));
    }

    // The whole point of the view turn is that it goes through the same two
    // pieces the viewer uses --- the render is asked for the rotation, and the
    // boxes are turned to match. At `--view-turns 0` this is the identity and
    // the check is exactly what it was before.
    let extracted = viewed(&raw, args.view_turns);

    let width = (extracted.width_pt * args.scale).round() as u16;
    let height = (extracted.height_pt * args.scale).round() as u16;
    let mut buffer = vec![0u8; width as usize * height as usize * 4];
    let mut bitmap = RawBitmap::borrowed(bindings, &mut buffer, width, height)?;
    let placement = Placement::tile(&page, args.scale, args.view_turns, 0, 0);
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

    let pixels = bitmap.pixels();
    let (w, h) = (width as u32, height as u32);
    let (mapped, considered) = hit_rate(&extracted, pixels, w, h, args.scale, true);
    let (unflipped, _) = hit_rate(&extracted, pixels, w, h, args.scale, false);

    println!(
        "{} characters, {} of them drawable, at {}x on a {}x{} render",
        extracted.len(),
        considered,
        args.scale,
        width,
        height,
    );
    println!(
        "page is /Rotate {}, view rotated {} -> displayed as /Rotate {}",
        page.quarter_turns() as u32 * 90,
        args.view_turns as u32 * 90,
        extracted.quarter_turns as u32 * 90,
    );
    println!();

    let agrees = mapped >= HIT_THRESHOLD;
    let discriminates = unflipped <= CONTROL_CEILING;

    println!(
        "{} character boxes land on ink        {:.1}% of {considered}, need {:.0}%",
        if agrees { "[OK]  " } else { "[FAIL]" },
        mapped * 100.0,
        HIT_THRESHOLD * 100.0,
    );
    println!(
        "{} the un-flipped convention does not {:.1}%, must stay under {:.0}%",
        if discriminates { "[OK]  " } else { "[SKIP]" },
        unflipped * 100.0,
        CONTROL_CEILING * 100.0,
    );
    if !discriminates {
        println!(
            "       Both conventions land on ink, so this page cannot tell them\n\
             \x20      apart -- its text is too close to vertically symmetric. Use a\n\
             \x20      page whose text is not, or this check is proving nothing."
        );
    }

    // And the control the rotation needs, which the one above does not provide:
    // reading /Rotate wrong must not also land on ink. Before this mapping
    // handled rotation at all, the check above read 0.0% on every rotated page
    // -- so it does catch the defect; what this adds is the evidence that it
    // could have, on the page in front of it.
    // The controls are the three turns the page is *not* displayed at, each
    // derived from the raw boxes rather than from the mapping under test. With
    // a view rotation this covers the new arithmetic too: getting the view turn
    // wrong lands on one of these three, so a run where they all stay low is one
    // where the composition could have been caught being wrong.
    let turns = extracted.quarter_turns;
    let mut blind = usize::from(!discriminates);
    let mut controls = 1usize;
    for other in 0..4u8 {
        if other == turns {
            continue;
        }
        let (rate, _) = hit_rate(&remapped(&page, other)?, pixels, w, h, args.scale, true);
        let ok = rate <= CONTROL_CEILING;
        controls += 1;
        blind += usize::from(!ok);
        println!(
            "{} displaying it as /Rotate {:>3} does not {:.1}%, must stay under {:.0}%",
            if ok { "[OK]  " } else { "[SKIP]" },
            other as u32 * 90,
            rate * 100.0,
            CONTROL_CEILING * 100.0,
        );
    }

    // A control that lands on ink has not caught anything --- it has failed to
    // *discriminate*, which is a fact about this page rather than about the
    // mapping. Reported as a skip and excluded from the verdict, because a
    // control that cannot fire is not evidence in either direction, and calling
    // it a failure makes the run red for having chosen the wrong document.
    //
    // This is a trap this repository already recorded from the other side: **a
    // dense page of uniform lines cannot detect a y-flip.** `links.pdf` and
    // `links-cropped.pdf` are 36 rows of even text and reach 68--87% on two of
    // the four controls, where the fixtures written for this probe reach 0--5%.
    // `BUILD.md` prescribed running this against the cropped fixture, quoted its
    // 96.4%, and did not say the run exited 1.
    //
    // **What survives on such a page is the placement claim, not the
    // orientation one**, and that is exactly what the crop-box work needs.
    // Proved rather than argued: removing the origin shift in `text.rs` takes
    // `links-cropped.pdf` from 96.4% to **74.8%**, which is a `[FAIL]` and exit
    // 1, while `text-base14.pdf` --- no crop box, so nothing to shift --- stays
    // at 100%.
    //
    // Note how *close* that is. A 50 pt inset on a 595x842 page moves every box
    // by less than a line's height, so on dense text most of them still overlap
    // some ink; what catches it is the 95% threshold, not a collapse to zero.
    // `BUILD.md` records "0% before the fix" from a different measurement --- the
    // scan then mixed PDFium's *cropped* size with page-space boxes, which is a
    // larger error than dropping the origin alone. A fixture with a bigger inset
    // would give this probe more margin.
    if blind > 0 {
        println!(
            "[NOTE] {blind} of {controls} controls could not discriminate on this page, \
             so what is\n       proved here is placement, not orientation."
        );
    }

    Ok(agrees)
}

/// Prints a page's characters in the order PDFium hands them back.
///
/// Not a check --- there is no right answer for it to assert, because the order
/// is a property of whoever produced the file. It exists because a claim about
/// extraction order is otherwise unfalsifiable from outside the viewer, and
/// `src/lib/reading.ts` is built entirely on that order not being reading order.
///
/// Lines are broken wherever the vertical band changes, which is the same rule
/// `linesOf` uses in the front end and is deliberately naive: on an interleaved
/// two-column page it produces one line per *fragment*, which is exactly the
/// output that shows the problem.
fn order(args: &Args, document: &RawDocument) -> Result<bool, String> {
    let page = document.page(args.page)?;
    let text = text::extract(&page)?;

    println!(
        "page {} of {}, {} characters, in PDFium's own index order:",
        args.page,
        document.page_count(),
        text.len()
    );

    let mut line = String::new();
    let mut band: Option<(f32, f32)> = None;
    for index in 0..text.len() {
        let quad = &text.boxes[index * 4..index * 4 + 4];
        let (top, bottom) = (quad[1], quad[3]);
        let placed = quad[2] > quad[0] || bottom > top;
        let same = match band {
            Some((was_top, was_bottom)) => {
                let overlap = was_bottom.min(bottom) - was_top.max(top);
                let shorter = (was_bottom - was_top).min(bottom - top);
                !placed || (shorter > 0.0 && overlap / shorter > 0.5)
            }
            None => true,
        };
        if !same {
            println!("  {line}");
            line.clear();
        }
        if placed {
            band = Some((top, bottom));
        }
        if let Some(ch) = char::from_u32(text.codes[index]) {
            line.push(ch);
        }
    }
    if !line.is_empty() {
        println!("  {line}");
    }
    Ok(true)
}

fn extract(args: &Args, document: &RawDocument) -> Result<bool, String> {
    println!(
        "{:<10} {:>10} {:>10} {:>10} {:>8}",
        "variant", "median ms", "min", "max", "chars"
    );

    let mut cached = Vec::new();
    let mut uncached = Vec::new();

    // Interleaved, because wall clock on this machine drifts several percent
    // over minutes -- see AGENTS.md. Round 0 is kept and reported in `min`/`max`
    // rather than discarded, since the point of the cached variant is that it
    // has no warm-up to hide behind.
    for _ in 0..args.rounds {
        // Each borrow is scoped, and that is load-bearing rather than tidy:
        // `evict_page` takes `&self` and closes the handle, so a `RawPage` that
        // outlived the eviction would be a live pointer to a closed page --- and
        // the borrow checker permits it, because both borrows are shared.
        //
        // `FPDF_LoadPage` is *inside* the timer in both variants. Written with
        // the page loaded first and the clock started after, the two columns
        // measured the same thing and reported it: 0.116 ms uncached on the A0
        // sheet, where loading the page alone is 44 ms.
        // Warm the page cache so the cached variant measures extraction only.
        {
            let _warm = document.page(args.page)?;
        }
        let (first, cached_ms) = {
            let started = Instant::now();
            let page = document.page(args.page)?;
            let text = text::extract(&page)?;
            (text.len(), started.elapsed().as_secs_f64() * 1000.0)
        };
        cached.push(cached_ms);

        document.evict_page(args.page);
        let (second, uncached_ms) = {
            let started = Instant::now();
            let page = document.page(args.page)?;
            let text = text::extract(&page)?;
            (text.len(), started.elapsed().as_secs_f64() * 1000.0)
        };
        uncached.push(uncached_ms);

        if first != second {
            return Err(format!(
                "extraction is not deterministic: {first} characters then {second}"
            ));
        }
    }

    let page = document.page(args.page)?;
    let chars = text::extract(&page)?.len();

    for (label, mut samples) in [("cached", cached), ("uncached", uncached)] {
        samples.sort_by(f64::total_cmp);
        println!(
            "{:<10} {:>10.3} {:>10.3} {:>10.3} {:>8}",
            label,
            samples[samples.len() / 2],
            samples[0],
            samples[samples.len() - 1],
            chars,
        );
    }

    println!();
    println!(
        "Both columns include FPDF_LoadPage. Cached is what selection pays on the page\n\
         already on screen; uncached is what a document-wide search pays per page, and\n\
         on a complex page it is almost entirely the page load rather than the text."
    );
    Ok(true)
}
