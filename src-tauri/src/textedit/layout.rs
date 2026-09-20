//! Local text layout. Explicit text-state restoration leaves later content fixed.
use super::*;
use lopdf::content::Operation;

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
}

pub(super) struct Prepared {
    pub operations: Vec<Operation>,
    pub fallback: Option<fonts::fallback::Font>,
    pub name: Vec<u8>,
    pub label: String,
    pub rect: [f32; 4],
    pub lines: usize,
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
            Room::Page => "There is no room for more text on this line: it reaches the edge of the page. Shorten the text or reduce the font size.",
            Room::Clip => "There is no room for more text on this line: the document clips the space after it. Shorten the text or reduce the font size.",
        }
    }
}

/// Every hit rectangle on the page a replacement must not land on: the other
/// runs, the read-only text preserved beside them, and the form fields.
///
/// The growth limit and the collision check in `prepare` read this one list, so
/// the box cannot be grown into something the check would then refuse it for.
/// Two lists would be two answers to one question, and the drift between them
/// would show up as a refusal the reader cannot act on.
fn obstacles<'a>(page: &'a Inspection, run: &'a Run) -> impl Iterator<Item = &'a [f32; 4]> {
    page.runs
        .runs
        .iter()
        .chain(&page.preserved)
        .filter(move |other| other.operator != run.operator && !other.text.trim().is_empty())
        .map(|other| &other.display_rect)
        .chain(&page.form_text_bounds)
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
/// - A *neighbour* counts as on this line when its rectangle overlaps the grown
///   box's cross-axis span by more than the 0.1 pt the collision check ignores,
///   and it stops the box at its near edge, so the box comes up to it rather
///   than over it. The cross span is the box's own together with the run's hit
///   rectangle, whose glyphs may reach above and below the box.
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
    (zero, wide): ([f64; 4], [f64; 4]),
    probe: f64,
    page: [f64; 2],
    clip: Option<[f64; 4]>,
    own: [f64; 4],
    obstacles: impl Iterator<Item = [f64; 4]>,
    width: f64,
) -> (f64, Room) {
    let moved = [
        (0_usize, 1_f64, wide[2] - zero[2]),
        (0, -1., zero[0] - wide[0]),
        (1, 1., wide[3] - zero[3]),
        (1, -1., zero[1] - wide[1]),
    ];
    let Some(&(axis, sign, moved)) = moved
        .iter()
        .filter(|(_, _, moved)| *moved > 0. && moved.is_finite())
        .max_by(|a, b| a.2.total_cmp(&b.2))
    else {
        return (width, Room::Page);
    };
    // Positive and finite: the filter above took the displacement and `probe` is
    // a positive finite width, so there is no second guard here to go stale.
    let rate = moved / probe;
    // A rectangle's edge along the growth axis: the one furthest along it when
    // `ahead`, the one facing us when not. Which array index that is depends on
    // the direction, and saying so once is what keeps the four cases below from
    // each having their own opinion about it.
    let edge =
        |rect: [f64; 4], ahead: bool| rect[if (sign > 0.) == ahead { axis + 2 } else { axis }];
    let lead = edge(zero, true);
    // A limit coordinate, as a box width.
    let at = |limit: f64| sign * (limit - lead) / rate;
    let mut free = at(edge([0., 0., page[0], page[1]], true));
    let mut stop = Room::Page;
    if let Some(clip) = clip {
        let limit = at(edge(clip, true));
        if limit < free {
            free = limit;
            stop = Room::Clip;
        }
    }
    let cross = 1 - axis;
    let (low, high) = (
        zero[cross].min(own[cross]),
        zero[cross + 2].max(own[cross + 2]),
    );
    for other in obstacles {
        if other[cross + 2].min(high) - other[cross].max(low) <= 0.1 {
            continue;
        }
        let near = at(edge(other, false));
        if near + 0.000_000_001 >= width && near < free {
            free = near;
            stop = Room::Line;
        }
    }
    (free.max(width), stop)
}

/// [`room`] for a discovered run, in the text-space units the box width uses.
fn free_width(
    page: &Inspection,
    run: &Run,
    context: &Context,
    geometry: &crate::pagetree::DisplayedPage,
    display: impl Fn([f64; 4]) -> [f32; 4],
    (xscale, width, height): (f64, f64, f64),
) -> (f64, Room) {
    let shape = |w: f64| text_bounds(run.matrix, [0., run.size - height, w, run.size]);
    let mapped = |w: f64| display(shape(w)).map(f64::from);
    // About a page point of box, whatever the run's own scale; see `room`.
    let probe = 1. / xscale.max(1e-9);
    let (free, stop) = room(
        (mapped(0.), mapped(probe)),
        probe,
        [f64::from(geometry.width), f64::from(geometry.height)],
        context.clip.map(|clip| display(clip).map(f64::from)),
        run.display_rect.map(f64::from),
        obstacles(page, run).map(|rect| rect.map(f64::from)),
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
        return (width, Room::Clip);
    }
    (free, stop)
}

fn line_breaks(
    text: &str,
    width: f64,
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
        let mut rest = paragraph;
        while !rest.is_empty() {
            let mut end = 0;
            let mut space = 0;
            for (index, ch) in rest.char_indices() {
                let next = index + ch.len_utf8();
                if measure(&rest[..next])? > width + 0.000001 {
                    break;
                }
                end = next;
                if ch.is_whitespace() {
                    space = next;
                }
            }
            if end == rest.len() {
                result.push(rest.to_owned());
                break;
            }
            if !wrap || end == 0 {
                return Err(too_wide.into());
            }
            if space > 0 {
                end = space;
            }
            result.push(rest[..end].to_owned());
            rest = &rest[end..];
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

pub(super) fn prepare(
    doc: &Document,
    page: &Inspection,
    change: &Change,
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
    // A box the reader has not sized follows what they type, as far as the room
    // after the run allows (`free_width`); one they have sized is theirs. The
    // ceiling is never below the box that arrived, so growth only ever adds
    // room, and the page check below is the same check it always was for a box
    // already wider than the page.
    let (ceiling, stop) = if settings.grow {
        free_width(
            page,
            run,
            context,
            &geometry,
            display,
            (xscale, width, height),
        )
    } else {
        (width, Room::Page)
    };
    // 14400 pt is the widest box the request may name; a grown one stays inside
    // the same range rather than reaching a size a reader could not have asked
    // for.
    let ceiling = ceiling.min((14400. / xscale).max(width)).max(width);
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
    let box_bounds = text_bounds(run.matrix, [0., run.size - height, width, run.size]);
    let rect = display(box_bounds);
    if rect[0] < -0.001
        || rect[1] < -0.001
        || rect[2] > geometry.width + 0.001
        || rect[3] > geometry.height + 0.001
    {
        return Err("The editing box extends beyond the page".into());
    }
    // One writer: the replacement either at the source's own positioning
    // (`kept`) or laid out afresh, inside a box of `limit`. `prepare` runs it
    // over the plans below, taking the first that succeeds.
    let lay_out = |kept: Option<&(Vec<Object>, f64, [f64; 2], bool)>,
                   limit: f64|
     -> Result<(Vec<Operation>, usize, f64), String> {
        // The widest the text itself turned out to be. The box reported
        // back is this rather than the whole ceiling, so a grown box is
        // the size of what the reader typed and the dashed outline they
        // see follows their text instead of the room it had.
        let mut used = 0_f64;
        let lines = if kept.is_some() {
            vec![change.replacement.clone()]
        } else {
            line_breaks(
                &change.replacement,
                limit,
                settings.wrap,
                too_wide,
                |text| {
                    metrics
                        .gapped_layout(text.trim_end_matches(' '), size, spacing, word_spacing, gap)
                        .map(|(width, _)| width)
                },
            )?
        };
        let [bottom, top] = metrics.vertical_bounds.unwrap_or([-250., 1000.]);
        let line_height = size * 1.25;
        let first_baseline = run.size - size;
        let minimum = first_baseline - (lines.len().saturating_sub(1) as f64) * line_height
            + bottom * size / 1000.;
        if minimum < run.size - height - 0.000001
            || first_baseline + top * size / 1000. > run.size + 0.000001
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
            // Ink is measured without that trailing space, as `line_breaks`
            // measured the line: a space draws nothing, and counting its advance
            // refused lines whose words fit the box exactly.
            // Kept items stay where the source put them: `source_items` has
            // already held their ink to the box and the source's own ink.
            let (ink, inset) = if let Some((_, advance, ink, _)) = kept {
                used = used.max(advance.max(ink[1]));
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
                used = used.max(advance.max(ink[1] + inset));
                (ink, inset)
            };
            let ink = text_bounds(
                run.matrix,
                [
                    ink[0] + inset,
                    dy + bottom * size / 1000.,
                    ink[1] + inset,
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
                let shown = display(ink);
                for other in obstacles(page, run) {
                    let intersection = [
                        shown[0].max(other[0]),
                        shown[1].max(other[1]),
                        shown[2].min(other[2]),
                        shown[3].min(other[3]),
                    ];
                    if intersection[2] > intersection[0] + 0.1
                        && intersection[3] > intersection[1] + 0.1
                        && (intersection[0] < run.display_rect[0] - 0.1
                            || intersection[1] < run.display_rect[1] - 0.1
                            || intersection[2] > run.display_rect[2] + 0.1
                            || intersection[3] > run.display_rect[3] + 0.1)
                    {
                        return Err("Text would overlap another line. Reduce the font size or change the box dimensions.".into());
                    }
                }
            }
            let mut matrix = context.shown;
            let (dx, dy) = (
                inset * matrix[0] + dy * matrix[2],
                inset * matrix[1] + dy * matrix[3],
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
        Ok((operations, lines.len(), used))
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
        outcome = lay_out(kept.as_ref(), limit);
        if outcome.is_ok() {
            break;
        }
    }
    let (mut operations, line_count, used) = outcome?;
    // The box that goes back to the reader: what they set, or what their text
    // needed, never past the room it had.
    let rect = display(text_bounds(
        run.matrix,
        [
            0.,
            run.size - height,
            width.max(used).min(ceiling),
            run.size,
        ],
    ));
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
    })
}
