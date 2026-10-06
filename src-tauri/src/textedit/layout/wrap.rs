//! Wrapping an edit onto new lines of its own paragraph.
//!
//! When the text a reader typed no longer fits the room on its line and the
//! room ends at the page edge, the wrap needs to know which runs are lines of
//! the same block. A tagged page says so: the structure element that owns
//! them. A page without tags has its blocks read off its lines (`blocks.rs`),
//! and two things are held to more there, because the geometry can be wrong
//! where the tags cannot: a block of one line does not wrap, having shown no
//! pitch and no measure of its own, and text beside a line that moves refuses
//! the wrap rather than coming apart from it (`layout::wrap_room`). `BUILD.md`,
//! *Wrapping on pages without tags*, measured the rule by what it does to a
//! wrap on pages whose tags give the answer.
//!
//! The edit is laid out at the block's own measure and line pitch, its
//! continuation lines start at the block's left edge, and every line of the
//! block below it moves down by the lines the edit added. What is below a
//! paragraph is another paragraph most of the time, and when the moved lines
//! would land on it, or close the paragraph break above it to less than a
//! blank line, it moves down by the same distance, and so does whatever it
//! would reach in turn (`layout::cascade`): the first gap below with a blank
//! line to spare takes the added lines, and nothing after it moves. On a page
//! with no such gap, each break may give up half its blank line instead
//! (`layout::BREAK_GIVE`), tried only once the whole-break layout is refused. Only a
//! whole block moves, and only one that is entirely below the edited line and
//! whose every show can be drawn somewhere else ([`Plan::beneath`],
//! [`carried`]); text that is not such a block -- untagged, an artifact, fixed
//! where it is, beside the paragraph -- stays, and an edit whose lines would
//! land on it is refused with the reason.
//!
//! Moving is not rewriting. Text the editor keeps read-only -- a bullet it
//! cannot write, a hyphen or a paragraph the tags pin, a link's words, what an
//! ActualText span describes -- goes down with its line or its block like any
//! other, from the bytes it has ([`carried`]). It never flows along the edited
//! line: only text the writer may rewrite does.
//!
//! Text of the block after the edit on its own line flows too, a run at a
//! time: each run after the edit keeps its gap to the one before it and stays
//! on the edit's last line while it fits the block's measure. One that does
//! not fit is cut at a space: the words that fit stay, the rest start the next
//! line at the block's left edge, and the space at the break is written
//! nowhere, so no line starts with one. Each piece is drawn from the run's own
//! glyph bytes and the kerns and spaces between its words (`kerning::words`),
//! in the run's own font and state. A run that cannot be cut -- a grouped run,
//! one with two spaces in a row -- moves to the next line whole.
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
    /// Other blocks the wrap may move down with its own lines, each as all of
    /// its shows: every one of their runs is below the edited line, set the
    /// same way, and one the writer can move. Which of them do move is decided
    /// once the edit's lines are known: the ones those lines, or a block moved
    /// before, would land on (`layout::cascade`).
    pub beneath: Vec<Vec<u32>>,
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

/// Where one piece of a unit goes: its line below the edit's first (0 is the
/// edit's own line) and how far along the line it starts, in the edited run's
/// text-space units. `words` is `None` for the whole unit, drawn from its own
/// show unchanged, and otherwise the range of its words (`kerning::words`) the
/// piece holds.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Piece {
    pub unit: usize,
    pub line: usize,
    pub at: f64,
    pub words: Option<std::ops::Range<usize>>,
}

/// Where each unit goes, as pieces. `None` when something is wider than a whole
/// line of the block: a unit that cannot be cut, or one word of one that can.
/// Every line reaches the block's measure, `plan.first`, the first included:
/// `rest` is that measure less where continuation lines start.
///
/// `line` and `end` are where the replacement's own last line is and where it
/// ends. A unit keeps the gap it had to whatever came before it on the line --
/// the edited run for the first, the unit before it for the rest -- and moves
/// whole while it fits. One that does not fit is cut at a space when `words`
/// has its words' spans (start and end along the line, in the same units as
/// `Unit::start`): as many words as fit stay on the line, the rest start the
/// next one at the block's left edge, and the space at the break is written
/// nowhere. A unit that cannot be cut starts the next line whole, with no gap
/// in front of it.
pub(super) fn flow(
    plan: &Plan,
    words: &[Vec<(f64, f64)>],
    run_advance: f64,
    (line, end): (usize, f64),
) -> Option<Vec<Piece>> {
    let fits = |at: f64, width: f64| at + width <= plan.first + 0.000_001;
    let (mut line, mut cursor, mut previous) = (line, end, run_advance);
    let mut pieces = Vec::new();
    for (unit_index, unit) in plan.after.iter().enumerate() {
        let width = unit.end - unit.start;
        let at = cursor + (unit.start - previous).max(0.);
        let spans = words.get(unit_index).map_or(&[][..], Vec::as_slice);
        if fits(at, width) || spans.is_empty() {
            let at = if fits(at, width) {
                at
            } else if width > plan.rest + 0.000_001 {
                return None;
            } else {
                line += 1;
                plan.start
            };
            pieces.push(Piece {
                unit: unit_index,
                line,
                at,
                words: None,
            });
            cursor = at + width;
            previous = unit.end;
            continue;
        }
        let mut at = cursor + (spans[0].0 - previous).max(0.);
        let (mut next, mut fresh) = (0, false);
        while next < spans.len() {
            let last = (next..spans.len())
                .take_while(|&last| fits(at, spans[last].1 - spans[next].0))
                .last();
            match last {
                Some(last) => {
                    pieces.push(Piece {
                        unit: unit_index,
                        line,
                        at,
                        words: Some(next..last + 1),
                    });
                    cursor = at + spans[last].1 - spans[next].0;
                    next = last + 1;
                    if next < spans.len() {
                        (line, at, fresh) = (line + 1, plan.start, true);
                    }
                }
                None if fresh => return None,
                None => (line, at, fresh) = (line + 1, plan.start, true),
            }
        }
        previous = spans[spans.len() - 1].1;
    }
    Some(pieces)
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
pub(super) const BESIDE: &str = "There is no room for more text on this line, and this paragraph cannot wrap: its lines would move out of line with the text beside them. Shorten the text or reduce the font size.";
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

/// Whether a wrap may draw this show lower, with its line or with its block:
/// its bytes kept and only its position written ([`drawn`]).
///
/// That is a show the writer may rewrite, and also read-only text the scan
/// recorded a position for (`Inspection::held`): a bullet in a font whose
/// bullet the editor cannot write, a hyphen its producer added at a line end
/// and pinned under a Span, a justified paragraph. Moving one changes nothing
/// the editor could not read, because nothing of it is read again: the same
/// operator is drawn from an explicit `Tm`, and the line matrix and cursor are
/// put back for whatever follows. A show inside an inline ActualText span is
/// moved the same way, inside its span, whose ActualText still describes it.
///
/// Not a show under a compound clip, a set of rectangles with holes that no
/// moved rectangle is checked against; not a show with no run at all, a
/// spacer whose span admits one position and one show; and not read-only text
/// with no recorded position (turned, patterned, in a layer, placed artwork).
/// Those stay, and what would land on them or leave them behind is refused.
pub(super) fn carried(page: &Inspection, show: u32) -> bool {
    context_of(page, show).is_some() && !page.compound_run_clips.contains_key(&show)
}

/// The measure a block of one line wraps at: the furthest any line reaches in
/// the page's other tagged blocks of several lines, set the same way as the
/// run. A paragraph its producer broke shows where lines end on this page, and
/// a one-line item of a list ends its lines where the items beside it do.
/// Without it the line ran on to the room it had, which on a page whose only
/// limit is the sheet is the paper's edge, through the right margin.
///
/// `None` when no block has several lines (the run's own has one, or this is
/// not asked), and when the line already
/// reaches past what the others show: then that measure is not this line's.
/// The caller holds the answer to the room the line has, so this narrows a
/// wrap and never widens one.
fn borrowed(
    page: &Inspection,
    run: &Run,
    known: &BTreeMap<u32, &Run>,
    leader: &BTreeMap<u32, u32>,
) -> Option<f64> {
    let tolerance = SAME_LINE_EM * run.size;
    // (a baseline, whether another was seen, the furthest end) per block.
    let mut seen: BTreeMap<ObjectId, (f64, bool, f64)> = BTreeMap::new();
    for (&show, &owner) in &page.blocks {
        let Some(other) = known.get(leader.get(&show).unwrap_or(&show)) else {
            continue;
        };
        if other.text.trim().is_empty() {
            continue;
        }
        let Some(((_, y), end)) = span(run, other) else {
            continue;
        };
        let entry = seen.entry(owner).or_insert((y, false, end));
        entry.1 |= (y - entry.0).abs() > tolerance;
        entry.2 = entry.2.max(end);
    }
    seen.values()
        .filter(|(_, several, _)| *several)
        .map(|(.., end)| *end)
        .fold(None, |far: Option<f64>, end| {
            Some(far.map_or(end, |far| far.max(end)))
        })
        .filter(|far| *far + tolerance >= run.advance)
}

/// The pitch a block of one line wraps at: the least distance between two
/// lines of the nearest of the page's other tagged blocks that has two, among
/// text of the run's own size set the same way. One line shows no pitch, and the editor's own, 1.25
/// em, set a wrapped item's lines closer together than every paragraph around
/// it where the producer spaced them wider. `None` when the page shows none
/// inside [`PITCH_EM`], and the editor's own is used as before.
fn borrowed_pitch(
    page: &Inspection,
    run: &Run,
    known: &BTreeMap<u32, &Run>,
    leader: &BTreeMap<u32, u32>,
) -> Option<f64> {
    let tolerance = SAME_LINE_EM * run.size;
    let mut baselines: BTreeMap<ObjectId, Vec<f64>> = BTreeMap::new();
    for (&show, &owner) in &page.blocks {
        let Some(other) = known.get(leader.get(&show).unwrap_or(&show)) else {
            continue;
        };
        if other.text.trim().is_empty() || other.size != run.size {
            continue;
        }
        if let Some(((_, y), _)) = span(run, other) {
            baselines.entry(owner).or_default().push(y);
        }
    }
    // The nearest such block's own: the items beside a list item, not a
    // heading's or a footnote's further away.
    baselines
        .values_mut()
        .filter_map(|lines| {
            lines.sort_by(f64::total_cmp);
            let pitch = lines
                .windows(2)
                .map(|pair| pair[1] - pair[0])
                .filter(|gap| *gap > tolerance)
                .filter(|gap| (PITCH_EM.0 * run.size..=PITCH_EM.1 * run.size).contains(gap))
                .min_by(f64::total_cmp)?;
            let away = lines.iter().map(|y| y.abs()).min_by(f64::total_cmp)?;
            Some((away, pitch))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, pitch)| pitch)
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
    // Where the label of a list item starts, when the block is one: the
    // number or the bullet, set before the item's words on its first line.
    let mut label = f64::NEG_INFINITY;
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
            if !carried(page, show) {
                return Err(Refused::Blocked(UNMOVABLE));
            }
            below.push(show);
        }
        if !other.text.trim().is_empty() {
            lines.push((y, x, end));
            if page.labels.contains(&show) {
                label = label.max(x);
            }
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
        borrowed_pitch(page, run, &known, &leader).unwrap_or(run.size * 1.25)
    };
    if !(PITCH_EM.0 * run.size..=PITCH_EM.1 * run.size).contains(&pitch) {
        return Err(Refused::NotApplicable);
    }
    let top = lines
        .iter()
        .map(|(y, ..)| *y)
        .fold(f64::NEG_INFINITY, f64::max);
    let several = lines.iter().any(|(y, ..)| (*y - top).abs() > tolerance);
    // A block read off the geometry is a block because its lines agree with
    // each other; one line has shown neither a pitch nor a measure.
    if !several && blocks::geometric_page(page) {
        return Err(Refused::NotApplicable);
    }
    // The block's measure is the furthest any of its lines reaches. A block of
    // one line has none of its own and borrows the page's (`borrowed`); where
    // the page shows none either, its first line keeps the room it had.
    let far = if several {
        lines
            .iter()
            .map(|(.., end)| *end)
            .fold(run.advance, f64::max)
            .min(first)
    } else {
        borrowed(page, run, &known, &leader).map_or(first, |measure| measure.min(first))
    };
    // Continuation lines start where the block's lines after its first do:
    // that is the body margin under a first-line indent, and the hanging
    // position of a list item. An item of one line has no such line to read
    // it from, and its left edge is its label's: its new lines start under
    // its words instead, the first text after the label, as its producer
    // hangs them. Without this a one-line item wrapped under its own number.
    let start = lines
        .iter()
        .filter(|(y, ..)| !several || *y < top - tolerance)
        .map(|(_, x, _)| *x)
        .filter(|x| several || *x > label)
        .fold(f64::INFINITY, f64::min);
    let rest = far - start;
    if !start.is_finite() || !rest.is_finite() || rest <= 0. || far <= 0. {
        return Err(Refused::NotApplicable);
    }
    let beneath = beneath(page, run, &known, &leader);
    Ok(Plan {
        first: far,
        start,
        rest,
        pitch,
        below,
        after,
        beneath,
    })
}

/// The blocks that could move down with the edited one, each as its shows in
/// stream order: see [`Plan::beneath`]. A block with one show the writer
/// cannot move, one run above the bottom of the edited line or one run turned
/// against it is not offered, so text it would be in the way of stays where it
/// is and refuses the wrap as before. The edited block is never offered: its
/// own run is on the edited line.
fn beneath(
    page: &Inspection,
    run: &Run,
    known: &BTreeMap<u32, &Run>,
    leader: &BTreeMap<u32, u32>,
) -> Vec<Vec<u32>> {
    let tolerance = SAME_LINE_EM * run.size;
    let mut blocks: BTreeMap<ObjectId, Option<Vec<u32>>> = BTreeMap::new();
    for (&show, &owner) in &page.blocks {
        let entry = blocks.entry(owner).or_insert_with(|| Some(Vec::new()));
        let movable = carried(page, show)
            && known
                .get(leader.get(&show).unwrap_or(&show))
                .and_then(|other| span(run, other))
                .is_some_and(|((_, y), _)| y < -tolerance);
        match (entry.as_mut(), movable) {
            (Some(shows), true) => shows.push(show),
            _ => *entry = None,
        }
    }
    blocks.into_values().flatten().collect()
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
    drawn(page, show, &[(offset, None)])
}

/// A painted path drawn `offset` further along its page, in the page's
/// original user space: its first operator preceded by a saved state and a
/// translation, its last one followed by the restore. The path's own operators
/// are kept byte for byte, and nothing after it sees the translation. The
/// offset is taken through the inverse of the path's `transform`, as a show's
/// is in [`drawn`].
pub(super) fn translated(
    page: &Inspection,
    (first, last): (usize, usize),
    transform: [f64; 6],
    offset: (f64, f64),
) -> Result<Vec<(u32, Vec<Operation>)>, String> {
    let operation = |index: usize| {
        page.content
            .operations
            .get(index)
            .cloned()
            .ok_or_else(|| "painted path no longer exists".to_string())
    };
    let [a, b, c, d, ..] = transform;
    let det = a * d - b * c;
    if det.abs() < 1e-12 || first >= last {
        return Err("painted path cannot be moved".into());
    }
    let local = (
        (d * offset.0 - c * offset.1) / det,
        (a * offset.1 - b * offset.0) / det,
    );
    let number = |value: f64| Object::Real(value as f32);
    Ok(vec![
        (
            u32::try_from(first).map_err(|_| "painted path no longer exists")?,
            vec![
                Operation::new("q", vec![]),
                Operation::new(
                    "cm",
                    vec![
                        number(1.),
                        number(0.),
                        number(0.),
                        number(1.),
                        number(local.0),
                        number(local.1),
                    ],
                ),
                operation(first)?,
            ],
        ),
        (
            u32::try_from(last).map_err(|_| "painted path no longer exists")?,
            vec![operation(last)?, Operation::new("Q", vec![])],
        ),
    ])
}

/// One piece of a show [`drawn`] writes: how far from the show's source origin,
/// and the items to draw there, `None` for the show's own operator.
pub(super) type Drawn<'a> = ((f64, f64), Option<&'a [Object]>);

/// [`lowered`] for a show drawn in pieces: each piece is its own `Tm`, placed
/// `offset` from the show's source origin, and either the show's own operator
/// (`None`) or a `TJ` of the items given, which are the show's own glyph bytes
/// and kerns for some of its words (`kerning::words`). The state every piece
/// is drawn in -- font, spacing, colour -- is the show's, since nothing between
/// them sets any. One restoration follows the last piece.
pub(super) fn drawn(
    page: &Inspection,
    show: u32,
    pieces: &[Drawn<'_>],
) -> Result<Vec<Operation>, String> {
    let context = context_of(page, show).ok_or("missing text layout context")?;
    let operation = page
        .content
        .operations
        .get(show as usize)
        .ok_or("text run no longer exists")?;
    let [a, b, c, d, ..] = context.transform;
    let det = a * d - b * c;
    let mut operations = Vec::new();
    for (offset, items) in pieces {
        let local = (
            (d * offset.0 - c * offset.1) / det,
            (a * offset.1 - b * offset.0) / det,
        );
        if !local.0.is_finite() || !local.1.is_finite() {
            return Err("text position exceeds its limit".into());
        }
        let mut matrix = context.shown;
        shift_position(&mut matrix, local.0, local.1)?;
        let show_op = match items {
            Some(items) => Operation::new("TJ", vec![Object::Array(items.to_vec())]),
            None => {
                let mut show_op = operation.clone();
                // The shown matrix already stands after a leading TJ number,
                // which says where the run starts; keeping it would move the
                // run twice.
                if page.leads.contains_key(&show) {
                    let Some(Object::Array(items)) = show_op.operands.first_mut() else {
                        return Err("invalid text patch".into());
                    };
                    items.remove(0);
                }
                show_op
            }
        };
        operations.push(numeric("Tm", &matrix));
        operations.push(show_op);
    }
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
