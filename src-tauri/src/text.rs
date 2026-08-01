//! Characters, where they sit on the page, and what asking costs.
//!
//! Selection, search and the accessibility tree all need the same thing: the
//! page's characters with their positions. This is that layer, and it is
//! deliberately the *only* one --- three features reading three different
//! extractions would disagree with each other in ways no test would catch,
//! because each would be self-consistent.
//!
//! ## Codes, not a string
//!
//! [`PageText`] carries one Unicode scalar per PDFium character index, not a
//! string. `FPDFText_GetText` exists and would be shorter, but it extracts
//! UCS-2 and, in its own words, "ignores characters without UCS-2
//! representations" --- so the string it returns and the character indices the
//! boxes are keyed by can silently fall out of step, on exactly the documents
//! (CJK, symbol fonts, anything astral) where nobody would notice until a
//! selection highlighted the wrong glyphs.
//!
//! One code per index cannot desync, and the caller builds whatever string it
//! wants from the range it selected. It is the same lesson `AGENTS.md` records
//! for `set_text()`: work in the code space the document uses, not in a
//! re-encoding of it.
//!
//! ## Page space, and why the boxes are turned here
//!
//! PDFium reports character boxes in page space --- y upwards, origin at the
//! bottom-left. Every consumer here works in device space --- y downwards,
//! origin top-left --- because that is what the tiles and the viewport are in.
//! The conversion happens once, in [`to_device`], and doing it anywhere else
//! means two conventions in the codebase and an inevitable off-by-a-page-height.
//!
//! It is a **rotation**, not only a flip, and that is not a generalisation for
//! its own sake. A page carrying `/Rotate 90` --- which is what a scanner emits,
//! not an edge case --- is displayed turned a quarter clockwise, and PDFium
//! reports it in two different coordinate systems at once:
//!
//! * `FPDF_GetPageWidthF` / `GetPageHeightF` give the size **after** rotation,
//!   and a render comes out rotated to match. Layout and tiles are already right.
//! * `FPDFText_GetCharBox` gives boxes in the page's own **unrotated** space.
//!
//! So the obvious flip --- `height_pt - y` against the reported height --- is
//! correct at `/Rotate 0` and wrong at every other value. Measured before it was
//! fixed, with `text-probe --mode align` on `testdata/rotated.pdf`: **100% of
//! character boxes landed on ink at 0 and 0.0% at 90, 180 and 270.** Not
//! approximately wrong; every selection and every search highlight on a scanned
//! page was somewhere else entirely, in tidy rectangles.

use std::os::raw::{c_double, c_int};

use pdfium_render::prelude::*;

use crate::progressive::{Bindings, RawPage};

/// A loaded `FPDF_TEXTPAGE`, closed on drop.
///
/// Borrows its page for the same reason [`RawPage`] borrows its document:
/// PDFium does not tolerate a text page outliving the page it was loaded from,
/// and the lifetime is what makes that unrepresentable rather than merely
/// documented.
pub struct RawTextPage<'page> {
    bindings: Bindings,
    handle: FPDF_TEXTPAGE,
    _page: std::marker::PhantomData<&'page ()>,
}

impl<'page> RawTextPage<'page> {
    /// Loads the text of a page.
    pub fn load(page: &'page RawPage<'_>) -> Result<Self, String> {
        let bindings = page.bindings();
        // SAFETY: the handle is valid for the borrow of `page`.
        let handle = unsafe { bindings.FPDFText_LoadPage(page.handle()) };
        if handle.is_null() {
            return Err("could not load the page's text".to_string());
        }
        Ok(Self {
            bindings,
            handle,
            _page: std::marker::PhantomData,
        })
    }

    /// The raw handle, for the few callers that need a binding this does not wrap.
    ///
    /// `structure.rs` is the one: `FPDFText_GetTextObject` is what relates a
    /// character to the marked content it was drawn inside, and wrapping it here
    /// would mean this module owning a page-object type it has no other use for.
    pub(crate) fn handle(&self) -> FPDF_TEXTPAGE {
        self.handle
    }

    /// Characters on the page, including ones that draw nothing.
    pub fn count(&self) -> u32 {
        // SAFETY: `self.handle` is non-null for the lifetime of `self`.
        let count = unsafe { self.bindings.FPDFText_CountChars(self.handle) };
        count.max(0) as u32
    }

    /// The Unicode scalar at a character index.
    pub fn code(&self, index: u32) -> u32 {
        // SAFETY: as above; an out-of-range index returns 0 rather than faulting.
        unsafe {
            self.bindings
                .FPDFText_GetUnicode(self.handle, index as c_int)
        }
    }

    /// The tight box of a character, in page space: `[left, bottom, right, top]`.
    ///
    /// `None` when PDFium declines, which it does for characters that occupy no
    /// area --- a space at the end of a line is the common one.
    pub fn char_box(&self, index: u32) -> Option<[f64; 4]> {
        let (mut left, mut right, mut bottom, mut top) = (0f64, 0f64, 0f64, 0f64);
        // SAFETY: four writable doubles, and the index is bounds-checked by
        // PDFium, which returns false rather than writing on a bad one.
        let ok = unsafe {
            self.bindings.FPDFText_GetCharBox(
                self.handle,
                index as c_int,
                &mut left as *mut c_double,
                &mut right as *mut c_double,
                &mut bottom as *mut c_double,
                &mut top as *mut c_double,
            )
        };
        // Note the argument order: PDFium takes left, *right*, bottom, top --
        // not the left, bottom, right, top that every rect in this file uses.
        (ok != 0).then_some([left, bottom, right, top])
    }
}

impl Drop for RawTextPage<'_> {
    fn drop(&mut self) {
        // SAFETY: loaded by `load`, closed exactly once, and its page outlives
        // it by construction.
        unsafe { self.bindings.FPDFText_ClosePage(self.handle) };
    }
}

/// A page's characters and where they are, in device-space units of one point.
///
/// The arrays are flat and parallel rather than a `Vec` of structs: this
/// crosses to the webview as JSON, where a struct per character would repeat
/// four field names a few thousand times per page.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PageText {
    /// One Unicode scalar per character index. See the module docs.
    pub codes: Vec<u32>,
    /// Four values per character --- `left, top, right, bottom` --- with y
    /// increasing downwards and the origin at the page's top-left corner, in
    /// PDF points. A character PDFium gave no box for is four zeroes; `codes`
    /// still carries it, so the indices stay aligned.
    pub boxes: Vec<f32>,
    /// Page height in points, so a caller can scale without a second request.
    pub height_pt: f32,
    /// Page width in points.
    pub width_pt: f32,
    /// Quarter-turns clockwise the page is displayed rotated by: 0, 1, 2 or 3.
    ///
    /// Sent to the frontend because the *boxes* have already been turned but the
    /// consumers' assumptions have not: grouping characters into lines by their
    /// vertical overlap is right for a page read left-to-right and gives one
    /// character per line on a page read top-to-bottom, which is what `/Rotate
    /// 90` produces. See `src/lib/text.ts`.
    pub quarter_turns: u8,
    /// Time spent inside PDFium extracting this, in milliseconds.
    pub extract_ms: f64,
}

impl PageText {
    /// Characters in this page.
    pub fn len(&self) -> usize {
        self.codes.len()
    }

    /// Whether the page has no extractable characters --- a scan, typically.
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }
}

/// Maps one character box from unrotated page space into displayed device space.
///
/// `page_box` is `[left, bottom, right, top]` with y upwards, as PDFium reports
/// it. `width_pt` and `height_pt` are the page's **displayed** size, so they are
/// already swapped for a quarter or three-quarter turn; the unrotated size is
/// recovered here rather than asked for twice.
///
/// Returns `[left, top, right, bottom]` with y downwards from the displayed
/// page's top-left corner.
///
/// Pure, and separate from [`extract`] for the reason every mapping in this
/// repository ends up separate: the failure mode is a plausible rectangle in the
/// wrong place, which no amount of looking at a render reliably catches, and a
/// function taking four numbers can be asserted against known corners.
pub fn to_device(turns: u8, width_pt: f32, height_pt: f32, page_box: [f64; 4]) -> [f32; 4] {
    let [left, bottom, right, top] = page_box.map(|value| value as f32);
    // The page's own size, before `/Rotate` was applied to get the displayed one.
    let (w0, h0) = match turns % 2 {
        0 => (width_pt, height_pt),
        _ => (height_pt, width_pt),
    };

    match turns % 4 {
        // No rotation: the flip alone. `top` is the larger y in page space and
        // becomes the smaller y here, which is why it is written first.
        0 => [left, h0 - top, right, h0 - bottom],
        // A quarter turn clockwise sends the page's top-left to the top-right,
        // so the page's y becomes the display's x and its x becomes the y.
        1 => [bottom, left, top, right],
        2 => [w0 - right, bottom, w0 - left, top],
        _ => [h0 - top, w0 - right, h0 - bottom, w0 - left],
    }
}

/// Turns a device-space box by `turns` quarter-turns clockwise.
///
/// The view rotation, as distinct from the page's own: [`to_device`] has already
/// placed the box on the page as the document says it is displayed, and this
/// turns that result again because a reader asked to look at it sideways.
///
/// `width`/`height` are the displayed page size **going in**; a quarter turn
/// swaps them coming out. This is the same operation the frontend performs on the
/// boxes it receives (`src/lib/text.ts`), and the reason it exists in Rust as
/// well is that the two can then be pinned to the same rule: composing this with
/// [`to_device`] must equal `to_device` of the summed turn, which is what
/// `composing_a_view_turn_equals_turning_the_page_further` asserts.
pub fn turn_device(turns: u8, width: f32, height: f32, quad: [f32; 4]) -> [f32; 4] {
    let [left, top, right, bottom] = quad;
    match turns % 4 {
        0 => quad,
        // Clockwise: the left edge becomes the top, and what was the bottom of
        // the box is now its left, measured back from the old height.
        1 => [height - bottom, left, height - top, right],
        2 => [width - right, height - bottom, width - left, height - top],
        _ => [top, width - right, bottom, width - left],
    }
}

/// Extracts a page's text and character geometry.
pub fn extract(page: &RawPage<'_>) -> Result<PageText, String> {
    let started = std::time::Instant::now();
    let height_pt = page.height_pt();
    let width_pt = page.width_pt();
    let turns = page.quarter_turns();

    let text = RawTextPage::load(page)?;
    let count = text.count();

    let mut codes = Vec::with_capacity(count as usize);
    let mut boxes = Vec::with_capacity(count as usize * 4);

    for index in 0..count {
        codes.push(text.code(index));
        match text.char_box(index) {
            Some(page_box) => {
                boxes.extend_from_slice(&to_device(turns, width_pt, height_pt, page_box));
            }
            None => boxes.extend_from_slice(&[0.0; 4]),
        }
    }

    Ok(PageText {
        codes,
        boxes,
        height_pt,
        width_pt,
        quarter_turns: turns,
        extract_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

#[cfg(test)]
mod tests {
    use super::{to_device, turn_device};

    /// An unrotated page, portrait.
    const W0: f32 = 600.0;
    const H0: f32 = 800.0;

    /// A small box in the **top-left** of the unrotated page.
    ///
    /// Asymmetric in both axes on purpose. A box in the middle, or one as wide
    /// as it is tall, maps to something plausible under all four turns and can
    /// distinguish none of them --- which is the whole hazard here, since a
    /// wrong turn still produces a tidy rectangle.
    const CORNER: [f64; 4] = [10.0, 700.0, 40.0, 780.0];

    /// The displayed page size for a given number of quarter turns.
    fn displayed(turns: u8) -> (f32, f32) {
        if turns % 2 == 0 {
            (W0, H0)
        } else {
            (H0, W0)
        }
    }

    fn map(turns: u8) -> [f32; 4] {
        let (width, height) = displayed(turns);
        to_device(turns, width, height, CORNER)
    }

    #[test]
    fn an_unrotated_box_keeps_its_x_and_flips_its_y() {
        // 700..780 up from the bottom of an 800 pt page is 20..100 down from its
        // top, and `top` becoming the smaller number is the flip.
        assert_eq!(map(0), [10.0, 20.0, 40.0, 100.0]);
    }

    #[test]
    fn each_turn_sends_the_top_left_corner_somewhere_different() {
        // The load-bearing test. A box in the page's top-left must appear in the
        // display's top-left, top-right, bottom-right and bottom-left in turn ---
        // and it is the only assertion here that can tell a quarter turn from a
        // three-quarter one, since both swap the page's dimensions identically.
        for turns in 0..4u8 {
            let [left, top, right, bottom] = map(turns);
            let (width, height) = displayed(turns);
            let near_left = left < width / 2.0;
            let near_top = top < height / 2.0;
            let expected = match turns {
                0 => (true, true),
                1 => (false, true),
                2 => (false, false),
                _ => (true, false),
            };
            assert_eq!(
                (near_left, near_top),
                expected,
                "turn {turns} put the page's top-left corner at \
                 [{left}, {top}, {right}, {bottom}] on a {width}x{height} page"
            );
        }
    }

    #[test]
    fn a_quarter_turn_swaps_the_box_proportions() {
        // 30 wide by 80 tall becomes 80 wide by 30 tall. Without this, a turn
        // that got the corner right but transposed the extents would pass the
        // check above.
        let [left, top, right, bottom] = map(1);
        assert_eq!(right - left, 80.0);
        assert_eq!(bottom - top, 30.0);
    }

    #[test]
    fn every_turn_keeps_the_box_inside_the_displayed_page() {
        for turns in 0..4u8 {
            let [left, top, right, bottom] = map(turns);
            let (width, height) = displayed(turns);
            assert!(
                left >= 0.0 && right <= width,
                "turn {turns} left the page in x"
            );
            assert!(
                top >= 0.0 && bottom <= height,
                "turn {turns} left the page in y"
            );
        }
    }

    #[test]
    fn a_box_is_never_returned_inside_out() {
        // `left <= right` and `top <= bottom` is what every consumer assumes ---
        // a highlight with a negative width simply does not draw, which reads as
        // "selection is broken" rather than as a coordinate bug.
        for turns in 0..4u8 {
            let [left, top, right, bottom] = map(turns);
            assert!(left <= right, "turn {turns} returned right of left");
            assert!(top <= bottom, "turn {turns} returned bottom above top");
        }
    }

    #[test]
    fn two_turns_are_a_point_reflection_of_none() {
        // 180 degrees is the one turn whose answer can be checked against turn 0
        // without redoing the arithmetic under test: it is the same box mirrored
        // in both axes.
        let none = map(0);
        let half = map(2);
        assert_eq!(half[0], W0 - none[2]);
        assert_eq!(half[2], W0 - none[0]);
        assert_eq!(half[1], H0 - none[3]);
        assert_eq!(half[3], H0 - none[1]);
    }

    #[test]
    fn a_rotation_beyond_three_quarter_turns_wraps() {
        // PDFium is documented to return 0..=3 and `quarter_turns` clamps, so
        // this is defence rather than a case that occurs --- but the modulo is
        // free and the alternative is a panic on a value nobody validated.
        assert_eq!(map(4), map(0));
        assert_eq!(map(5), map(1));
    }

    #[test]
    fn composing_a_view_turn_equals_turning_the_page_further() {
        // The load-bearing test for `turn_device`, and the reason it is worth
        // having two functions: this one is derived independently --- it turns a
        // box that is already in device space, knowing nothing about page space
        // or the flip --- so agreeing with `to_device` on every one of the
        // sixteen combinations is evidence rather than a tautology.
        //
        // It is also what pins the frontend, which performs exactly this turn on
        // the boxes it receives and cannot be reached from a Rust test.
        for page_turns in 0..4u8 {
            for view_turns in 0..4u8 {
                let (width, height) = displayed(page_turns);
                let composed = turn_device(
                    view_turns,
                    width,
                    height,
                    to_device(page_turns, width, height, CORNER),
                );

                let total = (page_turns + view_turns) % 4;
                let (total_width, total_height) = displayed(total);
                let direct = to_device(total, total_width, total_height, CORNER);

                assert_eq!(
                    composed,
                    direct,
                    "page /Rotate {} viewed at {} does not agree with /Rotate {}",
                    page_turns as u32 * 90,
                    view_turns as u32 * 90,
                    total as u32 * 90,
                );
            }
        }
    }

    #[test]
    fn a_view_turn_moves_the_box_it_is_given() {
        // The control for the test above, which a `turn_device` that returned its
        // argument unchanged would pass for `view_turns == 0` and fail loudly
        // elsewhere --- but a *wrong* composition could still be self-consistent.
        // This says the turn does something, in the direction claimed: a box in
        // the top-left goes to the top-right under one quarter turn clockwise.
        let upright = to_device(0, W0, H0, CORNER);
        let turned = turn_device(1, W0, H0, upright);
        assert!(turned[0] > H0 / 2.0, "a quarter turn did not move it right");
        assert!(turned[1] < W0 / 2.0, "a quarter turn did not keep it high");
        assert_eq!(turn_device(0, W0, H0, upright), upright);
    }
}
