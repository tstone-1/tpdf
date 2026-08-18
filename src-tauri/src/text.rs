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
use crate::structure;

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
    ///
    /// **Scalar, not UTF-16 code unit**, and that distinction is paid for rather
    /// than free: `FPDFText_GetUnicode` is a UTF-16 API, so PDFium reports a code
    /// point above the BMP as *two* characters --- a high surrogate and a low one,
    /// each with the same box. [`extract`] joins them back into one entry, which
    /// is what makes this field mean what the line above says it does.
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
    /// What the document's own tags say the reading order is, where it has them.
    ///
    /// Half-open ranges over `codes`, in the order the document means them to be
    /// read --- which is not ascending: a margin note drawn beside the first
    /// paragraph and tagged after the last one is exactly the case tagging exists
    /// for. Empty for an untagged page, and empty for a page whose walk was
    /// truncated, so **present means complete** (see
    /// [`crate::structure::PageStructure::complete_runs`]).
    ///
    /// Carried here rather than fetched by a request of its own because
    /// `PageText` already crosses the worker boundary and reaches every consumer,
    /// and because `reading.ts` needs the characters and the runs *together* to
    /// decide whether the tags cover the page. A second command would have put
    /// that decision at two call sites and left one of them to be forgotten.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<crate::structure::TaggedRun>,
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

/// Maps a display-space rectangle back into the page's own unrotated space.
///
/// The inverse of [`to_device`], and it exists because writing an annotation
/// runs the mapping the other way: a reader drags across glyphs whose boxes are
/// in display space, and `/QuadPoints` is written in the page's own space with y
/// upwards. Placing that quad by a second, hand-derived rotation table is how a
/// highlight ends up one quarter-turn away from the words it was made from ---
/// `docs/TRAPS.md` records two such tables disagreeing at every turn but zero.
///
/// Takes `[left, top, right, bottom]` with y downwards from the displayed page's
/// top-left corner, and returns `[left, bottom, right, top]` with y upwards, the
/// order [`to_device`] accepts. `width_pt` and `height_pt` are the **displayed**
/// size, exactly as [`to_device`] takes them.
///
/// **A proper rectangle in gives a proper rectangle out, at every turn, and
/// there is no normalisation step here saying so.** There was one, and a
/// mutation deleting it survived: each arm below already emits its corners in
/// ascending order whenever the display box has `left <= right` and
/// `top <= bottom`, so the `min`/`max` pass could not change a single value it
/// was ever given. Unreachable defence reads as load-bearing and quietly becomes
/// wrong, so what pins the property now is
/// `a_mapped_back_rectangle_is_proper` --- which fails the moment an arm emits
/// two corners the wrong way round, where the normalisation would have hidden
/// exactly that.
pub fn from_device(turns: u8, width_pt: f32, height_pt: f32, device: [f32; 4]) -> [f64; 4] {
    let [left, top, right, bottom] = device;
    // The page's own size, recovered the same way `to_device` recovers it.
    let (w0, h0) = match turns % 2 {
        0 => (width_pt, height_pt),
        _ => (height_pt, width_pt),
    };

    let page = match turns % 4 {
        0 => [left, h0 - bottom, right, h0 - top],
        1 => [top, left, bottom, right],
        2 => [w0 - right, top, w0 - left, bottom],
        _ => [w0 - bottom, h0 - right, w0 - top, h0 - left],
    };
    page.map(f64::from)
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

/// What PDFium reports at `index`, as a scalar, and how many indices it used.
///
/// `FPDFText_GetUnicode` is a UTF-16 API. A code point above the BMP therefore
/// arrives as two characters, and everything downstream treats a character index
/// as a code point: `char::from_u32` of a lone surrogate is `None`, so the fold
/// in `search.rs` drops both halves and an Extension B ideograph is unfindable.
/// Measured on `testdata/multilingual.pdf`, where U+20000 came back as U+D840
/// and U+DC00 with one box each --- and JavaScript reassembled them by accident,
/// because two adjacent lone surrogates concatenate into the right character
/// there. Two consumers of one array disagreeing about how many characters it
/// holds is the failure `text.rs` opens by saying this module exists to prevent.
///
/// An **unpaired** surrogate is a broken `/ToUnicode` CMap rather than an astral
/// character, and becomes U+FFFD: it is what every other decoder does, it keeps
/// one index per character, and it is visible. Dropping it would silently shorten
/// the page and shift every box after it.
/// Split from [`scalar_at`] so it can be tested without a document: the pairing
/// rule is arithmetic over two numbers, and a test that has to open a PDF to
/// reach it would be testing PDFium.
fn scalar_of(code: u32, next: Option<u32>) -> (u32, u32) {
    const REPLACEMENT: u32 = 0xFFFD;
    if !(0xD800..=0xDFFF).contains(&code) {
        return (code, 1);
    }
    if !(0xD800..0xDC00).contains(&code) {
        return (REPLACEMENT, 1);
    }
    match next {
        Some(low) if (0xDC00..0xE000).contains(&low) => {
            (0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00), 2)
        }
        _ => (REPLACEMENT, 1),
    }
}

fn scalar_at(text: &RawTextPage<'_>, index: u32, count: u32) -> (u32, u32) {
    let next = (index + 1 < count).then(|| text.code(index + 1));
    scalar_of(text.code(index), next)
}

/// Moves tagged runs from PDFium's character indices into ours.
///
/// `structure.rs` speaks to PDFium, so its ranges count a surrogate pair twice.
/// Translating them here keeps that module free of the distinction --- it indexes
/// what PDFium indexes --- and makes this the single place the two spaces meet.
///
/// **No fixture reaches this**, and that is why it is a function of its own rather
/// than four lines inside [`extract`]. It fires only on a page that is both tagged
/// *and* carries a character above the BMP: `tagged.pdf` has no astral character
/// and `multilingual.pdf` has no tags, so a mutation that switched the whole
/// translation off passed both probes. A tagged Japanese document with an
/// Extension B ideograph in a name is not exotic, so the answer is a unit test on
/// arithmetic rather than either a new corpus or a guard nothing watches.
///
/// `ours` is one longer than PDFium's character count, so an exclusive end index
/// has somewhere to land. An `end` that falls *between* the halves of a pair maps
/// to the pair's own index and the run comes back empty --- which is the honest
/// answer for a mark that claims half a character.
fn retarget(runs: &mut [crate::structure::TaggedRun], ours: &[u32], len: usize) {
    if ours.len() == len + 1 {
        // No pair anywhere, so the two spaces are the same one.
        return;
    }
    let at = |index: u32| ours.get(index as usize).copied().unwrap_or(len as u32);
    for run in runs.iter_mut() {
        run.start = at(run.start);
        run.end = at(run.end);
    }
}

/// Extracts a page's text and character geometry.
pub fn extract(page: &RawPage<'_>) -> Result<PageText, String> {
    let started = std::time::Instant::now();
    let height_pt = page.height_pt();
    let width_pt = page.width_pt();
    let turns = page.quarter_turns();
    // The page's own origin, which is (0, 0) for most documents and is not for
    // one with an inset `/CropBox`. PDFium reports the *cropped* size above and
    // answers `FPDFText_GetCharBox` in the page's own space, so on such a
    // document the two are different spaces --- see `RawPage::origin_pt`, where
    // the measurement is.
    let (origin_x, origin_y) = page.origin_pt();

    let text = RawTextPage::load(page)?;
    let count = text.count();

    let mut codes = Vec::with_capacity(count as usize);
    let mut boxes = Vec::with_capacity(count as usize * 4);
    // PDFium's character index to ours. One entry longer than the character
    // count, so an exclusive end index has somewhere to land.
    let mut ours = Vec::with_capacity(count as usize + 1);

    let mut index = 0;
    while index < count {
        ours.push(codes.len() as u32);
        let (scalar, units) = scalar_at(&text, index, count);
        if units == 2 {
            // Both halves map to the one entry they became, so a range that
            // starts or ends between them still lands on the whole character.
            ours.push(codes.len() as u32);
        }
        codes.push(scalar);

        let mut quad = text.char_box(index);
        if units == 2 {
            // The two halves carry identical boxes in every case measured, and a
            // union is still the right answer: one character occupies one area,
            // and taking only the first half's box would be trusting that they
            // agree.
            //
            // Which does mean **no fixture can distinguish the two**: a mutation
            // that drops the union passes `search-probe`, because the operands are
            // equal. Recorded rather than dressed up as tested --- it is one line
            // of defence against a PDFium that reports the halves differently, and
            // if that ever happens the box is right instead of arbitrary.
            quad = match (quad, text.char_box(index + 1)) {
                (Some(first), Some(second)) => Some([
                    first[0].min(second[0]),
                    first[1].min(second[1]),
                    first[2].max(second[2]),
                    first[3].max(second[3]),
                ]),
                (only, None) | (None, only) => only,
            };
        }
        match quad {
            Some(page_box) => {
                // Into crop space before the turn, because `to_device` works in
                // the displayed page's coordinates and the displayed page starts
                // at the crop box's corner.
                let shifted = [
                    page_box[0] - origin_x as f64,
                    page_box[1] - origin_y as f64,
                    page_box[2] - origin_x as f64,
                    page_box[3] - origin_y as f64,
                ];
                boxes.extend_from_slice(&to_device(turns, width_pt, height_pt, shifted));
            }
            None => boxes.extend_from_slice(&[0.0; 4]),
        }
        index += units;
    }
    ours.push(codes.len() as u32);

    // The tags, using the text page already loaded. An untagged document ---
    // which is most of them --- pays one FFI call for this, because
    // `structure::read_using` returns before anything per-character when the page
    // has no tree at all. A tagged one pays two calls per character, which is
    // what relating a mark to a character index costs and is why it is not done
    // on a page nobody asked about.
    let mut runs = structure::read_using(page, &text)?.complete_runs();
    retarget(&mut runs, &ours, codes.len());

    Ok(PageText {
        codes,
        boxes,
        height_pt,
        width_pt,
        quarter_turns: turns,
        extract_ms: started.elapsed().as_secs_f64() * 1000.0,
        runs,
    })
}

#[cfg(test)]
mod tests {
    use super::{from_device, retarget, scalar_of, to_device, turn_device};
    use crate::structure::TaggedRun;

    /// A run over PDFium's indices, which is what `structure.rs` returns.
    fn run(start: u32, end: u32) -> TaggedRun {
        TaggedRun {
            tag: "P".to_string(),
            path: vec!["P".to_string()],
            start,
            end,
        }
    }

    /// `ours` for `a` `b` <pair> `c`: four characters over five code units.
    ///
    /// Both halves of the pair map to entry 2, and the trailing entry is the
    /// exclusive end of the whole page.
    const PAIRED: [u32; 6] = [0, 1, 2, 2, 3, 4];

    #[test]
    fn a_page_with_no_pair_leaves_its_runs_alone() {
        // The control, and the common case: `ours` is the identity, so the
        // translation must be a no-op rather than an off-by-one.
        let identity: Vec<u32> = (0..=4).collect();
        let mut runs = vec![run(0, 2), run(2, 4)];
        retarget(&mut runs, &identity, 4);
        assert_eq!(
            runs.iter().map(|r| (r.start, r.end)).collect::<Vec<_>>(),
            vec![(0, 2), (2, 4)]
        );
    }

    #[test]
    fn a_run_after_a_pair_moves_back_by_the_units_it_saved() {
        // PDFium indices 3..5 are the low half and the `c`; ours are 2..4.
        let mut runs = vec![run(3, 5)];
        retarget(&mut runs, &PAIRED, 4);
        assert_eq!((runs[0].start, runs[0].end), (2, 4));
    }

    #[test]
    fn a_run_spanning_a_pair_still_covers_the_whole_character() {
        // 0..4 in PDFium's space is `a`, `b` and both halves; ours is 0..3.
        let mut runs = vec![run(0, 4)];
        retarget(&mut runs, &PAIRED, 4);
        assert_eq!((runs[0].start, runs[0].end), (0, 3));
    }

    #[test]
    fn a_run_ending_inside_a_pair_comes_back_empty() {
        // An end index on the low half claims half a character, which no producer
        // means. Empty is the honest answer; silently rounding it outwards would
        // hand a screen reader a character the tag did not cover.
        let mut runs = vec![run(2, 3)];
        retarget(&mut runs, &PAIRED, 4);
        assert_eq!((runs[0].start, runs[0].end), (2, 2));
    }

    #[test]
    fn an_index_past_the_end_lands_on_the_end() {
        // `structure.rs` closes an unterminated mark at the character count, so an
        // end equal to it is ordinary rather than a bug --- and anything beyond it
        // must still produce a range that can be sliced.
        let mut runs = vec![run(4, 99)];
        retarget(&mut runs, &PAIRED, 4);
        assert_eq!((runs[0].start, runs[0].end), (3, 4));
    }

    /// U+20000, as PDFium reports it: two UTF-16 code units.
    const HIGH: u32 = 0xD840;
    const LOW: u32 = 0xDC00;
    const ASTRAL: u32 = 0x20000;
    const REPLACEMENT: u32 = 0xFFFD;

    #[test]
    fn an_ordinary_code_point_is_itself_and_one_unit_wide() {
        assert_eq!(scalar_of(0x41, None), (0x41, 1));
        // A CJK ideograph inside the BMP: two *bytes* in the content stream and
        // one code unit here, which is the case that already worked and the
        // control for the ones below.
        assert_eq!(scalar_of(0x6587, Some(0x20)), (0x6587, 1));
    }

    #[test]
    fn a_surrogate_pair_becomes_one_scalar_over_two_units() {
        assert_eq!(scalar_of(HIGH, Some(LOW)), (ASTRAL, 2));
        // The top of the plane, so a mistake in the shift shows up as a wildly
        // wrong number rather than an off-by-one.
        assert_eq!(scalar_of(0xDBFF, Some(0xDFFF)), (0x10FFFF, 2));
    }

    #[test]
    fn a_high_surrogate_with_nothing_after_it_is_replaced() {
        // The last character on a page. Dropping it would shorten the page and
        // shift every box after it; keeping the raw surrogate would leave a
        // number `char::from_u32` refuses.
        assert_eq!(scalar_of(HIGH, None), (REPLACEMENT, 1));
    }

    #[test]
    fn a_high_surrogate_followed_by_anything_else_is_replaced() {
        assert_eq!(scalar_of(HIGH, Some(0x41)), (REPLACEMENT, 1));
        // Two highs in a row: the second is not a low, so the first cannot pair
        // with it. Consuming two units here would swallow a real character.
        assert_eq!(scalar_of(HIGH, Some(HIGH)), (REPLACEMENT, 1));
    }

    #[test]
    fn a_lone_low_surrogate_is_replaced_and_never_paired_backwards() {
        // A low surrogate first is not the second half of anything --- the pair is
        // consumed by the high that precedes it, so reaching one here means the
        // document is broken. It must still be one unit wide, or the character
        // after it disappears.
        assert_eq!(scalar_of(LOW, Some(0x41)), (REPLACEMENT, 1));
        assert_eq!(scalar_of(LOW, Some(LOW)), (REPLACEMENT, 1));
    }

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
    fn a_display_box_maps_back_to_the_page_box_it_came_from() {
        // The property that lets a highlight be written where the reader made
        // it: `from_device` is the inverse of `to_device` at every turn, not
        // only at zero, which is where a hand-derived second table agrees by
        // accident.
        for turns in 0..4u8 {
            let (width, height) = displayed(turns);
            let back = from_device(
                turns,
                width,
                height,
                to_device(turns, width, height, CORNER),
            );
            for (at, (got, want)) in back.iter().zip(CORNER.iter()).enumerate() {
                assert!(
                    (got - want).abs() < 1e-3,
                    "corner {at} at /Rotate {}: {got} is not {want}",
                    turns as u32 * 90,
                );
            }
        }
    }

    #[test]
    fn mapping_back_with_the_wrong_turn_moves_the_box() {
        // The control. The round trip above would pass for a `from_device` that
        // ignored `turns` and always undid the flip --- three of its four arms
        // would then be wrong and nothing would say so, because each is only
        // ever composed with its own partner. This asserts the arms differ: a
        // box mapped down at one turn and back at another does not come home.
        //
        // Skips the 180-degree pair on purpose rather than quietly passing it.
        // `CORNER` is 30 x 80 points on a 600 x 800 page, so a half turn about
        // the centre sends it somewhere else entirely --- but at turns 1 and 3
        // the *displayed* size is swapped, and undoing turn 1 with arm 3 on a
        // square page would be an identity. The page here is not square, which
        // is what makes every pair below discriminating.
        for turns in 0..4u8 {
            let (width, height) = displayed(turns);
            let down = to_device(turns, width, height, CORNER);
            for wrong in 0..4u8 {
                if wrong == turns {
                    continue;
                }
                let back = from_device(wrong, width, height, down);
                assert!(
                    back.iter()
                        .zip(CORNER.iter())
                        .any(|(got, want)| (got - want).abs() > 1.0),
                    "undoing /Rotate {} with the arm for {} came home anyway: {back:?}",
                    turns as u32 * 90,
                    wrong as u32 * 90,
                );
            }
        }
    }

    #[test]
    fn a_mapped_back_rectangle_is_proper() {
        // Half the turns produce their corners in an order that is not
        // `[left, bottom, right, top]`, so this is the normalisation being
        // asserted rather than the algebra. A `/QuadPoints` written from an
        // improper rectangle is one PDF 32000-1 tells every consumer to
        // normalise -- and a consumer that does not draws nothing at all.
        for turns in 0..4u8 {
            let (width, height) = displayed(turns);
            let back = from_device(
                turns,
                width,
                height,
                to_device(turns, width, height, CORNER),
            );
            assert!(
                back[0] < back[2],
                "/Rotate {}: left is not < right",
                turns as u32 * 90
            );
            assert!(
                back[1] < back[3],
                "/Rotate {}: bottom is not < top",
                turns as u32 * 90
            );
        }
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
