//! Local text layout. Explicit text-state restoration leaves later content fixed.
use super::*;
use lopdf::content::Operation;

mod wrap;

pub(super) use wrap::CONFLICT as WRAP_CONFLICT;

pub(super) struct Context {
    // The operator that last set the line matrix (BT or Tm) before the run,
    // and the leading in effect there; see `restore_line`.
    pub line_origin: (usize, f64),
    pub shown: [f64; 6],
    pub cursor_after: f64,
    pub clip: Option<[f64; 4]>,
    pub regions: Vec<clipping::Region>,
    // Half the stroke width around text drawn in render mode 1 or 2, in page
    // units; a replacement inherits the mode and reaches as far.
    pub stroke: f64,
    // The Tf size in force at this show, and the horizontal scale of its
    // composed page matrix. `push` needs both: a TJ displacement is written in
    // thousandths of an em, so turning a distance in page points into one takes
    // the show's own size and scale rather than the edited run's.
    pub size: f64,
    pub scale: f64,
    // The page transform in force at this show (the CTM), which a wrap needs to
    // turn a distance measured down the edited run's page into a `Tm` operand
    // in this show's own space.
    pub transform: [f64; 6],
}

pub(super) struct Prepared {
    pub operations: Vec<Operation>,
    pub fallback: Option<fonts::fallback::Font>,
    pub name: Vec<u8>,
    pub label: String,
    pub rect: [f32; 4],
    pub lines: usize,
    /// The shows this edit pushes along its line, and how far each has to end
    /// up from where its source put it, in page points along the text axis.
    ///
    /// The distance is the total, not this edit's share: `push` builds the
    /// operand from the source's own array, so an operand written for a second
    /// edit on the same line replaces the first edit's rather than adding to it.
    pub moved: Vec<(u32, f64)>,
    /// Every show on the line this edit pushes, `moved` together with the ones
    /// the batch replaces itself. `write` tells each of those where it ended up
    /// so that its own `prepare` starts from there.
    pub line: BTreeSet<u32>,
    /// How far this edit pushes the text after it, page points, again a total.
    pub shift: f64,
    /// The box together with every pushed run's new hit rectangle, so that the
    /// preview crop shows the reader the text that moved as well as their own.
    pub extent: [f32; 4],
    /// The shows a wrap moves down its paragraph, each with the operations
    /// that replace it (`wrap::lowered`). Empty unless this edit wraps.
    pub lowered: Vec<(u32, Vec<Operation>)>,
    /// Every discovered run this edit moves, pushed along its line or down its
    /// paragraph, and its hit rectangle where it ends up, so that the editor
    /// can outline the text where the reader now sees it.
    pub placed: Vec<(u32, [f32; 4])>,
}

/// Where an edit sits by the time it is written, and who else is being written
/// with it.
#[derive(Default)]
pub(super) struct Placement {
    /// How far an earlier edit on this same line has already pushed this run,
    /// in page points along its text axis. Its own replacement is written that
    /// much further along, and so is everything it pushes in turn.
    pub inherited: f64,
    /// The other shows this batch replaces on this page. They are written by
    /// their own `prepare`, which is given `inherited` instead, so a push never
    /// hands one of them a displacement that its own expansion would discard.
    pub edited: BTreeSet<u32>,
}

fn numeric(op: &str, values: &[f64]) -> Operation {
    Operation::new(
        op,
        values
            .iter()
            .map(|value| Object::Real(*value as f32))
            .collect(),
    )
}

// The line matrix a following Td, TD or T* starts from, put back exactly.
//
// A reader keeps the text matrix and the line position apart and adds each
// line move to the position in single precision: PDFium's CPDF_AllStates holds
// `text_matrix_` and `text_line_pos_` as floats, `MoveTextPoint` does
// `text_line_pos_ += point` and a glyph lands at `text_matrix_.Transform(pos)`.
// One Tm carrying the accumulated origin is the same point in exact arithmetic
// and a different one in floats, because float addition is not associative:
// after `56.8 724.6 Td ... 451 0 Td ... -451 -23 Td` the source's lines start at
// 56.799988 and a restored `Tm` put them at 56.8, which moved glyphs by a pixel
// wherever that crossed a rounding boundary. So the restore replays what set
// the line: the Tm (or the identity a BT sets) and every line move after it,
// as the same operands, with the leading a replayed T* read reinstated first.
// Whatever reads the page then accumulates the same operations in the same
// order, in whatever precision it uses.
fn restore_line(
    operations: &[Operation],
    (origin, leading): (usize, f64),
    run: usize,
) -> Result<Vec<Operation>, String> {
    let start = operations.get(origin).ok_or("missing text line origin")?;
    let moves = operations
        .get(origin + 1..run)
        .ok_or("missing text line origin")?;
    let mut restored = Vec::new();
    if moves.iter().any(|op| op.operator == "T*") {
        restored.push(numeric("TL", &[leading]));
    }
    restored.push(match start.operator.as_str() {
        "Tm" => start.clone(),
        "BT" => Operation::new("Tm", [1, 0, 0, 1, 0, 0].map(Object::Integer).to_vec()),
        _ => return Err("missing text line origin".into()),
    });
    for op in moves {
        match op.operator.as_str() {
            "Td" | "TD" | "T*" | "TL" => restored.push(op.clone()),
            "Tm" | "BT" | "ET" | "'" | "\"" => {
                return Err("Cannot preserve the following line origin precisely".into())
            }
            _ => {}
        }
    }
    Ok(restored)
}

/// What stops the editing box from growing, and so what to say when the text
/// the reader typed no longer fits the room after the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Room {
    /// Other text on the same line: the next run, preserved read-only text or
    /// a form field.
    Line,
    /// The line's own text was pushed along until it met text the editor may
    /// not move -- read-only text, a preserved form's text, a form field.
    Fixed,
    /// A painted rectangle, a painted path or an image, which only the push
    /// reads (see `Inspection::graphics`).
    Drawn,
    /// The edge of the page as the reader sees it.
    Page,
    /// A clip the document has in force over the space after the run.
    Clip,
}

impl Room {
    /// Every one of these says the line is full rather than naming a box the
    /// reader never set, which is the whole point of growing it for them.
    fn refusal(self) -> &'static str {
        match self {
            Room::Line => "There is no room for more text on this line: other text follows it. Shorten the text or reduce the font size.",
            Room::Fixed => "There is no room for more text on this line: the text after it cannot be moved. Shorten the text or reduce the font size.",
            Room::Drawn => "There is no room for more text on this line: a picture or a drawing follows it. Shorten the text or reduce the font size.",
            Room::Page => "There is no room for more text on this line: it reaches the edge of the page. Shorten the text or reduce the font size.",
            Room::Clip => "There is no room for more text on this line: the document clips the space after it. Shorten the text or reduce the font size.",
        }
    }
}

/// One entry of the page's hit list: a rectangle, and what is at it.
enum Around<'a> {
    /// A discovered run the writer may rewrite, so a candidate for being pushed
    /// along its line. Whether it really is one is [`pushable`]'s question.
    Run(u32, &'a [f32; 4]),
    /// Text the editor keeps byte for byte: read-only text, and the text a
    /// preserved Form XObject draws.
    Fixed(&'a [f32; 4]),
}

/// Every hit rectangle on the page a replacement must not land on: the other
/// runs, the read-only text preserved beside them, and the form fields.
///
/// The growth limit and the collision check in `prepare` read this one list, so
/// the box cannot be grown into something the check would then refuse it for.
/// Two lists would be two answers to one question, and the drift between them
/// would show up as a refusal the reader cannot act on. The push reads it too,
/// and its extra population -- `page.graphics` -- is deliberately not in here:
/// see the field's own comment for why the box may grow over a picture that the
/// push may not put somebody else's text on.
fn obstacles<'a>(page: &'a Inspection, run: &'a Run) -> impl Iterator<Item = Around<'a>> {
    page.runs
        .runs
        .iter()
        .filter(move |other| other.operator != run.operator && !other.text.trim().is_empty())
        .map(|other| Around::Run(other.operator, &other.display_rect))
        .chain(
            page.preserved
                .iter()
                .filter(|other| !other.text.trim().is_empty())
                .map(|other| Around::Fixed(&other.display_rect)),
        )
        .chain(page.form_text_bounds.iter().map(Around::Fixed))
}

impl Around<'_> {
    fn rect(&self) -> &[f32; 4] {
        match self {
            Around::Run(_, rect) | Around::Fixed(rect) => rect,
        }
    }
}

/// How wide the editing box may be, and what stops it there.
///
/// Pure geometry in the displayed page's own space, so that the rule can be
/// stated and tested without a document: `zero` and `wide` are the box's display
/// rectangle at width 0 and at width `probe`, `page` is the displayed page's
/// size, `clip` the clip in force, `own` the run's own hit rectangle and
/// `obstacles` every other hit rectangle on the page. `width` is the box the
/// reader already has, and the answer is never smaller than it -- growth adds
/// room, it never takes any away.
///
/// `probe` is a box width and not a fixed 1, because a display rectangle is
/// `f32` and a run may be set at any scale: `Tf 1` under a matrix of 11 and a
/// `0.001` page transform are both real. The caller picks a probe worth about a
/// page point, so the displacement the direction and the rate are read from is
/// large against `f32`'s spacing at a page coordinate whatever the run's own
/// scale is.
///
/// The rule: **the box grows along the run's own text axis until it meets the
/// first thing on its line, and no further than the page's edge or the clip in
/// force.**
///
/// - The *direction* is read off the two mapped rectangles rather than derived
///   from the page's quarter turns. An editable run's matrix is a quarter-turn
///   multiple, so exactly one display edge moves with the width; which one is a
///   measurement here, not a table, because a second hand-written table of
///   turns is what `text::to_device`'s own comment warns about.
/// - A *neighbour* counts as on this line when it shares more than half of the
///   shorter of its own cross-axis span and the box's ([`Axis::beside`]), and it
///   stops the box at its near edge, so the box comes up to it rather than
///   over it. The box's span is its own together with the run's hit rectangle,
///   whose glyphs may reach above and below the box.
/// - The *page* is the displayed page the box is already refused for.
///
/// What it does not do is grow to the **left**. The writer places a replacement
/// at the run's own origin and replays the source's own positioning to get
/// there (`source_items`, `restore_line`), so starting a line further left is a
/// move of the line rather than a wider box for it -- that is reflow, and it is
/// the next increment. A centred or right-aligned line therefore grows into
/// whatever follows it, exactly as a left-aligned one does, and is refused when
/// that is not enough even though the space it wants is sitting to its left.
pub(super) fn room(
    edges: ([f64; 4], [f64; 4]),
    probe: f64,
    page: [f64; 2],
    clip: Option<[f64; 4]>,
    own: [f64; 4],
    obstacles: impl Iterator<Item = [f64; 4]>,
    width: f64,
) -> (f64, Room) {
    let Some(axis) = Axis::of(edges, probe) else {
        return (width, Room::Page);
    };
    let (mut free, mut stop) = axis.ahead([0., 0., page[0], page[1]], clip);
    let band = axis.band(edges.0, own);
    for other in obstacles {
        if !axis.beside(band, other) {
            continue;
        }
        let near = axis.at(axis.edge(other, false));
        if near + 0.000_000_001 >= width && near < free {
            free = near;
            stop = Room::Line;
        }
    }
    (free.max(width), stop)
}

/// The direction the editing box grows in, and what a coordinate along it is
/// worth as a box width.
///
/// One measurement, shared by the box's own limit ([`room`]) and by how far the
/// line's text may be pushed ([`reach`]), because a second reading of the same
/// direction is a second opinion about it.
struct Axis {
    axis: usize,
    sign: f64,
    rate: f64,
    lead: f64,
}

impl Axis {
    fn of((zero, wide): ([f64; 4], [f64; 4]), probe: f64) -> Option<Self> {
        let moved = [
            (0_usize, 1_f64, wide[2] - zero[2]),
            (0, -1., zero[0] - wide[0]),
            (1, 1., wide[3] - zero[3]),
            (1, -1., zero[1] - wide[1]),
        ];
        let &(axis, sign, moved) = moved
            .iter()
            .filter(|(_, _, moved)| *moved > 0. && moved.is_finite())
            .max_by(|a, b| a.2.total_cmp(&b.2))?;
        // Positive and finite: the filter above took the displacement and
        // `probe` is a positive finite width, so there is no second guard here
        // to go stale.
        let lead = zero[if sign > 0. { axis + 2 } else { axis }];
        Some(Axis {
            axis,
            sign,
            rate: moved / probe,
            lead,
        })
    }

    /// A rectangle's edge along the growth axis: the one furthest along it when
    /// `ahead`, the one facing us when not. Which array index that is depends on
    /// the direction, and saying so once is what keeps the callers below from
    /// each having their own opinion about it.
    fn edge(&self, rect: [f64; 4], ahead: bool) -> f64 {
        rect[if (self.sign > 0.) == ahead {
            self.axis + 2
        } else {
            self.axis
        }]
    }

    /// A limit coordinate, as a box width.
    fn at(&self, limit: f64) -> f64 {
        self.sign * (limit - self.lead) / self.rate
    }

    fn cross(&self) -> usize {
        1 - self.axis
    }

    /// The span across the axis that decides what counts as being on this line:
    /// the box's own together with a hit rectangle whose glyphs may reach above
    /// and below it.
    fn band(&self, boxed: [f64; 4], own: [f64; 4]) -> (f64, f64) {
        let cross = self.cross();
        (
            boxed[cross].min(own[cross]),
            boxed[cross + 2].max(own[cross + 2]),
        )
    }

    /// Whether `rect` is on this line: across the line, it shares more than
    /// half of whichever of the two spans is shorter.
    ///
    /// Not "overlaps at all". Hit rectangles are em boxes, a quarter em below
    /// the baseline and a whole em above it, so at ordinary leading the next
    /// line's box reaches a point into this one's: 12 pt on a 14 pt pitch does,
    /// and Word's single spacing is about 1.2 em. Counting that as this line
    /// stopped the box at the next line's runs and pushed them along with this
    /// one. A superscript, a larger word or a run whose own glyphs reach below
    /// the box all share more than half of the shorter span; the next line
    /// shares a sliver. Whether new ink actually meets the next line's glyphs
    /// is the collision check's question, not this one's.
    fn beside(&self, (low, high): (f64, f64), rect: [f64; 4]) -> bool {
        let cross = self.cross();
        let shared = rect[cross + 2].min(high) - rect[cross].max(low);
        let shorter = (rect[cross + 2] - rect[cross]).min(high - low);
        shared > 0.1 && shared > shorter / 2.
    }

    /// How far along the axis the page, and a clip if there is one, allow.
    fn ahead(&self, page: [f64; 4], clip: Option<[f64; 4]>) -> (f64, Room) {
        let mut free = self.at(self.edge(page, true));
        let mut stop = Room::Page;
        if let Some(clip) = clip {
            let limit = self.at(self.edge(clip, true));
            if limit < free {
                free = limit;
                stop = Room::Clip;
            }
        }
        (free, stop)
    }
}

/// How far the text after a run may be pushed along its line, and what stops it.
///
/// `pushed` is every run the writer would rewrite, as its hit rectangle and the
/// clip in force over it; `fixed` is everything on the page that stays where it
/// is, each with what it is. The answer is the **smallest** room any one of them
/// has, because they all move together: a line half pushed is worse than one not
/// pushed, so the whole set is refused or none of it is.
///
/// Each run is measured against what lies ahead of **it** and across **its own**
/// span, not the edited run's: a line is not one rectangle, and a run further
/// along it may be taller, may sit under a tighter clip, and certainly has
/// different things in front of it. Anything already behind a run's leading edge
/// is skipped rather than counted as no room at all, because a rectangle behind
/// it is one the push moves away from; one that overlaps it stops it dead,
/// unless it is a drawing that starts behind the run and so holds it, which
/// allows as far as its own far edge.
fn reach(
    axis: &Axis,
    page: [f64; 2],
    pushed: &[([f64; 4], Option<[f64; 4]>)],
    fixed: &[([f64; 4], Room)],
) -> (f64, Room) {
    let sheet = [0., 0., page[0], page[1]];
    let mut shift = f64::INFINITY;
    let mut stop = Room::Page;
    for (rect, clip) in pushed {
        let far = axis.at(axis.edge(*rect, true));
        let (limit, kind) = axis.ahead(sheet, *clip);
        let mut best = (limit - far, kind);
        let span = (rect[axis.cross()], rect[axis.cross() + 2]);
        for (other, kind) in fixed {
            if !axis.beside(span, *other) || axis.at(axis.edge(*other, true)) <= far {
                continue;
            }
            // A drawing that starts behind the run holds it -- a frame, a
            // background -- and the run may move as far as it still does; the
            // wrap asks the same of the lines it moves. Anything else that
            // overlaps the run leaves it no room.
            let near = axis.at(axis.edge(*other, false));
            let room = if *kind == Room::Drawn && near <= axis.at(axis.edge(*rect, false)) {
                axis.at(axis.edge(*other, true)) - far
            } else {
                (near - far).max(0.)
            };
            if room < best.0 {
                best = (room, *kind);
            }
        }
        if best.0 < shift {
            (shift, stop) = best;
        }
    }
    (shift.max(0.), stop)
}

/// What the box may reach, at whose expense, and what is in the way meanwhile.
struct Free {
    /// The widest the box may be, in the run's own text-space units.
    ceiling: f64,
    /// What stops it there.
    stop: Room,
    /// The zero point the distance the line is pushed by is measured from: the
    /// text has to end past this before anything moves, and everything that
    /// moves moves by however far past it the text ends.
    ///
    /// It is **not** the same as the room the box had without pushing anything,
    /// and the difference is a whole neighbouring run wide. `room` skips a
    /// neighbour that starts inside the box the reader already has, because the
    /// box may never shrink -- and the box the editor opens is the run's own
    /// advance rounded *up*, so a run set flush against the end of this one
    /// starts a rounding inside it and is skipped every time. Measuring the
    /// push from `room` then moved that run by the distance the *next* one
    /// needed, which is the width of a word too little: found in a Word minute
    /// where `7:00 p.m.` is three runs, and poppler read the saved line back as
    /// `7:0called 0 thp.m.`
    ///
    /// It is also never before the run's **own** far edge, and that second half
    /// cost a run its own unchanged text before it was there: on a ReportLab
    /// page whose next run begins inside this one's ink, the source already
    /// overlaps, so measuring from the neighbour alone turned an *identity*
    /// edit into a push of a few points and refused it for a wall the reader
    /// was nowhere near. A push may be as long as the text grew and no longer;
    /// an overlap the document already had is the document's.
    from: f64,
    /// How far the line allows with every run the push moves out of the way:
    /// the page, the clip, and whatever the push stops at. A wrap flows those
    /// runs after the edit instead of pushing them, which gives the edit the
    /// line up to here. Without a push it is `ceiling`.
    whole: f64,
    /// Every show the push rewrites -- the discovered runs on the line and
    /// their grouped members.
    line: BTreeSet<u32>,
    /// Those, and every other show the push may legitimately drag along the
    /// line through the text cursor: a blank run, a grouped member of one.
    carry: BTreeSet<u32>,
    /// Every hit rectangle as the collision check must see it: a run the push
    /// moves is drawn where an earlier edit on this line left it, and is left
    /// out of the check entirely while this edit is moving it further.
    ///
    /// The flag is membership of `line`, not whether the writer *could* rewrite
    /// that run. A run it could rewrite and is not moving -- one behind the
    /// edit, one past something fixed -- stays in the way, and marking it by
    /// what the writer is capable of instead exempted it from the check while
    /// leaving it exactly where it was.
    hits: Vec<([f64; 4], bool)>,
    /// The run's own hit rectangle, where an earlier edit on this line left it.
    /// Ink inside it is not a collision: that is where the source's own glyphs
    /// already are.
    own: [f64; 4],
}

/// Whether the writer may rewrite this run's shows at all, which is what makes
/// it a candidate for being pushed along its line.
///
/// A run this batch also replaces counts: it does move, because its own
/// `prepare` places it `inherited` further along, it is only not given a
/// displacement here. What is refused is anything whose show the writer cannot
/// simply open with a displacement -- a show inside an ActualText span, whose
/// logical text the writer owns and whose grammar admits one position and one
/// space-only show; a run under a compound clip, which is a set of rectangles
/// with holes rather than an edge to measure a shift against; a run drawn
/// before this one, whose cursor this edit has already gone past.
fn pushable(page: &Inspection, run: &Run, other: u32) -> bool {
    let shows = shows_of(page, other);
    other > run.operator
        && shows.iter().all(|show| page.contexts.contains_key(show))
        && shows.iter().all(|show| {
            !page.actual_text.contains_key(show) && !page.compound_run_clips.contains_key(show)
        })
}

/// A discovered run's own show and the shows grouped into it, which move
/// together because each grouped member carries its own `Td`.
fn shows_of(page: &Inspection, operator: u32) -> Vec<u32> {
    std::iter::once(operator)
        .chain(
            page.groups
                .get(&operator)
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .copied(),
        )
        .collect()
}

/// [`room`] and [`reach`] for a discovered run, in the text-space units the box
/// width uses.
///
/// `room` is asked twice over the same list: once with every hit rectangle,
/// which is the box's own limit and exactly the answer it gave before a line
/// could be pushed, and once with only the rectangles that stay where they are.
/// Those two are a partition of one list rather than two opinions about it, and
/// the gap between the answers is the whole question -- if the nearer one is
/// the answer that counted everything, then the first thing in the way is
/// something the writer may push along, and [`reach`] says how far. Equal
/// answers do not prove the opposite: `room` skips a run starting inside the
/// box, so the walk over the line decides.
///
/// The push stops at the first thing that cannot move: something fixed between
/// two runs separates them, and the run beyond it never has to give way.
///
/// `inherited` is how far an earlier edit on this same line has already pushed
/// this run, in the run's own text-space units. It moves the run's own box, and
/// it moved every run after it on the line by the same amount, so the geometry
/// between them is unchanged and only the distance to the page, the clip and
/// everything fixed has shrunk.
fn free_width(
    page: &Inspection,
    run: &Run,
    context: &Context,
    geometry: &crate::pagetree::DisplayedPage,
    display: impl Fn([f64; 4]) -> [f32; 4],
    (xscale, width, height): (f64, f64, f64),
    (inherited, grow): (f64, bool),
) -> Free {
    let shape = |w: f64| {
        text_bounds(
            run.matrix,
            [inherited, run.size - height, inherited + w, run.size],
        )
    };
    let mapped = |w: f64| display(shape(w)).map(f64::from);
    // About a page point of box, whatever the run's own scale; see `room`.
    let probe = 1. / xscale.max(1e-9);
    let sheet = [f64::from(geometry.width), f64::from(geometry.height)];
    let clip = context.clip.map(|clip| display(clip).map(f64::from));
    let edges = (mapped(0.), mapped(probe));
    // The same displacement, in the displayed page's own space, so that the
    // rectangles of the runs it also moved can be put where they now are.
    let offset = {
        let corner = |x: f64| {
            display(text_bounds(run.matrix, [x, run.size - height, x, run.size])).map(f64::from)
        };
        let (here, origin) = (corner(inherited), corner(0.));
        [here[0] - origin[0], here[1] - origin[1]]
    };
    let shifted = |rect: &[f32; 4]| {
        let rect = rect.map(f64::from);
        [
            rect[0] + offset[0],
            rect[1] + offset[1],
            rect[2] + offset[0],
            rect[3] + offset[1],
        ]
    };
    // One walk of the one list: each rectangle where it is now, and the id of
    // the run at it when the writer could push that run along.
    let hits: Vec<([f64; 4], Option<u32>)> = obstacles(page, run)
        .map(|other| match other {
            Around::Run(id, rect) if grow && pushable(page, run, id) => (shifted(rect), Some(id)),
            other => (other.rect().map(f64::from), None),
        })
        .collect();
    let own = shifted(&run.display_rect);
    let boxed = |ceiling: f64,
                 stop: Room,
                 line: BTreeSet<u32>,
                 carry: BTreeSet<u32>,
                 from: f64,
                 whole: f64| Free {
        ceiling,
        stop,
        from,
        whole,
        carry,
        hits: hits
            .iter()
            .map(|(rect, id)| (*rect, id.is_some_and(|id| line.contains(&id))))
            .collect(),
        line,
        own,
    };
    if !grow {
        return boxed(
            width,
            Room::Page,
            BTreeSet::new(),
            BTreeSet::new(),
            width,
            width,
        );
    }
    let (free, stop) = room(
        edges,
        probe,
        sheet,
        clip,
        own,
        hits.iter().map(|(rect, _)| *rect),
        width,
    );
    // A compound clip is a set of rectangles with holes, not an edge, so the
    // box it would produce is handed to the region rather than reduced to one
    // coordinate: growth is given up whenever a region refuses that box.
    if free > width
        && context
            .regions
            .iter()
            .any(|region| region.contains(shape(free)).is_err())
    {
        return boxed(
            width,
            Room::Clip,
            BTreeSet::new(),
            BTreeSet::new(),
            width,
            width,
        );
    }
    let nothing = |stop| boxed(free, stop, BTreeSet::new(), BTreeSet::new(), free, free);
    let Some(axis) = Axis::of(edges, probe) else {
        return nothing(stop);
    };
    let (hard, _) = room(
        edges,
        probe,
        sheet,
        clip,
        own,
        hits.iter()
            .filter(|(_, id)| id.is_none())
            .map(|(rect, _)| *rect),
        width,
    );
    // `free` reaching `hard` does not mean nothing movable is in the way:
    // `room` skips a run that starts inside the box, and a run set flush
    // against this one does (see `Free::from`). With nothing movable after it
    // the two limits agree, and stopping here refused the edit at the page edge
    // with that run as the thing in the way: half of all lines whose rest
    // flowed after a wrap, and every such line on an untagged page. The walk
    // below finds it either way, and finds nothing when there is nothing.
    let band = axis.band(edges.0, own);
    // Stream order alone does not say which side of the run a show is drawn on.
    let own_far = axis.at(axis.edge(own, true));
    let mut pushed = Vec::new();
    let (mut line, mut carry) = (BTreeSet::new(), BTreeSet::new());
    for other in &page.runs.runs {
        if !pushable(page, run, other.operator) {
            continue;
        }
        if other.text.trim().is_empty() {
            // Invisible, so it is in no hit list and stops nothing; it still
            // rides the cursor when the run before it on the line moves.
            carry.extend(shows_of(page, other.operator));
            continue;
        }
        let rect = shifted(&other.display_rect);
        let near = axis.at(axis.edge(rect, false));
        if !axis.beside(band, rect) || near >= hard || near + 0.1 < own_far {
            continue;
        }
        pushed.push((
            rect,
            page.contexts[&other.operator]
                .clip
                .map(|clip| display(clip).map(f64::from)),
        ));
        line.extend(shows_of(page, other.operator));
        carry.extend(shows_of(page, other.operator));
    }
    if pushed.is_empty() {
        return nothing(stop);
    }
    // What stays put, for the runs that do not: everything fixed, every run the
    // push leaves behind, and the page's own graphics, which only the push reads.
    let mut fixed: Vec<([f64; 4], Room)> = page
        .graphics
        .iter()
        .map(|rect| (rect.map(f64::from), Room::Drawn))
        .collect();
    fixed.extend(
        hits.iter()
            .zip(obstacles(page, run))
            .filter(|((_, id), _)| id.is_none_or(|id| !line.contains(&id)))
            .map(|((rect, _), other)| {
                (
                    *rect,
                    match other {
                        Around::Run(..) => Room::Line,
                        Around::Fixed(_) => Room::Fixed,
                    },
                )
            }),
    );
    let (shift, pushed_stop) = reach(&axis, sheet, &pushed, &fixed);
    if shift <= 0. {
        return nothing(pushed_stop);
    }
    // Measured from the nearest run the push moves, and bounded by how far that
    // set may go -- but never below what the box could already reach on its own,
    // because a replacement that fits `free` pushes nothing and must go on being
    // accepted exactly as it was.
    // ... and never before the box that arrived, which is what keeps an edit
    // that merely fills its own box from pushing anything. The box the editor
    // opens is the run's advance rounded **up** to the thousandth, so text that
    // fills it measures a hair past the run's own far edge; read as growth,
    // that hair refused a run its own unchanged text wherever the line could
    // not move -- a one-character run on page 104 of the ReportLab guide, found
    // by the corpus comparison and by nothing else. The run's own far edge was
    // the bound here until then, and it is subsumed: a grown box is never
    // narrower than the advance it was opened from, and a box the reader sized
    // never grows at all.
    let from = pushed
        .iter()
        .map(|(rect, _)| axis.at(axis.edge(*rect, false)))
        .fold(f64::INFINITY, f64::min)
        .max(width);
    // The pushed runs are everything the writer may move between the run and
    // the first thing it may not, so with them gone the box reaches that thing,
    // the page or the clip: `hard`, the same rule `free` is, over the same list
    // less the runs that leave. Drawings are no more in the box's way here than
    // they are anywhere else (see `obstacles`).
    boxed(
        // `free` past `from` is `room` having skipped a run that starts inside
        // the box: text reaching it would land on that run, which only the push
        // moves, so the box that stands without pushing ends at `from`.
        (from + shift).max(free.min(from)),
        pushed_stop,
        line,
        carry,
        from,
        hard.max(free),
    )
}

/// Which of the shows after an edit have to be given a displacement of their
/// own, walking the stream the way a reader executes it.
///
/// A displacement moves the text cursor, and every show that follows without
/// setting a position of its own starts from that cursor -- so the first show
/// of a continued run is pushed by writing one, and the rest of that run is
/// pushed by having done so. Writing a second one would push them twice. The
/// walk therefore carries one bit, whether the cursor is already displaced,
/// and:
///
/// - a positioning operator (`Td`, `TD`, `T*`, `Tm`, `BT`) clears it, because
///   each of those sets the line matrix and resets the cursor to it;
/// - a show this batch replaces clears it too: its expansion ends by putting
///   the cursor back exactly where the source left it (`cursor_after`), and its
///   own `prepare` is given `inherited` instead of a displacement;
/// - a show the push owns is given a displacement only when the cursor does not
///   already carry one;
/// - and a show the push does **not** own, arriving on a displaced cursor, is
///   refused. That is text the editor would move without ever having decided to
///   -- a read-only run continuing the line, a spacer inside an ActualText
///   span -- and moving half a line is worse than moving none of it.
fn drag(
    page: &Inspection,
    run: &Run,
    free: &Free,
    placement: &Placement,
    shift: f64,
) -> Result<Vec<(u32, f64)>, String> {
    let mut moved = Vec::new();
    let mut displaced = false;
    for (index, op) in page
        .content
        .operations
        .iter()
        .enumerate()
        .skip(run.operator as usize + 1)
    {
        let show = index as u32;
        match op.operator.as_str() {
            "Td" | "TD" | "T*" | "Tm" | "BT" => displaced = false,
            "Tj" | "TJ" => {
                if placement.edited.contains(&show) {
                    displaced = false;
                } else if free.line.contains(&show) {
                    if !displaced {
                        moved.push((show, shift));
                    }
                    displaced = true;
                } else if displaced && !free.carry.contains(&show) {
                    return Err(Room::Fixed.refusal().into());
                }
            }
            _ => {}
        }
    }
    Ok(moved)
}

/// The operand that puts a show `shift` page points further along its own text
/// axis than its source put it.
///
/// A `TJ` displacement is the only positioning this touches, and that is the
/// whole reason no other line moves: `Td`, `TD`, `T*` and `Tm` all set the
/// **line** matrix, which every following line is measured from, while a number
/// inside a `TJ` array moves only the text cursor. Push a line by rewriting its
/// own shows and the lines after it are bit-identical -- the property
/// `restore_line` exists to keep, and the one a rewritten `Td` would give up.
///
/// It has to be **one** number, not the integer-and-remainder pair
/// `continuation_adjustment` writes: `array_text` reads a single leading number
/// as where the run starts and refuses an array whose next item is a number
/// too, so a split pair would leave the run it just moved undiscoverable. Where
/// the source already opens with one, this replaces it rather than adding a
/// second, which is also what keeps the displacement exact rather than
/// accumulating: it is always measured from the source's own bytes.
pub(super) fn push(page: &Inspection, operator: u32, shift: f64) -> Result<Object, String> {
    let imprecise = || "Cannot move the following text at PDF number precision".to_string();
    let context = page
        .contexts
        .get(&operator)
        .ok_or("missing text layout context")?;
    let show = page
        .content
        .operations
        .get(operator as usize)
        .ok_or("text run no longer exists")?;
    // A TJ number is in thousandths of an em of the show's own font size, before
    // its own matrix, so this is what one of them is worth in page points. It is
    // positive and finite for every discovered show -- the scanner refuses a
    // size of zero and a singular matrix -- and a denormal one would send the
    // division below to infinity, which the check on `written` catches. There is
    // no guard here, because a guard nothing can redden is not a guard.
    let per_unit = context.size * context.scale / 1000.;
    let items = match (show.operator.as_str(), show.operands.as_slice()) {
        ("TJ", [Object::Array(items)]) => items.clone(),
        ("Tj", [string @ Object::String(..)]) => vec![string.clone()],
        _ => return Err("invalid text patch".into()),
    };
    let leading = matches!(items.first(), Some(Object::Integer(_) | Object::Real(_)));
    let existing = if leading { number(&items[0])? } else { 0. };
    let written = (existing - shift / per_unit) as f32;
    if !written.is_finite()
        || f64::from(written).abs() > 1_000_000.
        || ((existing - f64::from(written)) * per_unit - shift).abs() > 0.0001
    {
        return Err(imprecise());
    }
    let mut items = items;
    if leading {
        items[0] = Object::Real(written);
    } else {
        items.insert(0, Object::Real(written));
    }
    Ok(Object::Array(items))
}

fn line_breaks(
    text: &str,
    (width, rest): (f64, f64),
    wrap: bool,
    too_wide: &str,
    measure: impl Fn(&str) -> Result<f64, String>,
) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut remaining = paragraph;
        while !remaining.is_empty() {
            // The first line of the replacement may be given a different width
            // from the lines after it: a wrap starts them at the paragraph's
            // left edge rather than at the run.
            let width = if result.is_empty() { width } else { rest };
            let mut end = 0;
            let mut space = 0;
            for (index, ch) in remaining.char_indices() {
                let next = index + ch.len_utf8();
                if measure(&remaining[..next])? > width + 0.000001 {
                    break;
                }
                end = next;
                if ch.is_whitespace() {
                    space = next;
                }
            }
            if end == remaining.len() {
                result.push(remaining.to_owned());
                break;
            }
            if !wrap || end == 0 {
                return Err(too_wide.into());
            }
            if space > 0 {
                end = space;
            }
            result.push(remaining[..end].to_owned());
            remaining = &remaining[end..];
            if result.len() > 128 {
                return Err("Text exceeds 128 lines".into());
            }
        }
    }
    if result.len() > 128 {
        return Err("Text exceeds 128 lines".into());
    }
    Ok(result)
}

// The size control shows a thousandth of a point and the editor rounds the
// source size up to it (`defaultTextLayout`), so pdfTeX's 9.96264 pt arrives as
// 9.963 and a Word run at `Tf 1` under a scale of 11.0417 as 11.042. Laid out at
// that size a run's own text is wider than its own advance, by 0.01 pt on a
// 300 pt line, and the box refused it. A requested size within one step of the
// source is therefore the source's size, in text space, exactly; the reader
// cannot type a finer one. The step is widened by a hair so that a product one
// rounding over it still counts.
const SIZE_STEP: f64 = 0.001;

fn own_size(source: f64, requested: f64, yscale: f64) -> f64 {
    if (requested - source * yscale).abs() <= SIZE_STEP * (1. + 1e-9) {
        source
    } else {
        requested / yscale
    }
}

// A one-line replacement in the run's own font and size is written as the
// source positions it (`own_items`, shared with the byte-patch writer): the
// source's kerns and word gaps kept wherever the text around them is unchanged,
// and the run placed at its own origin. The box the editor opens is the run's
// own advance, kerns included, so the unchanged text fits it exactly; laid out
// again from glyph widths, a run its producer tightened with kerning is wider
// than that and was refused. The ink may reach wherever the source's did as well
// as across the box, since that is where the source's glyphs already are (and
// the source's ink always includes its origin). When neither version fits, the
// replacement is laid out like any other, which can still wrap it.
//
// The flag says whether the ink stays within the source's own: such a
// replacement is clipped exactly as the source is, which the byte-patch writer
// has always accepted, so a partly clipped run (Word draws a clip around many
// lines) is not refused for a clip that was already there.
//
// Returns the items with their advance and their horizontal ink, so that a box
// following the typed text is sized from what this actually wrote rather than
// from a second measurement of the same string.
fn source_items(
    page: &Inspection,
    run: &Run,
    replacement: &str,
    metrics: &fonts::Metrics,
    gap: f64,
    (size, spacing, word_spacing): (f64, f64, f64),
    width: f64,
) -> Option<(Vec<Object>, f64, [f64; 2], bool)> {
    if size != run.size || replacement.contains('\n') {
        return None;
    }
    let original = *page.horizontal_bounds.get(&run.operator)?;
    let fits = |advance: f64, bounds: [f64; 2]| {
        advance <= width + 0.000_001
            && bounds[0] >= original[0]
            && bounds[1] <= original[1].max(width) + 0.000_001
    };
    let (items, advance, bounds) = super::own_items(
        &page.content.operations[run.operator as usize],
        page.groups.contains_key(&run.operator),
        page.leads.contains_key(&run.operator),
        replacement,
        metrics,
        gap,
        (size, spacing, word_spacing),
        fits,
    )
    .ok()?;
    let inside = bounds[1] <= original[1] + 0.000_001;
    fits(advance, bounds).then_some((items, advance, bounds, inside))
}

/// Whether text a wrap does not move sits inside one of the lines it does:
/// a run the editor may not move, sharing a line with runs that do, would be
/// left behind on its own -- a read-only word in the middle of a moved line.
/// `wrap::plan` refuses the ones the structure tree puts in the block; this
/// catches one the tree puts elsewhere and the page puts in the middle of the
/// paragraph's line. Asked before the text is laid out, because it does not
/// depend on how many lines the text takes, and the new line landing on that
/// word would otherwise be refused as a lack of room.
fn left_behind(
    page: &Inspection,
    below: &BTreeSet<u32>,
    hits: &[([f64; 4], bool)],
) -> Result<(), String> {
    let moving: Vec<[f64; 4]> = page
        .runs
        .runs
        .iter()
        .filter(|other| below.contains(&other.operator) && !other.text.trim().is_empty())
        .map(|other| other.display_rect.map(f64::from))
        .collect();
    // The paragraph's lines run along whichever display axis its runs are
    // longer in; a quarter-turned page turns that axis too.
    let vertical = moving
        .iter()
        .map(|rect| (rect[2] - rect[0]) - (rect[3] - rect[1]))
        .sum::<f64>()
        >= 0.;
    let (along, cross) = if vertical { (0, 1) } else { (1, 0) };
    let span = moving
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), rect| {
            (low.min(rect[along]), high.max(rect[along + 2]))
        });
    for (other, moves) in hits {
        let centre = (other[along] + other[along + 2]) / 2.;
        if *moves || centre < span.0 || centre > span.1 {
            continue;
        }
        for old in &moving {
            let shared = old[cross + 2].min(other[cross + 2]) - old[cross].max(other[cross]);
            let smaller = (old[cross + 2] - old[cross]).min(other[cross + 2] - other[cross]);
            if shared > smaller / 2. {
                return Err(wrap::UNMOVABLE.into());
            }
        }
    }
    Ok(())
}

/// Whether the runs a wrap moves can go where it puts them, and where each of
/// their hit rectangles ends up.
///
/// `moves` is each moved run's own operator with how far it goes on the
/// displayed page: the block's lines below the edit all go down by the lines it
/// added, and each run after the edit on its line goes wherever it flowed to.
/// `hits` is the page's hit list with each of those runs flagged, and `down` one
/// of the block's line pitches down the displayed page. Two things refuse it
/// here; [`left_behind`] has already been asked, before the text was laid out:
///
/// - **The page, a clip, and text that stays.** A moved run must stay on the
///   page and inside the clip in force over it, and must not land on text that
///   does not move. "Land on" allows exactly the overlap the block's own lines
///   already have with each other: hit rectangles are em boxes, and at ordinary
///   leading two lines of one paragraph overlap by a sliver. A moved line may
///   come as close to what is below as its own lines are to each other, and no
///   closer.
/// - **Drawings and annotations over the move.** A rule under a word, a
///   highlight, a link's rectangle: anything partly over the area the moved
///   runs sweep would stay where it is while the text under it left. One that
///   holds the whole area -- a page background, a coloured box around the
///   paragraph -- still holds it afterwards.
///
/// A blank run has a hit rectangle and no ink, so it moves with its line and is
/// checked against nothing.
fn wrap_room(
    page: &Inspection,
    moves: &BTreeMap<u32, [f64; 2]>,
    hits: &[([f64; 4], bool)],
    down: [f64; 2],
    geometry: &crate::pagetree::DisplayedPage,
    display: &impl Fn([f64; 4]) -> [f32; 4],
) -> Result<Vec<(u32, [f32; 4])>, String> {
    let cross = usize::from(down[1].abs() >= down[0].abs());
    let pitch = down[cross].abs();
    let shift = |rect: [f64; 4], by: [f64; 2]| {
        [
            rect[0] + by[0],
            rect[1] + by[1],
            rect[2] + by[0],
            rect[3] + by[1],
        ]
    };
    let moving: Vec<_> = page
        .runs
        .runs
        .iter()
        .filter(|other| !other.text.trim().is_empty())
        .filter_map(|other| {
            let by = *moves.get(&other.operator)?;
            Some((
                other.operator,
                other.display_rect.map(f64::from),
                by,
                page.contexts[&other.operator]
                    .clip
                    .map(|clip| display(clip).map(f64::from)),
            ))
        })
        .collect();
    let height = moving
        .iter()
        .map(|(_, rect, ..)| rect[cross + 2] - rect[cross])
        .fold(0., f64::max);
    let mut slack = [0.1; 2];
    slack[cross] = (height - pitch).max(0.) + 0.1;
    let (width, depth) = (f64::from(geometry.width), f64::from(geometry.height));
    let mut placed = Vec::new();
    for (operator, old, by, clip) in &moving {
        let new = shift(*old, *by);
        if new[0] < -0.001 || new[1] < -0.001 || new[2] > width + 0.001 || new[3] > depth + 0.001 {
            return Err(wrap::NO_ROOM.into());
        }
        if clip.is_some_and(|clip| !wrap::holds(clip, new)) {
            return Err(wrap::NO_ROOM.into());
        }
        if hits
            .iter()
            .any(|(other, moves)| !*moves && wrap::overlaps(new, *other, slack))
        {
            return Err(wrap::NO_ROOM.into());
        }
        placed.push((*operator, new.map(|value| value as f32)));
    }
    let swept: Vec<[f64; 4]> = moving
        .iter()
        .map(|(_, old, by, _)| {
            let new = shift(*old, *by);
            [
                old[0].min(new[0]),
                old[1].min(new[1]),
                old[2].max(new[2]),
                old[3].max(new[3]),
            ]
        })
        .collect();
    // An annotation list the scan could not read may hold one over the lines.
    let annotations = page.annotations.as_deref().ok_or(wrap::DRAWN)?;
    for thing in page.graphics.iter().chain(annotations) {
        let thing = thing.map(f64::from);
        if swept
            .iter()
            .any(|rect| wrap::overlaps(*rect, thing, [0.1, 0.1]))
            && !swept.iter().all(|rect| wrap::holds(thing, *rect))
        {
            return Err(wrap::DRAWN.into());
        }
    }
    Ok(placed)
}

/// The lines a replacement is laid out in: how wide the first may be, where the
/// ones after it start and how wide they may be, and their pitch. A box gives
/// every line its own width at the run's origin and the writer's default pitch,
/// inside the box's height; a wrap (`wrap::plan`) gives its paragraph's.
struct Shape {
    first: f64,
    start: f64,
    rest: f64,
    wrap: bool,
    /// The paragraph's own line pitch, for a wrap. `None` is a box, whose lines
    /// are set at the writer's default and must fit the box's height.
    pitch: Option<f64>,
}

impl Shape {
    fn boxed(limit: f64, wrap: bool) -> Self {
        Shape {
            first: limit,
            start: 0.,
            rest: limit,
            wrap,
            pitch: None,
        }
    }
}

pub(super) fn prepare(
    doc: &Document,
    page: &Inspection,
    change: &Change,
    placement: &Placement,
) -> Result<Prepared, String> {
    let run = page
        .runs
        .runs
        .iter()
        .find(|run| run.operator == change.operator)
        .ok_or("text run no longer exists")?;
    if change.revision != page.runs.revision || change.original != run.text {
        return Err("text changed since this run was inspected".into());
    }
    let settings = change.layout.as_ref().ok_or("missing text layout")?;
    if [settings.width, settings.height, settings.size]
        .iter()
        .any(|n| !n.is_finite())
        || !(0.1..=14400.).contains(&settings.width)
        || !(0.1..=14400.).contains(&settings.height)
        || !(1.0..=512.).contains(&settings.size)
    {
        return Err("Use a font size from 1 to 512 pt and a box from 0.1 to 14400 pt".into());
    }
    let context = page
        .contexts
        .get(&run.operator)
        .ok_or("missing text layout context")?;
    let xscale = run.matrix[0].hypot(run.matrix[1]);
    let yscale = run.matrix[2].hypot(run.matrix[3]);
    let width = settings.width / xscale;
    let height = settings.height / yscale;
    let size = own_size(run.size, settings.size, yscale);
    if size > 1000.
        || ![width, height, size]
            .iter()
            .all(|n| n.is_finite() && *n <= 1_000_000.)
    {
        return Err("Text transformation exceeds its limit".into());
    }
    let font_at = *page
        .font_operators
        .get(&run.operator)
        .ok_or("missing text font")?;
    let original_name = page.content.operations[font_at].operands[0]
        .as_name()
        .map_err(|e| e.to_string())?;
    let resources = resources(doc, page.id)?;
    let original_metrics = font(doc, resources, original_name)?.preferring(&super::shown(
        &page.content,
        &page.groups,
        run.operator,
    ));
    let font_table = dictionary(doc, resources.get(b"Font").map_err(|e| e.to_string())?)?;
    let font_dict = dictionary(
        doc,
        font_table.get(original_name).map_err(|e| e.to_string())?,
    )?;
    let original_label = font_dict
        .get(b"BaseFont")
        .and_then(Object::as_name)
        .ok()
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .unwrap_or_else(|| "Original font".into());
    // The run's own word gap, for a font that shows spaces as displacements.
    let gap = page
        .gaps
        .get(&run.operator)
        .copied()
        .unwrap_or(super::DEFAULT_GAP);
    let encodable = change
        .replacement
        .split('\n')
        .all(|line| original_metrics.items(line, gap).is_ok());
    let chosen = match settings.font {
        _ if change.replacement.is_empty() => None,
        EditFont::Auto if encodable => None,
        EditFont::Original => None,
        EditFont::Auto => {
            let name = original_label.to_ascii_lowercase();
            Some(fonts::fallback::automatic(
                u8::from(name.contains("bold"))
                    + 2 * u8::from(name.contains("italic") || name.contains("oblique")),
                &change.replacement,
            )?)
        }
        EditFont::NotoSans => Some(0),
        EditFont::NotoSansBold => Some(1),
        EditFont::NotoSansItalic => Some(2),
        EditFont::NotoSansBoldItalic => Some(3),
        EditFont::NotoSansCjkSc => Some(4),
        EditFont::NotoSansCjkScBold => Some(5),
    };
    let (fallback, metrics, name, label, spacing, word_spacing) = if let Some(style) = chosen {
        let (font, metrics) = fonts::fallback::Font::new(style, &change.replacement)?;
        let mut name = format!("TPDFEdit{}", run.operator).into_bytes();
        while font_table.has(&name) {
            name.push(b'_');
            if name.len() > 256 {
                return Err("too many replacement font resources".into());
            }
        }
        (
            Some(font),
            metrics,
            name,
            fonts::fallback::label(style).to_owned(),
            0.,
            0.,
        )
    } else {
        let (spacing, word_spacing) = page.text_spacing[&run.operator];
        (
            None,
            original_metrics,
            original_name.to_vec(),
            original_label,
            spacing,
            word_spacing,
        )
    };
    let geometry = crate::pagetree::displayed_page(doc, page.id);
    let (ox, oy) = (f64::from(geometry.origin.0), f64::from(geometry.origin.1));
    let display = |bounds: [f64; 4]| {
        crate::text::to_device(
            geometry.turns,
            geometry.width,
            geometry.height,
            [
                bounds[0] - ox,
                bounds[1] - oy,
                bounds[2] - ox,
                bounds[3] - oy,
            ],
        )
    };
    // Where an earlier edit on this line has already put this run, in the run's
    // own text-space units: everything below is written that much further along.
    let inherited = placement.inherited / xscale;
    if !inherited.is_finite() || inherited.abs() > 1_000_000. {
        return Err("text position exceeds its limit".into());
    }
    // A box the reader has not sized follows what they type, as far as the room
    // after the run allows, and then as far as the text after it on the line can
    // be pushed along (`free_width`); one they have sized is theirs, and pushes
    // nothing. The ceiling is never below the box that arrived, so growth only
    // ever adds room, and the page check below is the same check it always was
    // for a box already wider than the page.
    let free = free_width(
        page,
        run,
        context,
        &geometry,
        display,
        (xscale, width, height),
        (inherited, settings.grow),
    );
    let stop = free.stop;
    // 14400 pt is the widest box the request may name; a grown one stays inside
    // the same range rather than reaching a size a reader could not have asked
    // for.
    let ceiling = free.ceiling.min((14400. / xscale).max(width)).max(width);
    let (too_wide, too_wide_ink) = if settings.grow {
        (stop.refusal(), stop.refusal())
    } else {
        (
            "Text exceeds the box width. Widen the box, reduce the font size, or enable wrapping.",
            "Text ink exceeds the box. Increase its width or choose another font.",
        )
    };
    let source_at = |limit: f64| {
        if fallback.is_none() && !change.replacement.is_empty() {
            source_items(
                page,
                run,
                &change.replacement,
                &metrics,
                gap,
                (size, spacing, word_spacing),
                limit,
            )
        } else {
            None
        }
    };
    // The box that arrived, not the ceiling: `room` already holds a grown box
    // inside the displayed page, so checking the grown one here would be a
    // second guard for one property that could never fire on its own.
    let box_bounds = text_bounds(
        run.matrix,
        [inherited, run.size - height, inherited + width, run.size],
    );
    let rect = display(box_bounds);
    if rect[0] < -0.001
        || rect[1] < -0.001
        || rect[2] > geometry.width + 0.001
        || rect[3] > geometry.height + 0.001
    {
        return Err("The editing box extends beyond the page".into());
    }
    // One writer: the replacement either at the source's own positioning
    // (`kept`) or laid out afresh, in the lines `shape` describes. `prepare`
    // runs it over the plans below, taking the first that succeeds, and a wrap
    // runs it once more with its own shape.
    let lay_out = |kept: Option<&(Vec<Object>, f64, [f64; 2], bool)>,
                   shape: &Shape,
                   moving: bool,
                   hits: &[([f64; 4], bool)]|
     -> Result<(Vec<Operation>, usize, f64, f64), String> {
        // The widest the text itself turned out to be, measured from the
        // run's origin. The box reported back is this rather than the whole
        // ceiling, so a grown box is the size of what the reader typed and
        // the dashed outline they see follows their text instead of the room
        // it had.
        let mut used = 0_f64;
        // Where the last line ends, from the same origin, at the farther of
        // its advance and its ink: a wrap flows the text after the run on from
        // there, as the push measures a line from `used`, so an overhanging
        // last glyph is not drawn into the run that follows it.
        let mut end = 0_f64;
        let lines = if kept.is_some() {
            vec![change.replacement.clone()]
        } else {
            line_breaks(
                &change.replacement,
                (shape.first, shape.rest),
                shape.wrap,
                too_wide,
                |text| {
                    metrics
                        .gapped_layout(text.trim_end_matches(' '), size, spacing, word_spacing, gap)
                        .map(|(width, _)| width)
                },
            )?
        };
        let [bottom, top] = metrics.vertical_bounds.unwrap_or([-250., 1000.]);
        let line_height = shape.pitch.unwrap_or(size * 1.25);
        let first_baseline = run.size - size;
        let minimum = first_baseline - (lines.len().saturating_sub(1) as f64) * line_height
            + bottom * size / 1000.;
        if shape.pitch.is_none()
            && (minimum < run.size - height - 0.000001
                || first_baseline + top * size / 1000. > run.size + 0.000001)
        {
            return Err(
                "Text exceeds the box height. Enlarge the box or reduce the font size.".into(),
            );
        }
        let mut operations = vec![
            Operation::new(
                "Tf",
                vec![Object::Name(name.clone()), Object::Real(size as f32)],
            ),
            numeric("Tc", &[spacing]),
            numeric("Tw", &[word_spacing]),
        ];
        let gap_spaces = !metrics.writes_space();
        for (index, text) in lines.iter().enumerate() {
            // A wrapped line keeps the space it broke at. A font that writes
            // spaces shows it as before; a gap font has nothing to show.
            let text = if gap_spaces {
                text.trim_end_matches(' ')
            } else {
                text.as_str()
            };
            let dy = first_baseline - index as f64 * line_height;
            // Where this line starts and how far it may reach from there.
            let (start, limit) = if index == 0 {
                (0., shape.first)
            } else {
                (shape.start, shape.rest)
            };
            // Ink is measured without that trailing space, as `line_breaks`
            // measured the line: a space draws nothing, and counting its advance
            // refused lines whose words fit the box exactly.
            // Kept items stay where the source put them: `source_items` has
            // already held their ink to the box and the source's own ink.
            let (ink, inset) = if let Some((_, advance, ink, _)) = kept {
                used = used.max(advance.max(ink[1]));
                end = advance.max(ink[1]);
                (*ink, 0.)
            } else {
                let (advance, ink) = metrics.gapped_layout(
                    text.trim_end_matches(' '),
                    size,
                    spacing,
                    word_spacing,
                    gap,
                )?;
                let inset = (-ink[0]).max(0.);
                if ink[1] + inset > limit + 0.001 {
                    return Err(too_wide_ink.into());
                }
                used = used.max(start + advance.max(ink[1] + inset));
                end = start + (inset + advance).max(ink[1] + inset);
                (ink, inset)
            };
            let ink = text_bounds(
                run.matrix,
                [
                    inherited + start + ink[0] + inset,
                    dy + bottom * size / 1000.,
                    inherited + start + ink[1] + inset,
                    dy + top * size / 1000.,
                ],
            );
            let ink = [
                ink[0] - context.stroke,
                ink[1] - context.stroke,
                ink[2] + context.stroke,
                ink[3] + context.stroke,
            ];
            if !text.is_empty() {
                if !kept.is_some_and(|(_, _, _, inside)| *inside) {
                    clipping::contains(context.clip, ink).map_err(|_| "The document clips this area. Reduce the box or font size to keep the text visible.")?;
                }
                for region in &context.regions {
                    region.contains(ink)?;
                }
                let shown = display(ink).map(f64::from);
                // A box keeps its lines inside itself, and the box is already
                // held to the page; a wrap's lines are below the box, so each
                // one is held to the page on its own.
                if shape.pitch.is_some()
                    && (shown[0] < -0.001
                        || shown[1] < -0.001
                        || shown[2] > f64::from(geometry.width) + 0.001
                        || shown[3] > f64::from(geometry.height) + 0.001)
                {
                    return Err(wrap::NO_ROOM.into());
                }
                for (other, moves) in hits {
                    // A run this plan pushes along is not in the way: it ends up
                    // exactly as far on as the text that displaced it, which is
                    // what `reach` measured and what the shift written below is.
                    // A run a wrap moves down is not in the way either: it ends
                    // up the lines this text adds further down, at its own
                    // block's pitch, and `wrap_room` checks where it lands.
                    if moving && *moves {
                        continue;
                    }
                    let intersection = [
                        shown[0].max(other[0]),
                        shown[1].max(other[1]),
                        shown[2].min(other[2]),
                        shown[3].min(other[3]),
                    ];
                    if intersection[2] > intersection[0] + 0.1
                        && intersection[3] > intersection[1] + 0.1
                        && (intersection[0] < free.own[0] - 0.1
                            || intersection[1] < free.own[1] - 0.1
                            || intersection[2] > free.own[2] + 0.1
                            || intersection[3] > free.own[3] + 0.1)
                    {
                        return Err("Text would overlap another line. Reduce the font size or change the box dimensions.".into());
                    }
                }
            }
            let mut matrix = context.shown;
            let (dx, dy) = (
                (inherited + start + inset) * matrix[0] + dy * matrix[2],
                (inherited + start + inset) * matrix[1] + dy * matrix[3],
            );
            shift_position(&mut matrix, dx, dy)?;
            operations.push(numeric("Tm", &matrix));
            let items = match kept {
                Some((items, _, _, _)) => items.clone(),
                None => metrics.items(text, gap)?,
            };
            operations.push(if items.len() > 1 {
                Operation::new("TJ", vec![Object::Array(items)])
            } else {
                Operation::new("Tj", items)
            });
        }
        Ok((operations, lines.len(), used, end))
    };
    // Four writers, each a weaker claim than the one before it, and the last two
    // are the writer exactly as it was before the box could grow.
    //
    // The order is quality first: the source's own positioning in the grown box,
    // then in the box that arrived, then the general layout in each. It matters
    // that the ungrown pair is there at all. Growing the ceiling changes which
    // version `own_items` picks -- a producer's kerning that did not fit the
    // run's own advance fits a wider box, and the kept items are wider than the
    // rewrite they displace -- so a *same-length* edit that used to be written
    // at the source's origin could be handed to the general layout instead,
    // which insets the line by an overhang and put ink where the source had
    // none. Tried over the public sample with only the grown pair, that turned
    // 14 verdicts from accepted into refused: 13 same-length edits in Word 2016
    // (eleven as an overlap with the next line, two as the line being full) and
    // one in XeLaTeX where the general layout has no glyph for a character the
    // source's own items carry. Growth may add room; it may never take a
    // writer away. `--compare` over the corpus is what says so.
    let plans: &[(f64, bool)] = if ceiling > width {
        &[
            (ceiling, true),
            (width, true),
            (ceiling, false),
            (width, false),
        ]
    } else {
        &[(width, true), (width, false)]
    };
    let mut outcome = Err("missing text layout".to_string());
    for &(limit, positioned) in plans {
        let kept = positioned.then(|| source_at(limit)).flatten();
        if positioned && kept.is_none() {
            continue;
        }
        outcome = lay_out(
            kept.as_ref(),
            &Shape::boxed(limit, settings.wrap),
            limit > free.from,
            &free.hits,
        );
        if outcome.is_ok() {
            break;
        }
    }
    // The line is full and it is the page that filled it: on a tagged page the
    // text may still wrap onto a new line of its own paragraph (`wrap`). That
    // refusal is only ever given to a box the editor opened, since a box the
    // reader sized is refused for its own width instead, and a box the reader
    // sized is theirs. Only at the run's own size, which is the size the
    // paragraph's pitch belongs to, and only with nothing already pushing the
    // run along its line, whose geometry the plan does not carry.
    let full = outcome
        .as_ref()
        .is_err_and(|error| error == Room::Page.refusal());
    let mut wrapped = None;
    if full && placement.inherited == 0. && size == run.size {
        // The room the line has without pushing anything along it.
        let room = if free.line.is_empty() {
            ceiling
        } else {
            free.from.min(ceiling)
        };
        // With the text after the run flowing onto the edit's lines instead of
        // being pushed, the line is the edit's as far as the push could have
        // gone, held to the widest box a request may name, like the ceiling.
        let whole = free.whole.min((14400. / xscale).max(width)).max(width);
        match wrap::plan(page, run, (room.max(width), whole), &free.line) {
            Err(wrap::Refused::NotApplicable) => {}
            Err(wrap::Refused::Blocked(reason)) => return Err(reason.into()),
            Ok(plan) => {
                let below: BTreeSet<u32> = plan.below.iter().copied().collect();
                let flowing: BTreeSet<u32> = plan
                    .after
                    .iter()
                    .flat_map(|unit| unit.shows.iter().copied())
                    .collect();
                let moving: BTreeSet<u32> = below.union(&flowing).copied().collect();
                // Every show of every edit in the batch, grouped members
                // included: a replacement is written where its source was, so
                // one this wrap moved would land on the wrap's own lines.
                if placement
                    .edited
                    .iter()
                    .flat_map(|edited| shows_of(page, *edited))
                    .any(|show| moving.contains(&show))
                {
                    return Err(wrap::CONFLICT.into());
                }
                let hits: Vec<([f64; 4], bool)> = obstacles(page, run)
                    .map(|other| match other {
                        Around::Run(id, rect) => (
                            rect.map(f64::from),
                            shows_of(page, id).iter().any(|show| moving.contains(show)),
                        ),
                        other => (other.rect().map(f64::from), false),
                    })
                    .collect();
                // Only the lines below: the runs after the edit leave a line
                // whose start stays, and what stays there is the block's own.
                left_behind(page, &below, &hits)?;
                let shape = Shape {
                    first: plan.first,
                    start: plan.start,
                    rest: plan.rest,
                    wrap: true,
                    pitch: Some(plan.pitch),
                };
                let (operations, lines, used, end) =
                    lay_out(None, &shape, true, &hits).map_err(|error| {
                        if error.starts_with("Text would overlap another line") {
                            wrap::NO_ROOM.to_string()
                        } else {
                            error
                        }
                    })?;
                // A run after the edit wider than a whole line of the block
                // cannot flow anywhere, and the edit keeps the refusal it had.
                let places = wrap::flow(&plan, run.advance, (lines.saturating_sub(1), end))
                    .ok_or_else(|| Room::Page.refusal().to_string())?;
                // How far the block's lines below go: the lines this edit and
                // the text after it added, at the block's own pitch, down the
                // run's own text axis.
                let added = places
                    .iter()
                    .map(|(line, _)| *line)
                    .fold(lines.saturating_sub(1), usize::max);
                let drop = added as f64 * plan.pitch;
                // A distance in the run's own text space, as the page-space
                // vector `wrap::lowered` takes and as the displayed one the
                // hit rectangles move by.
                let along = |dx: f64, dy: f64| {
                    (
                        dx * run.matrix[0] + dy * run.matrix[2],
                        dx * run.matrix[1] + dy * run.matrix[3],
                    )
                };
                let corner = |dx: f64, dy: f64| {
                    let (to, from) = (
                        display(text_bounds(run.matrix, [dx, dy, dx, dy])),
                        display(text_bounds(run.matrix, [0., 0., 0., 0.])),
                    );
                    [
                        f64::from(to[0]) - f64::from(from[0]),
                        f64::from(to[1]) - f64::from(from[1]),
                    ]
                };
                let mut moves = BTreeMap::new();
                let mut lowered = Vec::new();
                for show in &plan.below {
                    moves.insert(*show, corner(0., -drop));
                    lowered.push((*show, wrap::lowered(page, *show, along(0., -drop))?));
                }
                for (unit, (line, at)) in plan.after.iter().zip(&places) {
                    let (dx, dy) = (at - unit.start, -(*line as f64) * plan.pitch);
                    for show in &unit.shows {
                        moves.insert(*show, corner(dx, dy));
                        lowered.push((*show, wrap::lowered(page, *show, along(dx, dy))?));
                    }
                }
                let placed = wrap_room(
                    page,
                    &moves,
                    &hits,
                    corner(0., -plan.pitch),
                    &geometry,
                    &display,
                )?;
                let rect = display(text_bounds(
                    run.matrix,
                    [
                        plan.start.min(0.),
                        run.size - height - lines.saturating_sub(1) as f64 * plan.pitch,
                        used.max(width),
                        run.size,
                    ],
                ));
                // Where the runs that flowed were: a run leaving the end of the
                // edit's line for the start of the next is outside both the box
                // and where it lands, so the preview names the place it left.
                let vacated: Vec<[f32; 4]> = page
                    .runs
                    .runs
                    .iter()
                    .filter(|other| flowing.contains(&other.operator))
                    .map(|other| other.display_rect)
                    .collect();
                wrapped = Some((operations, lines, rect, lowered, (placed, vacated)));
            }
        }
    }
    let (mut operations, line_count, used, wrap) = match wrapped {
        Some((operations, lines, rect, lowered, moved)) => {
            (operations, lines, 0., Some((rect, lowered, moved)))
        }
        None => {
            let (operations, lines, used, _) = outcome?;
            (operations, lines, used, None)
        }
    };
    // How far the text ran past the room the line already had, which is exactly
    // how far the text after it has to go.
    //
    // It is measured from the room rather than from the run's own advance, so
    // an edit that fits the gap the producer left moves nothing at all and is
    // written exactly as it was before this increment; the gap is spent first,
    // and only what is typed past it pushes. That also makes the push
    // continuous with growth: the point at which text starts moving is the
    // point at which the box stopped being able to grow.
    let delta = (used - free.from).max(0.);
    let shift = placement.inherited + delta * xscale;
    // The line is pushed by rewriting the shows' own arrays, never a Td or a
    // Tm, so a walk of the stream has to say which of them already carry the
    // displacement through the text cursor, and refuse to drag anything the
    // editor does not own.
    let moved = if delta > 0. {
        drag(page, run, &free, placement, shift)?
    } else {
        Vec::new()
    };
    for (show, shift) in &moved {
        // Every operand is built here, so a refusal names this edit instead of
        // surfacing later as a corrupt array; `write` builds them again from
        // the same source bytes once the whole batch has been prepared.
        push(page, *show, *shift)?;
    }
    // The box that goes back to the reader: what they set, or what their text
    // needed, never past the room it had. A wrap's box is every line it set.
    let rect = match &wrap {
        Some((rect, ..)) => *rect,
        None => display(text_bounds(
            run.matrix,
            [
                inherited,
                run.size - height,
                inherited + width.max(used).min(ceiling),
                run.size,
            ],
        )),
    };
    // Every run this edit moves, where it ends up: along its line when pushed,
    // down its paragraph when wrapped. The runs a push moves are the whole
    // line it rewrites, not only the shows given a displacement of their own,
    // since the rest ride the cursor; a run this batch also replaces is placed
    // by its own edit.
    let mut placed = Vec::new();
    if delta > 0. {
        let corner = |x: f64| display(text_bounds(run.matrix, [x, run.size, x, run.size]));
        let (here, origin) = (corner(shift / xscale), corner(0.));
        let (dx, dy) = (here[0] - origin[0], here[1] - origin[1]);
        for other in &page.runs.runs {
            if free.line.contains(&other.operator) && !placement.edited.contains(&other.operator) {
                let rect = other.display_rect;
                placed.push((
                    other.operator,
                    [rect[0] + dx, rect[1] + dy, rect[2] + dx, rect[3] + dy],
                ));
            }
        }
    }
    let (lowered, (down, vacated)) = match wrap {
        Some((_, lowered, moved)) => (lowered, moved),
        None => (Vec::new(), (Vec::new(), Vec::new())),
    };
    placed.extend(down);
    // What the preview has to show: the box, and every run this edit moved,
    // where it ends up. Without the second the crop stops at the reader's own
    // text and the text it moved is half outside the picture. Where a wrapped
    // line was needs nothing of its own: the extent is one rectangle, from the
    // box down to the lowest line moved, and every place a line left is
    // between the two. Where a run that flowed along the edit's line was does:
    // it can be right of everything else.
    let mut extent = rect;
    for other in placed.iter().map(|(_, other)| other).chain(&vacated) {
        extent = [
            extent[0].min(other[0]),
            extent[1].min(other[1]),
            extent[2].max(other[2]),
            extent[3].max(other[3]),
        ];
    }
    // q/Q cannot restore the text matrices. Restore both explicitly, including
    // the pre-existing line origin that a later Td/T* will use.
    operations.push(page.content.operations[font_at].clone());
    let (spacing, word_spacing) = page.text_spacing[&run.operator];
    operations.push(numeric("Tc", &[spacing]));
    operations.push(numeric("Tw", &[word_spacing]));
    operations.extend(restore_line(
        &page.content.operations,
        context.line_origin,
        run.operator as usize,
    )?);
    let adjustment = (-context.cursor_after * 1000. / run.size) as f32;
    number(&Object::Real(adjustment))?;
    if !adjustment.is_finite()
        || ((-f64::from(adjustment) * run.size / 1000. - context.cursor_after) * xscale).abs()
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
    Ok(Prepared {
        operations,
        fallback,
        name,
        label,
        rect,
        lines: line_count,
        line: if delta > 0. {
            free.line.clone()
        } else {
            BTreeSet::new()
        },
        moved,
        shift,
        extent,
        lowered,
        placed,
    })
}
