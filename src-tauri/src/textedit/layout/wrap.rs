//! Wrapping an edit onto new lines of its own paragraph, on a tagged page.
//!
//! When the text a reader typed no longer fits the room on its line and the
//! room ends at the page edge, a tagged page can say which runs are lines of the
//! same block: the structure element that owns them. This module uses that
//! answer and nothing geometric in its place. `docs/PLAN.md` §7, *What wrapping
//! should be scoped to*, has the measurement that chose it: a rule read off the
//! geometry joined one pair of paragraphs in six on pages whose tags gave the
//! real answer, and the pair it got wrong is the one a wrap damages.
//!
//! The edit is laid out at the block's own measure and line pitch, its
//! continuation lines start at the block's left edge, and every line of the
//! block below it moves down by the lines the edit added. Nothing else moves:
//! the next block has to have that much clear space already, and an edit whose
//! block has none is refused with the reason. What is below a paragraph is
//! another paragraph most of the time, and moving that too is a larger
//! capability than this one.
//!
//! Text of the block after the edit on its own line flows too, a run at a
//! time: each run after the edit keeps its gap to the one before it, stays on
//! the edit's last line while it fits the block's measure, and otherwise starts
//! the next line at the block's left edge. A run is not split, so a line can
//! end short of the measure where a run too wide for what is left of it moved
//! down whole. Splitting one at a space means writing part of another run's
//! text again in its own font, which is the next increment rather than this one.
//!
//! A moved show keeps its own bytes. It is placed with an explicit `Tm` and
//! followed by the same bit-exact restoration of the line matrix and the text
//! cursor a replacement ends with (`restore_line`), so every show after it that
//! does not move -- in the block or anywhere else on the page -- is drawn from
//! exactly the state it always was. A `Td` rewritten in place would have moved
//! every later line of the text object as well.
use super::*;

/// How far the text of one line may sit above or below another and still be
/// that line, in ems of the edited run: a superscript or a subscript is on its
/// line, the next line at any real leading is not.
const SAME_LINE_EM: f64 = 0.5;

/// The pitch a block's lines may have, in ems of the edited run. Outside this
/// the "next line" is not a line of running text -- a block continued past a
/// figure, a stack of fragments -- and the wrap is not offered.
const PITCH_EM: (f64, f64) = (0.8, 3.0);

/// What a wrap does to the page, decided before any text is laid out.
pub(super) struct Plan {
    /// Where the first line may reach, in the run's own text-space units from
    /// its origin: the block's measure, and never past the room on the line.
    pub first: f64,
    /// Where continuation lines start, in the same units: the block's left
    /// edge, which may be left of the run itself.
    pub start: f64,
    /// How wide a continuation line may be.
    pub rest: f64,
    /// The distance between two of the block's baselines, positive, in the
    /// run's own text-space units.
    pub pitch: f64,
    /// Every show of the block drawn below the edited line, in stream order.
    /// Each moves down by the lines the edit adds.
    pub below: Vec<u32>,
    /// The block's runs after the edit on its own line, in the order they are
    /// read along it. Each flows after the edit (`flow`).
    pub after: Vec<Unit>,
}

/// One run of the block after the edit on its line, which moves whole.
#[derive(Clone, Debug)]
pub(super) struct Unit {
    /// Its show and the shows grouped into it, which move together.
    pub shows: Vec<u32>,
    /// Where it starts and ends along the line, in the edited run's text-space
    /// units from its origin.
    pub start: f64,
    pub end: f64,
}

/// Where each unit goes: its line below the edit's first (0 is the edit's own
/// line) and how far along the line it starts, in the edited run's text-space
/// units. `None` when one of them is wider than a whole line of the block.
/// Every line reaches the block's measure, `plan.first`, the first included:
/// `rest` is that measure less where continuation lines start.
///
/// `line` and `end` are where the replacement's own last line is and where it
/// ends. A unit keeps the gap it had to whatever came before it on the line --
/// the edited run for the first, the unit before it for the rest -- while it
/// fits, and a unit that does not fit starts the next line at the block's left
/// edge, with no gap in front of it.
pub(super) fn flow(
    plan: &Plan,
    run_advance: f64,
    (line, end): (usize, f64),
) -> Option<Vec<(usize, f64)>> {
    let (mut line, mut cursor, mut previous) = (line, end, run_advance);
    let mut places = Vec::new();
    for unit in &plan.after {
        let width = unit.end - unit.start;
        if width > plan.rest + 0.000_001 {
            return None;
        }
        let mut at = cursor + (unit.start - previous).max(0.);
        if at + width > plan.first + 0.000_001 {
            line += 1;
            at = plan.start;
        }
        places.push((line, at));
        cursor = at + width;
        previous = unit.end;
    }
    Some(places)
}

/// Why a wrap cannot be offered, split by whether the reader should hear about
/// it: `NotApplicable` leaves the refusal the edit already had, `Blocked`
/// replaces it with one that names what is in the way.
pub(super) enum Refused {
    NotApplicable,
    Blocked(&'static str),
}

pub(super) const NO_ROOM: &str = "There is no room for more text on this line, and this paragraph cannot wrap: its lines would move onto what is below it. Shorten the text or reduce the font size.";
pub(super) const UNMOVABLE: &str = "There is no room for more text on this line, and this paragraph cannot wrap: part of it below cannot be moved. Shorten the text or reduce the font size.";
pub(super) const DRAWN: &str = "There is no room for more text on this line, and this paragraph cannot wrap: a drawing or an annotation is placed over the lines that would move. Shorten the text or reduce the font size.";
/// Said to whichever of the two edits the reader is making, so it names both.
pub(in crate::textedit) const CONFLICT: &str = "One edit in this paragraph wraps onto a new line and moves text that another pending edit changes. Save first, then make the second edit again.";

/// A point of page space in the run's own text space.
fn to_text(matrix: [f64; 6], (x, y): (f64, f64)) -> Option<(f64, f64)> {
    let [a, b, c, d, e, f] = matrix;
    let det = a * d - b * c;
    if det == 0. || !det.is_finite() {
        return None;
    }
    let (x, y) = (x - e, y - f);
    Some(((d * x - c * y) / det, (a * y - b * x) / det))
}

/// Where a run starts and ends in the edited run's text space, and whether it is
/// set in the same direction. `None` for a run turned against it.
fn span(run: &Run, other: &Run) -> Option<((f64, f64), f64)> {
    let m = other.matrix;
    let origin = to_text(run.matrix, (m[4], m[5]))?;
    // Same direction: the other run's text axis runs along +x of the edited
    // run, not against it and not across it.
    let unit = to_text(run.matrix, (m[4] + m[0], m[5] + m[1]))?;
    let axis = (unit.0 - origin.0, unit.1 - origin.1);
    (axis.0 > 0. && axis.1.abs() <= axis.0 * 1e-9)
        .then_some((origin, origin.0 + other.advance * axis.0))
}

/// The plan, or why there is none.
///
/// `room` is how far the line allows with the text after the run where it is,
/// and `whole` how far it allows with the text the push would move out of the
/// way; `line` is every show that push would move. When the block has text
/// after the run on its line, that text flows after the edit instead, so the
/// line is the edit's as far as `whole`, and every show of the push has to be
/// the block's: another block's text sharing the line is not this one's to
/// move onto a new line. The block's measure narrows the answer, never widens
/// it.
pub(super) fn plan(
    page: &Inspection,
    run: &Run,
    (room, whole): (f64, f64),
    line: &BTreeSet<u32>,
) -> Result<Plan, Refused> {
    let block = *page
        .blocks
        .get(&run.operator)
        .ok_or(Refused::NotApplicable)?;
    // Grouped members were merged into their leader's run, on the same line.
    let mut leader = BTreeMap::new();
    for (lead, members) in &page.groups {
        for member in members {
            leader.insert(*member, *lead);
        }
    }
    let known: BTreeMap<u32, &Run> = page
        .runs
        .runs
        .iter()
        .chain(&page.preserved)
        .map(|other| (other.operator, other))
        .collect();
    let own = shows_of(page, run.operator);
    let tolerance = SAME_LINE_EM * run.size;
    let mut below = Vec::new();
    // (baseline, start, end) of every run of the block with text, in the
    // edited run's text space; the run's own line is baseline zero.
    let mut lines: Vec<(f64, f64, f64)> = Vec::new();
    let mut after: BTreeMap<u32, Unit> = BTreeMap::new();
    for (&show, &owner) in &page.blocks {
        if owner != block || own.contains(&show) {
            continue;
        }
        let Some(other) = known.get(leader.get(&show).unwrap_or(&show)) else {
            // A show with no run: a spacer inside an ActualText span. Its line
            // is unknown, so it may be under the edit, and nothing here can
            // move it.
            if show > run.operator {
                return Err(Refused::Blocked(UNMOVABLE));
            }
            continue;
        };
        let ((x, y), end) = span(run, other).ok_or(Refused::NotApplicable)?;
        if y.abs() <= tolerance {
            // On the edited line: text after the run flows after the edit, as
            // long as the writer can move it. The push along the line is not
            // the judge of that: it skips a neighbour that starts a rounding
            // inside the box (`Free::from`), which moves as well as any other.
            if x > 0. && !other.text.trim().is_empty() {
                if !page.contexts.contains_key(&show)
                    || page.actual_text.contains_key(&show)
                    || page.compound_run_clips.contains_key(&show)
                {
                    return Err(Refused::NotApplicable);
                }
                after.entry(other.operator).or_insert_with(|| Unit {
                    shows: shows_of(page, other.operator),
                    start: x,
                    end,
                });
            }
        } else if y < 0. {
            if !page.contexts.contains_key(&show)
                || page.actual_text.contains_key(&show)
                || page.compound_run_clips.contains_key(&show)
            {
                return Err(Refused::Blocked(UNMOVABLE));
            }
            below.push(show);
        }
        if !other.text.trim().is_empty() {
            lines.push((y, x, end));
        }
    }
    lines.push((0., 0., run.advance));
    if !after.is_empty()
        && !line
            .iter()
            .all(|show| page.blocks.get(show) == Some(&block))
    {
        return Err(Refused::NotApplicable);
    }
    let first = if after.is_empty() { room } else { whole };
    let mut after: Vec<Unit> = after.into_values().collect();
    after.sort_by(|a, b| a.start.total_cmp(&b.start));
    // The pitch: to the next line down, else to the one above, else the
    // writer's own single-block default.
    let next = lines
        .iter()
        .filter(|(y, ..)| *y < -tolerance)
        .map(|(y, ..)| -y)
        .fold(f64::INFINITY, f64::min);
    let previous = lines
        .iter()
        .filter(|(y, ..)| *y > tolerance)
        .map(|(y, ..)| *y)
        .fold(f64::INFINITY, f64::min);
    let pitch = if next.is_finite() {
        next
    } else if previous.is_finite() {
        previous
    } else {
        run.size * 1.25
    };
    if !(PITCH_EM.0 * run.size..=PITCH_EM.1 * run.size).contains(&pitch) {
        return Err(Refused::NotApplicable);
    }
    let top = lines
        .iter()
        .map(|(y, ..)| *y)
        .fold(f64::NEG_INFINITY, f64::max);
    let several = lines.iter().any(|(y, ..)| (*y - top).abs() > tolerance);
    // The block's measure is the furthest any of its lines reaches; a block of
    // one line has none, and its first line keeps the room it had.
    let far = if several {
        lines
            .iter()
            .map(|(.., end)| *end)
            .fold(run.advance, f64::max)
            .min(first)
    } else {
        first
    };
    // Continuation lines start where the block's lines after its first do:
    // that is the body margin under a first-line indent, and the hanging
    // position of a list item.
    let start = lines
        .iter()
        .filter(|(y, ..)| !several || *y < top - tolerance)
        .map(|(_, x, _)| *x)
        .fold(f64::INFINITY, f64::min);
    let rest = far - start;
    if !start.is_finite() || !rest.is_finite() || rest <= 0. || far <= 0. {
        return Err(Refused::NotApplicable);
    }
    Ok(Plan {
        first: far,
        start,
        rest,
        pitch,
        below,
        after,
    })
}

/// The operations that draw one show `offset` further along its page than its
/// source did, keeping every byte of the show itself, then put the line matrix
/// and the text cursor back exactly where the source left them.
///
/// `offset` is in the page's original user space. A show's `Tm` operand lives
/// in the space of the transform in force at it, so the offset is taken through
/// that transform's inverse first.
pub(super) fn lowered(
    page: &Inspection,
    show: u32,
    offset: (f64, f64),
) -> Result<Vec<Operation>, String> {
    let context = page
        .contexts
        .get(&show)
        .ok_or("missing text layout context")?;
    let operation = page
        .content
        .operations
        .get(show as usize)
        .ok_or("text run no longer exists")?;
    let [a, b, c, d, ..] = context.transform;
    let det = a * d - b * c;
    let local = (
        (d * offset.0 - c * offset.1) / det,
        (a * offset.1 - b * offset.0) / det,
    );
    if !local.0.is_finite() || !local.1.is_finite() {
        return Err("text position exceeds its limit".into());
    }
    let mut matrix = context.shown;
    shift_position(&mut matrix, local.0, local.1)?;
    let mut show_op = operation.clone();
    // The shown matrix already stands after a leading TJ number, which says
    // where the run starts; keeping it would move the run twice.
    if page.leads.contains_key(&show) {
        let Some(Object::Array(items)) = show_op.operands.first_mut() else {
            return Err("invalid text patch".into());
        };
        items.remove(0);
    }
    let mut operations = vec![numeric("Tm", &matrix), show_op];
    operations.extend(restore_line(
        &page.content.operations,
        context.line_origin,
        show as usize,
    )?);
    let adjustment = (-context.cursor_after * 1000. / context.size) as f32;
    number(&Object::Real(adjustment))?;
    if !adjustment.is_finite()
        || ((-f64::from(adjustment) * context.size / 1000. - context.cursor_after) * context.scale)
            .abs()
            > 0.0001
    {
        return Err("Cannot preserve the following text position precisely".into());
    }
    operations.push(Operation::new(
        "TJ",
        vec![Object::Array(vec![
            Object::string_literal(Vec::new()),
            Object::Real(adjustment),
        ])],
    ));
    Ok(operations)
}

/// Whether `rect` overlaps `other` by more than `slack` across the lines and a
/// tenth of a point along them.
pub(super) fn overlaps(rect: [f64; 4], other: [f64; 4], slack: [f64; 2]) -> bool {
    rect[2].min(other[2]) - rect[0].max(other[0]) > slack[0]
        && rect[3].min(other[3]) - rect[1].max(other[1]) > slack[1]
}

/// Whether `outer` holds `inner` whole.
pub(super) fn holds(outer: [f64; 4], inner: [f64; 4]) -> bool {
    outer[0] <= inner[0] + 0.001
        && outer[1] <= inner[1] + 0.001
        && outer[2] >= inner[2] - 0.001
        && outer[3] >= inner[3] - 0.001
}
