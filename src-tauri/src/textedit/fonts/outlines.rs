//! Bounds from transformed outline points, including curve controls. Their convex
//! hull contains the curves. Keep fractional composite-glyph coordinates: the
//! parser's public integer Rect truncates them, which can underestimate ink.

use ttf_parser::{Face, GlyphId, OutlineBuilder};

struct Bounds {
    rect: Option<[f64; 4]>,
    finite: bool,
}

impl Bounds {
    fn point(&mut self, x: f32, y: f32) {
        self.finite &= x.is_finite() && y.is_finite();
        let (x, y) = (f64::from(x), f64::from(y));
        self.rect = Some(match self.rect {
            Some([left, bottom, right, top]) => {
                [left.min(x), bottom.min(y), right.max(x), top.max(y)]
            }
            None => [x, y, x, y],
        });
    }
}

impl OutlineBuilder for Bounds {
    fn move_to(&mut self, x: f32, y: f32) {
        self.point(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.point(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.point(x1, y1);
        self.point(x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.point(x1, y1);
        self.point(x2, y2);
        self.point(x, y);
    }
    fn close(&mut self) {}
}

pub(super) fn bounds(face: &Face<'_>, glyph: GlyphId) -> Option<[f64; 4]> {
    let mut bounds = Bounds {
        rect: None,
        finite: true,
    };
    face.outline_glyph(glyph, &mut bounds)?;
    bounds.finite.then_some(bounds.rect).flatten()
}

// A width-only CFF parse can stop before a malformed suffix. Only a complete
// outline parse ending in ZeroBBox with no points proves a blank space.
pub(super) fn cff_bounds(
    face: &ttf_parser::cff::Table<'_>,
    glyph: GlyphId,
) -> Result<Option<[f64; 4]>, String> {
    let mut bounds = Bounds {
        rect: None,
        finite: true,
    };
    match face.outline(glyph, &mut bounds) {
        Ok(_) if bounds.finite => Ok(bounds.rect),
        Err(ttf_parser::CFFError::ZeroBBox) if bounds.finite && bounds.rect.is_none() => Ok(None),
        _ => Err("invalid CFF glyph outline".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textedit_outline_envelope_includes_curve_controls_and_rejects_nonfinite_points() {
        let mut bounds = Bounds {
            rect: None,
            finite: true,
        };
        bounds.move_to(0., 0.);
        bounds.quad_to(-10.25, 900.5, 400., 0.);
        bounds.curve_to(420.5, -50.25, 200., 950.75, 0., 0.);
        bounds.close();
        assert_eq!(bounds.rect, Some([-10.25, -50.25, 420.5, 950.75]));
        assert!(bounds.finite);
        bounds.line_to(f32::NAN, 0.);
        assert!(!bounds.finite);
    }
}
