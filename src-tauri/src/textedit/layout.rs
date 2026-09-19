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

fn line_breaks(
    text: &str,
    width: f64,
    wrap: bool,
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
                return Err("Text exceeds the box width. Widen the box, reduce the font size, or enable wrapping.".into());
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
fn source_items(
    page: &Inspection,
    run: &Run,
    replacement: &str,
    metrics: &fonts::Metrics,
    gap: f64,
    (size, spacing, word_spacing): (f64, f64, f64),
    width: f64,
) -> Option<(Vec<Object>, [f64; 2], bool)> {
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
    fits(advance, bounds).then_some((items, bounds, inside))
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
    let kept = if fallback.is_none() && !change.replacement.is_empty() {
        source_items(
            page,
            run,
            &change.replacement,
            &metrics,
            gap,
            (size, spacing, word_spacing),
            width,
        )
    } else {
        None
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
    let box_bounds = text_bounds(run.matrix, [0., run.size - height, width, run.size]);
    let rect = display(box_bounds);
    if rect[0] < -0.001
        || rect[1] < -0.001
        || rect[2] > geometry.width + 0.001
        || rect[3] > geometry.height + 0.001
    {
        return Err("The editing box extends beyond the page".into());
    }
    // The source's own positioning is tried first; when the edit it makes
    // collides with something (another line, a clip) the replacement is laid
    // out as any other would be, which places it differently and may fit, so
    // keeping the source's positioning never refuses what was accepted before.
    let lay_out =
        |kept: Option<&(Vec<Object>, [f64; 2], bool)>| -> Result<(Vec<Operation>, usize), String> {
            let lines = if kept.is_some() {
                vec![change.replacement.clone()]
            } else {
                line_breaks(&change.replacement, width, settings.wrap, |text| {
                    metrics
                        .gapped_layout(text.trim_end_matches(' '), size, spacing, word_spacing, gap)
                        .map(|(width, _)| width)
                })?
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
                let (ink, inset) = if let Some((_, ink, _)) = kept {
                    (*ink, 0.)
                } else {
                    let (_, ink) = metrics.gapped_layout(
                        text.trim_end_matches(' '),
                        size,
                        spacing,
                        word_spacing,
                        gap,
                    )?;
                    let inset = (-ink[0]).max(0.);
                    if ink[1] + inset > width + 0.001 {
                        return Err(
                            "Text ink exceeds the box. Increase its width or choose another font."
                                .into(),
                        );
                    }
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
                    if !kept.is_some_and(|(_, _, inside)| *inside) {
                        clipping::contains(context.clip, ink).map_err(|_| "The document clips this area. Reduce the box or font size to keep the text visible.")?;
                    }
                    for region in &context.regions {
                        region.contains(ink)?;
                    }
                    let shown = display(ink);
                    for other in page
                        .runs
                        .runs
                        .iter()
                        .chain(&page.preserved)
                        .filter(|other| {
                            other.operator != run.operator && !other.text.trim().is_empty()
                        })
                        .map(|other| &other.display_rect)
                        .chain(&page.form_text_bounds)
                    {
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
                    Some((items, _, _)) => items.clone(),
                    None => metrics.items(text, gap)?,
                };
                operations.push(if items.len() > 1 {
                    Operation::new("TJ", vec![Object::Array(items)])
                } else {
                    Operation::new("Tj", items)
                });
            }
            Ok((operations, lines.len()))
        };
    let (mut operations, line_count) = match kept.as_ref().map(|kept| lay_out(Some(kept))) {
        Some(Ok(done)) => done,
        _ => lay_out(None)?,
    };
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
