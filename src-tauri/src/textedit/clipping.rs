//! Preserve a rectangle-only clipping path without editing partly hidden text.
//! Coordinates are in original page space, fixed when the path is constructed.

use lopdf::{content::Operation, Object};

#[cfg(test)]
mod tests;

pub(super) type Rect = [f64; 4];

// PDF 1.6, 4.4.3: W/W* takes effect at the path-ending operator, and clips
// intersect. Consume only the complete `re W n` / `re W* n` sequence. A single
// rectangle has the same interior under either winding rule; n paints nothing.
pub(super) fn apply(
    previous: Option<Rect>,
    ops: &[Operation],
    ctm: [f64; 6],
) -> Result<Rect, String> {
    let invalid = || "only complete unpainted rectangular clips are editable".to_string();
    let [rect, rule, end, ..] = ops else {
        return Err(invalid());
    };
    if rect.operator != "re"
        || rect.operands.len() != 4
        || !matches!(rule.operator.as_str(), "W" | "W*")
        || !rule.operands.is_empty()
        || end.operator != "n"
        || !end.operands.is_empty()
    {
        return Err(invalid());
    }
    let mut values = [0.; 4];
    for (dest, value) in values.iter_mut().zip(&rect.operands) {
        *dest = super::number(value)?;
    }
    let [x, y, width, height] = values;
    if width <= 0. || height <= 0. {
        return Err("empty or reversed text clip is not editable".into());
    }
    let mut next = [
        x * ctm[0] + ctm[4],
        y * ctm[3] + ctm[5],
        (x + width) * ctm[0] + ctm[4],
        (y + height) * ctm[3] + ctm[5],
    ];
    if next.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.) {
        return Err("text clipping coordinates exceed their limit".into());
    }
    // A reflected CTM reverses corners, not the rectangle's interior. Normalize
    // in page space before intersecting with the already established clip.
    next = [
        next[0].min(next[2]),
        next[1].min(next[3]),
        next[0].max(next[2]),
        next[1].max(next[3]),
    ];
    if let Some(old) = previous {
        next = [
            old[0].max(next[0]),
            old[1].max(next[1]),
            old[2].min(next[2]),
            old[3].min(next[3]),
        ];
    }
    if next[0] >= next[2] || next[1] >= next[3] {
        return Err("empty text clipping intersection".into());
    }
    Ok(next)
}

pub(super) fn contains(clip: Option<Rect>, text: Rect) -> Result<(), String> {
    if let Some(clip) = clip {
        if text[0] < clip[0] || text[1] < clip[1] || text[2] > clip[2] || text[3] > clip[3] {
            return Err("partly clipped text is not editable yet".into());
        }
    }
    Ok(())
}

pub(super) fn line_width(value: &Object) -> Result<(), String> {
    // This setting cannot affect filled text: Tr is fixed at 0 and all stroking
    // operators remain refused. Bound and preserve it without using its value.
    if super::number(value)? < 0. {
        return Err("negative line width is not editable".into());
    }
    Ok(())
}
