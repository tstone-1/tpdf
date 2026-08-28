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

use crate::ocr::{ControlWord, Legibility, NotVerifiedCause};
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

/// Pixels per point to render at so something `size_pt` tall is readable.
///
/// **Which height that is belongs to the caller, and getting it wrong is
/// silent.** [`geometry_for`] passes the control *word*'s own height, because
/// that is what the engine is shown. Passing
/// [`crate::ocr::ControlChoice::size_pt`] --- the smallest box a region covered,
/// which the safety rule guarantees is no *shorter* --- looks equivalent and
/// systematically under-renders; `docs/TRAPS.md` has the entry.
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
    let mut scale = scale_wanted(size_pt).clamp(MIN_SCALE, MAX_SCALE);
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

/// What a page's probe image will be rendered at, and how tall the control
/// lands there.
///
/// **The two numbers are one decision and are returned together on purpose.**
/// The height the engine is shown is the product of the two, and until
/// 2026-08-28 no caller had it: `gate_one_page` kept the scale and threw the
/// control's own height away, so the one quantity [`MIN_CONTROL_PX`] is about
/// existed nowhere. Returning only the scale would leave every caller to
/// multiply it back out, and two derivations of one number are the drift
/// `docs/TRAPS.md` records under *two copies of a distinction drift*.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbeGeometry {
    /// Pixels per point the strips are rendered at.
    pub scale: f32,
    /// How tall the control word itself lands in the probe image, in pixels.
    ///
    /// [`MIN_CONTROL_PX`] is the constant this is meant to clear, and since
    /// 2026-08-28 it is what the scale is chosen to clear rather than a
    /// by-product of it. It can still come out below the floor, by either of the
    /// two clamps in [`scale_for`]: a control under 2 pt is under 16 px even at
    /// the [`MAX_SCALE`] ceiling, and a probe image too large for the buffer is
    /// halved toward [`MIN_SCALE`] regardless of what the control needs.
    ///
    /// **Separated 2026-08-28, and it is entirely the first.** `redact-reach-probe`
    /// attributes every sub-floor control to a clamp, using the two inputs it
    /// already holds --- a control under `MIN_CONTROL_PX / MAX_SCALE` cannot
    /// reach the floor at any scale, and a scale below what that control asked
    /// for means the halving loop ran. Over 40 documents at two sampling
    /// densities: 24 and 40 attributed to the ceiling, **0 to the halving**, and
    /// 0 short for neither reason. The buffer clamp has never fired on real
    /// input, so `MIN_SCALE` is a bound the corpus does not reach rather than
    /// one it works against --- worth re-measuring rather than assuming if
    /// `capacity` or the region sampling changes.
    ///
    /// The bucket that clamp leaves behind is not a marginal case: **every one
    /// of those controls failed**, 24 of 24 and 40 of 40. A control the scale
    /// rule structurally cannot serve is one the gate cannot certify against.
    pub control_px: f32,
    /// The probe image's shape in points: width, then height.
    ///
    /// **The planned shape, which [`stack`] then rounds.** It rounds the margin
    /// and the gap to whole rows and takes each strip's real row count, so the
    /// image it builds differs from `height_pt * scale` by a few pixels. The
    /// ratio does not move materially and the ratio is what this is for, but a
    /// caller quoting a pixel height should quote it as planned rather than as
    /// rendered.
    ///
    /// Here because a probe image a page wide and a few dozen rows tall is the
    /// standing suspect for the gate's remaining refusals --- the engine
    /// answering and returning no spans at all --- and until 2026-08-28 no caller
    /// could say what shape the gate had actually asked for. `ocr-probe` swept
    /// four fixtures from 7.0:1 to 0.9:1 and Vision returned a span at every
    /// one, which bounds the question rather than answering it: nothing said
    /// whether 7:1 is the shape a real page produces.
    pub image_pt: (f32, f32),
}

/// The geometry for one page's probe image.
///
/// Split out of `gate_one_page` so a measurement can ask what the gate would
/// render at without running it, and without a second copy of the arithmetic ---
/// which is the whole reason it is public. `examples/redact_reach_probe.rs` is
/// the caller that needs it.
///
/// # Errors
///
/// Two refusals with different remedies, and each carries the cause it belongs
/// to rather than leaving the caller to pick one. Until 2026-08-28 this returned
/// a bare `String` and its only caller filed everything under
/// [`crate::ocr::NotVerifiedCause::ScaleRefused`] --- one predicate answering two
/// questions, which is right until a second kind of failure makes them disagree.
///
/// - [`crate::ocr::NotVerifiedCause::ControlTooSmall`]: no scale in
///   `MIN_SCALE..=MAX_SCALE` renders this control at [`MIN_CONTROL_PX`]. The
///   image would be fine; the control is smaller than
///   `MIN_CONTROL_PX / MAX_SCALE`.
/// - [`crate::ocr::NotVerifiedCause::ScaleRefused`]: whatever [`scale_for`]
///   refuses --- the probe image will not fit even at [`MIN_SCALE`].
pub fn geometry_for(
    page: &GatePage,
    choice: &crate::ocr::ControlChoice,
) -> Result<ProbeGeometry, (String, crate::ocr::NotVerifiedCause)> {
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
    // **`control_pt`, not `choice.size_pt`** --- the engine is shown the control
    // *word*, and a surviving word with neither ascender nor descender is
    // shorter than the box the scale used to be chosen from. Measured
    // 2026-08-28 over 40 documents: of 38 regions the gate could not verify
    // because the control was not read back, 34 had a control rendering under
    // [`MIN_CONTROL_PX`] --- the floor this very call exists to clear. The two
    // quantities are not interchangeable and only one of them is what the
    // engine reads.
    //
    // `size_pt` keeps its own job, which is the *safety* rule: a control may not
    // be set larger than what was removed. That is enforced in
    // [`crate::ocr::control_from_page`], and this does not weaken it --- since
    // `control_pt <= size_pt`, the scale can only come out larger than before.
    let height_pt = tallest + control_pt + padding;
    // Refused before the render is sized, because this is not a question about
    // the buffer. A control under `MIN_CONTROL_PX / MAX_SCALE` cannot be brought
    // to the floor by any scale the gate may pick, and showing the engine one
    // anyway is how 24 of 24 and 40 of 40 such regions came back reported as
    // *the engine did not read the control back* --- true, and pointing at the
    // wrong subsystem.
    let wanted = scale_wanted(control_pt);
    if wanted > MAX_SCALE {
        return Err((
            format!(
                "the smallest thing this page removed is {control_pt:.1} pt, and a control that                  size needs {wanted:.1}x to reach {MIN_CONTROL_PX:.0} px --- past the {MAX_SCALE:.0}x                  ceiling, so no rendering of this page can prove the removal"
            ),
            crate::ocr::NotVerifiedCause::ControlTooSmall,
        ));
    }
    let scale = scale_for(control_pt, page.width_pt, height_pt, capacity)
        .map_err(|why| (why, crate::ocr::NotVerifiedCause::ScaleRefused))?;
    Ok(ProbeGeometry {
        scale,
        control_px: control_pt * scale,
        image_pt: (page.width_pt, height_pt),
    })
}

/// The scale a control of `size_pt` needs to clear [`MIN_CONTROL_PX`], before
/// any clamp is applied to it.
///
/// [`scale_for`] is the only caller that matters and it clamps the answer into
/// `MIN_SCALE..=MAX_SCALE`. This exists **unclamped** so a measurement can ask
/// what a control would have needed, which is the question that decides whether
/// raising [`MAX_SCALE`] would serve the bucket it cannot serve today: over 40
/// documents, every control under `MIN_CONTROL_PX / MAX_SCALE` went unread ---
/// 24 of 24 and 40 of 40 --- and whether a higher ceiling reaches them or merely
/// runs them into `capacity` instead is arithmetic nobody had done.
///
/// Split out rather than written twice, because the clamped and unclamped forms
/// of one rule drifting apart is the failure this file has an entry about.
pub fn scale_wanted(size_pt: f32) -> f32 {
    MIN_CONTROL_PX / size_pt
}

/// How many bytes a probe image of this shape costs at this scale.
///
/// Public so a measurement can ask whether an image *would* fit at a scale the
/// gate did not choose, without a second copy of the arithmetic.
pub fn bytes_at(width_pt: f32, height_pt: f32, scale: f32) -> usize {
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

/// Blanks every pixel of a strip outside a rectangle's columns.
///
/// **The strip is the page's full width and the region usually is not.**
/// [`strip`] renders the rows a rectangle covers as a full-width tile, because
/// two crops of different widths do not stack; without this, everything on those
/// rows is shown to the engine, and `adjudicate` calls anything it reads *the
/// removed area still reads as text*. A reader who marks a name in the middle of
/// a sentence --- which is what *Redact selection* and *Redact every match* both
/// produce --- would be told their redaction could not be shown clean because
/// the rest of the sentence is still there, which it is supposed to be. Measured
/// over 40 real documents before this existed: 54 of 104 regions the removal
/// took whole came back that way.
///
/// **Blanking is sound rather than approximate, and the reason is route B.**
/// [`crate::redact::covered`] marks a text object when it *overlaps* the region,
/// and a removal takes the whole text-showing operation, so after a correct
/// removal no glyph overlapping the region survives. Everything the mask erases
/// is therefore something the reader did not mark and the removal was right to
/// keep --- there is no half-erased survivor to misread, because a survivor
/// straddling the edge would have been removed.
///
/// White (`0xFF` in every channel) is what [`stack`] fills its margins and gap
/// with, so the mask reads to the engine as more of the same blank space rather
/// than as an edge.
///
/// # Errors
///
/// If the strip is not a whole number of `width_px`-pixel rows --- the same
/// refusal [`stack`] makes, and for the same reason: pixels that came across a
/// process boundary must be checked rather than sliced on trust.
///
/// If the rectangle's columns miss the strip entirely. That is the column
/// analogue of [`rows_of`] answering `None`, and it must be a refusal for the
/// same reason: a fully blanked strip reads as nothing, and reading nothing is
/// the answer that certifies.
pub fn mask_columns(
    strip: &mut [u8],
    width_px: u32,
    rect: [f32; 4],
    scale: f32,
) -> Result<(), String> {
    let stride = (width_px as usize).saturating_mul(4);
    if stride == 0 || scale <= 0.0 || !rect.iter().all(|v| v.is_finite()) {
        return Err(format!(
            "a region at {rect:?} pt cannot be masked on a strip {width_px} px wide at scale \
             {scale}"
        ));
    }
    if strip.is_empty() || strip.len() % stride != 0 {
        return Err(format!(
            "the region strip is {} bytes, which is not a whole number of {width_px}-pixel rows",
            strip.len()
        ));
    }
    let left = (rect[0].min(rect[2]) * scale).floor().max(0.0) as usize;
    let right = ((rect[0].max(rect[2]) * scale).ceil().max(0.0) as usize).min(width_px as usize);
    if left >= right {
        return Err(format!(
            "the area at {rect:?} pt is not on a page {width_px} px wide, so there was nothing to \
             read for the check"
        ));
    }
    for row in strip.chunks_exact_mut(stride) {
        row[..left * 4].fill(0xFF);
        row[right * 4..].fill(0xFF);
    }
    Ok(())
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
        Legibility::NotVerified { why, .. } => Some(format!(
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
    match judge_all(service, path, password, pages) {
        Judged::Refused(why) => vec![why],
        Judged::Pages(pages) => pages
            .iter()
            .flat_map(|page| match &page.outcome {
                PageOutcome::Whole(verdict) => reason(page.page, verdict).into_iter().collect(),
                PageOutcome::Regions(verdicts) => verdicts
                    .iter()
                    .filter_map(|verdict| reason(page.page, verdict))
                    .collect::<Vec<_>>(),
            })
            .collect(),
    }
}

/// What the gate decided about one page.
///
/// **Two shapes rather than one, because a page has two ways to end.** A page
/// that could not be judged at all --- no control survived the removal, the probe
/// image will not fit, the control strip would not render --- has one answer, not
/// one per region, and flattening those into a list of the same length as
/// `regions` would say the gate looked at each of them when it looked at none.
#[derive(Debug, Clone, PartialEq)]
pub enum PageOutcome {
    /// The page could not be judged. Always a non-certifying verdict.
    Whole(Legibility),
    /// One verdict per region, in the order [`GatePage::regions`] gave them.
    Regions(Vec<Legibility>),
}

/// One page's regions and what the engine made of them.
#[derive(Debug, Clone, PartialEq)]
pub struct PageVerdicts {
    /// The page this is about, zero-based, as [`GatePage::page`] carried it.
    pub page: u32,
    /// What the gate decided.
    pub outcome: PageOutcome,
}

/// What a whole gate run decided.
#[derive(Debug, Clone, PartialEq)]
pub enum Judged {
    /// Nothing was judged, and the reason is about the machine or the file
    /// rather than about any region: there is no engine on this platform, or the
    /// written file would not reopen. One sentence, not one per region.
    Refused(String),
    /// One entry per [`GatePage`] handed in, in that order.
    Pages(Vec<PageVerdicts>),
}

/// [`run`], with the verdicts kept instead of flattened into sentences.
///
/// **The rectangles are the point.** [`reason`] turns a
/// [`Legibility::Legible`] into a sentence quoting the first few spans, and the
/// boxes the engine reported go with the rest --- so a caller could not ask
/// *where* on the page the surviving text was, which is the difference between a
/// leak inside the region and a neighbour beside it. Since
/// [`mask_columns`] there should be no neighbours left, and a measurement that
/// can only take that on trust is not a measurement.
///
/// `run` is a wrapper over this rather than a second walk of the same pages, so
/// the two cannot come to disagree about what a page's outcome was.
pub fn judge_all(
    service: &crate::render::RenderService,
    path: &str,
    password: Option<String>,
    pages: &[GatePage],
) -> Judged {
    if pages.is_empty() {
        return Judged::Pages(Vec::new());
    }
    // The engine first, because it is the one refusal that is about the machine
    // rather than about this document: on a platform with no engine there is no
    // point opening the file, and the reader gets one sentence rather than one
    // per region saying the same thing.
    let mut engine = match crate::ocr_worker::OcrWorker::spawn() {
        Ok(worker) => worker,
        Err(why) => {
            return Judged::Refused(format!("the removed areas could not be read back. {why}"))
        }
    };

    let opened =
        match wait(|reply| service.open(std::path::PathBuf::from(path), false, password, reply)) {
            Ok(info) => info,
            Err(refusal) => {
                return Judged::Refused(format!(
                    "the written file could not be reopened to read the removed areas back, so \
                     they could not be shown unreadable: {}",
                    refusal.reason
                ))
            }
        };

    let judged = pages
        .iter()
        .map(|page| PageVerdicts {
            page: page.page,
            outcome: gate_one_page(service, &mut engine, opened.id, page),
        })
        .collect();

    let _: Result<(), String> = wait(|reply| service.close(opened.id, reply));
    Judged::Pages(judged)
}

/// One page of [`judge_all`]: choose a control, render the strips, adjudicate.
///
/// Split out because the page loop has three ways to end early and a `continue`
/// carrying a reason is how the version written inline came to have a path that
/// skipped a region silently.
///
/// Those three ends are [`PageOutcome::Whole`] and the fourth is
/// [`PageOutcome::Regions`]. They are a type rather than a convention because a
/// page-wide refusal used to be returned as a one-element list of *sentences*,
/// which a caller counting them would have read as one region judged.
fn gate_one_page(
    service: &crate::render::RenderService,
    engine: &mut crate::ocr_worker::OcrWorker,
    doc: u32,
    page: &GatePage,
) -> PageOutcome {
    let survivors = surviving(&page.words, &page.regions, &page.taking);
    let choice = match crate::ocr::control_from_page(&survivors, &page.regions) {
        Ok(choice) => choice,
        Err(too_easy) => {
            // The cause comes off the refusal rather than being decided here.
            // One bucket for all four of its reasons is what the 2026-08-28
            // measurement found holding 90% of the answer, and a caller
            // re-deciding it would be a second copy of the rule.
            return PageOutcome::Whole(Legibility::NotVerified {
                cause: too_easy.cause(),
                why: format!("{too_easy}"),
                evidence: None,
            });
        }
    };

    let geometry = match geometry_for(page, &choice) {
        Ok(geometry) => geometry,
        Err((why, cause)) => {
            return PageOutcome::Whole(Legibility::NotVerified {
                why,
                cause,
                evidence: None,
            })
        }
    };
    let scale = geometry.scale;
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
            return PageOutcome::Whole(Legibility::NotVerified {
                why,
                cause: NotVerifiedCause::ControlStrip,
                evidence: None,
            })
        }
    };

    PageOutcome::Regions(
        page.regions
            .iter()
            .map(|region| {
                judge(
                    service, engine, doc, page, *region, &choice, &control, width_px, height_px,
                    scale,
                )
            })
            .collect(),
    )
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
    let mut under = match strip(service, doc, page.page, region, width_px, height_px, scale) {
        Ok(rows) => rows,
        Err(why) => {
            return crate::ocr::Legibility::NotVerified {
                why,
                cause: NotVerifiedCause::RegionStrip,
                evidence: None,
            }
        }
    };
    // The strip is the page's full width and the region usually is not, so
    // everything beside it on those rows would otherwise be read back as though
    // the removal had left it. It did, and it was right to.
    if let Err(why) = mask_columns(&mut under, width_px, region, scale) {
        return crate::ocr::Legibility::NotVerified {
            why,
            cause: NotVerifiedCause::Mask,
            evidence: None,
        };
    }
    let (pixels, height, band) = match stack(&under, control, width_px, scale) {
        Ok(built) => built,
        Err(why) => {
            return crate::ocr::Legibility::NotVerified {
                why,
                cause: NotVerifiedCause::Stack,
                evidence: None,
            }
        }
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
        cause: NotVerifiedCause::EngineError,
        evidence: None,
    }
}

/// The rows one point rectangle covers, rendered as a full-width tile.
///
/// Full width because the two strips have to stack, and two crops of different
/// widths do not. Raw rather than PNG: the pixels are going straight into
/// another process's buffer, and an encode and a decode either side of that
/// would be paid for nothing.
///
/// **Full width is not what the engine is shown.** [`mask_columns`] blanks the
/// region strip outside the region's own columns before it is stacked, because
/// a full-width band judges a region together with everything beside it on those
/// rows --- which for a name marked in the middle of a sentence is the rest of
/// the sentence. This function is the render; the crop that matters happens
/// after it and in memory, since the width is what makes stacking possible at
/// all.
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

    /// A strip of `rows` rows, `width` pixels wide, every channel `0x11` --- a
    /// value nothing here writes, so a pixel still holding it was not masked.
    fn strip_of(width: u32, rows: u32) -> Vec<u8> {
        vec![0x11; (width as usize) * (rows as usize) * 4]
    }

    /// The columns of `strip` that are **not** blank white, on its first row.
    fn inked(strip: &[u8], width: u32) -> Vec<usize> {
        (0..width as usize)
            .filter(|x| strip[x * 4..x * 4 + 4] != [0xFF; 4])
            .collect()
    }

    #[test]
    fn masking_keeps_the_region_s_own_columns_and_blanks_the_rest() {
        // 10 pt wide at scale 2 is 20 px; the region is points 3..7, so pixels
        // 6..14. Two rows, because a mask that only did the first would pass a
        // check that read one.
        let mut strip = strip_of(20, 2);
        mask_columns(&mut strip, 20, [3.0, 0.0, 7.0, 4.0], 2.0).expect("masked");
        assert_eq!(inked(&strip, 20), (6..14).collect::<Vec<_>>());
        assert_eq!(inked(&strip[20 * 4..], 20), (6..14).collect::<Vec<_>>());
    }

    #[test]
    fn masking_widens_to_whole_pixels_rather_than_clipping_the_region() {
        // 3.4..6.6 pt at scale 1 covers parts of pixels 3 and 6, and a region is
        // a claim about what disappears --- so the pixel a glyph's edge lands in
        // belongs to the region rather than to its neighbour.
        let mut strip = strip_of(10, 1);
        mask_columns(&mut strip, 10, [3.4, 0.0, 6.6, 1.0], 1.0).expect("masked");
        assert_eq!(inked(&strip, 10), (3..7).collect::<Vec<_>>());
    }

    #[test]
    fn a_region_wider_than_the_page_leaves_every_column() {
        let mut strip = strip_of(8, 1);
        mask_columns(&mut strip, 8, [-50.0, 0.0, 500.0, 1.0], 1.0).expect("masked");
        assert_eq!(inked(&strip, 8), (0..8).collect::<Vec<_>>());
    }

    #[test]
    fn a_region_reversed_left_to_right_masks_the_same_columns() {
        // `Rect` is not normalised anywhere it comes from, and a drag rightwards
        // and a drag leftwards are the same region.
        let mut forwards = strip_of(20, 1);
        let mut backwards = strip_of(20, 1);
        mask_columns(&mut forwards, 20, [3.0, 0.0, 7.0, 4.0], 2.0).expect("masked");
        mask_columns(&mut backwards, 20, [7.0, 0.0, 3.0, 4.0], 2.0).expect("masked");
        assert_eq!(forwards, backwards);
    }

    #[test]
    fn a_region_beside_the_page_is_refused_rather_than_blanked() {
        // The column analogue of `rows_of` answering None, and it has to be a
        // refusal for that function's reason: a fully blank strip reads as
        // nothing, and reading nothing is the answer that certifies.
        let mut strip = strip_of(10, 1);
        let why = mask_columns(&mut strip, 10, [40.0, 0.0, 60.0, 1.0], 1.0)
            .expect_err("a region off the page is not maskable");
        assert!(why.contains("nothing to"), "{why}");
        assert_eq!(inked(&strip, 10), (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn a_strip_that_is_not_whole_rows_is_refused() {
        let mut strip = vec![0x11; 41];
        let why = mask_columns(&mut strip, 10, [0.0, 0.0, 10.0, 1.0], 1.0)
            .expect_err("41 bytes is not a whole number of 10-pixel rows");
        assert!(why.contains("41 bytes"), "{why}");
    }

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
    fn the_reported_shape_is_the_page_wide_and_the_two_strips_tall() {
        // Concrete numbers rather than the formula, because a test that
        // recomputes `tallest + control_pt + padding` is the writer agreeing
        // with its own reader. Page 600 pt wide; tallest region 10 pt; control
        // word 8 pt; `stack` adds a 6 pt margin at each end and a 12 pt gap.
        // 10 + 8 + 24 = 42, so 600 by 42 and an aspect of 14.3:1.
        //
        // The aspect is what this is for: measured 2026-08-28, every silent
        // refusal in a 40-document corpus sat outside the 8:1--16:1 band, which
        // holds four fifths of the population and fails at 9.5%.
        let page = GatePage {
            page: 0,
            regions: vec![[0.0, 0.0, 40.0, 10.0], [0.0, 30.0, 40.0, 36.0]],
            words: Vec::new(),
            taking: String::new(),
            width_pt: 600.0,
            height_pt: 800.0,
        };
        let choice = crate::ocr::ControlChoice {
            crop: [0.0, 50.0, 40.0, 58.0],
            token: "control".into(),
            size_pt: 10.0,
        };
        let geometry = geometry_for(&page, &choice).expect("a geometry");
        assert_eq!(
            geometry.image_pt,
            (600.0, 42.0),
            "the probe image is the page's width by the two strips plus what stack adds"
        );
    }

    #[test]
    fn the_scale_clears_the_floor_for_the_control_and_not_for_the_box() {
        // The two heights that used to be one. `size_pt` is the smallest box a
        // region covered, 10 pt here; the surviving control word draws 5 pt,
        // because it has neither an ascender nor a descender. Choosing the
        // scale from the box gives 16/10 clamped to the 2x floor and renders
        // the control at 10 px -- under `MIN_CONTROL_PX`, which is the number
        // that whole call exists to clear.
        //
        // Measured 2026-08-28: 34 of 38 regions the gate could not verify for
        // want of a readable control were below the floor for exactly this.
        let page = GatePage {
            page: 0,
            regions: vec![[0.0, 0.0, 40.0, 10.0]],
            words: Vec::new(),
            taking: String::new(),
            width_pt: 600.0,
            height_pt: 800.0,
        };
        let choice = crate::ocr::ControlChoice {
            crop: [0.0, 20.0, 40.0, 25.0],
            token: "control".into(),
            size_pt: 10.0,
        };
        let geometry = geometry_for(&page, &choice).expect("a geometry");
        assert!(
            geometry.control_px >= MIN_CONTROL_PX,
            "a 5 pt control rendered at {} px, under the {MIN_CONTROL_PX} px floor, \
             so the engine is being shown something it was never claimed to read",
            geometry.control_px
        );
    }

    #[test]
    fn a_control_no_smaller_than_its_box_is_scaled_the_same_as_before() {
        // The control: where the two heights agree, nothing moves. Without this
        // the test above is satisfied by any change that raises the scale --
        // including raising the floor or dropping the clamp -- and neither of
        // those is the fix.
        let page = GatePage {
            page: 0,
            regions: vec![[0.0, 0.0, 40.0, 8.0]],
            words: Vec::new(),
            taking: String::new(),
            width_pt: 600.0,
            height_pt: 800.0,
        };
        let choice = crate::ocr::ControlChoice {
            crop: [0.0, 20.0, 40.0, 28.0],
            token: "control".into(),
            size_pt: 8.0,
        };
        let geometry = geometry_for(&page, &choice).expect("a geometry");
        assert_eq!(
            geometry.scale, 2.0,
            "an 8 pt control is 16 px at the 2x floor"
        );
        assert_eq!(geometry.control_px, 16.0);
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

    fn tiny_control_page(control_pt: f32) -> (GatePage, crate::ocr::ControlChoice) {
        (
            GatePage {
                page: 0,
                regions: vec![[0.0, 0.0, 40.0, 2.0]],
                words: Vec::new(),
                taking: String::new(),
                width_pt: 600.0,
                height_pt: 800.0,
            },
            crate::ocr::ControlChoice {
                crop: [0.0, 50.0, 40.0, 50.0 + control_pt],
                token: "control".into(),
                size_pt: 10.0,
            },
        )
    }

    #[test]
    fn a_control_no_scale_can_render_is_refused_with_a_cause_of_its_own() {
        // 1.5 pt needs 16/1.5 = 10.7x, past the 8x ceiling. The *image* is
        // fine, so this is not ScaleRefused -- reporting it as one sends a
        // reader to the buffer, and before the split every such region came back
        // ControlUnread, which sends them to the engine. Neither is where the
        // problem is.
        //
        // Measured over 40 documents at two densities: every region whose
        // control was under 2 pt went unread, 24 of 24 and 40 of 40, so refusing
        // here costs no region that was ever judged.
        let (page, choice) = tiny_control_page(1.5);
        let (why, cause) = geometry_for(&page, &choice).expect_err("1.5 pt is unservable");
        assert_eq!(cause, crate::ocr::NotVerifiedCause::ControlTooSmall);
        // The message has to carry both numbers a reader needs to believe it:
        // what the page removed, and what reading it would have taken.
        assert!(why.contains("1.5 pt"), "no control size in: {why}");
        assert!(why.contains("10.7x"), "no required scale in: {why}");
    }

    #[test]
    fn a_control_exactly_at_the_smallest_servable_size_is_served() {
        // 2.0 pt reaches 16 px at the 8x ceiling exactly. The comparison is `>`
        // and not `>=`, which is what makes 2 pt the smallest control the gate
        // can work with rather than the largest one it turns away -- and the
        // first draft of an earlier test got this backwards and failed with
        // `16 px`, which is the boundary being right.
        let (page, choice) = tiny_control_page(2.0);
        let geometry = geometry_for(&page, &choice).expect("2.0 pt is servable");
        assert_eq!(geometry.scale, MAX_SCALE);
        assert_eq!(geometry.control_px, MIN_CONTROL_PX);
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
                cause: NotVerifiedCause::EngineError,
                evidence: None,
            },
        )
        .unwrap();
        assert!(why.starts_with("page 3:"), "{why}");
        assert!(why.ends_with("the engine died"), "{why}");
    }
}
