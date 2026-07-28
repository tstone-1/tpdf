//! Inverting a page's lightness without inverting its colours.
//!
//! Reading a white page at night is the complaint dark mode exists to answer,
//! and the obvious answer --- `255 - c` per channel --- also rotates every hue
//! half a turn. Blue headings come out yellow, a red stamp comes out cyan, and
//! a document that was merely bright becomes a document that is wrong.
//!
//! What is wanted is HSL's **lightness** inverted, `L -> 1 - L`, with hue and
//! saturation left alone. That normally means a round trip through HSL, and it
//! does not have to. Writing `M` and `m` for the largest and smallest channel:
//!
//! * `L = (M + m) / 2`, so inverting it asks for `M' + m' = 2 - (M + m)`.
//! * HSL saturation divides chroma by `1 - |2L - 1|`, and `|2(1-L) - 1|` is the
//!   same number --- so holding saturation fixed holds **chroma** fixed, and
//!   `M' - m' = M - m`.
//!
//! Two equations with the differences equal: every channel moves by the same
//! amount. The whole transform is one offset per pixel,
//!
//! ```text
//! d = 255 - M - m,   c' = c + d
//! ```
//!
//! which has three properties worth having. It needs no floating point and no
//! clamp --- `M' = 255 - m` and `m' = 255 - M` are in range by construction, so
//! nothing can saturate and quietly lose a colour. It is an **exact
//! involution**: applying it twice returns the original bytes, because the
//! second offset is the negation of the first. And on a neutral pixel it
//! reduces to `255 - c`, so ordinary black text on ordinary white paper behaves
//! exactly as anyone would expect.
//!
//! ## What it does not do
//!
//! It inverts photographs, and there is no version of this that does not. A
//! photograph's lightness is its content, so a face comes out as a negative with
//! the right hues --- less alarming than a full inversion, still not the
//! picture. Every reader that offers this has the same limitation; the honest
//! answer is that the mode is off by default and asked for explicitly, never
//! that the inversion is clever enough to be safe.
//!
//! Excluding image regions is possible --- PDFium can report each page object's
//! type and bounds --- and is not done here, because nothing has measured what
//! enumerating objects per tile costs on a page with two hundred thousand of
//! them.

/// Inverts lightness in place over an RGBA buffer, leaving alpha alone.
///
/// Any trailing bytes that do not form a whole pixel are left untouched rather
/// than being an error: this runs on a buffer Pdfium sized itself, so a partial
/// pixel means the caller's arithmetic is wrong and refusing here would report
/// it in the one place that cannot say anything useful about why.
pub fn invert_lightness(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let (r, g, b) = (pixel[0], pixel[1], pixel[2]);
        let high = r.max(g).max(b);
        let low = r.min(g).min(b);
        // Both ends stay in range: the largest channel becomes `255 - low` and
        // the smallest becomes `255 - high`. `i16` only because the offset is
        // negative on a light pixel.
        let offset = 255i16 - high as i16 - low as i16;
        pixel[0] = (r as i16 + offset) as u8;
        pixel[1] = (g as i16 + offset) as u8;
        pixel[2] = (b as i16 + offset) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::invert_lightness;

    /// One pixel through the transform, as a triple.
    fn one(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
        let mut buf = [r, g, b, 255];
        invert_lightness(&mut buf);
        (buf[0], buf[1], buf[2])
    }

    #[test]
    fn paper_becomes_ink_and_ink_becomes_paper() {
        assert_eq!(one(255, 255, 255), (0, 0, 0));
        assert_eq!(one(0, 0, 0), (255, 255, 255));
    }

    #[test]
    fn a_grey_inverts_exactly() {
        // The neutral case reduces to `255 - c`, which is what makes ordinary
        // text and paper behave the way anyone would predict.
        for value in [1u8, 17, 64, 128, 200, 254] {
            let (r, g, b) = one(value, value, value);
            assert_eq!((r, g, b), (255 - value, 255 - value, 255 - value));
        }
    }

    #[test]
    fn a_fully_saturated_hue_is_left_alone() {
        // Lightness 0.5 already, so inverting it changes nothing --- and this is
        // the case a plain `255 - c` gets wrong, turning red into cyan.
        assert_eq!(one(255, 0, 0), (255, 0, 0));
        assert_eq!(one(0, 255, 0), (0, 255, 0));
        assert_eq!(one(0, 0, 255), (0, 0, 255));
        assert_eq!(one(255, 255, 0), (255, 255, 0));
    }

    #[test]
    fn a_pale_colour_becomes_a_dark_one_of_the_same_hue() {
        // Pale blue paper-and-ink is the ordinary case in a document: it has to
        // come back dark blue, not orange.
        let (r, g, b) = one(200, 200, 255);
        assert_eq!((r, g, b), (0, 0, 55));
        // Hue is the ordering and spacing of the channels, and both survive.
        assert_eq!(b as i16 - r as i16, 255 - 200);
        assert_eq!(r, g);
    }

    #[test]
    fn applying_it_twice_returns_the_original_bytes() {
        // Exact, not approximate: the second offset is the negation of the
        // first. It is what lets a mode toggle be reasoned about at all.
        let mut buf: Vec<u8> = (0..=255u8)
            .flat_map(|v| [v, v.wrapping_mul(7), 255 - v, 255])
            .collect();
        let original = buf.clone();
        invert_lightness(&mut buf);
        assert_ne!(buf, original, "a buffer this varied must actually change");
        invert_lightness(&mut buf);
        assert_eq!(buf, original);
    }

    #[test]
    fn alpha_is_never_touched() {
        let mut buf = [10u8, 20, 30, 7, 200, 100, 50, 0];
        invert_lightness(&mut buf);
        assert_eq!(buf[3], 7);
        assert_eq!(buf[7], 0);
    }

    #[test]
    fn nothing_ever_saturates() {
        // The claim in the module doc that no clamp is needed, made checkable:
        // every channel lands inside `[255 - high, 255 - low]`, so `as u8`
        // cannot be wrapping a negative or a value above 255. Stepping by 17
        // covers all 16 values per channel, so this is exhaustive over the grid.
        for r in (0..=255u8).step_by(17) {
            for g in (0..=255u8).step_by(17) {
                for b in (0..=255u8).step_by(17) {
                    let high = r.max(g).max(b);
                    let low = r.min(g).min(b);
                    let (nr, ng, nb) = one(r, g, b);
                    for channel in [nr, ng, nb] {
                        assert!(
                            channel >= 255 - high && channel <= 255 - low,
                            "{r},{g},{b} -> {nr},{ng},{nb}"
                        );
                    }
                    // Chroma is the invariant the derivation rests on.
                    assert_eq!(
                        nr.max(ng).max(nb) - nr.min(ng).min(nb),
                        high - low,
                        "chroma changed at {r},{g},{b}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_partial_pixel_at_the_end_is_left_alone() {
        let mut buf = [255u8, 255, 255, 255, 9, 9];
        invert_lightness(&mut buf);
        assert_eq!(buf, [0, 0, 0, 255, 9, 9]);
    }
}
