//! The box a page's ink actually occupies, for cropping the margins away.
//!
//! ## Why pixels rather than the object graph
//!
//! PDFium can hand back every page object's bounding box, and taking their union
//! is the obvious implementation. It is also useless for the document a reader
//! most wants this on: **a scan is one image object covering the whole sheet**,
//! so the object union is the sheet and the answer is "no margins to remove" for
//! precisely the file with the widest margins in the corpus.
//!
//! A low-resolution render answers for both kinds. It costs one render call ---
//! the fixed ~1 s floor on a dense A0 page (`docs/PLAN.md` §4), and milliseconds
//! on ordinary text --- which is affordable for a command a reader invokes and
//! would not be on a scroll.
//!
//! ## What "ink" means here, and the two ways a pixel is not it
//!
//! The renderer fills the *page* rectangle white and leaves anything outside it
//! as it found it, which for a fresh buffer is transparent black
//! (`progressive::render`). So a pixel is background when it is white **or**
//! when it is transparent, and a scan that tested colour alone would report the
//! overhang beyond a page's right edge as ink and never crop anything.
//!
//! The channel order does not matter to this and is worth saying out loud, since
//! `docs/TRAPS.md` records a wash that read as zero everywhere because a reader
//! assumed BGRA: "is this pixel white" is symmetric in R and B, so the question
//! this module asks is one of the few that either order answers the same way.
//!
//! ## The margin is deliberate and is not tuning
//!
//! The scan is a lower bound on where ink is: at the target width a pixel is
//! roughly 1.5 points on a letter page, and antialiasing puts the faintest edge
//! of a glyph below any threshold. Cropping to exactly the measured box shaves
//! descenders. [`MARGIN_PT`] puts back more than a pixel's worth, so the result
//! errs towards keeping a hair of white rather than towards clipping type.

use crate::progressive::{self, Bindings, CancelToken, RawPage, TileSpec};
use crate::text::from_device;

/// How wide the page is rendered when looking for its ink, in pixels.
///
/// Small on purpose: this is a bounding box, not a picture. 400 px across a
/// letter page is ~1.5 points per pixel, which is finer than the margin below
/// and far cheaper than a legible render --- and on an A0 sheet it is the
/// difference between a scan that costs the render floor once and one that
/// costs it several times over.
const TARGET_PX: f32 = 400.0;

/// How much white to leave outside the measured ink, in points.
///
/// Two points, which is more than one pixel of the scan on any page size this
/// bounds. See the module note: the scan under-reports, so the correction is
/// one-directional and this is not a number to tune by eye.
pub const MARGIN_PT: f64 = 2.0;

/// How far a channel may sit below full before a pixel counts as ink.
///
/// 240 rather than 255, because antialiasing against white produces a fringe of
/// near-white pixels around every glyph and a strict test would find ink one
/// pixel outside the ink. Anything a reader would call blank is above this.
const WHITE: u8 = 240;

/// The smallest rectangle in device pixels containing every inked pixel.
///
/// `[left, top, right, bottom]`, right and bottom **exclusive**, or `None` when
/// the page is blank. Pure, and separate from the render for the reason every
/// mapping in this repository is separate: it can then be asserted against a
/// buffer whose ink is at known coordinates, which no render can be.
pub fn ink_bounds(pixels: &[u8], width: u16, height: u16) -> Option<[u16; 4]> {
    let (width, height) = (width as usize, height as usize);
    if pixels.len() < width * height * 4 {
        return None;
    }
    let (mut left, mut top) = (width, height);
    let (mut right, mut bottom) = (0usize, 0usize);
    for y in 0..height {
        for x in 0..width {
            let at = (y * width + x) * 4;
            let px = &pixels[at..at + 4];
            // Transparent is the paper beyond the page's own rectangle, which
            // the renderer never touched. White is the paper itself.
            if px[3] == 0 || (px[0] >= WHITE && px[1] >= WHITE && px[2] >= WHITE) {
                continue;
            }
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    if right == 0 || bottom == 0 {
        return None;
    }
    Some([left as u16, top as u16, right as u16, bottom as u16])
}

/// The page's content box in the page's own space, or `None` if it is blank.
///
/// `[llx, lly, urx, ury]` with y upwards, which is what `/CropBox` and
/// [`crate::docmodel::Rect`] both use. Measured on the page **as it is loaded**,
/// so a page already carrying a crop is measured inside it --- cropping twice
/// tightens rather than reinterpreting the original sheet, which is what a
/// reader pressing the command a second time means.
///
/// The result is clamped to the page it was measured on, so the margin cannot
/// push the box outside the sheet on a page whose ink runs to the edge.
///
/// # Errors
///
/// A render that fails. A render the caller cancelled comes back as `Ok(None)`,
/// which reads as "no ink found" and is the safe answer: the caller crops
/// nothing rather than cropping to a partial page.
pub fn content_box(
    bindings: Bindings,
    page: &RawPage<'_>,
    cancel: &CancelToken,
) -> Result<Option<[f64; 4]>, String> {
    let width_pt = page.width_pt();
    let height_pt = page.height_pt();
    if !(width_pt > 0.0 && height_pt > 0.0) {
        return Ok(None);
    }
    let scale = TARGET_PX / width_pt;
    let width = TARGET_PX.round().max(1.0) as u16;
    let height = (height_pt * scale).round().max(1.0) as u16;

    // `turns: 0`, which is the page as its own `/Rotate` displays it and is the
    // space `width_pt` and `height_pt` are already in. Deliberately not the
    // reader's view rotation: the answer is a property of the page, and a crop
    // that came out different depending on which way the window was turned is a
    // crop nobody could reproduce.
    let spec = TileSpec {
        scale,
        turns: 0,
        x: 0,
        y: 0,
        width,
        height,
    };
    let (pixels, _) = progressive::render_tile(bindings, page, spec, None, cancel)?;
    let Some([left, top, right, bottom]) = ink_bounds(&pixels, width, height) else {
        return Ok(None);
    };

    // Device pixels to displayed points, then out of display space through the
    // one implementation that knows the turn --- `text::from_device`. A second
    // rotation table here is the trap `docs/TRAPS.md` records as two tables
    // disagreeing at every turn but zero.
    let to_pt = scale.recip();
    let margin = MARGIN_PT as f32;
    let display = [
        f32::from(left) * to_pt - margin,
        f32::from(top) * to_pt - margin,
        f32::from(right) * to_pt + margin,
        f32::from(bottom) * to_pt + margin,
    ];
    let page_box = from_device(page.quarter_turns(), width_pt, height_pt, display);

    // Back into the file's own space, and clipped to the page. `origin_pt` is
    // the loaded page's lower-left, so this composes with an existing crop
    // rather than assuming the sheet starts at zero.
    let (origin_x, origin_y) = page.origin_pt();
    let crop = page.crop_pt();
    let found = [
        (page_box[0] + f64::from(origin_x)).max(f64::from(crop[0])),
        (page_box[1] + f64::from(origin_y)).max(f64::from(crop[1])),
        (page_box[2] + f64::from(origin_x)).min(f64::from(crop[2])),
        (page_box[3] + f64::from(origin_y)).min(f64::from(crop[3])),
    ];
    // A box the clamp collapsed is not a crop. It cannot happen from a real
    // measurement --- the ink was inside the page --- and it is what a
    // pathological page size would produce, where refusing beats writing a
    // degenerate crop box the model would refuse one layer later anyway.
    if found[2] <= found[0] || found[3] <= found[1] {
        return Ok(None);
    }
    Ok(Some(found))
}

#[cfg(test)]
mod tests {
    use super::{ink_bounds, MARGIN_PT};

    /// A buffer with one inked pixel at `(x, y)` and paper everywhere else.
    fn one_pixel(width: u16, height: u16, x: u16, y: u16) -> Vec<u8> {
        let mut pixels = vec![255u8; width as usize * height as usize * 4];
        let at = (y as usize * width as usize + x as usize) * 4;
        pixels[at..at + 3].copy_from_slice(&[0, 0, 0]);
        pixels
    }

    #[test]
    fn one_inked_pixel_bounds_itself_and_nothing_else() {
        // Right and bottom are exclusive, which is what makes a one-pixel box
        // one pixel wide rather than zero. A half-open convention read as closed
        // shrinks every crop by a pixel on two sides, which on a 400-wide scan
        // is three points of ink.
        assert_eq!(
            ink_bounds(&one_pixel(10, 10, 3, 4), 10, 10),
            Some([3, 4, 4, 5])
        );
    }

    #[test]
    fn paper_alone_has_no_bounds() {
        // The control for every assertion here: a blank page must answer `None`
        // rather than a rectangle. Without it a scan that always returned the
        // whole buffer would satisfy the test above.
        let paper = vec![255u8; 10 * 10 * 4];
        assert_eq!(ink_bounds(&paper, 10, 10), None);
    }

    #[test]
    fn transparent_pixels_are_paper_too() {
        // The renderer fills the *page* rectangle white and leaves the overhang
        // beyond it as it found the buffer, which is transparent black. A scan
        // testing colour alone reads that overhang as ink --- black, after all ---
        // and every page comes back uncroppable.
        let mut pixels = vec![255u8; 10 * 10 * 4];
        for x in 8..10 {
            for y in 0..10 {
                let at = (y * 10 + x) * 4;
                pixels[at..at + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        assert_eq!(ink_bounds(&pixels, 10, 10), None);
    }

    #[test]
    fn a_near_white_fringe_is_paper_and_a_grey_is_not() {
        // Antialiasing puts a fringe of near-white pixels around every glyph, so
        // a strict test finds ink one pixel outside the ink. The pair is the
        // check: 250 is paper and 200 is not, and a threshold at either extreme
        // fails one of them.
        let mut pixels = vec![255u8; 4 * 4 * 4];
        pixels[0..3].copy_from_slice(&[250, 250, 250]);
        assert_eq!(ink_bounds(&pixels, 4, 4), None);
        pixels[0..3].copy_from_slice(&[200, 200, 200]);
        assert_eq!(ink_bounds(&pixels, 4, 4), Some([0, 0, 1, 1]));
    }

    #[test]
    fn a_single_channel_of_colour_is_ink() {
        // Red text on white paper: two channels are full and one is not. A test
        // requiring all three to be below the threshold would read a page of
        // coloured type as blank.
        let mut pixels = vec![255u8; 4 * 4 * 4];
        pixels[0..3].copy_from_slice(&[255, 255, 0]);
        assert_eq!(ink_bounds(&pixels, 4, 4), Some([0, 0, 1, 1]));
    }

    #[test]
    fn ink_at_opposite_corners_bounds_both() {
        // Two pixels, and the box has to reach both. A scan that stopped at the
        // first would pass every test above.
        let mut pixels = one_pixel(10, 10, 1, 2);
        let at = (7 * 10 + 8) * 4;
        pixels[at..at + 3].copy_from_slice(&[0, 0, 0]);
        assert_eq!(ink_bounds(&pixels, 10, 10), Some([1, 2, 9, 8]));
    }

    #[test]
    fn a_buffer_smaller_than_it_claims_is_refused() {
        // Not a hypothetical: the size comes from a render that can be cut
        // short. Reading past the end would be a panic in the process holding
        // the reader's document.
        assert_eq!(ink_bounds(&[255u8; 8], 10, 10), None);
    }

    #[test]
    fn the_margin_is_larger_than_a_pixel_of_the_scan() {
        // The scan under-reports --- see the module note --- so the correction
        // has to exceed the resolution it corrects. At 400 px across a 612 pt
        // page a pixel is 1.53 pt; the margin must be more than that or a
        // descender is still shaved.
        let pixel_pt = 612.0 / super::TARGET_PX as f64;
        assert!(
            MARGIN_PT > pixel_pt,
            "margin {MARGIN_PT} is not more than a pixel's {pixel_pt}"
        );
    }
}
