//! Consume bounded paths while preserving their paint and rectangular clipping.
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
    let mut next = rectangle(rect, ctm)?;
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

// ISO 32000-1, 8.5.3: painting ends the current path. With no W/W*,
// this complete sequence changes pixels but cannot change a later text clip.
// https://pdf-issues.pdfa.org/32000-2-2020/clause08.html#853-path-painting-operators
pub(super) fn painted(ops: &[Operation], ctm: [f64; 6]) -> Result<Option<usize>, String> {
    let count = ops.iter().take_while(|op| op.operator == "re").count();
    let Some(end) = ops.get(count).filter(|_| count > 0) else {
        return Ok(None);
    };
    if !matches!(
        end.operator.as_str(),
        "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n"
    ) {
        return Ok(None);
    }
    if !end.operands.is_empty() {
        return Err("invalid rectangle paint operands".into());
    }
    for rect in &ops[..count] {
        rectangle_bounds(rect, ctm, false)?;
    }
    Ok(Some(count + 1))
}

// Complete line/Bezier subpaths, finished before any state or text operator.
// Painting without W/W* cannot alter a later text clip. Keep every operand;
// transformed control points bound each cubic's convex hull without flattening.
// MAX_OPERATIONS bounds the entire stream, and the caller skips consumed ops.
pub(super) fn path(ops: &[Operation], ctm: [f64; 6]) -> Result<usize, String> {
    let invalid = || "only complete bounded painted paths are editable".to_string();
    let mut segments = 0;
    let mut closed = false;
    for (index, op) in ops.iter().enumerate() {
        let coordinates = match op.operator.as_str() {
            "m" if index == 0 || segments > 0 => {
                segments = 0;
                closed = false;
                2
            }
            "l" | "c" | "v" | "y" if index > 0 && !closed => {
                segments += 1;
                match op.operator.as_str() {
                    "l" => 2,
                    "c" => 6,
                    _ => 4,
                }
            }
            "h" if segments > 0 && !closed && op.operands.is_empty() => {
                closed = true;
                continue;
            }
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n"
                if segments > 0 && op.operands.is_empty() =>
            {
                return Ok(index + 1);
            }
            _ => return Err(invalid()),
        };
        if op.operands.len() != coordinates {
            return Err(invalid());
        }
        for pair in op.operands.chunks_exact(2) {
            let x = super::number(&pair[0])?;
            let y = super::number(&pair[1])?;
            let point = [x * ctm[0] + ctm[4], y * ctm[3] + ctm[5]];
            if point
                .iter()
                .any(|value| !value.is_finite() || value.abs() > 1_000_000.)
            {
                return Err("path coordinates exceed their limit".into());
            }
        }
    }
    Err(invalid())
}

fn rectangle(rect: &Operation, ctm: [f64; 6]) -> Result<Rect, String> {
    rectangle_bounds(rect, ctm, true)
}

fn rectangle_bounds(rect: &Operation, ctm: [f64; 6], clipping: bool) -> Result<Rect, String> {
    if rect.operator != "re" || rect.operands.len() != 4 {
        return Err("invalid rectangle operands".into());
    }
    let mut values = [0.; 4];
    for (dest, value) in values.iter_mut().zip(&rect.operands) {
        *dest = super::number(value)?;
    }
    let [x, y, width, height] = values;
    // A reversed painted rectangle retains its winding and all authored
    // operators. Clipping keeps its independently validated positive subset.
    if width == 0. || height == 0. || (clipping && (width < 0. || height < 0.)) {
        return Err("empty or reversed rectangle is not editable".into());
    }
    let next = [
        x * ctm[0] + ctm[4],
        y * ctm[3] + ctm[5],
        (x + width) * ctm[0] + ctm[4],
        (y + height) * ctm[3] + ctm[5],
    ];
    if next.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.) {
        return Err("rectangle coordinates exceed their limit".into());
    }
    // A reflected CTM reverses corners, not the rectangle's interior. Normalize
    // in page space before intersecting with the already established clip.
    Ok([
        next[0].min(next[2]),
        next[1].min(next[3]),
        next[0].max(next[2]),
        next[1].max(next[3]),
    ])
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
    // Tr is fixed at 0, so line width cannot affect the edited glyphs.
    // Supported strokes retain this authored setting unchanged.
    if super::number(value)? < 0. {
        return Err("negative line width is not editable".into());
    }
    Ok(())
}
