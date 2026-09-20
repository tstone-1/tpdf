//! Consume bounded paths while preserving their paint and rectangular clipping.
//! Coordinates are in original page space, fixed when the path is constructed.

use lopdf::{content::Operation, Object};

#[cfg(test)]
mod tests;

pub(super) type Rect = [f64; 4];

#[derive(Clone)]
pub(super) struct Region {
    rectangles: Vec<(Rect, i32)>,
    even_odd: bool,
}

impl Region {
    // Partition the candidate envelope at every clip edge. Testing every cell
    // catches holes and disjoint interiors that a corner-only test would miss.
    pub(super) fn contains(&self, text: Rect) -> Result<(), String> {
        let mut xs = vec![text[0], text[2]];
        let mut ys = vec![text[1], text[3]];
        for (rect, _) in &self.rectangles {
            xs.extend(
                [rect[0], rect[2]]
                    .into_iter()
                    .filter(|x| *x > text[0] && *x < text[2]),
            );
            ys.extend(
                [rect[1], rect[3]]
                    .into_iter()
                    .filter(|y| *y > text[1] && *y < text[3]),
            );
        }
        xs.sort_by(f64::total_cmp);
        ys.sort_by(f64::total_cmp);
        for x in xs.windows(2).filter(|p| p[0] < p[1]) {
            for y in ys.windows(2).filter(|p| p[0] < p[1]) {
                let px = (x[0] + x[1]) / 2.;
                let py = (y[0] + y[1]) / 2.;
                let winding: i32 = self
                    .rectangles
                    .iter()
                    .filter(|(r, _)| px > r[0] && px < r[2] && py > r[1] && py < r[3])
                    .map(|(_, direction)| direction)
                    .sum();
                if winding == 0 || (self.even_odd && winding % 2 == 0) {
                    return Err("partly clipped text is not editable yet".into());
                }
            }
        }
        Ok(())
    }
}

// Rectangular subpaths emitted as m/l/h rather than re. Preserve the original
// winding: nested rectangles can describe a hole, not a larger bounding box.
pub(super) fn compound(
    ops: &[Operation],
    ctm: [f64; 6],
) -> Result<Option<(usize, Region)>, String> {
    let mut rectangles = Vec::new();
    let mut index = 0;
    while ops.get(index).is_some_and(|op| op.operator == "m") {
        let mut points = Vec::new();
        for expected in ["m", "l", "l", "l"] {
            let Some(op) = ops
                .get(index)
                .filter(|op| op.operator == expected && op.operands.len() == 2)
            else {
                return Ok(None);
            };
            points.push([
                super::number(&op.operands[0])?,
                super::number(&op.operands[1])?,
            ]);
            index += 1;
        }
        if let Some(op) = ops
            .get(index)
            .filter(|op| op.operator == "l" && op.operands.len() == 2)
        {
            if [
                super::number(&op.operands[0])?,
                super::number(&op.operands[1])?,
            ] != points[0]
            {
                return Ok(None);
            }
            index += 1;
        }
        if !ops
            .get(index)
            .is_some_and(|op| op.operator == "h" && op.operands.is_empty())
        {
            return Ok(None);
        }
        index += 1;
        let [a, b, c, d] = points.as_slice() else {
            unreachable!()
        };
        if !((a[1] == b[1] && b[0] == c[0] && c[1] == d[1] && d[0] == a[0])
            || (a[0] == b[0] && b[1] == c[1] && c[0] == d[0] && d[1] == a[1]))
        {
            return Ok(None);
        }
        // Keep computed differences in f64, like the existing rectangle path.
        let xs = [a[0] * ctm[0] + ctm[4], c[0] * ctm[0] + ctm[4]];
        let ys = [a[1] * ctm[3] + ctm[5], c[1] * ctm[3] + ctm[5]];
        let exact = [
            xs[0].min(xs[1]),
            ys[0].min(ys[1]),
            xs[0].max(xs[1]),
            ys[0].max(ys[1]),
        ];
        if exact.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.)
            || exact[0] >= exact[2]
            || exact[1] >= exact[3]
        {
            return Err("compound clip coordinates exceed their limit".into());
        }
        let direction = if (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]) > 0. {
            1
        } else {
            -1
        };
        rectangles.push((exact, direction));
        if rectangles.len() > 32 {
            return Err("too many compound clip rectangles".into());
        }
    }
    let Some(rule) = ops
        .get(index)
        .filter(|op| matches!(op.operator.as_str(), "W" | "W*") && op.operands.is_empty())
    else {
        return Ok(None);
    };
    if !ops
        .get(index + 1)
        .is_some_and(|op| op.operator == "n" && op.operands.is_empty())
    {
        return Ok(None);
    }
    Ok(Some((
        index + 2,
        Region {
            rectangles,
            even_odd: rule.operator == "W*",
        },
    )))
}

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
        if super::diagonal(ctm) {
            rectangle(rect, ctm)?;
        } else {
            // Painting only: under a rotated or skewed CTM the rectangle is a
            // parallelogram, and all that is needed is its corners in range.
            if rect.operands.len() != 4 {
                return Err("invalid rectangle operands".into());
            }
            let [x, y, width, height] = [0, 1, 2, 3].map(|i| super::number(&rect.operands[i]));
            let (x, y, width, height) = (x?, y?, width?, height?);
            if width == 0. || height == 0. {
                return Err("empty rectangle is not editable".into());
            }
            for (px, py) in [
                (x, y),
                (x + width, y),
                (x, y + height),
                (x + width, y + height),
            ] {
                bounded(
                    point(ctm, px, py),
                    "rectangle coordinates exceed their limit",
                )?;
            }
        }
    }
    Ok(Some(count + 1))
}

// A point in page space under any affine CTM.
fn point(ctm: [f64; 6], x: f64, y: f64) -> [f64; 2] {
    [
        x * ctm[0] + y * ctm[2] + ctm[4],
        x * ctm[1] + y * ctm[3] + ctm[5],
    ]
}

fn bounded(point: [f64; 2], message: &str) -> Result<(), String> {
    if point
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 1_000_000.)
    {
        return Err(message.into());
    }
    Ok(())
}

// Complete line/Bezier subpaths, finished before any state or text operator.
// Painting without W/W* cannot alter a later text clip. Keep every operand;
// transformed control points bound each cubic's convex hull without flattening.
// MAX_OPERATIONS bounds the entire stream, and the caller skips consumed ops.
//
// A moveto may start a subpath that draws nothing: TikZ opens paths with two
// and ends them with one after `h`. Such a point paints nothing (ISO 32000-1
// 8.5.3.2 paints a lone point only when it is a closed subpath) and is kept
// byte for byte; the path as a whole still has to draw a segment.
pub(super) fn path(ops: &[Operation], ctm: [f64; 6]) -> Result<usize, String> {
    let invalid = || "only complete bounded painted paths are editable".to_string();
    let mut segments = 0;
    let mut drawn = false;
    let mut closed = false;
    for (index, op) in ops.iter().enumerate() {
        let coordinates = match op.operator.as_str() {
            "m" => {
                segments = 0;
                closed = false;
                2
            }
            "l" | "c" | "v" | "y" if index > 0 && !closed => {
                segments += 1;
                drawn = true;
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
                if drawn && op.operands.is_empty() =>
            {
                return Ok(index + 1);
            }
            _ => return Err(invalid()),
        };
        if op.operands.len() != coordinates {
            return Err(invalid());
        }
        // Painted paths are preserved, never clipped against, so any affine
        // CTM will do; only the points' range is checked.
        for pair in op.operands.chunks_exact(2) {
            let x = super::number(&pair[0])?;
            let y = super::number(&pair[1])?;
            bounded(point(ctm, x, y), "path coordinates exceed their limit")?;
        }
    }
    Err(invalid())
}

/// The page-space envelope of a path sequence `painted` or `path` has already
/// accepted, or `None` when the sequence paints nothing.
///
/// Every coordinate is already known to be finite and in range -- both callers
/// check each point before returning -- so this only takes extrema. A cubic's
/// control-point hull bounds the curve, which is why the operands are taken as
/// they are written rather than flattened. The sequence's own ending operator
/// decides whether there is anything to bound: `n` ends a path without painting
/// it, which is the same test `tags.paint()` is guarded by.
pub(super) fn drawn(
    ops: &[Operation],
    consumed: usize,
    ctm: [f64; 6],
) -> Result<Option<Rect>, String> {
    let end = consumed
        .checked_sub(1)
        .and_then(|last| ops.get(last))
        .ok_or("invalid painted path")?;
    if end.operator == "n" {
        return Ok(None);
    }
    let mut envelope: Option<Rect> = None;
    for op in &ops[..consumed] {
        let mut corners = Vec::new();
        if op.operator == "re" {
            let mut values = [0.; 4];
            if op.operands.len() != 4 {
                return Err("invalid rectangle operands".into());
            }
            for (dest, value) in values.iter_mut().zip(&op.operands) {
                *dest = super::number(value)?;
            }
            let [x, y, w, h] = values;
            corners.extend([[x, y], [x + w, y], [x, y + h], [x + w, y + h]]);
        } else {
            for pair in op.operands.chunks_exact(2) {
                corners.push([super::number(&pair[0])?, super::number(&pair[1])?]);
            }
        }
        for [x, y] in corners {
            let [px, py] = point(ctm, x, y);
            envelope = Some(match envelope {
                None => [px, py, px, py],
                Some(r) => [r[0].min(px), r[1].min(py), r[2].max(px), r[3].max(py)],
            });
        }
    }
    Ok(envelope)
}

fn rectangle(rect: &Operation, ctm: [f64; 6]) -> Result<Rect, String> {
    if rect.operator != "re" || rect.operands.len() != 4 {
        return Err("invalid rectangle operands".into());
    }
    let mut values = [0.; 4];
    for (dest, value) in values.iter_mut().zip(&rect.operands) {
        *dest = super::number(value)?;
    }
    let [x, y, width, height] = values;
    // re uses x + width and y + height as its opposite corner (PDF 1.6,
    // Table 4.9). A single reversed rectangle has the same clipping interior;
    // retain its winding and all authored operators in the saved stream.
    if width == 0. || height == 0. {
        return Err("empty rectangle is not editable".into());
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
    // Stroked text (Tr 1 and 2) reaches half the width beyond its outlines;
    // the scanner widens its ink by that. The authored setting is kept.
    if super::number(value)? < 0. {
        return Err("negative line width is not editable".into());
    }
    Ok(())
}
