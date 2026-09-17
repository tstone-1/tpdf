//! Local text layout. Explicit text-state restoration leaves later content fixed.
use super::*;
use lopdf::content::Operation;

pub(super) struct Context {
    pub line: [f64; 6],
    pub shown: [f64; 6],
    pub cursor_after: f64,
    pub clip: Option<[f64; 4]>,
    pub regions: Vec<clipping::Region>,
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
    let size = settings.size / yscale;
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
    let original_metrics = font(doc, resources, original_name)?;
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
    let encodable = change
        .replacement
        .split('\n')
        .all(|line| original_metrics.encode(line).is_ok());
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
    let lines = line_breaks(&change.replacement, width, settings.wrap, |text| {
        metrics
            .spaced_layout(text, size, spacing, word_spacing)
            .map(|(width, _)| width)
    })?;
    let [bottom, top] = metrics.vertical_bounds.unwrap_or([-250., 1000.]);
    let line_height = size * 1.25;
    let first_baseline = run.size - size;
    let minimum = first_baseline - (lines.len().saturating_sub(1) as f64) * line_height
        + bottom * size / 1000.;
    if minimum < run.size - height - 0.000001
        || first_baseline + top * size / 1000. > run.size + 0.000001
    {
        return Err("Text exceeds the box height. Enlarge the box or reduce the font size.".into());
    }
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
    let mut operations = vec![
        Operation::new(
            "Tf",
            vec![Object::Name(name.clone()), Object::Real(size as f32)],
        ),
        numeric("Tc", &[spacing]),
        numeric("Tw", &[word_spacing]),
    ];
    for (index, text) in lines.iter().enumerate() {
        let dy = first_baseline - index as f64 * line_height;
        let (_, ink) = metrics.spaced_layout(text, size, spacing, word_spacing)?;
        let inset = (-ink[0]).max(0.);
        if ink[1] + inset > width + 0.001 {
            return Err(
                "Text ink exceeds the box. Increase its width or choose another font.".into(),
            );
        }
        let ink = text_bounds(
            run.matrix,
            [
                ink[0] + inset,
                dy + bottom * size / 1000.,
                ink[1] + inset,
                dy + top * size / 1000.,
            ],
        );
        if !text.is_empty() {
            clipping::contains(context.clip, ink).map_err(|_| "The document clips this area. Reduce the box or font size to keep the text visible.")?;
            for region in &context.regions {
                region.contains(ink)?;
            }
            let shown = display(ink);
            for other in page.runs.runs.iter().chain(&page.preserved) {
                if other.operator == run.operator || other.text.trim().is_empty() {
                    continue;
                }
                let intersection = [
                    shown[0].max(other.display_rect[0]),
                    shown[1].max(other.display_rect[1]),
                    shown[2].min(other.display_rect[2]),
                    shown[3].min(other.display_rect[3]),
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
        operations.push(Operation::new(
            "Tj",
            vec![Object::string_literal(metrics.encode(text)?)],
        ));
    }
    // q/Q cannot restore the text matrices. Restore both explicitly, including
    // the pre-existing line origin that a later Td/T* will use.
    operations.push(page.content.operations[font_at].clone());
    let (spacing, word_spacing) = page.text_spacing[&run.operator];
    operations.push(numeric("Tc", &[spacing]));
    operations.push(numeric("Tw", &[word_spacing]));
    let local_xscale = context.line[0].hypot(context.line[1]);
    let local_yscale = context.line[2].hypot(context.line[3]);
    // A Tm round trip must not move the line origin accumulated by authored Td
    // operations. Compare in page points, including the graphics transform.
    let page_scale = (xscale / local_xscale).max(yscale / local_yscale);
    if context.line[4..]
        .iter()
        .any(|value| (f64::from(*value as f32) - value).abs() * page_scale > 0.0001)
    {
        return Err("Cannot preserve the following line origin precisely".into());
    }
    operations.push(numeric("Tm", &context.line));
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
        lines: lines.len(),
    })
}
