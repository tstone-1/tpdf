//! Cutting a straight drawing at a redaction region's edge.
//!
//! [`crate::redact::covered`] removes a path the region holds all of and
//! reports one that reaches beyond it. This is the third answer, for the two
//! shapes that can be split exactly: a rule and a rectangle. What is inside
//! the region goes and what is outside stays where it was, drawn by the same
//! operator under the same graphics state.
//!
//! **What is cut, and nothing else:**
//!
//! - A **filled** path made only of rectangles (`re`, or four straight sides).
//!   The region is subtracted from each and the remainder is written as
//!   rectangles again.
//! - A **stroked** path made only of horizontal and vertical lines, under a
//!   known line width above zero, a known cap and no dash. A line is cut where
//!   the region covers its whole thickness.
//!
//! Both need the matrix in effect to keep the axes: a scale, a flip or a
//! quarter turn. Everything else answers [`Verdict::Unsupported`] and is
//! reported as before --- a curve, a path that also clips, fill and stroke
//! together, a dashed or hairline stroke, a region that covers only part of a
//! line's thickness, a skewed matrix, a graphics state this cannot read.
//!
//! **The geometry is done in the path's own space.** The region is carried
//! into it through the inverse matrix, which maps a rectangle to a rectangle
//! exactly when the axes are kept. A stroke has one width there, and a
//! coordinate that is not at a cut is written back as the number it was.
//!
//! **A stroke's ink is modelled, and the model errs outward.** A line covers
//! half its width either side. At a corner it is taken to reach half a width
//! past the corner, which holds a miter, a round and a bevel join at a right
//! angle; at an open end it reaches half a width past for a round or a
//! projecting cap and stops at the end for a butt cap. A new end made by a cut
//! is pulled back by that reach, so its cap stops at the region's edge.
//! [`Drawing::cut`] checks the result against the same model before answering
//! and refuses when anything is still inside.

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};

/// A rectangle as two corners, in whichever space the caller says.
pub type Rect = [f32; 4];

/// What cutting one path against a page's regions comes to.
#[derive(Debug, Clone)]
pub enum Verdict {
    /// None of its ink is inside a region. PDFium's bounds for a stroke reach
    /// a whole line width past the path on every side, whatever the cap, so a
    /// path can overlap a region by its bounds and not by what it draws.
    Outside,
    /// These operations draw what is left of it, and replace all of its own.
    Cut(Vec<Operation>),
    /// All of its ink is inside the regions, so the whole path goes.
    Gone,
    /// It cannot be split exactly. It stays, and is reported.
    Unsupported,
}

/// A matrix as PDF writes one: `[a b c d e f]`.
pub(crate) type Matrix = [f64; 6];

pub(crate) const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// The part of the graphics state a cut depends on.
///
/// `None` is *unknown*, not a default: an operand that would not read or an
/// `ExtGState` that could not be found leaves the value unknown until an
/// operator sets it again or `Q` restores one that was known.
#[derive(Debug, Clone, PartialEq)]
struct State {
    ctm: Option<Matrix>,
    width: Option<f64>,
    /// Whether a cap reaches past the end of its line: round and projecting
    /// square do, by half the width; butt does not.
    cap_reaches: Option<bool>,
    dashed: Option<bool>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            ctm: Some(IDENTITY),
            width: Some(1.0),
            cap_reaches: Some(false),
            dashed: Some(false),
        }
    }
}

fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
}

fn numbers<const N: usize>(operation: &Operation) -> Option<[f64; N]> {
    if operation.operands.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (slot, operand) in out.iter_mut().zip(&operation.operands) {
        *slot = number(operand).filter(|value| value.is_finite())?;
    }
    Some(out)
}

/// `first` applied, then `then`: what `cm` does to the current matrix.
pub(crate) fn concat(first: Matrix, then: Matrix) -> Matrix {
    [
        first[0] * then[0] + first[1] * then[2],
        first[0] * then[1] + first[1] * then[3],
        first[2] * then[0] + first[3] * then[2],
        first[2] * then[1] + first[3] * then[3],
        first[4] * then[0] + first[5] * then[2] + then[4],
        first[4] * then[1] + first[5] * then[3] + then[5],
    ]
}

fn resolved<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    match object {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}

fn dictionary<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    resolved(doc, object)?.as_dict().ok()
}

/// Whose resources a content stream's `gs` names are looked up in.
#[derive(Clone, Copy)]
enum Names {
    /// The page's own content.
    Page(ObjectId),
    /// A Form XObject the page draws. A form with resources of its own reads
    /// those and nothing else; one without reads the page's.
    Form { form: ObjectId, page: ObjectId },
}

/// The `ExtGState` these resources name `name`.
fn ext_g_state<'a>(doc: &'a Document, names: Names, name: &[u8]) -> Option<&'a Dictionary> {
    let page = match names {
        Names::Page(page) => page,
        Names::Form { form, page } => {
            let own = doc
                .get_object(form)
                .and_then(Object::as_stream)
                .ok()
                .and_then(|stream| stream.dict.get(b"Resources").ok());
            match own {
                Some(resources) => {
                    let states = dictionary(doc, resources)?.get(b"ExtGState").ok()?;
                    return dictionary(doc, dictionary(doc, states)?.get(name).ok()?);
                }
                None => page,
            }
        }
    };
    let (inline, inherited) = doc.get_page_resources(page).ok()?;
    let own = inline.into_iter();
    let above = inherited
        .into_iter()
        .filter_map(|id| doc.get_dictionary(id).ok());
    for resources in own.chain(above) {
        let Ok(states) = resources.get(b"ExtGState") else {
            continue;
        };
        let states = dictionary(doc, states)?;
        if let Ok(state) = states.get(name) {
            return dictionary(doc, state);
        }
    }
    None
}

/// Whether a dash array, as `d` or `/D` carries it, draws any gap.
fn dash_of(doc: &Document, array: &Object) -> Option<bool> {
    let array = resolved(doc, array)?.as_array().ok()?;
    Some(!array.is_empty())
}

fn cap_of(value: f64) -> Option<bool> {
    // 0 butt, 1 round, 2 projecting square.
    if value == 0.0 {
        Some(false)
    } else if value == 1.0 || value == 2.0 {
        Some(true)
    } else {
        None
    }
}

/// The state in effect at each of `at`, which is ascending operation indices,
/// in a content stream that begins in the state `start`.
fn states_at(
    doc: &Document,
    names: Names,
    content: &Content,
    at: &[usize],
    start: &State,
) -> Vec<State> {
    let mut found = Vec::with_capacity(at.len());
    let mut wanted = at.iter().copied().peekable();
    let mut state = start.clone();
    let mut stack: Vec<State> = Vec::new();
    for (index, operation) in content.operations.iter().enumerate() {
        if wanted.peek().is_none() {
            break;
        }
        match operation.operator.as_str() {
            "q" => stack.push(state.clone()),
            "Q" => {
                if let Some(saved) = stack.pop() {
                    state = saved;
                }
            }
            "cm" => {
                state.ctm = match (numbers::<6>(operation), state.ctm) {
                    (Some(matrix), Some(ctm)) => Some(concat(matrix, ctm)),
                    _ => None,
                };
            }
            "w" => state.width = numbers::<1>(operation).map(|[width]| width),
            "J" => state.cap_reaches = numbers::<1>(operation).and_then(|[cap]| cap_of(cap)),
            "d" => {
                state.dashed = operation
                    .operands
                    .first()
                    .and_then(|array| dash_of(doc, array));
            }
            "gs" => {
                let named = operation
                    .operands
                    .first()
                    .and_then(|name| name.as_name().ok())
                    .and_then(|name| ext_g_state(doc, names, name));
                match named {
                    Some(named) => {
                        if let Ok(width) = named.get(b"LW") {
                            state.width = resolved(doc, width).and_then(number);
                        }
                        if let Ok(cap) = named.get(b"LC") {
                            state.cap_reaches =
                                resolved(doc, cap).and_then(number).and_then(cap_of);
                        }
                        if let Ok(dash) = named.get(b"D") {
                            state.dashed = resolved(doc, dash)
                                .and_then(|dash| dash.as_array().ok())
                                .and_then(|dash| dash.first())
                                .and_then(|array| dash_of(doc, array));
                        }
                    }
                    None => {
                        state.width = None;
                        state.cap_reaches = None;
                        state.dashed = None;
                    }
                }
            }
            _ => {}
        }
        while wanted.peek() == Some(&index) {
            found.push(state.clone());
            wanted.next();
        }
    }
    // An index past the end has no state; the caller's paths never name one.
    found.resize(at.len(), start.clone());
    found
}

type Point = (f64, f64);

/// A run of straight lines.
#[derive(Debug, Clone, PartialEq)]
struct Poly {
    points: Vec<Point>,
    closed: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Ink {
    /// Rectangles, each as two corners with `[0] < [2]` and `[1] < [3]`, and
    /// whether `re` should be written running clockwise.
    Fill {
        rects: Vec<[f64; 4]>,
        clockwise: bool,
    },
    /// Lines under one width.
    Stroke {
        polys: Vec<Poly>,
        half: f64,
        cap_reaches: bool,
    },
}

/// One painted path this can cut: its geometry in its own space, the matrix
/// that places it, and the operator that paints it.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    ink: Ink,
    ctm: Matrix,
    paint: String,
}

/// Whether a matrix keeps the axes: no skew, and it can be inverted.
fn keeps_axes(matrix: Matrix) -> bool {
    let [a, b, c, d, ..] = matrix;
    let scale = a.abs().max(b.abs()).max(c.abs()).max(d.abs());
    if scale <= 0.0 || !scale.is_finite() {
        return false;
    }
    let small = scale * 1e-9;
    let straight = b.abs() <= small && c.abs() <= small && a.abs() > small && d.abs() > small;
    let turned = a.abs() <= small && d.abs() <= small && b.abs() > small && c.abs() > small;
    straight || turned
}

/// A page-space rectangle in the space `ctm` maps from, as ordered corners.
fn into_space(ctm: Matrix, rect: Rect) -> [f64; 4] {
    let [a, b, c, d, e, f] = ctm;
    let det = a * d - b * c;
    let back = |x: f64, y: f64| -> Point {
        let (x, y) = (x - e, y - f);
        ((d * x - c * y) / det, (a * y - b * x) / det)
    };
    let one = back(f64::from(rect[0]), f64::from(rect[1]));
    let two = back(f64::from(rect[2]), f64::from(rect[3]));
    [
        one.0.min(two.0),
        one.1.min(two.1),
        one.0.max(two.0),
        one.1.max(two.1),
    ]
}

/// The subpaths a path's construction operators build, or `None` when one of
/// them is not a move, a line, a rectangle or a close.
fn subpaths(construction: &[&Operation]) -> Option<Vec<Poly>> {
    let mut polys: Vec<Poly> = Vec::new();
    // Whether a line may be added to the last subpath.
    let mut open = false;
    for operation in construction {
        match operation.operator.as_str() {
            "m" => {
                let [x, y] = numbers::<2>(operation)?;
                // A move after a move replaces it, as PDFium counts points.
                if open && polys.last().is_some_and(|poly| poly.points.len() == 1) {
                    polys.pop();
                }
                polys.push(Poly {
                    points: vec![(x, y)],
                    closed: false,
                });
                open = true;
            }
            "l" => {
                let [x, y] = numbers::<2>(operation)?;
                if !open {
                    return None;
                }
                polys.last_mut()?.points.push((x, y));
            }
            "re" => {
                let [x, y, w, h] = numbers::<4>(operation)?;
                if open && polys.last().is_some_and(|poly| poly.points.len() == 1) {
                    polys.pop();
                }
                polys.push(Poly {
                    points: vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)],
                    closed: true,
                });
                open = false;
            }
            "h" => {
                if !open {
                    return None;
                }
                polys.last_mut()?.closed = true;
                open = false;
            }
            _ => return None,
        }
    }
    polys.retain(|poly| poly.points.len() >= 2);
    for poly in &mut polys {
        // A closed run that already returns to its start has a closing line of
        // no length; drop the repeated point so every line has one.
        if poly.closed && poly.points.len() > 2 && poly.points.first() == poly.points.last() {
            poly.points.pop();
        }
    }
    (!polys.is_empty()).then_some(polys)
}

/// The rectangle four points draw, and whether they run clockwise; `None` when
/// they are not the corners of one with sides along the axes.
fn as_rect(points: &[Point]) -> Option<([f64; 4], bool)> {
    let &[p0, p1, p2, p3] = points else {
        return None;
    };
    let along_x = p0.1 == p1.1 && p1.0 == p2.0 && p2.1 == p3.1 && p3.0 == p0.0;
    let along_y = p0.0 == p1.0 && p1.1 == p2.1 && p2.0 == p3.0 && p3.1 == p0.1;
    if !(along_x || along_y) {
        return None;
    }
    let rect = [
        p0.0.min(p2.0),
        p0.1.min(p2.1),
        p0.0.max(p2.0),
        p0.1.max(p2.1),
    ];
    if rect[0] >= rect[2] || rect[1] >= rect[3] {
        return None;
    }
    // Twice the signed area: positive runs counter-clockwise.
    let area = (p0.0 * p1.1 - p1.0 * p0.1)
        + (p1.0 * p2.1 - p2.0 * p1.1)
        + (p2.0 * p3.1 - p3.0 * p2.1)
        + (p3.0 * p0.1 - p0.0 * p3.1);
    Some((rect, area < 0.0))
}

fn parse(path: &[&Operation], state: &State) -> Option<Drawing> {
    let (paint, construction) = path.split_last()?;
    let ctm = state.ctm.filter(|ctm| keeps_axes(*ctm))?;
    let mut polys = subpaths(construction)?;
    let ink = match paint.operator.as_str() {
        "f" | "F" | "f*" => {
            let mut rects = Vec::with_capacity(polys.len());
            let mut turns: Vec<bool> = Vec::with_capacity(polys.len());
            for poly in &mut polys {
                // Filling closes a subpath, so an open one that returns to its
                // start is the same four corners.
                if !poly.closed && poly.points.len() == 5 && poly.points[0] == poly.points[4] {
                    poly.points.pop();
                }
                let (rect, clockwise) = as_rect(&poly.points)?;
                rects.push(rect);
                turns.push(clockwise);
            }
            // Two rectangles that overlap fill their overlap under the nonzero
            // rule only when they run the same way, and never under even-odd.
            // Then the fill is their union, and the union less a region is the
            // union of each less the region.
            let one_way = turns.iter().all(|turn| *turn == turns[0]);
            if rects.len() > 1 && (paint.operator == "f*" || !one_way) {
                return None;
            }
            Ink::Fill {
                rects,
                clockwise: turns[0],
            }
        }
        "S" | "s" => {
            if paint.operator == "s" {
                // `s` is `h S`: it closes the subpath still being built.
                let last = polys.last_mut()?;
                if !last.closed {
                    last.closed = true;
                    if last.points.len() > 2 && last.points.first() == last.points.last() {
                        last.points.pop();
                    }
                }
            }
            let width = state.width.filter(|width| *width > 0.0)?;
            let cap_reaches = state.cap_reaches?;
            if state.dashed != Some(false) {
                return None;
            }
            for poly in &polys {
                if poly.closed && poly.points.len() < 3 {
                    return None;
                }
                let count = poly.points.len();
                let lines = if poly.closed { count } else { count - 1 };
                for at in 0..lines {
                    let (from, to) = (poly.points[at], poly.points[(at + 1) % count]);
                    // Exactly one coordinate changes: along an axis, with length.
                    if (from.0 == to.0) == (from.1 == to.1) {
                        return None;
                    }
                }
            }
            Ink::Stroke {
                polys,
                half: width / 2.0,
                cap_reaches,
            }
        }
        _ => return None,
    };
    Some(Drawing {
        ink,
        ctm,
        paint: if paint.operator == "s" {
            "S".to_string()
        } else {
            paint.operator.clone()
        },
    })
}

/// What each of a page's painted paths is, for cutting; `None` for one this
/// cannot cut.
///
/// `paths` is each path's operation indices, ascending, the paint last, in the
/// order the page paints them.
#[must_use]
pub fn drawings(
    doc: &Document,
    page: ObjectId,
    content: &Content,
    paths: &[&[usize]],
) -> Vec<Option<Drawing>> {
    drawings_from(doc, Names::Page(page), content, paths, &State::default())
}

/// [`drawings`] for the paths a Form XObject paints.
///
/// A form's content starts in the graphics state the page is in at the `Do`
/// that draws it, with the form's `/Matrix` applied first. `page_content` is
/// the page's own content and `drawn_at` the index of that `Do` in it; `form`
/// is the form's stream and `content` what it decodes to. The answer is in
/// page space, as [`drawings`]' is, so [`Drawing::cut`] takes the same regions.
///
/// Every entry is `None` when the `Do` is not where `drawn_at` says, or the
/// form's `/Matrix` will not read: a path placed by a matrix this cannot read
/// is not one it can cut.
#[must_use]
pub fn drawings_in_form(
    doc: &Document,
    page: ObjectId,
    page_content: &Content,
    drawn_at: usize,
    form: ObjectId,
    content: &Content,
    paths: &[&[usize]],
) -> Vec<Option<Drawing>> {
    let nothing = || vec![None; paths.len()];
    if page_content
        .operations
        .get(drawn_at)
        .is_none_or(|operation| operation.operator != "Do")
    {
        return nothing();
    }
    let Ok(stream) = doc.get_object(form).and_then(Object::as_stream) else {
        return nothing();
    };
    let matrix = match stream.dict.get(b"Matrix") {
        Err(_) => Some(IDENTITY),
        Ok(value) => resolved(doc, value)
            .and_then(|value| value.as_array().ok())
            .filter(|array| array.len() == 6)
            .and_then(|array| {
                let mut out = [0.0; 6];
                for (slot, entry) in out.iter_mut().zip(array) {
                    *slot = resolved(doc, entry)
                        .and_then(number)
                        .filter(|value| value.is_finite())?;
                }
                Some(out)
            }),
    };
    let mut start = states_at(
        doc,
        Names::Page(page),
        page_content,
        &[drawn_at],
        &State::default(),
    )
    .pop()
    .unwrap_or_default();
    start.ctm = match (matrix, start.ctm) {
        (Some(matrix), Some(ctm)) => Some(concat(matrix, ctm)),
        _ => None,
    };
    drawings_from(doc, Names::Form { form, page }, content, paths, &start)
}

fn drawings_from(
    doc: &Document,
    names: Names,
    content: &Content,
    paths: &[&[usize]],
    start: &State,
) -> Vec<Option<Drawing>> {
    let paints: Vec<usize> = paths
        .iter()
        .map(|path| path.last().copied().unwrap_or(usize::MAX))
        .collect();
    let states = states_at(doc, names, content, &paints, start);
    paths
        .iter()
        .zip(&states)
        .map(|(path, state)| {
            // Its operators have to be next to each other. One in between
            // could change the matrix or the line width partway through.
            let (first, last) = (*path.first()?, *path.last()?);
            if last - first + 1 != path.len() {
                return None;
            }
            let operations: Vec<&Operation> = path
                .iter()
                .map(|at| content.operations.get(*at))
                .collect::<Option<_>>()?;
            parse(&operations, state)
        })
        .collect()
}

/// A line of a stroke as the rectangle it inks: `[x0, y0, x1, y1]`.
fn inked(from: Point, to: Point, half: f64, reach_from: f64, reach_to: f64) -> [f64; 4] {
    if from.1 == to.1 {
        let (left, right) = if from.0 <= to.0 {
            (from.0 - reach_from, to.0 + reach_to)
        } else {
            (to.0 - reach_to, from.0 + reach_from)
        };
        [left, from.1 - half, right, from.1 + half]
    } else {
        let (low, high) = if from.1 <= to.1 {
            (from.1 - reach_from, to.1 + reach_to)
        } else {
            (to.1 - reach_to, from.1 + reach_from)
        };
        [from.0 - half, low, from.0 + half, high]
    }
}

/// Whether two rectangles share more than `eps` of area in both directions.
fn overlap(a: [f64; 4], b: [f64; 4], eps: f64) -> bool {
    a[0] < b[2] - eps && b[0] + eps < a[2] && a[1] < b[3] - eps && b[1] + eps < a[3]
}

/// How far a line reaches past each of its ends.
fn reaches(poly: &Poly, at: usize, half: f64, cap_reaches: bool) -> (f64, f64) {
    let count = poly.points.len();
    let lines = if poly.closed { count } else { count - 1 };
    let cap = if cap_reaches { half } else { 0.0 };
    let from = if poly.closed || at > 0 { half } else { cap };
    let to = if poly.closed || at + 1 < lines {
        half
    } else {
        cap
    };
    (from, to)
}

/// Whether any line of `polys` inks inside `region`.
fn inks_inside(polys: &[Poly], half: f64, cap_reaches: bool, region: [f64; 4], eps: f64) -> bool {
    polys.iter().any(|poly| {
        let count = poly.points.len();
        let lines = if poly.closed { count } else { count - 1 };
        (0..lines).any(|at| {
            let (reach_from, reach_to) = reaches(poly, at, half, cap_reaches);
            let ink = inked(
                poly.points[at],
                poly.points[(at + 1) % count],
                half,
                reach_from,
                reach_to,
            );
            overlap(ink, region, eps)
        })
    })
}

/// One run of lines less `region`. `Ok(None)` when it was not touched.
fn cut_poly(
    poly: &Poly,
    half: f64,
    cap_reaches: bool,
    region: [f64; 4],
    eps: f64,
) -> Result<Option<Vec<Poly>>, ()> {
    let count = poly.points.len();
    let lines = if poly.closed { count } else { count - 1 };
    let pull = if cap_reaches { half } else { 0.0 };
    // Each kept stretch: its line, its two ends, and whether it still starts
    // and ends at the corner its line started and ended at.
    let mut kept: Vec<(usize, Point, Point, bool, bool)> = Vec::new();
    let mut touched = false;
    for at in 0..lines {
        let (from, to) = (poly.points[at], poly.points[(at + 1) % count]);
        let (reach_from, reach_to) = reaches(poly, at, half, cap_reaches);
        let ink = inked(from, to, half, reach_from, reach_to);
        if !overlap(ink, region, eps) {
            kept.push((at, from, to, true, true));
            continue;
        }
        touched = true;
        let level = from.1 == to.1;
        // `a` runs along the line and `p` across it.
        let (a_from, a_to, p) = if level {
            (from.0, to.0, from.1)
        } else {
            (from.1, to.1, from.0)
        };
        let (region_a, region_p) = if level {
            ((region[0], region[2]), (region[1], region[3]))
        } else {
            ((region[1], region[3]), (region[0], region[2]))
        };
        // The region has to take the line's whole thickness. A strip of it
        // left beside the region cannot be drawn by a stroke of this width.
        if !(region_p.0 <= p - half + eps && p + half - eps <= region_p.1) {
            return Err(());
        }
        let (low, high) = (a_from.min(a_to), a_from.max(a_to));
        let before = (low, high.min(region_a.0 - pull));
        let after = (low.max(region_a.1 + pull), high);
        let point = |a: f64| if level { (a, p) } else { (p, a) };
        let mut stretches: Vec<(Point, Point, bool, bool)> = Vec::new();
        if before.1 - before.0 > eps {
            stretches.push((point(before.0), point(before.1), true, before.1 == high));
        }
        if after.1 - after.0 > eps {
            stretches.push((point(after.0), point(after.1), after.0 == low, true));
        }
        if a_from > a_to {
            // The line runs toward the low end: walk its stretches that way.
            stretches.reverse();
            for (low_end, high_end, at_low, at_high) in stretches {
                kept.push((at, high_end, low_end, at_high, at_low));
            }
        } else {
            for (low_end, high_end, at_low, at_high) in stretches {
                kept.push((at, low_end, high_end, at_low, at_high));
            }
        }
    }
    if !touched {
        return Ok(None);
    }
    if kept.len() == lines && kept.iter().all(|stretch| stretch.3 && stretch.4) {
        // Touched only where a corner or a cap reaches past the path, and no
        // line can be shortened to fix that. The caller's check refuses it.
        return Ok(Some(vec![poly.clone()]));
    }

    // Stretches that still meet at a corner stay one run, so the join there is
    // drawn as it was.
    let mut runs: Vec<Vec<Point>> = Vec::new();
    // Only the next line can be joined to. A line between two others that left
    // no stretch at all is shorter than the tolerance, and the two either side
    // of it are then two runs.
    let mut previous: Option<(usize, bool)> = None;
    for (line, from, to, at_start, at_end) in &kept {
        let joined = *at_start && previous == Some((line.wrapping_sub(1), true));
        if joined {
            runs.last_mut().expect("a run is open").push(*to);
        } else {
            runs.push(vec![*from, *to]);
        }
        previous = Some((*line, *at_end));
    }
    if poly.closed && runs.len() > 1 {
        let starts = kept.first().is_some_and(|first| first.0 == 0 && first.3);
        let ends = kept
            .last()
            .is_some_and(|last| last.0 + 1 == lines && last.4);
        if starts && ends {
            let mut last = runs.pop().expect("more than one run");
            let first = runs.remove(0);
            last.extend(first.into_iter().skip(1));
            runs.insert(0, last);
        }
    }
    Ok(Some(
        runs.into_iter()
            .map(|points| Poly {
                points,
                closed: false,
            })
            .collect(),
    ))
}

/// One rectangle less `region`, as up to four rectangles.
fn cut_rect(rect: [f64; 4], region: [f64; 4], eps: f64) -> Option<Vec<[f64; 4]>> {
    if !overlap(rect, region, eps) {
        return None;
    }
    let mut left = Vec::new();
    if region[0] - rect[0] > eps {
        left.push([rect[0], rect[1], region[0], rect[3]]);
    }
    if rect[2] - region[2] > eps {
        left.push([region[2], rect[1], rect[2], rect[3]]);
    }
    let (from, to) = (rect[0].max(region[0]), rect[2].min(region[2]));
    if region[1] - rect[1] > eps {
        left.push([from, rect[1], to, region[1]]);
    }
    if rect[3] - region[3] > eps {
        left.push([from, region[3], to, rect[3]]);
    }
    Some(left)
}

fn real(value: f64) -> Object {
    #[allow(clippy::cast_possible_truncation)]
    Object::Real(value as f32)
}

fn operation(operator: &str, values: &[f64]) -> Operation {
    Operation::new(operator, values.iter().copied().map(real).collect())
}

impl Drawing {
    /// The largest coordinate in play, for a tolerance that follows the scale
    /// the path is drawn at.
    fn scale(&self, regions: &[[f64; 4]]) -> f64 {
        let mut most = 1.0f64;
        let mut see = |value: f64| most = most.max(value.abs());
        match &self.ink {
            Ink::Fill { rects, .. } => rects.iter().flatten().copied().for_each(&mut see),
            Ink::Stroke { polys, .. } => {
                polys
                    .iter()
                    .flat_map(|poly| &poly.points)
                    .for_each(|point| {
                        see(point.0);
                        see(point.1);
                    })
            }
        }
        regions.iter().flatten().copied().for_each(&mut see);
        most
    }

    /// This path less every one of `regions`, which are in page space.
    #[must_use]
    pub fn cut(&self, regions: &[Rect]) -> Verdict {
        let regions: Vec<[f64; 4]> = regions
            .iter()
            .map(|region| into_space(self.ctm, *region))
            .collect();
        // An `f32` holds about seven digits, and the result is written as one.
        let eps = self.scale(&regions) * 1e-6;
        let mut changed = false;
        match &self.ink {
            Ink::Fill { rects, clockwise } => {
                let mut rects = rects.clone();
                for region in &regions {
                    let mut next = Vec::with_capacity(rects.len());
                    for rect in rects {
                        match cut_rect(rect, *region, eps) {
                            Some(pieces) => {
                                changed = true;
                                next.extend(pieces);
                            }
                            None => next.push(rect),
                        }
                    }
                    rects = next;
                }
                if !changed {
                    return Verdict::Outside;
                }
                if rects.is_empty() {
                    return Verdict::Gone;
                }
                let mut operations: Vec<Operation> = rects
                    .iter()
                    .map(|rect| {
                        let (width, height) = (rect[2] - rect[0], rect[3] - rect[1]);
                        if *clockwise {
                            operation("re", &[rect[2], rect[1], -width, height])
                        } else {
                            operation("re", &[rect[0], rect[1], width, height])
                        }
                    })
                    .collect();
                operations.push(Operation::new(&self.paint, Vec::new()));
                Verdict::Cut(operations)
            }
            Ink::Stroke {
                polys,
                half,
                cap_reaches,
            } => {
                let mut polys = polys.clone();
                for region in &regions {
                    let mut next = Vec::with_capacity(polys.len());
                    for poly in polys {
                        match cut_poly(&poly, *half, *cap_reaches, *region, eps) {
                            Ok(Some(pieces)) => {
                                changed = true;
                                next.extend(pieces);
                            }
                            Ok(None) => next.push(poly),
                            Err(()) => return Verdict::Unsupported,
                        }
                    }
                    polys = next;
                }
                if !changed {
                    return Verdict::Outside;
                }
                // The result against the model that made it. A corner whose
                // join still reaches into a region fails here.
                if regions
                    .iter()
                    .any(|region| inks_inside(&polys, *half, *cap_reaches, *region, eps))
                {
                    return Verdict::Unsupported;
                }
                if polys.is_empty() {
                    return Verdict::Gone;
                }
                let mut operations = Vec::new();
                for poly in &polys {
                    for (at, point) in poly.points.iter().enumerate() {
                        let operator = if at == 0 { "m" } else { "l" };
                        operations.push(operation(operator, &[point.0, point.1]));
                    }
                    if poly.closed {
                        operations.push(Operation::new("h", Vec::new()));
                    }
                }
                operations.push(Operation::new(&self.paint, Vec::new()));
                Verdict::Cut(operations)
            }
        }
    }
}

#[cfg(test)]
mod tests;
