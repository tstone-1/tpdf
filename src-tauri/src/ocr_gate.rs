//! Step 4 of the redaction: render what was removed and ask an engine to read it.
//!
//! `docs/PLAN.md` §6 lists seven carriers a redaction has to defeat and gives
//! step 4 as the only instrument that can say anything about one of them. A byte
//! scan reads the file's own bytes, so it finds a word that is still spelled out
//! in a content stream and it is structurally blind to a *picture* of that word:
//! text converted to outlines is a path drawing the shape of the letters, and a
//! scanned page is a `/DCTDecode` stream whose pixels no parser here decodes.
//! Re-measured 2026-08-27 by `examples/redact_reach_probe.rs`, over 40 real
//! documents and 2,893 word-sized regions: **33.1% carry something the removal
//! cannot take**, and 68% of those are a path --- either one the page draws or
//! one inside a Form XObject. The figure this said before, 39.1% of 154,095
//! regions, was measured before the form and image carriers existed and over a
//! population of whole text objects; a ratio travels between the two and a count
//! does not, which is why only the ratio is written here.
//!
//! This module is the join. [`crate::ocr`] decides ([`crate::ocr::adjudicate`])
//! and chooses the control ([`crate::ocr::control_from_page`]);
//! [`crate::ocr_worker`] runs the engine somewhere else; what was missing was
//! everything in between --- which words the page has, how big to render it, how
//! to build the image the engine is shown, and what a verdict is called when it
//! reaches a reader.
//!
//! **Three coordinate spaces meet here, and only one of them is used.** The
//! removal works in the page's own space, because that is where PDFium reports a
//! page object's bounds; the reader's regions arrive in *display* space, turned
//! and with y increasing downwards; [`crate::text::PageText`] reports character
//! boxes in display space too. So everything in this module is display space,
//! and the regions used are the reader's own rather than the file-space ones
//! `render::redaction_plans_of` derives. That is safe because
//! [`crate::redact::overlaps`] is preserved by the map between them --- quarter
//! turns and a flip take an axis-aligned rectangle to an axis-aligned rectangle
//! --- and it is worth having because the render, the crop and the character
//! boxes then agree without a conversion anywhere.
//!
//! **The words come from before the removal and the pixels from after it**, and
//! neither may be taken from the other's moment. The control has to be set no
//! larger than the smallest thing the regions covered, and after the write those
//! boxes are gone by construction --- so the word list is captured while the
//! reader's document is still open. The pixels are the opposite: [`crate::ocr`]
//! makes it a type-level rule that only already-redacted pixels may be judged,
//! because OCR over the pre-redaction image reinstates the secret as a text
//! layer, which is one of the carriers §6 exists to defeat.

use crate::ocr::{ControlWord, Legibility};
use crate::text::PageText;

/// How tall the control word has to render before the gate will trust a reading.
///
/// A control proves the engine can read *at this size in this image*, so the
/// scale has to be chosen from the control rather than from the page. Sixteen
/// pixels is the shortest line the vendored Vision build read reliably in the
/// probe; below it the control starts failing on documents that are fine, and a
/// gate that fails when things are fine gets switched off.
pub const MIN_CONTROL_PX: f32 = 16.0;

/// The scale floor, in pixels per point.
///
/// Not 1.0: at 1x a 10 pt line is ten pixels tall and the engine reads almost
/// nothing, so the gate would report *not verified* for the ordinary case. This
/// is the scale every measurement in `BUILD.md` was taken at.
pub const MIN_SCALE: f32 = 2.0;

/// The scale ceiling, in pixels per point.
///
/// A bound on work rather than on quality. Eight puts a 2 pt glyph at 16 px,
/// which is past anything a document sets body text in, and the area grows with
/// the square --- so a higher ceiling buys resolution nothing reads and spends it
/// on a buffer that then will not fit.
pub const MAX_SCALE: f32 = 8.0;

/// A word on the page, with its box, grouped out of a page's characters.
///
/// Whitespace separates words, which is the definition the control chooser wants
/// rather than a linguistic one: what it needs is a run of glyphs the engine
/// will return inside one span. `docs/TRAPS.md` records the day the token was a
/// whole line and `adjudicate` --- which requires **one** span to contain it ---
/// refused a working engine.
///
/// A character PDFium gave no box for is kept in the text and left out of the
/// geometry, so a word is placed by the characters that have a position. A word
/// with no positioned character at all is dropped: it cannot be cropped to.
#[must_use]
pub fn words_from(page: &PageText) -> Vec<ControlWord> {
    let mut out: Vec<ControlWord> = Vec::new();
    let mut text = String::new();
    let mut rect: Option<[f32; 4]> = None;

    for (i, code) in page.codes.iter().enumerate() {
        let ch = char::from_u32(*code).unwrap_or(' ');
        if ch.is_whitespace() {
            flush(&mut out, &mut text, &mut rect);
            continue;
        }
        text.push(ch);
        let b = &page.boxes[i * 4..i * 4 + 4];
        let box_ = [b[0], b[1], b[2], b[3]];
        if box_ == [0.0; 4] || !box_.iter().all(|v| v.is_finite()) {
            continue;
        }
        rect = Some(match rect {
            None => box_,
            Some(r) => [
                r[0].min(box_[0]),
                r[1].min(box_[1]),
                r[2].max(box_[2]),
                r[3].max(box_[3]),
            ],
        });
    }
    flush(&mut out, &mut text, &mut rect);
    out
}

fn flush(out: &mut Vec<ControlWord>, text: &mut String, rect: &mut Option<[f32; 4]>) {
    if let (false, Some(r)) = (text.is_empty(), *rect) {
        out.push(ControlWord {
            rect: r,
            text: text.clone(),
        });
    }
    text.clear();
    *rect = None;
}

/// One page's worth of everything the gate needs that only the *source*
/// document can supply.
///
/// Captured while the reader's document is still open, because after the write
/// it cannot be: the control has to be no larger than the smallest box a region
/// covered, and those boxes are gone by construction. For
/// [`crate::redact_document`] the file itself is gone too.
#[derive(Debug, Clone, PartialEq)]
pub struct GatePage {
    /// The page's index in the file.
    pub page: u32,
    /// The reader's regions, in display space --- the same rectangles the marks
    /// carry, not the file-space ones `render::redaction_plans_of` derives.
    pub regions: Vec<[f32; 4]>,
    /// Every word on the page before the removal, with its box.
    pub words: Vec<ControlWord>,
    /// What the removed text-showing operations draw, joined.
    pub taking: String,
    /// The page's displayed width in points.
    ///
    /// Taken from the source's [`crate::text::PageText`] rather than from the
    /// written file's geometry, so it is guaranteed to be the same measurement
    /// the word boxes were made under. If the written page turns out to differ,
    /// the control fails to read back and the gate says *not verified* --- which
    /// is the right answer for a page that moved under the removal.
    pub width_pt: f32,
    /// The page's displayed height in points.
    pub height_pt: f32,
}

/// Drops words the removal will take with an operation but no region covers.
///
/// **Only words no region covers**, which is the whole care in it. A word a
/// region *does* cover is what sets the size the control has to match, so
/// dropping one here would leave [`crate::ocr::control_from_page`] measuring
/// against a larger box and choosing an easier control --- the failure
/// `docs/TRAPS.md` records as *a control that is easier than the check certifies
/// nothing*.
///
/// What this is for is the other end. The removal deletes whole **text-showing
/// operations**, so a word sharing an operation with the covered text disappears
/// from the written page even though nothing was drawn over it. Choosing such a
/// word makes the control fail to read back and the gate report *not verified*
/// --- honest, and a certification thrown away for nothing.
///
/// The test is textual because that is what the plan carries: `taking` is what
/// the removed operations draw. It over-removes when a word occurs both inside
/// and outside a removed operation, which costs coverage and never costs safety.
#[must_use]
pub fn surviving(words: &[ControlWord], regions: &[[f32; 4]], taking: &str) -> Vec<ControlWord> {
    let gone: Vec<&str> = taking.split_whitespace().collect();
    words
        .iter()
        .filter(|w| {
            regions.iter().any(|r| crate::redact::overlaps(w.rect, *r))
                || !gone.iter().any(|g| *g == w.text)
        })
        .cloned()
        .collect()
}

/// Pixels per point to render at so a control of `size_pt` is readable.
///
/// Clamped into [`MIN_SCALE`]..=[`MAX_SCALE`], then halved --- never below the
/// floor --- until the probe image fits `capacity`.
///
/// **`probe_height_pt` is the probe image, not the page**, and that is what makes
/// the scale worth choosing at all. Both mappings the pixels cross are 16 MB
/// ([`crate::worker::TILE_CAPACITY`] and
/// [`crate::ocr_worker::PIXELS_CAPACITY`]), and a whole A4 page at 4x is 32 MB
/// --- so a gate that rendered pages would be stuck at 2x and could never prove
/// anything about small text. It renders the two *strips* instead: a 20 pt region
/// with a 12 pt control beside it is 3.0 MB at 8x on A4. One bound covers both
/// mappings, because each strip is shorter than the two of them stacked.
///
/// # Errors
///
/// If the probe image will not fit even at [`MIN_SCALE`]. That is a *not
/// verified* with both numbers in it rather than a smaller render: shrinking the
/// subject until it fits the instrument is how a check comes to measure something
/// other than what it names, and here it would mean certifying a region nothing
/// could be read on.
pub fn scale_for(
    size_pt: f32,
    width_pt: f32,
    probe_height_pt: f32,
    capacity: usize,
) -> Result<f32, String> {
    // `is_finite` rather than a negated comparison, so NaN and an infinite page
    // are refused by the same clause that refuses a zero one.
    let unusable = |v: f32| !v.is_finite() || v <= 0.0;
    if unusable(size_pt) || unusable(width_pt) || unusable(probe_height_pt) {
        return Err(format!(
            "the probe image would be {width_pt} x {probe_height_pt} pt and the control is \
             {size_pt} pt, so no render scale can be chosen"
        ));
    }
    let mut scale = (MIN_CONTROL_PX / size_pt).clamp(MIN_SCALE, MAX_SCALE);
    while scale > MIN_SCALE && bytes_at(width_pt, probe_height_pt, scale) > capacity {
        scale = (scale * 0.5).max(MIN_SCALE);
    }
    let at_floor = bytes_at(width_pt, probe_height_pt, scale);
    if at_floor > capacity {
        return Err(format!(
            "the probe image for this region is {width_pt:.0} x {probe_height_pt:.0} pt, which at \
             the smallest scale the engine can read at is {at_floor} bytes against a {capacity} \
             byte buffer, so it could not be rendered for the check"
        ));
    }
    Ok(scale)
}

fn bytes_at(width_pt: f32, height_pt: f32, scale: f32) -> usize {
    let w = (width_pt * scale).ceil().max(0.0) as usize;
    let h = (height_pt * scale).ceil().max(0.0) as usize;
    w.saturating_mul(h).saturating_mul(4)
}

/// Blank points between the two strips, and around the whole probe image.
///
/// **Not decoration --- without it the engine misreads both lines.** Butted
/// together, the region's rows end exactly where the control's begin, so each
/// line is cropped flush against the other; on `text-base14` the control word
/// `quartz,` came back as `auartz,` and the gate refused a redaction that was
/// fine. `ocr_probe`'s own strip chooser learned the same thing from the other
/// direction: a recogniser needs the whitespace around a line as much as it
/// needs the line.
///
/// White rather than more page pixels, and that is the care in it. Rendering a
/// few points either side would give the engine real paper --- and would pull in
/// whatever line of text sits there, which is then read as a survivor and refuses
/// a region nothing was wrong with.
pub const SEPARATION_PT: f32 = 12.0;

/// Blank points above the region and below the control.
pub const MARGIN_PT: f32 = 6.0;

/// The image the engine is shown: the region under test, a blank gap, and a
/// control strip.
///
/// **Appended, never drawn over**, so the pixels being judged are exactly the
/// pixels the redaction produced. The band is returned in points because that is
/// what [`crate::ocr::adjudicate`] partitions by, and it is computed from the
/// stack rather than from the control's own place on the page ---
/// [`crate::ocr::ControlChoice`] carries a *crop*, and `docs/TRAPS.md` has more
/// than one entry about a rectangle produced in one space and read in another.
///
/// **The band's edge sits in the middle of the gap**, not at the control's first
/// row. [`crate::ocr::Control::contains`] tests a span's *centre*, because an
/// engine's box is a detection rather than a measurement and Vision's routinely
/// lands a point or two off the pixels it was given; putting the edge in blank
/// space means such a span still falls on the side it came from. Half a gap of
/// slack, in a gap nothing is drawn in.
///
/// Both strips are the page's full width, which is what lets them be
/// concatenated at all: two crops of different widths do not stack, and the
/// measured cost of the wide form is 9 ms for a 1190 x 128 probe.
///
/// # Errors
///
/// If either strip is not a whole number of `width`-pixel rows, or is empty.
/// Both mean the caller built a buffer that does not match what it says it is,
/// which for pixels that came across a process boundary must be a refusal rather
/// than an out-of-bounds slice.
pub fn stack(
    region: &[u8],
    control: &[u8],
    width: u32,
    scale: f32,
) -> Result<(Vec<u8>, u32, [f32; 4]), String> {
    let stride = (width as usize).saturating_mul(4);
    if stride == 0 || scale <= 0.0 {
        return Err(format!(
            "a probe image cannot be {width} px wide at scale {scale}"
        ));
    }
    let rows = |strip: &[u8], name: &str| -> Result<u32, String> {
        if strip.is_empty() || strip.len() % stride != 0 {
            return Err(format!(
                "the {name} strip is {} bytes, which is not a whole number of {width}-pixel rows",
                strip.len()
            ));
        }
        u32::try_from(strip.len() / stride)
            .map_err(|_| format!("the {name} strip has more rows than a tile can have"))
    };
    let under = rows(region, "region")?;
    let band_rows = rows(control, "control")?;
    let margin = (MARGIN_PT * scale).round().max(1.0) as u32;
    let gap = (SEPARATION_PT * scale).round().max(2.0) as u32;

    let total = margin + under + gap + band_rows + margin;
    let mut out = Vec::with_capacity(total as usize * stride);
    let blank =
        |out: &mut Vec<u8>, n: u32| out.extend(std::iter::repeat_n(0xFF, n as usize * stride));
    blank(&mut out, margin);
    out.extend_from_slice(region);
    blank(&mut out, gap);
    out.extend_from_slice(control);
    blank(&mut out, margin);

    let edge = margin + under + gap / 2;
    let placed = [
        0.0,
        edge as f32 / scale,
        width as f32 / scale,
        total as f32 / scale,
    ];
    Ok((out, total, placed))
}

/// The device rows a point rectangle covers, clamped to a page and never empty.
///
/// This is how a rectangle becomes a tile request: the strips are rendered at
/// exactly these rows, so the crop happens in the worker rather than by slicing
/// a page buffer that was never allocated.
///
/// Returns `None` for a rectangle that lands entirely off the page, which is a
/// reason not to certify rather than an empty strip --- an engine handed no rows
/// reads nothing, and reading nothing is the answer that certifies.
#[must_use]
pub fn rows_of(rect: [f32; 4], height_px: u32, scale: f32) -> Option<(u32, u32)> {
    if !rect.iter().all(|v| v.is_finite()) || scale <= 0.0 {
        return None;
    }
    let top = (rect[1].min(rect[3]) * scale).floor().max(0.0) as u32;
    let bottom = (rect[3].max(rect[1]) * scale).ceil().max(0.0) as u32;
    let top = top.min(height_px);
    let bottom = bottom.min(height_px);
    (bottom > top).then_some((top, bottom - top))
}

/// What a verdict is called when it reaches a reader, or `None` for a clean one.
///
/// Every non-`Illegible` verdict produces a sentence, which is
/// [`crate::redact::Applied`]'s rule rather than this function's: `why` is empty
/// exactly when `verified`, so a verdict that does not certify and says nothing
/// would be reported as a clean redaction.
#[must_use]
pub fn reason(page: u32, verdict: &Legibility) -> Option<String> {
    match verdict {
        Legibility::Illegible { .. } => None,
        Legibility::Legible { found } => {
            let mut read: Vec<&str> = found.iter().map(|i| i.text.trim()).collect();
            read.retain(|s| !s.is_empty());
            Some(format!(
                "page {}: the removed area still reads as text when the written page is \
                 rendered --- {}",
                page + 1,
                sample(&read)
            ))
        }
        Legibility::NotVerified { why } => Some(format!(
            "page {}: the removed area could not be shown unreadable. {why}",
            page + 1
        )),
    }
}

/// At most the first few spans, quoted, with the rest counted.
///
/// Bounded because a region over a paragraph yields a paragraph, and a refusal a
/// reader has to scroll is one they stop reading.
fn sample(read: &[&str]) -> String {
    const SHOWN: usize = 3;
    let quoted: Vec<String> = read.iter().take(SHOWN).map(|s| format!("{s:?}")).collect();
    match read.len().checked_sub(SHOWN) {
        Some(rest) if rest > 0 => format!("{} and {rest} more", quoted.join(", ")),
        _ if quoted.is_empty() => "something the engine could not quote".to_string(),
        _ => quoted.join(", "),
    }
}

// ------------------------------------------------------------ the gate run

/// Renders the written file's redacted regions and asks an engine to read them.
///
/// `docs/PLAN.md` §6 step 4, and the only instrument in this repository that can
/// say anything about a region whose carrier is a **picture**. The byte scan
/// beside it reads the file's own bytes, so it finds a word still spelled out in
/// a content stream and is structurally blind to text converted to outlines or
/// to a scan --- 33.1% of realistic regions, measured across 40 real documents
/// on 2026-08-27 by `examples/redact_reach_probe.rs`.
///
/// Returns the reasons not to certify, one per region that could not be shown
/// unreadable, and an empty list when every region was. **An empty list is not a
/// promise on its own**: [`reason`] returns `None` only for
/// [`crate::ocr::Legibility::Illegible`], so a region that was never reached produces a
/// sentence rather than silence.
///
/// The order is what makes it affordable. One text extraction and one control
/// choice per page, one strip render per region, and the engine sees strips
/// rather than pages --- 9 ms for a 1190 x 128 probe against 195 ms for a whole
/// A4 sheet, measured on this machine. Rendering pages would also cap the scale
/// at 2x, since a sheet at 4x is 32 MB against a 16 MB mapping.
///
/// **Nothing here refuses.** Every failure --- no engine on this platform, a file
/// that will not open, a page with no usable control --- is a reason the file
/// cannot be called clean, never a reason to report the redaction as having
/// failed. The words are gone either way, and §6's rule is *never claim clean*
/// rather than *never write*.
pub fn run(
    service: &crate::render::RenderService,
    path: &str,
    password: Option<String>,
    pages: &[GatePage],
) -> Vec<String> {
    if pages.is_empty() {
        return Vec::new();
    }
    // The engine first, because it is the one refusal that is about the machine
    // rather than about this document: on a platform with no engine there is no
    // point opening the file, and the reader gets one sentence rather than one
    // per region saying the same thing.
    let mut engine = match crate::ocr_worker::OcrWorker::spawn() {
        Ok(worker) => worker,
        Err(why) => return vec![format!("the removed areas could not be read back. {why}")],
    };

    let opened =
        match wait(|reply| service.open(std::path::PathBuf::from(path), false, password, reply)) {
            Ok(info) => info,
            Err(refusal) => {
                return vec![format!(
                "the written file could not be reopened to read the removed areas back, so they \
                 could not be shown unreadable: {}",
                refusal.reason
            )]
            }
        };

    let mut why = Vec::new();
    for page in pages {
        why.extend(gate_one_page(service, &mut engine, opened.id, page));
    }

    let _: Result<(), String> = wait(|reply| service.close(opened.id, reply));
    why
}

/// One page of [`run_ocr_gate`]: choose a control, render the strips, adjudicate.
///
/// Split out because the page loop has three ways to end early and a `continue`
/// carrying a reason is how the version written inline came to have a path that
/// skipped a region silently.
fn gate_one_page(
    service: &crate::render::RenderService,
    engine: &mut crate::ocr_worker::OcrWorker,
    doc: u32,
    page: &GatePage,
) -> Vec<String> {
    let survivors = surviving(&page.words, &page.regions, &page.taking);
    let choice = match crate::ocr::control_from_page(&survivors, &page.regions) {
        Ok(choice) => choice,
        Err(too_easy) => {
            return vec![reason(
                page.page,
                &crate::ocr::Legibility::NotVerified {
                    why: format!("{too_easy}"),
                },
            )
            .unwrap_or_default()]
        }
    };

    // The probe image is the tallest region on this page with the control strip
    // under it, which is what the scale has to fit rather than the page.
    let tallest = page
        .regions
        .iter()
        .map(|r| (r[3] - r[1]).abs())
        .fold(0.0f32, f32::max);
    let control_pt = (choice.crop[3] - choice.crop[1]).abs();
    // Plus what `stack` adds: a margin at each end and the gap between them.
    let padding = 2.0 * MARGIN_PT + SEPARATION_PT;
    let capacity = crate::ocr_worker::PIXELS_CAPACITY.min(crate::worker::TILE_CAPACITY);
    let scale = match scale_for(
        choice.size_pt,
        page.width_pt,
        tallest + control_pt + padding,
        capacity,
    ) {
        Ok(scale) => scale,
        Err(why) => {
            return vec![
                reason(page.page, &crate::ocr::Legibility::NotVerified { why }).unwrap_or_default(),
            ]
        }
    };
    let width_px = (page.width_pt * scale).ceil() as u32;
    let height_px = (page.height_pt * scale).ceil() as u32;

    // One control strip for the page, because the choice is the page's.
    let control = match strip(
        service,
        doc,
        page.page,
        choice.crop,
        width_px,
        height_px,
        scale,
    ) {
        Ok(rows) => rows,
        Err(why) => {
            return vec![
                reason(page.page, &crate::ocr::Legibility::NotVerified { why }).unwrap_or_default(),
            ]
        }
    };

    let mut why = Vec::new();
    for region in &page.regions {
        let verdict = judge(
            service, engine, doc, page, *region, &choice, &control, width_px, height_px, scale,
        );
        if let Some(reason) = reason(page.page, &verdict) {
            why.push(reason);
        }
    }
    why
}

/// One region's verdict: render it, stack the control under it, ask the engine.
#[allow(clippy::too_many_arguments)]
fn judge(
    service: &crate::render::RenderService,
    engine: &mut crate::ocr_worker::OcrWorker,
    doc: u32,
    page: &GatePage,
    region: [f32; 4],
    choice: &crate::ocr::ControlChoice,
    control: &[u8],
    width_px: u32,
    height_px: u32,
    scale: f32,
) -> crate::ocr::Legibility {
    let under = match strip(service, doc, page.page, region, width_px, height_px, scale) {
        Ok(rows) => rows,
        Err(why) => return crate::ocr::Legibility::NotVerified { why },
    };
    let (pixels, height, band) = match stack(&under, control, width_px, scale) {
        Ok(built) => built,
        Err(why) => return crate::ocr::Legibility::NotVerified { why },
    };
    let placed = choice.placed(band);
    let image = crate::ocr::Pixels {
        rgba: &pixels,
        width: width_px,
        height,
        scale,
    };
    match engine.recognise(image, &crate::ocr::Options::default()) {
        Ok((id, items)) => crate::ocr::adjudicate(&id, &placed, &Ok(items)),
        Err(e) => unanswered(&e),
    }
}

/// The verdict for an engine that did not answer at all.
///
/// **A function rather than two lines inside [`judge`]**, and the reason is
/// mechanical: `judge` needs a render service and a live worker, so nothing can
/// reach it from a test and a rule written there is a rule no mutation can aim
/// at. The first version of this *was* two lines, and the test written for it
/// built its own expected value inline --- a writer agreeing with its own reader.
/// The mutation harness caught that by surviving.
///
/// What it exists to avoid is fabricating an [`crate::ocr::EngineId`] for
/// [`crate::ocr::adjudicate`] to ignore. That function's first rule returns
/// before the identity is read, so any value would do --- and a name that is not
/// one of [`crate::ocr_worker::KNOWN_ENGINES`] is exactly what `Named::resolve`
/// exists to refuse, sitting in the type that records which engine said a
/// redaction was clean.
///
/// So this is a second *caller* of that rule and must not become a second copy of
/// it: `the_error_path_says_what_adjudicate_would` runs both against the same
/// five errors and requires the same answer.
#[must_use]
pub fn unanswered(e: &crate::ocr::RecogniseError) -> Legibility {
    Legibility::NotVerified {
        why: format!("{e}"),
    }
}

/// The rows one point rectangle covers, rendered as a full-width tile.
///
/// Full width because the two strips have to stack, and two crops of different
/// widths do not. Raw rather than PNG: the pixels are going straight into
/// another process's buffer, and an encode and a decode either side of that
/// would be paid for nothing.
fn strip(
    service: &crate::render::RenderService,
    doc: u32,
    page: u32,
    rect: [f32; 4],
    width_px: u32,
    height_px: u32,
    scale: f32,
) -> Result<Vec<u8>, String> {
    let (top, rows) = rows_of(rect, height_px, scale).ok_or_else(|| {
        format!(
            "the area at {rect:?} pt is not on a page of {height_px} px, so there was nothing to \
             render for the check"
        )
    })?;
    let width = u16::try_from(width_px).map_err(|_| {
        format!("this page is {width_px} px wide at scale {scale}, too wide to render as one strip")
    })?;
    let height = u16::try_from(rows).map_err(|_| {
        format!("this area is {rows} px tall at scale {scale}, too tall to render as one strip")
    })?;
    let request = crate::render::TileRequest {
        rid: 0,
        doc,
        page,
        scale,
        turns: 0,
        invert: false,
        x: 0,
        y: i32::try_from(top).unwrap_or(i32::MAX),
        width,
        height,
        format: crate::render::TileFormat::Raw,
        crop: None,
    };
    match wait(|reply| service.tile(request, reply))? {
        crate::render::TileOutcome::Rendered(tile) => Ok(tile.bytes),
        crate::render::TileOutcome::Abandoned => {
            Err("the render of this area was abandoned, so it could not be checked".into())
        }
    }
}

/// The longest this gate will wait for any single answer from the render service.
///
/// It exists because two of the waits here fail by **waiting** rather than by
/// answering wrongly --- `docs/TRAPS.md` records *a check whose failure mode is a
/// wait cannot fail*, and a redaction that never returns is worse than one
/// reported unverified. Well above the slowest legitimate answer, which is the
/// open of a large file, and well below forever.
const ANSWER_BOUND: std::time::Duration = std::time::Duration::from_secs(60);

/// Drives one of the render service's callback-shaped calls to an answer.
fn wait<T: Send + 'static, E: Send + 'static + From<String>>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, E>) + Send>),
) -> Result<T, E> {
    let (tx, rx) = std::sync::mpsc::channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    match rx.recv_timeout(ANSWER_BOUND) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(E::from(format!(
            "the render service did not answer within {} s while checking the removed areas",
            ANSWER_BOUND.as_secs()
        ))),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(E::from("the render service stopped".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::{EngineId, RecognisedItem};

    fn page(text: &str, boxes: &[[f32; 4]]) -> PageText {
        let codes: Vec<u32> = text.chars().map(|c| c as u32).collect();
        assert_eq!(
            codes.len(),
            boxes.len(),
            "the fixture must place every character"
        );
        PageText {
            codes,
            boxes: boxes.iter().flat_map(|b| b.iter().copied()).collect(),
            height_pt: 800.0,
            width_pt: 600.0,
            quarter_turns: 0,
            extract_ms: 0.0,
            runs: Vec::new(),
        }
    }

    /// One box per character on one line, advancing by `w`.
    fn line(n: usize, top: f32, w: f32, h: f32) -> Vec<[f32; 4]> {
        (0..n)
            .map(|i| [i as f32 * w, top, i as f32 * w + w, top + h])
            .collect()
    }

    #[test]
    fn whitespace_separates_words_and_each_keeps_the_box_of_its_own_glyphs() {
        let boxes = line(7, 100.0, 10.0, 12.0);
        let words = words_from(&page("ab cdef", &boxes));
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "ab");
        assert_eq!(words[0].rect, [0.0, 100.0, 20.0, 112.0]);
        assert_eq!(words[1].text, "cdef");
        assert_eq!(words[1].rect, [30.0, 100.0, 70.0, 112.0]);
    }

    #[test]
    fn a_word_at_the_very_end_of_the_page_is_not_lost() {
        // The flush after the loop. Without it the last word never leaves the
        // accumulator, and a one-word page yields nothing at all.
        let words = words_from(&page("solo", &line(4, 10.0, 8.0, 10.0)));
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "solo");
    }

    #[test]
    fn a_character_with_no_box_stays_in_the_text_and_out_of_the_geometry() {
        let mut boxes = line(4, 50.0, 10.0, 12.0);
        boxes[2] = [0.0; 4];
        let words = words_from(&page("abcd", &boxes));
        assert_eq!(words[0].text, "abcd");
        // c contributed nothing, so the right edge is d's.
        assert_eq!(words[0].rect, [0.0, 50.0, 40.0, 62.0]);
    }

    #[test]
    fn a_word_no_character_of_which_has_a_box_is_dropped() {
        // It cannot be cropped to, so it cannot be a control; keeping it would
        // put a rectangle of four zeroes into the chooser.
        let boxes = vec![[0.0; 4]; 3];
        assert!(words_from(&page("abc", &boxes)).is_empty());
    }

    #[test]
    fn a_word_the_removal_takes_with_its_operation_does_not_survive() {
        // "salary" is nowhere near the region and goes anyway, because the
        // operation drawing it is the one being removed.
        let words = vec![
            ControlWord {
                rect: [0.0, 0.0, 10.0, 10.0],
                text: "keep".into(),
            },
            ControlWord {
                rect: [0.0, 200.0, 10.0, 210.0],
                text: "salary".into(),
            },
        ];
        let left = surviving(&words, &[[0.0, 500.0, 10.0, 510.0]], "  salary  120000 ");
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].text, "keep");
    }

    #[test]
    fn a_word_a_region_covers_is_kept_even_when_the_removal_names_it() {
        // The one that must not be dropped: a covered word is what sets the size
        // the control has to be no easier than, and it is *always* named in
        // `taking`. Dropping it would leave the chooser measuring against a
        // bigger box and picking a control that proves less.
        let words = vec![ControlWord {
            rect: [0.0, 0.0, 10.0, 6.0],
            text: "tiny".into(),
        }];
        let left = surviving(&words, &[[0.0, 0.0, 10.0, 10.0]], "tiny");
        assert_eq!(left.len(), 1, "the covered word must survive the filter");
    }

    #[test]
    fn nothing_taken_leaves_every_word_standing() {
        let words = vec![ControlWord {
            rect: [0.0, 0.0, 10.0, 10.0],
            text: "keep".into(),
        }];
        assert_eq!(surviving(&words, &[], "").len(), 1);
    }

    #[test]
    fn a_smaller_control_is_rendered_larger() {
        // 16 px over an 8 pt word is 2x; over a 4 pt word it is 4x.
        assert_eq!(scale_for(8.0, 600.0, 800.0, 64 << 20).unwrap(), 2.0);
        assert_eq!(scale_for(4.0, 600.0, 800.0, 64 << 20).unwrap(), 4.0);
    }

    #[test]
    fn the_scale_never_leaves_its_bounds() {
        // A 40 pt heading would want 0.4x, and a 1 pt one 16x. The capacity is
        // deliberately far larger than either needs: this is about the clamp,
        // and a buffer the ceiling does not fit in would test the reduction
        // below instead --- which is what a 64 MB one did, at 4x.
        assert_eq!(
            scale_for(40.0, 600.0, 800.0, usize::MAX).unwrap(),
            MIN_SCALE
        );
        assert_eq!(scale_for(1.0, 600.0, 800.0, usize::MAX).unwrap(), MAX_SCALE);
    }

    #[test]
    fn a_probe_image_that_will_not_fit_at_the_chosen_scale_is_rendered_smaller() {
        // A whole A4 sheet, which is what the gate does *not* render: at 8x it
        // is 4760 x 6736 x 4 = 128 MB, so against a 32 MB buffer the only scale
        // that fits is 4x and the halving must find it rather than refusing.
        let scale = scale_for(1.0, 595.0, 842.0, 32 << 20).unwrap();
        assert_eq!(scale, 4.0);
        assert!(bytes_at(595.0, 842.0, scale) <= 32 << 20);
    }

    #[test]
    fn a_probe_image_that_will_not_fit_even_at_the_floor_is_refused_with_both_numbers() {
        // A0. Even at 2x the sheet is 6740 x 9536 x 4 = 257 MB.
        let why = scale_for(8.0, 3370.0, 4768.0, 16 << 20).unwrap_err();
        assert!(
            why.contains("16777216"),
            "the buffer size is missing: {why}"
        );
        assert!(
            why.contains("bytes against"),
            "the page size is missing: {why}"
        );
    }

    #[test]
    fn a_probe_image_with_no_size_yields_no_scale() {
        assert!(scale_for(8.0, 0.0, 800.0, 64 << 20).is_err());
        assert!(scale_for(0.0, 600.0, 800.0, 64 << 20).is_err());
    }

    /// A strip whose every row is its own row index, so a stack can be read back.
    fn striped(width: u32, first: u8, rows: u32) -> Vec<u8> {
        (0..rows)
            .flat_map(|y| std::iter::repeat_n(first + y as u8, width as usize * 4))
            .collect()
    }

    #[test]
    fn the_probe_image_is_the_region_a_blank_gap_and_then_the_control() {
        // At 1x: 6 margin, 10 region, 12 gap, 6 control, 6 margin = 40 rows.
        let (out, height, _) = stack(&striped(4, 10, 10), &striped(4, 60, 6), 4, 1.0).unwrap();
        assert_eq!(height, 40);
        assert_eq!(out.len(), 40 * 16);
        let row = |y: usize| out[y * 16];
        assert_eq!(row(0), 0xFF, "the top margin is blank");
        assert_eq!(row(6), 10, "the region's first row");
        assert_eq!(row(15), 19, "the region's last row");
        assert_eq!(row(16), 0xFF, "the gap is blank");
        assert_eq!(row(27), 0xFF, "still the gap");
        assert_eq!(row(28), 60, "the control's first row");
        assert_eq!(row(33), 65, "the control's last row");
        assert_eq!(row(39), 0xFF, "the bottom margin is blank");
    }

    #[test]
    fn the_two_strips_never_touch() {
        // The whole reason this function is not a concatenation. Butted together
        // the engine read `quartz,` as `auartz,` and the gate refused a clean
        // redaction; the gap is what isolates each line.
        // Neither strip may use 0xFF, which is what blank is: a fixture that
        // collides with the separator cannot tell the two apart, and the first
        // version of this test found the top margin as the control.
        let (out, _, _) = stack(&striped(4, 1, 4), &striped(4, 100, 4), 4, 1.0).unwrap();
        let rows: Vec<u8> = out.chunks_exact(16).map(|r| r[0]).collect();
        let last_region = rows.iter().rposition(|&v| (1..=4).contains(&v)).unwrap();
        let first_control = rows.iter().position(|&v| (100..=103).contains(&v)).unwrap();
        assert!(
            first_control - last_region > 1,
            "the strips are adjacent: region ends at {last_region}, control starts at {first_control}"
        );
        assert!(rows[last_region + 1..first_control]
            .iter()
            .all(|&v| v == 0xFF));
    }

    #[test]
    fn the_band_edge_sits_in_the_middle_of_the_gap_not_at_the_control() {
        // `Control::contains` tests a centre, and an engine's box is a detection
        // rather than a measurement --- so the edge goes where nothing is drawn,
        // and a span reported a point or two off still falls on its own side.
        let (_, total, band) = stack(&striped(4, 0, 10), &striped(4, 0, 6), 4, 1.0).unwrap();
        // 6 margin + 10 region + 6 (half the gap) = 22.
        assert_eq!(band[1], 22.0);
        assert_eq!(band[3], total as f32);
        // Six points of blank either side of the edge.
        assert!(band[1] - (6.0 + 10.0) >= SEPARATION_PT / 2.0 - 0.5);
    }

    #[test]
    fn the_band_is_reported_in_points_at_the_render_scale() {
        let (_, total, band) = stack(&striped(4, 0, 10), &striped(4, 0, 6), 4, 2.0).unwrap();
        // At 2x: 12 margin, 10 region, 24 gap, 6 control, 12 margin = 64 rows.
        assert_eq!(total, 64);
        assert_eq!(band, [0.0, (12.0 + 10.0 + 12.0) / 2.0, 2.0, 32.0]);
    }

    #[test]
    fn a_strip_that_is_not_whole_rows_is_refused_before_it_is_stacked() {
        // The alternative is a band computed from a row count that is wrong,
        // which puts the partition line in the wrong place and decides the
        // verdict --- a survivor counted as the control certifies the region.
        let mut ragged = striped(4, 0, 3);
        ragged.truncate(ragged.len() - 1);
        let why = stack(&ragged, &striped(4, 0, 2), 4, 1.0).unwrap_err();
        assert!(why.contains("region strip"), "{why}");
        let why = stack(&striped(4, 0, 3), &ragged, 4, 1.0).unwrap_err();
        assert!(why.contains("control strip"), "{why}");
    }

    #[test]
    fn an_empty_strip_is_refused_rather_than_stacked_as_no_rows() {
        // A control of no rows cannot be read back, so the verdict would be
        // `NotVerified` anyway --- but a region of no rows is the dangerous one:
        // nothing to read is what `Illegible` looks like.
        let why = stack(&[], &striped(4, 0, 2), 4, 1.0).unwrap_err();
        assert!(why.contains("region strip"), "{why}");
    }

    #[test]
    fn a_rectangle_becomes_the_rows_it_covers_at_this_scale() {
        assert_eq!(rows_of([0.0, 10.0, 100.0, 20.0], 200, 2.0), Some((20, 20)));
        // Fractional edges take every row they touch, in both directions.
        assert_eq!(rows_of([0.0, 10.4, 100.0, 20.1], 200, 1.0), Some((10, 11)));
    }

    #[test]
    fn a_rectangle_hanging_off_the_page_keeps_the_rows_that_are_on_it() {
        assert_eq!(rows_of([0.0, 90.0, 10.0, 120.0], 100, 1.0), Some((90, 10)));
    }

    #[test]
    fn a_rectangle_entirely_off_the_page_covers_no_rows_at_all() {
        assert_eq!(rows_of([0.0, 200.0, 10.0, 210.0], 100, 1.0), None);
        assert_eq!(rows_of([0.0, f32::NAN, 10.0, 4.0], 100, 1.0), None);
    }

    #[test]
    fn an_illegible_verdict_has_nothing_to_say() {
        let clean = Legibility::Illegible {
            engine: EngineId {
                name: "vision",
                build: "1".into(),
            },
        };
        assert_eq!(reason(0, &clean), None);
    }

    #[test]
    fn a_legible_verdict_quotes_what_survived_and_names_the_page_from_one() {
        let found = vec![RecognisedItem {
            text: "Ackerman".into(),
            rect: [0.0; 4],
            confidence: Some(1.0),
        }];
        let why = reason(4, &Legibility::Legible { found }).unwrap();
        assert!(why.starts_with("page 5:"), "{why}");
        assert!(why.contains("\"Ackerman\""), "{why}");
    }

    #[test]
    fn a_long_survivor_list_is_cut_short_and_the_rest_counted() {
        let found: Vec<RecognisedItem> = (0..7)
            .map(|i| RecognisedItem {
                text: format!("w{i}"),
                rect: [0.0; 4],
                confidence: Some(1.0),
            })
            .collect();
        let why = reason(0, &Legibility::Legible { found }).unwrap();
        assert!(why.contains("and 4 more"), "{why}");
        assert!(!why.contains("\"w5\""), "{why}");
    }

    #[test]
    fn the_error_path_says_what_adjudicate_would() {
        // `judge` builds its own `NotVerified` when the engine did not answer,
        // rather than fabricating an `EngineId` for `adjudicate` to ignore. That
        // is a second caller of the rule and must not become a second copy of
        // it, so the two are compared against the same error.
        let control = crate::ocr::ControlChoice {
            crop: [0.0, 0.0, 10.0, 10.0],
            token: "Fixture".into(),
            size_pt: 10.0,
        }
        .placed([0.0, 10.0, 10.0, 20.0]);
        for e in [
            crate::ocr::RecogniseError::Unavailable("no language pack".into()),
            crate::ocr::RecogniseError::Crashed("SIGTRAP".into()),
            crate::ocr::RecogniseError::TimedOut("10s".into()),
            crate::ocr::RecogniseError::Rejected("too large".into()),
            crate::ocr::RecogniseError::MalformedInput("short buffer".into()),
        ] {
            let theirs = crate::ocr::adjudicate(
                &EngineId {
                    name: "vision",
                    build: "1".into(),
                },
                &control,
                &Err(e.clone()),
            );
            assert_eq!(unanswered(&e), theirs, "the two readings of {e:?} disagree");
        }
    }

    #[test]
    fn a_not_verified_verdict_carries_its_own_reason_through() {
        let why = reason(
            2,
            &Legibility::NotVerified {
                why: "the engine died".into(),
            },
        )
        .unwrap();
        assert!(why.starts_with("page 3:"), "{why}");
        assert!(why.ends_with("the engine died"), "{why}");
    }
}
