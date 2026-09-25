//! Paragraph blocks read off the geometry of a page that has no tags.
//!
//! A wrap (`layout::wrap`) needs to know which runs are lines of one block.
//! A tagged page says so; on an untagged page this module answers from the
//! lines themselves. A line and the nearest line below it that overlaps it
//! along the line are one block when their pitch is at most three ems (two,
//! with another line of the page between them), most of their text is
//! set in one font at one size, their left edges agree (or the upper is a first line indented
//! by up to four ems, or a list item's first line hanging by as much), and
//! nothing is drawn between them; a line opening with a list label starts a
//! block, a line with three spaces in a row or a column gap inside one show is
//! a table row and joins nothing,
//! and a chain ends where its pitch steps. `BUILD.md`, *What a paragraph model would have to work
//! with*, measured the rule against the tags, and *Wrapping on pages without tags*
//! measures what it does to a wrap.
//!
//! A block is named by an object number no document can use -- object 0 is
//! the head of the free list (ISO 32000-1 7.5.4) -- so a block read off the
//! geometry is never mistaken for a structure element.
use super::*;

/// Two runs are on one line when their baselines agree to this, in points.
const BASELINE_TOL: f64 = 0.5;
/// A gap this wide along a line, in ems, separates two blocks side by side.
/// Words of prose are a space apart; a gutter between two columns can be under
/// two ems where a justified line reaches it, and a tab stop is wider than a
/// word space too.
const GUTTER_EM: f64 = 1.0;
/// Left edges of two lines of one block agree to this, in points.
const LEFT_TOL: f64 = 1.0;
/// A first line may start this far right of the rest of its block, in ems.
const INDENT_MAX_EM: f64 = 4.0;
/// The widest pitch that is still a line pitch, in ems.
const PITCH_MAX_EM: f64 = 3.0;
/// The widest pitch, in ems, of two lines with another line of the page
/// between them: a column's own line pitch, where a wider one is the gap
/// between two paragraphs (`BUILD.md`, *Two columns with staggered baselines*).
const SKIP_PITCH_EM: f64 = 2.0;
/// How far a block's pitch may step, in points, before the block ends.
const LEAD_TOL: f64 = 0.5;
/// How far below its baseline a line's own marks reach, underlines included,
/// and how far above it its capitals do, in ems.
const DESCENT_EM: f64 = 0.25;
const CAP_EM: f64 = 0.75;

/// Set by a probe to read blocks off the geometry on tagged pages too, so the
/// rule can be measured where the tags give the answer. Nothing in the
/// application sets it.
#[doc(hidden)]
pub static FORCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One upright run, in the page's user space.
struct Piece<'a> {
    operator: u32,
    baseline: f64,
    left: f64,
    right: f64,
    size: f64,
    font: Option<&'a str>,
    text: &'a str,
    /// Whether the show sets a column gap inside itself: a displacement of an
    /// em or more after its first item, where a word space is a third of one.
    gapped: bool,
    rect: [f64; 4],
}

/// The runs of one line with no gutter between them.
struct Segment<'a> {
    line: usize,
    pieces: Vec<usize>,
    baseline: f64,
    left: f64,
    right: f64,
    size: f64,
    /// The font and size most of the line's characters are set in -- a word
    /// in italics or a footnote mark leaves it alone -- and `None` for a line
    /// with no text, two that tie, or columns set with spaces or displacements.
    face: Option<(&'a str, i64)>,
    /// Whether the line opens with a list label -- a number, a letter or a
    /// bullet -- set apart from the text after it.
    label: bool,
    rect: [f64; 4],
}

/// Whether the page's blocks were read off its geometry rather than its tags.
pub(super) fn geometric_page(page: &Inspection) -> bool {
    page.blocks
        .values()
        .next()
        .is_some_and(|block| block.0 == 0)
}

/// Every show of an upright run on the page and the block it is a line of.
/// Empty when the page has more blocks than a block name can number.
pub(super) fn geometric(
    page: &Inspection,
    sheet: &crate::pagetree::DisplayedPage,
) -> BTreeMap<u32, ObjectId> {
    let origin = (f64::from(sheet.origin.0), f64::from(sheet.origin.1));
    let user = |rect: [f32; 4]| {
        let [l, b, r, t] = crate::text::from_device(sheet.turns, sheet.width, sheet.height, rect);
        [l + origin.0, b + origin.1, r + origin.0, t + origin.1]
    };
    let mut pieces: Vec<Piece> = page
        .runs
        .runs
        .iter()
        .chain(&page.preserved)
        .filter(|run| {
            let m = run.matrix;
            m[1] == 0. && m[2] == 0. && m[0] > 0. && m[3] > 0.
        })
        .map(|run| Piece {
            operator: run.operator,
            baseline: run.matrix[5],
            left: run.matrix[4],
            right: run.matrix[4] + run.advance * run.matrix[0],
            size: run.size * run.matrix[3],
            font: (!run.text.trim().is_empty()).then_some(run.font.as_str()),
            text: &run.text,
            gapped: gapped(page, run.operator),
            rect: user(run.display_rect),
        })
        .collect();
    pieces.sort_by(|a, b| {
        b.baseline
            .total_cmp(&a.baseline)
            .then(a.left.total_cmp(&b.left))
    });
    // Lines, top down, each split at its gutters.
    let mut segments: Vec<Segment> = Vec::new();
    let mut start = 0;
    let mut line = 0;
    while start < pieces.len() {
        let mut end = start + 1;
        while end < pieces.len() && pieces[start].baseline - pieces[end].baseline <= BASELINE_TOL {
            end += 1;
        }
        let mut order: Vec<usize> = (start..end).collect();
        order.sort_by(|&a, &b| pieces[a].left.total_cmp(&pieces[b].left));
        let mut current: Vec<usize> = Vec::new();
        for index in order {
            if let Some(far) = current.iter().map(|&i| pieces[i].right).reduce(f64::max) {
                let size = current
                    .iter()
                    .map(|&i| pieces[i].size)
                    .fold(pieces[index].size, f64::max);
                if pieces[index].left - far > GUTTER_EM * size {
                    segments.push(segment(&pieces, line, std::mem::take(&mut current)));
                }
            }
            current.push(index);
        }
        segments.push(segment(&pieces, line, current));
        start = end;
        line += 1;
    }
    let drawn: Vec<[f64; 4]> = page
        .graphics
        .iter()
        .chain(&page.form_text_bounds)
        .map(|rect| user(*rect))
        .collect();
    // The next line down that a segment is joined to, with the pitch. That is
    // the nearest line below that overlaps along the line, not the page's next
    // line: two columns whose baselines are staggered alternate on the page,
    // so the page's next line of each is always the other column's. Across
    // another line of the page the pitch is held to `SKIP_PITCH_EM`, since a
    // wider one there is a gap between paragraphs.
    let mut joins: BTreeMap<usize, (usize, f64)> = BTreeMap::new();
    let mut taken = BTreeSet::new();
    for (index, before) in segments.iter().enumerate() {
        let overlaps = |s: &Segment| before.right.min(s.right) - before.left.max(s.left) > 0.;
        let Some(next) = segments
            .iter()
            .filter(|s| s.line > before.line && overlaps(s))
            .map(|s| s.line)
            .min()
        else {
            continue;
        };
        let Some(after) = segments
            .iter()
            .enumerate()
            .filter(|(_, s)| s.line == next && overlaps(s))
            .min_by(|(_, a), (_, b)| {
                (a.left - before.left)
                    .abs()
                    .total_cmp(&(b.left - before.left).abs())
            })
            .map(|(i, _)| i)
        else {
            continue;
        };
        let size = before.size.max(segments[after].size);
        if next > before.line + 1
            && before.baseline - segments[after].baseline > SKIP_PITCH_EM * size
        {
            continue;
        }
        if !taken.contains(&after)
            && joined(before, &segments[after], !taken.contains(&index), &drawn)
        {
            joins.insert(index, (after, before.baseline - segments[after].baseline));
            taken.insert(after);
        }
    }
    let mut blocks = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut number: u16 = 0;
    for first in 0..segments.len() {
        if !seen.insert(first) {
            continue;
        }
        let Some(name) = number.checked_add(1) else {
            return BTreeMap::new();
        };
        number = name;
        let mut chain = vec![first];
        let mut pitch = None;
        let mut current = first;
        while let Some(&(next, step)) = joins.get(&current) {
            if seen.contains(&next) || pitch.is_some_and(|p: f64| (step - p).abs() > LEAD_TOL) {
                break;
            }
            pitch.get_or_insert(step);
            seen.insert(next);
            chain.push(next);
            current = next;
        }
        for &segment in &chain {
            for &piece in &segments[segment].pieces {
                for show in layout::shows_of(page, pieces[piece].operator) {
                    blocks.insert(show, (0, name));
                }
            }
        }
    }
    blocks
}

fn segment<'a>(pieces: &[Piece<'a>], line: usize, members: Vec<usize>) -> Segment<'a> {
    let of = |f: fn(&Piece) -> f64| members.iter().map(move |&i| f(&pieces[i]));
    let mut faces: BTreeMap<(&str, i64), usize> = BTreeMap::new();
    // Columns aligned with runs of spaces make a table row of one run: prose
    // does not set three spaces in a row, and a row is no paragraph's line.
    let spaced = members
        .iter()
        .any(|&i| pieces[i].gapped || pieces[i].text.contains("   "));
    for &i in members.iter().filter(|_| !spaced) {
        if let Some(font) = pieces[i].font {
            let characters = pieces[i]
                .text
                .chars()
                .filter(|c| !c.is_whitespace())
                .count();
            *faces
                .entry((font, (pieces[i].size * 100.).round() as i64))
                .or_default() += characters;
        }
    }
    let most = faces.values().copied().max();
    let mut main = faces.iter().filter(|(_, count)| Some(**count) == most);
    let face = match (main.next(), main.next()) {
        (Some((face, _)), None) => Some(*face),
        _ => None,
    };
    let size = of(|p| p.size).fold(0., f64::max);
    let rect = members.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |r, &i| {
            let p = pieces[i].rect;
            [
                r[0].min(p[0]),
                r[1].min(p[1]),
                r[2].max(p[2]),
                r[3].max(p[3]),
            ]
        },
    );
    Segment {
        line,
        baseline: pieces[members[0]].baseline,
        left: of(|p| p.left).fold(f64::INFINITY, f64::min),
        right: of(|p| p.right).fold(f64::NEG_INFINITY, f64::max),
        size,
        face,
        label: label(members.iter().map(|&i| pieces[i].text)),
        rect,
        pieces: members,
    }
}

/// Whether the show at `operator` is a `TJ` holding a displacement of an em or
/// more anywhere but first: a first one says where the run starts.
fn gapped(page: &Inspection, operator: u32) -> bool {
    match page.content.operations.get(operator as usize) {
        Some(operation) if operation.operator == "TJ" => match operation.operands.first() {
            Some(Object::Array(items)) => items
                .iter()
                .skip(1)
                .filter_map(|item| number(item).ok())
                .any(|value| value <= -1000.),
            _ => false,
        },
        _ => false,
    }
}

/// Whether the text of a line, run by run in reading order, opens with a list
/// label and has more text after it: `7.`, `iv)`, `(a)`, a bullet.
pub(super) fn label<'a>(texts: impl Iterator<Item = &'a str>) -> bool {
    // Runs are joined with a space: a label is often a show of its own, and
    // the gap after it is a position rather than a character.
    let text = texts.collect::<Vec<_>>().join(" ");
    let text = text.trim_start();
    let Some(end) = text.find(char::is_whitespace) else {
        return false;
    };
    if text[end..].trim().is_empty() {
        return false;
    }
    let token = &text[..end];
    if matches!(token, "•" | "·" | "▪" | "◦" | "‣" | "-" | "–" | "*" | "o")
        || token.chars().count() == 1
            && ('\u{e000}'..='\u{f8ff}').contains(&token.chars().next().unwrap())
    {
        return true;
    }
    let token = token.strip_prefix('(').unwrap_or(token);
    let Some(body) = token.strip_suffix(['.', ')']) else {
        return false;
    };
    let digits = !body.is_empty() && body.len() <= 3 && body.chars().all(|c| c.is_ascii_digit());
    let letter = body.chars().count() == 1 && body.chars().all(|c| c.is_ascii_alphabetic());
    let roman =
        !body.is_empty() && body.len() <= 6 && body.chars().all(|c| "ivxlcIVXLC".contains(c));
    digits || letter || roman
}

/// Whether `after`, on the line below `before`, continues its block.
/// `indent` allows `before` to be a first line set apart from the rest: it is
/// not one, when a line above has already been joined to it.
fn joined(before: &Segment, after: &Segment, indent: bool, drawn: &[[f64; 4]]) -> bool {
    let size = before.size.max(after.size);
    let pitch = before.baseline - after.baseline;
    if !(size > 0. && pitch > 0. && pitch <= PITCH_MAX_EM * size) {
        return false;
    }
    if before.face.is_none() || before.face != after.face {
        return false;
    }
    // Something drawn between the lines, across both of them, ends the block:
    // a rule, a cell border, a figure. A drawing holding both lines is their
    // background.
    // Between is below the upper line's descenders, where an underline sits,
    // and above the lower line's capitals.
    let (left, right) = (before.left.max(after.left), before.right.min(after.right));
    let (low, high) = {
        let (a, b) = (
            after.baseline + CAP_EM * after.size,
            before.baseline - DESCENT_EM * before.size,
        );
        if a < b {
            (a, b)
        } else {
            let middle = (a + b) / 2.;
            (middle, middle)
        }
    };
    let holds = |outer: [f64; 4], inner: [f64; 4]| {
        outer[0] <= inner[0] + 0.001
            && outer[1] <= inner[1] + 0.001
            && outer[2] >= inner[2] - 0.001
            && outer[3] >= inner[3] - 0.001
    };
    if drawn.iter().any(|g| {
        g[2].min(right) - g[0].max(left) > 0.1
            && g[1] <= high
            && g[3] >= low
            && !(holds(*g, before.rect) && holds(*g, after.rect))
    }) {
        return false;
    }
    // A line opening with a list label starts an item of its own. The item's
    // first line may hang out to the left of the rest; any other block's
    // first line may be indented.
    if after.label {
        return false;
    }
    let delta = after.left - before.left;
    delta.abs() <= LEFT_TOL
        || (indent && delta.abs() < INDENT_MAX_EM * size && (delta > 0.) == before.label)
}
