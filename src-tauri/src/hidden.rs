//! Finds text that is in a page's content and not on the rendered page.
//!
//! The commonest way to redact a document badly is to draw a black rectangle
//! over the words. The page looks redacted and the words are still in the file:
//! select, copy, paste. `tpdf redact` removes the words and reads its own
//! output back; this module is the check for a document **somebody else**
//! redacted, and for one nobody redacted that carries text a reader cannot see.
//!
//! ## What is compared
//!
//! Two answers from the same sandboxed worker about the same page: where each
//! character is ([`PageText`]), and what the page looks like (its pixels). A
//! character is **hidden** when the pixels where it should be show no trace of
//! it. Nothing here reads the document, so nothing here needs to understand how
//! the words came to be hidden --- a rectangle in the content, an annotation, an
//! image, the background's own colour, or a text mode that paints nothing. All
//! of them look the same in the pixels, and that is the point: this asks what a
//! reader of the page can see, which is the question a redaction answers.
//!
//! ## The rule for one character
//!
//! Its box is **uniform** when the brightest and the darkest pixel in it differ
//! by at most [`TOLERANCE`]. A glyph painted where a reader can see it puts ink
//! and background into its box, so a box that is not uniform is a visible
//! character.
//!
//! A uniform box is not yet a hidden one, and the first design stopped there.
//! Some glyphs *are* their box: an underscore, a hyphen, a full stop, a
//! lower-case L in a sans-serif face. Their boxes are solid ink, uniform, and
//! perfectly visible. What tells them from a covered character is the pixels
//! **beside** the box: next to a visible underscore is paper, next to a
//! character under a black bar is more black bar. So a character is hidden when
//! its box is uniform **and** a strip on its left or its right has the same
//! colour.
//!
//! Above and below are deliberately not asked. A careful redactor draws the
//! rectangle tight around the line, so above and below a covered character is
//! paper, and a rule that looked there would pass exactly the documents this
//! exists for.
//!
//! One family defeats the strips: glyphs that fill their box **and touch their
//! neighbours**, which is what a row of underscores or a dotted leader is. They
//! are in [`fills_its_box`] and are never judged; inside a hidden run they are
//! carried along as part of its words.
//!
//! ## What this cannot see, stated so that nobody reads silence as "clean"
//!
//!   - **A page with no text.** A scan with black bars burned into the picture
//!     has nothing to compare; so has a scan whose bars were drawn over the
//!     picture, where the *pixels* underneath are still in the file. [`Judged`]
//!     counts characters, and the caller reports a page that had none.
//!   - **A cover that is not one flat colour**: a photograph, a gradient, a
//!     noisy scan patch. The box under it is not uniform, so the character reads
//!     as visible.
//!   - **A character on the edge of a cover**, or a single character with a
//!     cover drawn exactly to its box. The strip beside it is paper.
//!   - **Characters too small to judge** at the rendered size, counted in
//!     [`Judged::unjudged`].
//!   - Everything that is not page text: comments, form values, attachments,
//!     metadata, earlier versions kept in the file.
//!
//! So the finding is one-sided. Text reported here is in the file and not on
//! the page. No text reported means none was *found*.

use crate::text::PageText;

/// Most that the brightest and darkest pixel of a box may differ by for the
/// box to count as one colour, on a scale of 0 to 255.
///
/// Wide enough for the compression noise in a flat area of a JPEG, narrow
/// enough that 60%-grey text on white (a difference near 100) is ink.
pub const TOLERANCE: u8 = 24;

/// Fewest pixels a box may measure on its shorter side and still be judged.
const MIN_SIDE: usize = 3;

/// Pixels between a box and the strip beside it. Antialiasing blurs a glyph's
/// edge by about one pixel, and a strip touching the box would read that blur.
const STRIP_GAP: usize = 1;

/// Width of the strip beside a box, in pixels.
const STRIP: usize = 2;

/// Fewest hidden characters that make a finding.
///
/// One uniform box with a matching strip is too little: an `l` that touches a
/// table rule is exactly that. Two in a row, with nothing visible between
/// them, has not been produced by any visible text measured so far.
pub const MIN_RUN: usize = 2;

/// Words the page does not show.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The characters, in the order the page's text has them.
    pub text: String,
    /// `left, top, right, bottom` in points from the page's top-left corner,
    /// the frame [`PageText::boxes`] is in.
    pub rect: [f32; 4],
    /// How many of the characters were judged hidden. The rest of `text` is
    /// spaces and characters that are never judged.
    pub hidden: usize,
    /// Whether the words lie outside the page altogether, which is what
    /// cropping a page to hide its margin leaves behind. `false` for words
    /// inside the page that the rendering does not show.
    pub off_page: bool,
}

/// What one page's comparison found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Judged {
    /// Runs of hidden characters, in reading order.
    pub found: Vec<Found>,
    /// Characters whose box was looked at and decided, hidden or visible.
    pub judged: usize,
    /// Characters with ink that could not be decided: no box, a box too small
    /// at this size, a glyph that fills its box, or a single hidden character
    /// with a visible one on each side.
    pub unjudged: usize,
}

/// One character's verdict.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Verdict {
    /// Ink and background are both in its box.
    Visible,
    /// Its box and a strip beside it are one colour.
    Hidden,
    /// Its box lies wholly outside the page.
    OffPage,
    /// It has a place and nothing could be decided about it.
    Unjudged,
    /// It paints nothing by nature: a space, a control, a soft hyphen.
    Blank,
}

/// Compares a page's characters with its pixels.
///
/// `pixels` is `width * height` RGBA, the page rendered upright with no crop at
/// `scale` pixels per point --- what `Request::Tile` returns for `turns: 0`.
/// A `pixels` of the wrong length judges nothing, and says so by counting every
/// character as unjudged.
#[must_use]
pub fn judge(text: &PageText, pixels: &[u8], width: usize, height: usize, scale: f32) -> Judged {
    let image = Image {
        pixels,
        width,
        height,
    };
    let sound = pixels.len() == width.saturating_mul(height).saturating_mul(4) && scale > 0.0;
    let mut out = Judged::default();
    let mut run = Run::default();
    for (at, code) in text.codes.iter().enumerate() {
        let ch = char::from_u32(*code).unwrap_or('\u{fffd}');
        let rect = text
            .boxes
            .get(at * 4..at * 4 + 4)
            .map(|b| [b[0], b[1], b[2], b[3]]);
        // A character's own turn is stored against the unturned page, and
        // the boxes and the pixels are both of the page as displayed.
        let own = text.char_turns.get(at).copied().unwrap_or(0);
        let turned = (own % 2 == 1) != (text.quarter_turns % 2 == 1);
        let verdict = if paints_nothing(ch) {
            Verdict::Blank
        } else if !sound || fills_its_box(ch) {
            Verdict::Unjudged
        } else {
            rect.map_or(Verdict::Unjudged, |rect| image.verdict(rect, scale, turned))
        };
        match verdict {
            Verdict::Visible => {
                out.judged += 1;
                run.close(&mut out);
            }
            Verdict::Hidden | Verdict::OffPage => {
                out.judged += 1;
                let rect = rect.unwrap_or_default();
                let off_page = verdict == Verdict::OffPage;
                if run.is_on_another_line(rect, turned)
                    || (run.hidden > 0 && run.off_page != off_page)
                {
                    run.close(&mut out);
                }
                run.off_page = off_page;
                run.push(ch, Some(rect), true);
            }
            Verdict::Unjudged => {
                out.unjudged += 1;
                run.push(ch, None, false);
            }
            Verdict::Blank => run.push(ch, None, false),
        }
    }
    run.close(&mut out);
    out
}

/// A character that puts no ink on a page whatever is done to it.
fn paints_nothing(ch: char) -> bool {
    ch.is_whitespace()
        || ch.is_control()
        || matches!(
            ch,
            '\u{ad}' | '\u{200b}'..='\u{200f}' | '\u{2060}'..='\u{2064}' | '\u{feff}' | '\u{fffe}' | '\u{ffff}'
        )
}

/// A glyph that is a solid bar or dot in most faces, and that producers set in
/// rows where each touches the next: rules, leaders, separators.
fn fills_its_box(ch: char) -> bool {
    matches!(
        ch,
        '_' | '-' | '.' | '|' | '\u{b7}' | '\u{2010}'..='\u{2015}' | '\u{2022}' | '\u{2026}'
            | '\u{2212}' | '\u{2500}'..='\u{259f}' | '\u{25a0}' | '\u{25aa}'
    )
}

/// A rendered page.
struct Image<'a> {
    pixels: &'a [u8],
    width: usize,
    height: usize,
}

/// The darkest and brightest luminance in an area.
#[derive(Clone, Copy)]
struct Range {
    low: u8,
    high: u8,
}

impl Range {
    /// Whether everything in `other` is within the tolerance of everything here.
    fn matches(self, other: Range) -> bool {
        self.high.max(other.high) - self.low.min(other.low) <= TOLERANCE
    }
}

impl Image<'_> {
    /// What the pixels say about a character whose box is `rect`, in points.
    fn verdict(&self, rect: [f32; 4], scale: f32, turned: bool) -> Verdict {
        let [left, top, right, bottom] = rect.map(|v| v * scale);
        if !(left.is_finite() && top.is_finite() && right.is_finite() && bottom.is_finite())
            || right <= left
            || bottom <= top
        {
            return Verdict::Unjudged;
        }
        if right <= 0.0 || bottom <= 0.0 || left >= self.width as f32 || top >= self.height as f32 {
            // In the text and not on the page at all.
            return Verdict::OffPage;
        }
        // Whole pixels inside the box, so the antialiased edge of a visible
        // glyph counts for it and a neighbour's edge does not.
        let x0 = left.max(0.0).ceil() as usize;
        let y0 = top.max(0.0).ceil() as usize;
        let x1 = (right.floor() as usize).min(self.width);
        let y1 = (bottom.floor() as usize).min(self.height);
        if x1 < x0 + MIN_SIDE || y1 < y0 + MIN_SIDE {
            return Verdict::Unjudged;
        }
        let Some(inside) = self.range(x0, y0, x1, y1) else {
            return Verdict::Unjudged;
        };
        // No separate test for "the box is one colour". `matches` asks that
        // the box and a strip together stay within the tolerance, which a box
        // with ink in it cannot do with any strip.
        // The two sides the text runs through: left and right of an upright
        // character, above and below one set at a quarter turn.
        let near = STRIP_GAP + STRIP;
        let sides = if turned {
            [
                y0.checked_sub(near)
                    .and_then(|y| self.range(x0, y, x1, y + STRIP)),
                self.range(x0, y1 + STRIP_GAP, x1, y1 + near),
            ]
        } else {
            [
                x0.checked_sub(near)
                    .and_then(|x| self.range(x, y0, x + STRIP, y1)),
                self.range(x1 + STRIP_GAP, y0, x1 + near, y1),
            ]
        };
        if sides.into_iter().flatten().any(|side| inside.matches(side)) {
            Verdict::Hidden
        } else {
            Verdict::Visible
        }
    }

    /// The luminance range of an area, or `None` when it reaches off the image
    /// or is empty.
    fn range(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> Option<Range> {
        if x1 <= x0 || y1 <= y0 || x1 > self.width || y1 > self.height {
            return None;
        }
        let mut range = Range { low: 255, high: 0 };
        for y in y0..y1 {
            let row = &self.pixels[(y * self.width + x0) * 4..(y * self.width + x1) * 4];
            for pixel in row.chunks_exact(4) {
                // Integer Rec. 601 luma. Only differences are read, so the
                // channel order of the renderer's pixels does not matter much
                // and the weights need not be exact.
                let luma = (u32::from(pixel[0]) * 77
                    + u32::from(pixel[1]) * 150
                    + u32::from(pixel[2]) * 29)
                    >> 8;
                let luma = luma.min(255) as u8;
                range.low = range.low.min(luma);
                range.high = range.high.max(luma);
            }
        }
        Some(range)
    }
}

/// Hidden characters collected since the last visible one.
#[derive(Default)]
struct Run {
    text: String,
    /// Length of `text` up to and including the last hidden character.
    solid: usize,
    rect: Option<[f32; 4]>,
    /// The box of the last hidden character, for telling a new line.
    last: Option<[f32; 4]>,
    hidden: usize,
    off_page: bool,
}

impl Run {
    /// Whether a character at `rect` is on another line than the last one:
    /// it shares no height with it, or no width when the text is set at a
    /// quarter turn and its lines run down the page.
    fn is_on_another_line(&self, rect: [f32; 4], turned: bool) -> bool {
        let (near, far) = if turned { (0, 2) } else { (1, 3) };
        self.last
            .is_some_and(|last| rect[far] <= last[near] || rect[near] >= last[far])
    }

    /// Adds a character. One that is not hidden joins only a run already open.
    fn push(&mut self, ch: char, rect: Option<[f32; 4]>, hidden: bool) {
        if !hidden && self.hidden == 0 {
            return;
        }
        self.text.push(ch);
        if hidden {
            self.hidden += 1;
            self.solid = self.text.len();
            self.last = rect;
            if let Some(rect) = rect {
                self.rect = Some(self.rect.map_or(rect, |all| {
                    [
                        all[0].min(rect[0]),
                        all[1].min(rect[1]),
                        all[2].max(rect[2]),
                        all[3].max(rect[3]),
                    ]
                }));
            }
        }
    }

    /// Ends the run: a finding when it is long enough, uncounted doubt when not.
    fn close(&mut self, out: &mut Judged) {
        let run = std::mem::take(self);
        if run.hidden >= MIN_RUN {
            out.found.push(Found {
                text: run.text[..run.solid].to_string(),
                rect: run.rect.unwrap_or_default(),
                hidden: run.hidden,
                off_page: run.off_page,
            });
        } else if run.hidden > 0 {
            // Judged hidden and not reported. It moves from decided to
            // undecided so that the two counts still add up to what was seen.
            out.judged -= run.hidden;
            out.unjudged += run.hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A white page of `width` by `height` pixels.
    fn paper(width: usize, height: usize) -> Vec<u8> {
        vec![255; width * height * 4]
    }

    /// Fills `x0..x1, y0..y1` with one grey level.
    fn fill(pixels: &mut [u8], width: usize, area: [usize; 4], level: u8) {
        for y in area[1]..area[3] {
            for x in area[0]..area[2] {
                let at = (y * width + x) * 4;
                pixels[at..at + 3].fill(level);
            }
        }
    }

    /// A page text of `words` set left to right from `x`, each character
    /// `w` by `h` points with `gap` points between two, at scale 1.
    fn line(words: &str, x: f32, y: f32, w: f32, h: f32, gap: f32) -> PageText {
        let mut text = PageText::default();
        let mut left = x;
        for ch in words.chars() {
            text.codes.push(u32::from(ch));
            text.boxes.extend([left, y, left + w, y + h]);
            left += w + gap;
        }
        text
    }

    /// Draws each character of `text` that paints as a glyph: a dark vertical
    /// stem in the middle third of its box, paper either side.
    fn ink(pixels: &mut [u8], width: usize, text: &PageText) {
        for (at, code) in text.codes.iter().enumerate() {
            let ch = char::from_u32(*code).expect("a character");
            if paints_nothing(ch) {
                continue;
            }
            let b = &text.boxes[at * 4..at * 4 + 4];
            let (x0, x1) = (b[0] as usize, b[2] as usize);
            let third = (x1 - x0) / 3;
            fill(
                pixels,
                width,
                [x0 + third, b[1] as usize, x1 - third, b[3] as usize],
                0,
            );
        }
    }

    const W: usize = 200;
    const H: usize = 60;

    fn found(judged: &Judged) -> Vec<&str> {
        judged.found.iter().map(|f| f.text.as_str()).collect()
    }

    /// Words drawn where a reader sees them are not reported, and every
    /// character is counted as compared. The control for everything below.
    #[test]
    fn visible_words_are_compared_and_not_reported() {
        let text = line("visible words", 10.0, 20.0, 9.0, 12.0, 2.0);
        let mut pixels = paper(W, H);
        ink(&mut pixels, W, &text);
        let judged = judge(&text, &pixels, W, H, 1.0);
        assert_eq!(found(&judged), Vec::<&str>::new());
        assert_eq!((judged.judged, judged.unjudged), (12, 0));
    }

    /// The words under a rectangle drawn tight around the line: paper above and
    /// below, the rectangle's colour to the left and right of each character.
    #[test]
    fn words_under_a_tight_black_rectangle_are_reported() {
        let text = line("Jane Example", 10.0, 20.0, 9.0, 12.0, 2.0);
        let mut pixels = paper(W, H);
        ink(&mut pixels, W, &text);
        fill(&mut pixels, W, [6, 19, 146, 33], 0);
        let judged = judge(&text, &pixels, W, H, 1.0);
        assert_eq!(found(&judged), ["Jane Example"]);
        assert_eq!(judged.found[0].hidden, 11, "the space is not a character");
        assert!(!judged.found[0].off_page);
        assert_eq!(judged.found[0].rect, [10.0, 20.0, 140.0, 32.0]);
    }

    /// Whatever the cover's colour, and with no cover at all: words that were
    /// never painted have paper in their boxes and paper beside them.
    #[test]
    fn words_under_any_flat_colour_and_words_never_painted_are_reported() {
        for level in [0u8, 90, 200, 255] {
            let text = line("secret", 10.0, 20.0, 9.0, 12.0, 2.0);
            let mut pixels = paper(W, H);
            fill(&mut pixels, W, [4, 10, 100, 44], level);
            let judged = judge(&text, &pixels, W, H, 1.0);
            assert_eq!(found(&judged), ["secret"], "under level {level}");
        }
    }

    /// Visible words beside covered ones on the same line end the finding.
    #[test]
    fn a_finding_stops_at_the_first_visible_character() {
        let text = line("Name: Jane Roe", 10.0, 20.0, 9.0, 12.0, 2.0);
        let mut pixels = paper(W, H);
        ink(&mut pixels, W, &text);
        // From just left of the J to the end of the line.
        fill(&mut pixels, W, [73, 19, 170, 33], 0);
        let judged = judge(&text, &pixels, W, H, 1.0);
        assert_eq!(found(&judged), ["Jane Roe"]);
    }

    /// A visible character between two covered words makes two findings, and
    /// the space before it belongs to neither.
    #[test]
    fn a_visible_character_divides_two_findings() {
        let text = line("ab c de", 10.0, 20.0, 9.0, 12.0, 2.0);
        let mut pixels = paper(W, H);
        ink(&mut pixels, W, &text);
        fill(&mut pixels, W, [6, 19, 36, 33], 0);
        fill(&mut pixels, W, [61, 19, 90, 33], 0);
        let judged = judge(&text, &pixels, W, H, 1.0);
        assert_eq!(found(&judged), ["ab", "de"]);
        assert_eq!(judged.judged, 5);
    }

    /// A glyph that is a solid block is visible: its box is one colour and the
    /// paper beside it is another. This is the case the strips exist for.
    #[test]
    fn a_glyph_that_fills_its_box_is_visible() {
        let text = line("lIlIlI", 10.0, 20.0, 4.0, 12.0, 4.0);
        let mut pixels = paper(W, H);
        for at in 0..text.codes.len() {
            let b = &text.boxes[at * 4..at * 4 + 4];
            fill(
                &mut pixels,
                W,
                [b[0] as usize, b[1] as usize, b[2] as usize, b[3] as usize],
                0,
            );
        }
        let judged = judge(&text, &pixels, W, H, 1.0);
        assert_eq!(found(&judged), Vec::<&str>::new());
        assert_eq!(judged.judged, 6);
    }

    /// A row of underscores is one solid bar, each touching the next, which
    /// looks exactly like words under a cover. They are never judged.
    #[test]
    fn a_rule_made_of_touching_glyphs_is_not_judged() {
        let text = line("________", 10.0, 30.0, 9.0, 3.0, 0.0);
        let mut pixels = paper(W, H);
        fill(&mut pixels, W, [10, 30, 82, 33], 0);
        let judged = judge(&text, &pixels, W, H, 1.0);
        assert_eq!(found(&judged), Vec::<&str>::new());
        assert_eq!((judged.judged, judged.unjudged), (0, 8));
    }

    /// Grey words on white, dark words on a shaded cell and white words on a
    /// black bar are all visible: each puts two levels into a box.
    #[test]
    fn low_contrast_and_reversed_words_are_visible() {
        for (ground, stem) in [(255u8, 150u8), (215, 0), (0, 255)] {
            let text = line("words", 10.0, 20.0, 9.0, 12.0, 2.0);
            let mut pixels = paper(W, H);
            fill(&mut pixels, W, [0, 0, W, H], ground);
            for at in 0..text.codes.len() {
                let b = &text.boxes[at * 4..at * 4 + 4];
                fill(
                    &mut pixels,
                    W,
                    [
                        b[0] as usize + 3,
                        b[1] as usize,
                        b[2] as usize - 3,
                        b[3] as usize,
                    ],
                    stem,
                );
            }
            let judged = judge(&text, &pixels, W, H, 1.0);
            assert_eq!(found(&judged), Vec::<&str>::new(), "{stem} on {ground}");
            assert_eq!(judged.judged, 5);
        }
    }

    /// A difference inside the tolerance is one colour, and one step past it
    /// is ink. The cover here is a flat area with noise of exactly that size.
    #[test]
    fn the_tolerance_is_where_one_colour_ends() {
        for (noise, hidden) in [(TOLERANCE, true), (TOLERANCE + 1, false)] {
            let text = line("secret", 10.0, 20.0, 9.0, 12.0, 2.0);
            let mut pixels = paper(W, H);
            fill(&mut pixels, W, [4, 10, 100, 44], 40);
            // One pixel in each box, off by `noise`.
            for at in 0..text.codes.len() {
                let b = &text.boxes[at * 4..at * 4 + 4];
                let (x, y) = (b[0] as usize + 4, b[1] as usize + 5);
                fill(&mut pixels, W, [x, y, x + 1, y + 1], 40 + noise);
            }
            let judged = judge(&text, &pixels, W, H, 1.0);
            assert_eq!(!judged.found.is_empty(), hidden, "noise of {noise}");
        }
    }

    /// One hidden character between visible ones is not a finding, and is not
    /// counted as compared either: it moves to the undecided.
    #[test]
    fn one_hidden_character_alone_is_not_reported() {
        let text = line("abc", 10.0, 20.0, 9.0, 12.0, 6.0);
        let mut pixels = paper(W, H);
        ink(&mut pixels, W, &text);
        // Over the b only, with room either side for the strips.
        fill(&mut pixels, W, [21, 19, 38, 33], 0);
        let judged = judge(&text, &pixels, W, H, 1.0);
        assert_eq!(found(&judged), Vec::<&str>::new());
        assert_eq!((judged.judged, judged.unjudged), (2, 1));
    }

    /// Two lines under one cover are two findings, each with its own box.
    #[test]
    fn a_finding_does_not_run_on_into_the_next_line() {
        let mut text = line("first", 10.0, 10.0, 9.0, 12.0, 2.0);
        let second = line("second", 10.0, 30.0, 9.0, 12.0, 2.0);
        text.codes.extend(second.codes);
        text.boxes.extend(second.boxes);
        let mut pixels = paper(W, H);
        fill(&mut pixels, W, [0, 0, W, H], 0);
        let judged = judge(&text, &pixels, W, H, 1.0);
        assert_eq!(found(&judged), ["first", "second"]);
        assert_eq!(judged.found[1].rect[1], 30.0);
    }

    /// Words outside the page are in the file and on no rendering of it. They
    /// are reported apart from words inside the page, even on one line.
    #[test]
    fn words_outside_the_page_are_reported_as_off_the_page() {
        let mut text = line("margin", -80.0, 20.0, 9.0, 12.0, 2.0);
        let inside = line("inside", 10.0, 20.0, 9.0, 12.0, 2.0);
        text.codes.extend(inside.codes);
        text.boxes.extend(inside.boxes);
        let judged = judge(&text, &paper(W, H), W, H, 1.0);
        assert_eq!(found(&judged), ["margin", "inside"]);
        assert_eq!(
            judged.found.iter().map(|f| f.off_page).collect::<Vec<_>>(),
            [true, false]
        );
    }

    /// Characters too small at the rendered size are counted and not guessed
    /// at, and so is a character with no box.
    #[test]
    fn what_cannot_be_judged_is_counted() {
        let mut text = line("tiny", 10.0, 20.0, 2.0, 2.0, 1.0);
        text.codes.push(u32::from('x'));
        text.boxes.extend([0.0, 0.0, 0.0, 0.0]);
        let judged = judge(&text, &paper(W, H), W, H, 1.0);
        assert_eq!(found(&judged), Vec::<&str>::new());
        assert_eq!((judged.judged, judged.unjudged), (0, 5));
    }

    /// The scale maps points to pixels: the same text at twice the scale is
    /// looked for twice as far from the corner.
    #[test]
    fn boxes_are_scaled_to_the_image() {
        let text = line("secret", 10.0, 10.0, 6.0, 8.0, 1.0);
        let mut pixels = paper(W, H);
        // Ink where the text is at scale 2, and nothing where it is at scale 1.
        let doubled = line("secret", 20.0, 20.0, 12.0, 16.0, 2.0);
        ink(&mut pixels, W, &doubled);
        assert!(judge(&text, &pixels, W, H, 2.0).found.is_empty());
        assert_eq!(found(&judge(&text, &pixels, W, H, 1.0)), ["secret"]);
    }

    /// Spaces, soft hyphens and controls paint nothing by nature. They are
    /// never a finding and never end one.
    #[test]
    fn characters_that_paint_nothing_are_neither_hidden_nor_visible() {
        let text = line("a\u{ad} \u{2}b", 10.0, 20.0, 9.0, 12.0, 2.0);
        let judged = judge(&text, &paper(W, H), W, H, 1.0);
        assert_eq!(found(&judged), ["a\u{ad} \u{2}b"]);
        assert_eq!(judged.found[0].hidden, 2);

        let blank = line(" \u{ad}\u{2} ", 10.0, 20.0, 9.0, 12.0, 2.0);
        assert_eq!(judge(&blank, &paper(W, H), W, H, 1.0), Judged::default());
    }

    /// A character set at a quarter turn is read along its own line: the
    /// strips are above and below it. A column of such characters under a
    /// cover as narrow as they are is found, where left and right are paper.
    #[test]
    fn a_turned_character_is_read_along_its_own_line() {
        let mut text = PageText::default();
        for (at, ch) in "down".chars().enumerate() {
            text.codes.push(u32::from(ch));
            let top = 4.0 + at as f32 * 13.0;
            text.boxes.extend([20.0, top, 32.0, top + 10.0]);
            text.char_turns.push(1);
        }
        let mut pixels = paper(W, H);
        fill(&mut pixels, W, [20, 0, 32, H], 0);
        let turned = judge(&text, &pixels, W, H, 1.0);
        assert_eq!(turned.found.len(), 1);
        assert_eq!(turned.found[0].hidden, 4);

        text.char_turns.clear();
        let upright = judge(&text, &pixels, W, H, 1.0);
        assert!(upright.found.is_empty(), "read left and right, it is ink");
    }

    /// On a page the document turns, upright characters run down the page as
    /// displayed, and a character turned against the page runs across it. The
    /// boxes and the pixels are both of the displayed page, so the page's own
    /// turn decides which sides are read.
    #[test]
    fn the_page_turn_and_the_character_turn_are_combined() {
        let mut text = PageText::default();
        for (at, ch) in "down".chars().enumerate() {
            text.codes.push(u32::from(ch));
            let top = 4.0 + at as f32 * 13.0;
            text.boxes.extend([20.0, top, 32.0, top + 10.0]);
        }
        let mut pixels = paper(W, H);
        fill(&mut pixels, W, [20, 0, 32, H], 0);
        text.quarter_turns = 1;
        assert_eq!(judge(&text, &pixels, W, H, 1.0).found.len(), 1);
        text.quarter_turns = 3;
        assert_eq!(judge(&text, &pixels, W, H, 1.0).found.len(), 1);
        text.quarter_turns = 2;
        assert!(judge(&text, &pixels, W, H, 1.0).found.is_empty());
        text.quarter_turns = 1;
        text.char_turns = vec![1; 4];
        assert!(
            judge(&text, &pixels, W, H, 1.0).found.is_empty(),
            "turned back against a turned page, it reads across"
        );
    }

    /// A pixel buffer of the wrong size judges nothing, and says so.
    #[test]
    fn a_buffer_of_the_wrong_size_judges_nothing() {
        let text = line("secret", 10.0, 20.0, 9.0, 12.0, 2.0);
        let judged = judge(&text, &paper(W, H)[4..], W, H, 1.0);
        assert_eq!(
            (judged.found.len(), judged.judged, judged.unjudged),
            (0, 0, 6)
        );
    }
}
