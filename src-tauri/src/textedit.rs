//! Conservative content-stream text editing, executed in the document worker.
//!
//! Supported text uses Helvetica with WinAnsi/default encoding or validated
//! embedded TrueType/CFF glyphs, with explicit positioning between shows.
//! Font/leading setup may precede a text block.
//! Complete painted rectangles and straight-line strokes are preserved, and
//! bounded character spacing is retained. Other graphics, custom text state and implicit
//! advances between shows are refused.
//! Addresses refer to decoded operators, never PDFium's text-object ordinals.

mod clipping;
mod colors;
mod filters;
mod fonts;
mod graphics;
mod streams;
mod tagging;

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{content::Content, Dictionary, Document, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_CONTENT: usize = 1024 * 1024;
const MAX_OPERATIONS: usize = 4096;
pub(crate) const MAX_TEXT: usize = 4096;
pub(crate) const MAX_CHANGES: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub operator: u32,
    pub text: String,
    pub font: String,
    pub size: f64,
    /// Text matrix in the page's original user space, before crop and rotation.
    pub matrix: [f64; 6],
    pub advance: f64,
    /// Hit rectangle in the original displayed page, before journal crop and turns.
    pub display_rect: [f32; 4],
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PageRuns {
    pub page: u32,
    /// Binds operator addresses to the exact decoded content that was inspected.
    pub revision: Vec<u8>,
    pub runs: Vec<Run>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub page: u32,
    pub revision: Vec<u8>,
    pub operator: u32,
    pub original: String,
    pub replacement: String,
}

fn dictionary<'a>(doc: &'a Document, value: &'a Object) -> Result<&'a Dictionary, String> {
    crate::encoding::resolve_dict(doc, value).map_err(|()| "invalid text resources".into())
}

fn resources(doc: &Document, page: ObjectId) -> Result<&Dictionary, String> {
    let mut id = page;
    let mut seen = BTreeSet::new();
    for _ in 0..64 {
        if !seen.insert(id) {
            break;
        }
        let node = doc.get_dictionary(id).map_err(|e| e.to_string())?;
        if let Ok(value) = node.get(b"Resources") {
            return dictionary(doc, value);
        }
        id = node
            .get(b"Parent")
            .and_then(Object::as_reference)
            .map_err(|e| e.to_string())?;
    }
    Err("text resource inheritance exceeds its limit".into())
}

fn font(doc: &Document, resources: &Dictionary, name: &[u8]) -> Result<fonts::Metrics, String> {
    let fonts = dictionary(doc, resources.get(b"Font").map_err(|e| e.to_string())?)?;
    let font = dictionary(doc, fonts.get(name).map_err(|e| e.to_string())?)?;
    if font.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type0") {
        return fonts::composite(doc, font);
    }
    if font.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"TrueType") {
        return fonts::embedded(doc, font);
    }
    if font.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type1")
        && font.has(b"FontDescriptor")
    {
        return fonts::cff(doc, font);
    }
    for (key, expected) in [
        (b"Type".as_slice(), b"Font".as_slice()),
        (b"Subtype", b"Type1"),
        (b"BaseFont", b"Helvetica"),
    ] {
        if font.get(key).and_then(Object::as_name).ok() != Some(expected) {
            return Err(
                "text editing requires standard Helvetica or a supported embedded TrueType font"
                    .into(),
            );
        }
    }
    if font.iter().any(|(key, _)| {
        !matches!(
            key.as_slice(),
            b"Type" | b"Subtype" | b"BaseFont" | b"Encoding" | b"Name"
        )
    }) {
        return Err("custom font metrics or character mappings are not editable yet".into());
    }
    match font.get(b"Encoding").ok() {
        None => Ok(fonts::Metrics::helvetica_default()),
        Some(Object::Name(name)) if name == b"WinAnsiEncoding" => Ok(fonts::Metrics::helvetica()),
        _ => Err("unsupported standard Helvetica encoding".into()),
    }
}

fn number(value: &Object) -> Result<f64, String> {
    let value = match value {
        Object::Integer(value) => *value as f64,
        Object::Real(value) => f64::from(*value),
        _ => return Err("invalid text position or size".into()),
    };
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        return Err("text position or size exceeds its limit".into());
    }
    Ok(value)
}

// WinAnsi agrees with Latin-1 in these ranges. Bytes 127..159 have different
// glyph mappings and must not be interpreted as Unicode control characters.
fn text_byte(byte: u8) -> bool {
    (32..=126).contains(&byte) || byte >= 160
}

fn decode_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_TEXT || !bytes.iter().all(|&byte| text_byte(byte)) {
        return Err("text editing currently supports printable Latin-1 only".into());
    }
    Ok(bytes.iter().map(|&byte| char::from(byte)).collect())
}

fn encode_text(text: &str) -> Result<Vec<u8>, String> {
    // Each supported character uses at most two UTF-8 bytes and one PDF byte.
    if text.len() > MAX_TEXT * 2 {
        return Err("text replacement exceeds its limit".into());
    }
    let bytes = text
        .chars()
        .map(|ch| {
            u8::try_from(ch as u32)
                .ok()
                .filter(|&byte| text_byte(byte))
                .ok_or("text editing currently supports printable Latin-1 only")
        })
        .collect::<Result<Vec<_>, _>>()?;
    if bytes.len() > MAX_TEXT {
        return Err("text replacement exceeds its limit".into());
    }
    Ok(bytes)
}

fn page_content(doc: &Document, id: ObjectId) -> Result<Vec<u8>, String> {
    let page = doc.get_dictionary(id).map_err(|e| e.to_string())?;
    let contents = page.get(b"Contents").map_err(|e| e.to_string())?;
    let contents = crate::encoding::resolve(doc, contents);
    let streams = match contents {
        Object::Array(values) => values.as_slice(),
        value => std::slice::from_ref(value),
    };
    if streams.len() > 128 {
        return Err("too many page content streams".into());
    }
    let mut bytes = Vec::new();
    for value in streams {
        let stream = crate::encoding::resolve(doc, value)
            .as_stream()
            .map_err(|e| e.to_string())?;
        let remaining = MAX_CONTENT.saturating_sub(bytes.len() + 1);
        let decoded = filters::decode(stream, remaining)?;
        bytes.extend(decoded);
        bytes.push(b'\n');
        if bytes.len() > MAX_CONTENT {
            return Err("page content exceeds its limit".into());
        }
    }
    Ok(bytes)
}

// Td/T* translate the line matrix, not the text matrix advanced by Tj.
// Only diagonal matrices reach here; reject accumulated positions outside the
// same bound used for authored coordinates.
fn move_line(matrix: &mut [f64; 6], x: f64, y: f64) -> Result<(), String> {
    shift_position(matrix, x * matrix[0], y * matrix[3])
}

// The same position bound applies to authored and accumulated line positions.
fn shift_position(matrix: &mut [f64; 6], x: f64, y: f64) -> Result<(), String> {
    matrix[4] += x;
    matrix[5] += y;
    if matrix[4..]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
    {
        return Err("text position exceeds its limit".into());
    }
    Ok(())
}

// ISO 32000-1, 8.3.4: a new matrix acts before the existing CTM. Both inputs
// have already been restricted to nonzero diagonal scales and translations.
// Reflections may cancel between page and text matrices. Require upright axes
// only at a text show, after composing both, before constructing its bounds.
// In particular, the existing scale acts on a new translation, not vice versa.
fn compose_diagonal(outer: [f64; 6], inner: [f64; 6]) -> Result<[f64; 6], String> {
    let result = [
        inner[0] * outer[0],
        0.0,
        0.0,
        inner[3] * outer[3],
        inner[4] * outer[0] + outer[4],
        inner[5] * outer[3] + outer[5],
    ];
    if result
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
        || result[0] == 0.0
        || result[3] == 0.0
    {
        return Err("composed text transform exceeds its limit".into());
    }
    Ok(result)
}

struct Inspection {
    id: ObjectId,
    content: Content,
    bytes: Vec<u8>,
    patched: BTreeSet<usize>,
    runs: PageRuns,
    // The active Tf can precede a restored state, not just the last Tf in the
    // stream. Keep its address privately; display font names can be lossy UTF-8.
    font_operators: BTreeMap<u32, usize>,
    // Unrounded horizontal bounds relative to the authored text origin.
    // A replacement must stay within these as well as the original advance.
    horizontal_bounds: BTreeMap<u32, [f64; 2]>,
    character_spacing: BTreeMap<u32, f64>,
}

// TJ offsets are subtracted in thousandths of text space, before the text/page
// matrices. Keep a single left-to-right envelope: no negative cursor, retreating
// fragment ends, or leading/trailing adjustments. A replacement drops kerning
// within this run and must fit the resulting original advance.
fn array_text(
    values: &[Object],
    metrics: &fonts::Metrics,
    size: f64,
    spacing: f64,
) -> Result<(String, f64, [f64; 2]), String> {
    if values.is_empty()
        || values.len() > MAX_TEXT
        || !matches!(values.first(), Some(Object::String(..)))
        || !matches!(values.last(), Some(Object::String(..)))
    {
        return Err("unsupported kerning array shape or size".into());
    }
    let mut text = String::new();
    let mut characters = 0;
    let mut advance = 0.0;
    let mut furthest = 0.0;
    let mut bounds = [0_f64; 2];
    for value in values {
        if let Object::String(bytes, _) = value {
            let fragment = metrics.decode(bytes)?;
            characters += fragment.chars().count();
            if characters > MAX_TEXT {
                return Err("kerning array text exceeds its limit".into());
            }
            let (width, [left, right]) = metrics.spaced_layout(&fragment, size, spacing)?;
            bounds[0] = bounds[0].min(advance + left);
            bounds[1] = bounds[1].max(advance + right);
            advance += width;
            if advance < furthest {
                return Err("backtracking kerning text is not editable yet".into());
            }
            furthest = advance;
            text.push_str(&fragment);
        } else {
            advance -= number(value)? * size / 1000.0;
        }
        if !advance.is_finite() || !(0.0..=1_000_000.0).contains(&advance) {
            return Err("kerning position exceeds its limit".into());
        }
    }
    Ok((text, advance, bounds))
}

fn inspect(doc: &Document, page: u32) -> Result<Inspection, String> {
    let pages = crate::pagetree::ordered_pages(doc);
    let id = *pages
        .get(page as usize)
        .ok_or("text page is not in this document")?;
    if pages.iter().filter(|&&other| other == id).count() != 1 {
        return Err("a repeated page object is not editable".into());
    }
    let mut tags = tagging::Tags::read(doc, id, &pages)?;
    let bytes = page_content(doc, id)?;
    let content = Content::decode_strict(&bytes).map_err(|e| e.to_string())?;
    if content.operations.len() > MAX_OPERATIONS {
        return Err("text operator count exceeds its limit".into());
    }
    // Discovery promises that deletion can use the byte-preserving writer too.
    streams::rewrite(&bytes, &content, &BTreeSet::new())?;
    let resources = resources(doc, id)?;
    let mut fill_components = colors::named(doc, resources, b"DeviceGray")?;
    let mut colour_spaces = BTreeMap::new();
    let mut graphics_states = BTreeSet::new();
    let mut result = PageRuns {
        page,
        revision: Sha256::digest(&bytes).to_vec(),
        runs: Vec::new(),
    };
    // Never skip unknown operators: graphics and text state can change a Tj's
    // meaning without changing its string. Tf and TL persist across BT/ET;
    // the text/line matrices reset at BT. Every accepted show has a position
    // independent of the preceding show's advance, so shorter edits cannot
    // move following text.
    let mut inside = false;
    let mut positioned = false;
    let mut selected_font = None;
    let mut font_metrics = BTreeMap::new();
    let mut leading = 0.0;
    let mut spacing = 0.0;
    let mut states = Vec::new();
    let mut clip = None;
    let mut path_until = 0;
    let mut page_transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut font_operators = BTreeMap::new();
    let mut horizontal_bounds = BTreeMap::new();
    let mut character_spacing = BTreeMap::new();
    let mut matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for (index, op) in content.operations.iter().enumerate() {
        if index < path_until {
            continue;
        }
        match (op.operator.as_str(), op.operands.as_slice()) {
            ("BMC", [tag]) if !inside => tags.begin(tag, None)?,
            ("BDC", [tag, properties]) if !inside => tags.begin(tag, Some(properties))?,
            ("EMC", []) if !inside => tags.end()?,
            // ISO 32000-1, 8.4.2: font, size and leading are graphics state.
            // Only accept saves outside BT/ET. Preserve every accepted state
            // component; the next BT resets both text matrices.
            ("q", []) if !inside => {
                if states.len() >= 64 {
                    return Err("text graphics-state stack exceeds its limit".into());
                }
                states.push((
                    selected_font,
                    leading,
                    page_transform,
                    fill_components,
                    clip,
                    spacing,
                ));
            }
            ("Q", []) if !inside => {
                (
                    selected_font,
                    leading,
                    page_transform,
                    fill_components,
                    clip,
                    spacing,
                ) = states.pop().ok_or("unmatched graphics-state restore")?;
            }
            ("re", _) if !inside => {
                if clipping::painted(&content.operations[index..], page_transform)? {
                    if content.operations[index + 1].operator != "n" {
                        tags.paint();
                    }
                    path_until = index + 2;
                } else {
                    clip = Some(clipping::apply(
                        clip,
                        &content.operations[index..],
                        page_transform,
                    )?);
                    path_until = index + 3;
                }
            }
            ("m", _) if !inside => {
                let consumed = clipping::stroked(&content.operations[index..], page_transform)?;
                if content.operations[index + consumed - 1].operator != "n" {
                    tags.paint();
                }
                path_until = index + consumed;
            }
            // Every accepted path is consumed as a complete sequence, so an
            // isolated n outside BT has no pending path or clip to apply.
            ("n", []) if !inside => {}
            ("w", [value]) => clipping::line_width(value)?,
            ("cm", values) if !inside && values.len() == 6 => {
                let mut next = [0.0; 6];
                for (dest, value) in next.iter_mut().zip(values) {
                    *dest = number(value)?;
                }
                if next[0] == 0.0 || next[3] == 0.0 || next[1] != 0.0 || next[2] != 0.0 {
                    return Err(
                        "collapsed, rotated or skewed page content is not editable yet".into(),
                    );
                }
                page_transform = compose_diagonal(page_transform, next)?;
            }
            ("BT", []) if !inside => {
                inside = true;
                positioned = false;
                matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
            }
            ("ET", []) if inside => inside = false,
            // Explicit defaults have the same semantics as an omitted setting.
            // Text state can be set outside BT/ET and persists across blocks.
            // Character spacing is saved by q/Q and checked against the active
            // font at each show. Other nondefault text state remains refused.
            ("Tc", [value]) => spacing = number(value)?,
            ("Tw" | "Ts", [value]) if number(value)? == 0.0 => {}
            ("Tz", [value]) if number(value)? == 100.0 => {}
            ("Tr", [Object::Integer(0)]) => {}
            ("ri", [Object::Name(name)]) => colors::intent(name)?,
            ("gs", [Object::Name(name)]) => {
                if !graphics_states.contains(name) {
                    if graphics_states.len() >= 32 {
                        return Err("too many external text graphics states".into());
                    }
                    graphics::normal(doc, resources, name)?;
                    graphics_states.insert(name.clone());
                }
            }
            ("cs", [Object::Name(name)]) => {
                if !colour_spaces.contains_key(name) {
                    if colour_spaces.len() >= 32 {
                        return Err("too many text colour spaces".into());
                    }
                    colour_spaces.insert(name.clone(), colors::named(doc, resources, name)?);
                }
                fill_components = colour_spaces[name];
            }
            ("sc" | "scn", values) => colors::values(values, fill_components)?,
            ("g" | "rg" | "k", values) => {
                fill_components = match op.operator.as_str() {
                    "g" => 1,
                    "rg" => 3,
                    _ => 4,
                };
                colors::values(values, fill_components)?;
            }
            ("G" | "RG" | "K", values) => {
                // Filled text cannot use stroke colour; preserve the validated
                // setter without changing the independently tracked fill space.
                let components = match op.operator.as_str() {
                    "G" => 1,
                    "RG" => 3,
                    _ => 4,
                };
                colors::values(values, components)?;
            }
            ("Tf", [name, size]) if inside => {
                let name = name.as_name().map_err(|e| e.to_string())?;
                if !font_metrics.contains_key(name) {
                    if font_metrics.len() >= 32 {
                        return Err("too many fonts on an editable page".into());
                    }
                    font_metrics.insert(name.to_vec(), font(doc, resources, name)?);
                }
                let size = number(size)?;
                if !(0.0..=1000.0).contains(&size) || size == 0.0 {
                    return Err("unsupported text size".into());
                }
                selected_font = Some((name, size, index));
            }
            ("TL", [value]) if inside => leading = number(value)?,
            ("Tm", values) if inside && values.len() == 6 => {
                for (dest, value) in matrix.iter_mut().zip(values) {
                    *dest = number(value)?;
                }
                if matrix[0] == 0.0 || matrix[3] == 0.0 || matrix[1] != 0.0 || matrix[2] != 0.0 {
                    return Err("rotated or skewed text is not editable yet".into());
                }
                positioned = true;
            }
            ("Td", [x, y]) if inside => {
                move_line(&mut matrix, number(x)?, number(y)?)?;
                positioned = true;
            }
            ("T*", []) if inside => {
                move_line(&mut matrix, 0.0, -leading)?;
                positioned = true;
            }
            ("Tj", [_]) | ("TJ", [Object::Array(_)]) if inside && positioned => tags.text()?,
            _ => return Err("unsupported text state or positioning between shows".into()),
        }
        if !matches!(op.operator.as_str(), "Tj" | "TJ") {
            continue;
        }
        let (name, size, font_operator) = selected_font.ok_or("text has no explicit font")?;
        positioned = false;
        let geometry = crate::pagetree::displayed_page(doc, id);
        let metrics = font_metrics.get(name).ok_or("missing text font")?;
        let (text, advance, horizontal) = if op.operator == "TJ" {
            array_text(
                op.operands[0].as_array().map_err(|e| e.to_string())?,
                metrics,
                size,
                spacing,
            )?
        } else {
            let text = metrics.decode(op.operands[0].as_str().map_err(|e| e.to_string())?)?;
            let (advance, horizontal) = metrics.spaced_layout(&text, size, spacing)?;
            (text, advance, horizontal)
        };
        let page_matrix = compose_diagonal(page_transform, matrix)?;
        if page_matrix[0] <= 0.0 || page_matrix[3] <= 0.0 {
            return Err("reflected text is not editable yet".into());
        }
        let bounds = [
            page_matrix[4] + horizontal[0] * page_matrix[0],
            page_matrix[5] - size * page_matrix[3] * 0.25,
            page_matrix[4] + horizontal[1] * page_matrix[0],
            page_matrix[5] + size * page_matrix[3],
        ];
        // Include actual horizontal overhang in hit boxes and clipping. The
        // vertical union covers every offered glyph. Writing additionally keeps
        // replacement ink inside these unrounded original horizontal bounds.
        // Standard-font widths cannot prove substituted glyph ink bounds.
        if clip.is_some() {
            let [bottom, top] = metrics
                .vertical_bounds
                .ok_or("clipped text requires validated embedded glyph outlines")?;
            let ink_bounds = [
                bounds[0],
                page_matrix[5] + bottom * size / 1000. * page_matrix[3],
                bounds[2],
                page_matrix[5] + top * size / 1000. * page_matrix[3],
            ];
            clipping::contains(clip, ink_bounds)?;
        }
        let [left, bottom, right, top] = bounds;
        let (ox, oy) = (f64::from(geometry.origin.0), f64::from(geometry.origin.1));
        let display_rect = crate::text::to_device(
            geometry.turns,
            geometry.width,
            geometry.height,
            [left - ox, bottom - oy, right - ox, top - oy],
        );
        if display_rect.iter().any(|v| !v.is_finite()) {
            return Err("text bounds exceed the display range".into());
        }
        font_operators.insert(index as u32, font_operator);
        horizontal_bounds.insert(index as u32, horizontal);
        character_spacing.insert(index as u32, spacing);
        result.runs.push(Run {
            display_rect,
            operator: index as u32,
            text,
            font: String::from_utf8_lossy(name).into_owned(),
            size,
            matrix: page_matrix,
            advance,
        });
    }
    if inside {
        return Err("unterminated text block".into());
    }
    if !states.is_empty() {
        return Err("unterminated graphics-state save".into());
    }
    tags.finish()?;
    Ok(Inspection {
        id,
        content,
        bytes,
        patched: BTreeSet::new(),
        runs: result,
        font_operators,
        horizontal_bounds,
        character_spacing,
    })
}

/// Discover a complete supported page, or explain why it cannot be edited yet.
///
/// # Errors
/// Unsupported content, invalid resources, or exhausted parsing limits.
pub fn scan(doc: &Document, page: u32) -> Result<PageRuns, String> {
    inspect(doc, page).map(|page| page.runs)
}

/// Validate the entire batch before changing any page. Shared streams are cloned.
///
/// # Errors
/// Unsupported, stale, duplicate, unchanged, or overflowing replacements.
pub fn write(doc: &mut Document, changes: &[Change]) -> Result<(), String> {
    if changes.len() > MAX_CHANGES {
        return Err("too many text replacements".into());
    }
    let mut prepared = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for change in changes {
        if !seen.insert((change.page, change.operator)) {
            return Err("duplicate text replacement".into());
        }
        encode_text(&change.replacement)?;
        if change.original == change.replacement {
            return Err("text replacement is unchanged".into());
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = prepared.entry(change.page) {
            entry.insert(inspect(doc, change.page)?);
        }
        let Inspection {
            id,
            content,
            runs,
            font_operators,
            horizontal_bounds,
            character_spacing,
            patched,
            ..
        } = prepared.get_mut(&change.page).ok_or("missing text page")?;
        let run = runs
            .runs
            .iter()
            .find(|run| run.operator == change.operator)
            .ok_or("text run no longer exists")?;
        if change.revision != runs.revision || change.original != run.text {
            return Err("text changed since this run was inspected".into());
        }
        // Use the original operand bytes, not the lossy display name, to resolve
        // a resource. Font names are PDF names and need not be valid UTF-8.
        let font_operator = font_operators
            .get(&change.operator)
            .ok_or("missing text font")?;
        let name = content.operations[*font_operator].operands[0]
            .as_name()
            .map_err(|e| e.to_string())?;
        let metrics = font(doc, resources(doc, *id)?, name)?;
        let spacing = *character_spacing
            .get(&change.operator)
            .ok_or("missing character spacing")?;
        let (replacement_advance, replacement_bounds) =
            metrics.spaced_layout(&change.replacement, run.size, spacing)?;
        if replacement_advance > run.advance + 0.000_001 {
            return Err("replacement would exceed the original text advance".into());
        }
        let original = horizontal_bounds
            .get(&change.operator)
            .ok_or("missing text ink bounds")?;
        if replacement_bounds[0] < original[0] || replacement_bounds[1] > original[1] {
            return Err("replacement ink would exceed the original text bounds".into());
        }
        let replacement = metrics.encode(&change.replacement)?;
        patched.insert(change.operator as usize);
        let show = &mut content.operations[change.operator as usize];
        let replacement = Object::string_literal(replacement);
        show.operands[0] = if show.operator == "TJ" {
            Object::Array(vec![replacement])
        } else {
            replacement
        };
    }
    let ready = prepared
        .into_values()
        .map(
            |Inspection {
                 id,
                 content,
                 bytes,
                 patched,
                 ..
             }| {
                streams::rewrite(&bytes, &content, &patched).map(|bytes| (id, bytes))
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    for (page, bytes) in ready {
        let stream = doc.add_object(Stream::new(Dictionary::new(), bytes));
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .map_err(|e| e.to_string())?
            .set("Contents", stream);
    }
    Ok(())
}

#[cfg(test)]
mod reflected_tests;

#[cfg(test)]
mod spacing_tests;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn textedit_kerning_geometry_and_rewrite_preserve_other_shows() {
        let mut doc = with_content(b"q 2 0 0 3 0 0 cm BT /F1 20 Tf 40 TL 10 50 Td [(A) 120 (W) -50 (AY)] TJ T* [(SECOND) -20 ( LINE)] TJ ET Q");
        let before = scan(&doc, 0).unwrap();
        let run = &before.runs[0];
        let expected = crate::textbox::advance("AWAY", 20.) - 1.4;
        assert_eq!(run.text, "AWAY");
        assert!((run.advance - expected).abs() < 1e-6);
        assert!(
            (f64::from(run.display_rect[2] - run.display_rect[0]) - expected * 2.).abs() < 1e-4
        );
        assert_eq!(run.matrix, [2., 0., 0., 3., 20., 150.]);
        let operations = inspect(&doc, 0).unwrap().content.operations;
        let update = Change {
            replacement: "A".into(),
            ..change(&doc)
        };
        write(&mut doc, std::slice::from_ref(&update)).unwrap();
        let after = inspect(&doc, 0).unwrap();
        assert_eq!(after.runs.runs[0].text, "A");
        assert_eq!(after.runs.runs[1], before.runs[1]);
        for (index, (actual, original)) in
            after.content.operations.iter().zip(&operations).enumerate()
        {
            assert_eq!(actual.operator, original.operator);
            if index == update.operator as usize {
                assert_eq!(
                    actual.operands,
                    vec![Object::Array(vec![Object::string_literal("A")])]
                );
            } else {
                assert_eq!(actual.operands, original.operands);
            }
        }
        let empty = Change {
            replacement: String::new(),
            ..change(&doc)
        };
        write(&mut doc, &[empty]).unwrap();
        assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "");
    }

    #[test]
    fn textedit_kerning_overflow_is_checked_against_adjusted_width() {
        let mut doc = with_content(b"BT /F1 12 Tf 40 180 Td [(W) 500 (W)] TJ ET");
        // WA fits the unkerned WW, but exceeds WW with its authored adjustment.
        let update = Change {
            replacement: "WA".into(),
            ..change(&doc)
        };
        let before = doc.objects.clone();
        assert!(write(&mut doc, &[update])
            .unwrap_err()
            .contains("exceed the original"));
        assert_eq!(doc.objects, before);
    }

    #[test]
    fn textedit_kerning_refuses_malformed_unbounded_and_retreating_arrays() {
        for array in [
            "[]",
            "[1 (TEXT)]",
            "[(TEXT) 1]",
            "[(A) [0] (B)]",
            "[(A) /Name (B)]",
            "[(A) null (B)]",
            "[(A) true (B)]",
            "[(A) 1000001 (B)]",
            "[(A) -1000001 (B)]",
            "[(A) 9999 (B)]",
            "[(WWW) 2000 (i)]",
            "[(A) -600000 -600000 (B)]",
        ] {
            let content = format!("BT /F1 1000 Tf 40 180 Td {array} TJ ET");
            assert!(
                scan(&with_content(content.as_bytes()), 0).is_err(),
                "accepted {array}"
            );
        }
        for content in [
            "BT /F1 12 Tf 40 180 Td [(TEXT)] Tj ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) TJ ET",
            "BT /F1 12 Tf 40 180 Td [(TEXT)] [(MORE)] TJ ET",
            "BT /F1 12 Tf [(TEXT)] TJ ET",
            "BT /F1 12 Tf 40 180 Td [(TEXT)] TJ (MORE) Tj ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj [(MORE)] TJ ET",
            "BT /F1 12 Tf 40 180 Td [(TEXT)] TJ [(MORE)] TJ ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj ET [(MORE)] TJ",
        ] {
            assert!(
                scan(&with_content(content.as_bytes()), 0).is_err(),
                "accepted {content}"
            );
        }
    }

    #[test]
    fn textedit_kerning_bounds_total_characters_and_array_items() {
        for (body, accepted) in [
            (
                format!(
                    "({}) ({})",
                    "A".repeat(MAX_TEXT / 2),
                    "A".repeat(MAX_TEXT / 2)
                ),
                true,
            ),
            (format!("({}) (B)", "A".repeat(MAX_TEXT)), false),
            ("() ".repeat(MAX_TEXT), true),
            ("() ".repeat(MAX_TEXT + 1), false),
        ] {
            let content = format!("BT /F1 12 Tf 40 180 Td [{body}] TJ ET");
            assert_eq!(scan(&with_content(content.as_bytes()), 0).is_ok(), accepted);
        }
        let mut content = b"BT /F1 12 Tf 40 180 Td [(".to_vec();
        content.extend(vec![0xe4; MAX_TEXT / 2]);
        content.extend(b") (".to_vec());
        content.extend(vec![0xdf; MAX_TEXT / 2]);
        content.extend(b")] TJ ET");
        assert_eq!(
            scan(&with_content(&content), 0).unwrap().runs[0]
                .text
                .chars()
                .count(),
            MAX_TEXT
        );
        for byte in [0, 31, 127, 159] {
            let content = [
                b"BT /F1 12 Tf 40 180 Td [(A) (".as_slice(),
                &[byte],
                b")] TJ ET",
            ]
            .concat();
            assert!(scan(&with_content(&content), 0).is_err());
        }
    }

    #[test]
    fn textedit_latin1_uses_single_pdf_bytes_and_bounds_characters() {
        assert_eq!(
            encode_text("ÄÖÜ äöü ß î ø").unwrap(),
            b"\xC4\xD6\xDC \xE4\xF6\xFC \xDF \xEE \xF8"
        );
        let bytes: Vec<u8> = (32..=126).chain(160..=255).collect();
        assert_eq!(encode_text(&decode_text(&bytes).unwrap()).unwrap(), bytes);
        for byte in (0..32).chain(127..160) {
            assert!(decode_text(&[byte]).is_err());
            assert!(encode_text(&char::from(byte).to_string()).is_err());
        }
        for text in ["α", "€", "a\u{308}", "日本語"] {
            assert!(encode_text(text).is_err());
        }
        assert_eq!(encode_text(&"ä".repeat(MAX_TEXT)).unwrap().len(), MAX_TEXT);
        assert!(encode_text(&"ä".repeat(MAX_TEXT + 1)).is_err());
        assert!(decode_text(&vec![0xE4; MAX_TEXT + 1]).is_err());
    }

    #[test]
    fn textedit_latin1_width_refuses_sharp_s_and_accented_i_overflow() {
        for (original, replacement) in [("s", "ß"), ("i", "î"), ("o", "ø")] {
            let raw = format!("BT /F1 12 Tf 40 180 Td ({original}) Tj ET");
            let mut doc = with_content(raw.as_bytes());
            let change = Change {
                replacement: replacement.into(),
                ..change(&doc)
            };
            let before = doc.objects.clone();
            assert!(write(&mut doc, &[change])
                .unwrap_err()
                .contains("exceed the original"));
            assert_eq!(doc.objects, before);
        }
    }

    #[test]
    fn textedit_latin1_rewrites_and_rediscovers_without_utf8_in_the_operand() {
        let mut doc = fixture();
        let update = Change {
            replacement: "GEPRÜFT ß".into(),
            ..change(&doc)
        };
        write(&mut doc, &[update]).unwrap();
        let Inspection { content, runs, .. } = inspect(&doc, 0).unwrap();
        assert_eq!(runs.runs[0].text, "GEPRÜFT ß");
        assert_eq!(
            content.operations[runs.runs[0].operator as usize].operands[0]
                .as_str()
                .unwrap(),
            b"GEPR\xDCFT \xDF"
        );
        assert_eq!(runs.runs[1].text, "SYNTHETIC SECOND");
        let update = Change {
            replacement: "ASCII".into(),
            ..change(&doc)
        };
        write(&mut doc, &[update]).unwrap();
        assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "ASCII");
    }

    pub(crate) fn fixture() -> Document {
        let mut doc = Document::with_version("1.7");
        let root = doc.new_object_id();
        let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
        let contents = doc.add_object(Stream::new(Dictionary::new(), b"BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET\nBT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET".to_vec()));
        let pages: Vec<Object> = (0..2)
            .map(|_| {
                doc.add_object(dictionary! {
                    "Type" => "Page", "Parent" => root, "Contents" => contents,
                })
                .into()
            })
            .collect();
        doc.objects.insert(
            root,
            dictionary! { "Type" => "Pages", "Kids" => pages, "Count" => 2,
                "MediaBox" => vec![0.into(), 0.into(), 300.into(), 240.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }
            }
            .into(),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        doc.trailer.set("Root", catalog);
        doc
    }

    pub(crate) fn change(doc: &Document) -> Change {
        let runs = scan(doc, 0).unwrap();
        Change {
            page: 0,
            revision: runs.revision,
            operator: runs.runs[0].operator,
            original: runs.runs[0].text.clone(),
            replacement: "EDITED FIRST".into(),
        }
    }

    fn with_content(bytes: &[u8]) -> Document {
        let mut doc = fixture();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.to_vec()));
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
        doc
    }

    #[test]
    fn textedit_reportlab_font_setup_and_multiline_positions_survive_shorter_edits() {
        // Produced independently by testdata/make_textedit_reportlab.py.
        let mut doc = with_content(b"1 0 0 1 0 0 cm BT /F1 12 Tf 14.4 TL ET\nBT 1 0 0 1 40 180 Tm 40 TL (SYNTHETIC FIRST) Tj T* (SYNTHETIC SECOND) Tj T* ET");
        let Inspection {
            content: before,
            runs: mapped,
            ..
        } = inspect(&doc, 0).unwrap();
        assert_eq!(
            mapped.runs.iter().map(|r| r.operator).collect::<Vec<_>>(),
            [8, 10]
        );
        assert_eq!(mapped.runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
        assert_eq!(mapped.runs[1].matrix, [1., 0., 0., 1., 40., 140.]);
        assert_eq!(mapped.runs[1].size, 12.);
        let mut edit = change(&doc);
        edit.replacement = "X".into();
        write(&mut doc, &[edit]).unwrap();
        let Inspection {
            content: after,
            runs: saved,
            ..
        } = inspect(&doc, 0).unwrap();
        assert_eq!(saved.runs[0].text, "X");
        assert_eq!(saved.runs[1], mapped.runs[1]);
        assert_eq!(before.operations.len(), after.operations.len());
        for (index, (a, b)) in before.operations.iter().zip(&after.operations).enumerate() {
            if index != 8 {
                assert_eq!(a.operator, b.operator, "changed operator {index}");
                assert_eq!(a.operands, b.operands, "changed operands {index}");
            }
        }
    }

    #[test]
    fn textedit_line_positions_use_scaled_line_matrix_and_reset_on_bt() {
        let doc = with_content(b"BT /F1 12 Tf 20 TL 2 0 0 3 40 180 Tm (FIRST) Tj 10 -10 Td (SECOND) Tj T* (THIRD) Tj ET BT 5 200 Td (FOURTH) Tj T* (FIFTH) Tj ET");
        let runs = scan(&doc, 0).unwrap().runs;
        assert_eq!(
            runs.iter()
                .map(|r| [r.matrix[4], r.matrix[5]])
                .collect::<Vec<_>>(),
            [[40., 180.], [60., 150.], [60., 90.], [5., 200.], [5., 180.]]
        );
        assert_eq!(runs[3].matrix[..4], [1., 0., 0., 1.]);
        assert!(runs.iter().all(|r| r.size == 12.));
    }

    #[test]
    fn textedit_graphics_stack_restores_font_size_and_leading() {
        let mut doc = with_content(b"BT /F1 12 Tf 40 TL ET q BT /F1 8 Tf 10 TL ET q BT /F1 6 Tf 5 TL ET Q BT 40 180 Td (INNER) Tj T* (LINE) Tj ET Q BT 40 140 Td (OUTER) Tj T* (LINE) Tj ET");
        let before = scan(&doc, 0).unwrap();
        assert_eq!(
            before.runs.iter().map(|r| r.size).collect::<Vec<_>>(),
            [8., 8., 12., 12.]
        );
        assert_eq!(
            before.runs.iter().map(|r| r.matrix[5]).collect::<Vec<_>>(),
            [180., 170., 140., 100.]
        );
        let update = Change {
            replacement: "IN".into(),
            ..change(&doc)
        };
        write(&mut doc, &[update]).unwrap();
        let after = scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1..], before.runs[1..]);
    }

    #[test]
    fn textedit_graphics_stack_requires_balanced_bounded_outer_saves() {
        for bytes in [
            "Q BT /F1 12 Tf 40 180 Td (TEXT) Tj ET",
            "q BT /F1 12 Tf 40 180 Td (TEXT) Tj ET",
            "BT /F1 12 Tf q 40 180 Td (TEXT) Tj Q ET",
            "q BT /F1 12 Tf Q 40 180 Td (TEXT) Tj ET",
            "q BT /F1 12 Tf ET Q BT 40 180 Td (TEXT) Tj ET",
            "1 q BT /F1 12 Tf 40 180 Td (TEXT) Tj ET Q",
        ] {
            let doc = with_content(bytes.as_bytes());
            assert!(scan(&doc, 0).is_err(), "accepted {bytes}");
        }
        for depth in [64, 65] {
            let bytes = format!(
                "{}BT /F1 12 Tf 40 180 Td (TEXT) Tj ET {}",
                "q ".repeat(depth),
                "Q ".repeat(depth)
            );
            assert_eq!(
                scan(&with_content(bytes.as_bytes()), 0).is_ok(),
                depth == 64
            );
        }
    }

    #[test]
    fn textedit_page_translations_compose_restore_and_preserve_following_runs() {
        let mut doc = with_content(b"BT /F1 12 Tf 40 TL ET q 1 0 0 1 20 100 cm q 1 0 0 1 10 20 cm BT 2 0 0 3 10 60 Tm (FIRST) Tj T* (SECOND) Tj ET Q BT 1 0 0 1 20 40 Tm (THIRD) Tj ET Q BT 40 100 Td (FOURTH) Tj ET");
        let before = scan(&doc, 0).unwrap();
        assert_eq!(
            before
                .runs
                .iter()
                .map(|r| [r.matrix[4], r.matrix[5]])
                .collect::<Vec<_>>(),
            [[40., 180.], [40., 60.], [40., 140.], [40., 100.]]
        );
        assert_eq!(before.runs[0].matrix[..4], [2., 0., 0., 3.]);
        let update = Change {
            replacement: "IN".into(),
            ..change(&doc)
        };
        write(&mut doc, &[update]).unwrap();
        let after = scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1..], before.runs[1..]);
    }

    #[test]
    fn textedit_translated_hitboxes_match_absolute_positions_after_crop_and_rotation() {
        let translated = b"q 1 0 0 1 30 150 cm BT /F1 12 Tf 2 0 0 3 10 30 Tm (TEXT) Tj ET Q";
        let absolute = b"BT /F1 12 Tf 2 0 0 3 40 180 Tm (TEXT) Tj ET";
        for rotation in [0, 90, 180, 270] {
            let mut results = Vec::new();
            for bytes in [translated.as_slice(), absolute.as_slice()] {
                let mut doc = with_content(bytes);
                let page = crate::pagetree::ordered_pages(&doc)[0];
                let page = doc.get_dictionary_mut(page).unwrap();
                page.set("Rotate", rotation);
                page.set(
                    "CropBox",
                    vec![10.into(), 20.into(), 290.into(), 220.into()],
                );
                let run = scan(&doc, 0).unwrap().runs.remove(0);
                results.push((run.matrix, run.display_rect));
            }
            assert_eq!(results[0], results[1], "rotation {rotation}");
        }
    }

    #[test]
    fn textedit_explicit_defaults_preserve_geometry_and_saved_operators() {
        let baseline = with_content(b"BT /F1 12 Tf 40 TL 40 180 Td (FIRST) Tj T* (SECOND) Tj ET");
        let mut doc = with_content(b"0 Tc 0 Tw 100 Tz 0 Ts 0 Tr q BT /F1 12 Tf 40 TL 40 180 Td 0.0 Tc -0.0 Tw 100.0 Tz 0.0 Ts 0 Tr (FIRST) Tj 0 Tc 0 Tw 100 Tz 0 Ts 0 Tr T* (SECOND) Tj ET Q");
        let expected = scan(&baseline, 0).unwrap();
        let before = scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), expected.runs.len());
        for (actual, expected) in before.runs.iter().zip(expected.runs) {
            let mut actual = actual.clone();
            actual.operator = expected.operator;
            assert_eq!(actual, expected);
        }
        // Untouched operands retain their original spelling, including real zero.
        let operations = inspect(&doc, 0).unwrap().content.operations;
        let update = Change {
            replacement: "IN".into(),
            ..change(&doc)
        };
        write(&mut doc, std::slice::from_ref(&update)).unwrap();
        let after = inspect(&doc, 0).unwrap();
        assert_eq!(after.runs.runs[0].text, "IN");
        assert_eq!(after.runs.runs[1], before.runs[1]);
        assert_eq!(after.content.operations.len(), operations.len());
        for (index, (actual, expected)) in
            after.content.operations.iter().zip(operations).enumerate()
        {
            if index != update.operator as usize {
                assert_eq!(actual.operator, expected.operator);
                assert_eq!(actual.operands, expected.operands);
            }
        }
    }

    #[test]
    fn textedit_default_setters_refuse_nondefault_and_malformed_operands() {
        for (operator, default) in [("Tw", "0"), ("Ts", "0"), ("Tz", "100"), ("Tr", "0")] {
            for operand in [
                "", "0 0", "(0)", "/Zero", "[0]", "true", "null", "1", "-1", "0.01", "99.99",
                "100.01", "1000001",
            ] {
                // A later default setter must not hide an earlier unsupported one.
                let bytes = format!(
                    "{operand} {operator} {default} {operator} BT /F1 12 Tf 40 180 Td (TEXT) Tj ET"
                );
                assert!(
                    scan(&with_content(bytes.as_bytes()), 0).is_err(),
                    "accepted {operand} {operator}"
                );
            }
        }
        // Rendering mode is an integer; modes 1..7 include stroke, hidden and clip.
        for mode in ["0.0", "1", "2", "3", "4", "5", "6", "7"] {
            let bytes = format!("BT /F1 12 Tf 40 180 Td {mode} Tr (TEXT) Tj ET");
            assert!(
                scan(&with_content(bytes.as_bytes()), 0).is_err(),
                "accepted {mode} Tr"
            );
        }
    }

    #[test]
    fn textedit_default_setters_do_not_position_a_following_show() {
        for setting in ["0 Tc", "0 Tw", "100 Tz", "0 Ts", "0 Tr"] {
            for content in [
                format!("BT /F1 12 Tf {setting} (TEXT) Tj ET"),
                format!("BT /F1 12 Tf 40 180 Td (FIRST) Tj {setting} (SECOND) Tj ET"),
            ] {
                assert!(
                    scan(&with_content(content.as_bytes()), 0).is_err(),
                    "accepted {content}"
                );
            }
        }
    }

    #[test]
    fn textedit_page_scales_compose_restore_and_preserve_following_runs() {
        let mut doc = with_content(b"BT /F1 12 Tf 4 TL ET q 2 0 0 3 20 30 cm q .5 0 0 2 -5 10 cm BT 2 0 0 3 10 20 Tm (FIRST) Tj T* (SECOND) Tj ET Q BT 1 0 0 1 10 20 Tm (THIRD) Tj ET Q BT 40 140 Td (FOURTH) Tj ET");
        let before = scan(&doc, 0).unwrap();
        assert_eq!(
            before.runs.iter().map(|r| r.matrix).collect::<Vec<_>>(),
            [
                [2., 0., 0., 18., 20., 180.],
                [2., 0., 0., 18., 20., 108.],
                [2., 0., 0., 3., 40., 90.],
                [1., 0., 0., 1., 40., 140.],
            ]
        );
        let update = Change {
            replacement: "IN".into(),
            ..change(&doc)
        };
        write(&mut doc, &[update]).unwrap();
        let after = scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1..], before.runs[1..]);
        let unchanged = doc.clone();
        let overflow = Change {
            replacement: "TOO LONG".into(),
            ..change(&doc)
        };
        assert!(write(&mut doc, &[overflow]).is_err());
        assert_eq!(doc.objects, unchanged.objects);
    }

    #[test]
    fn textedit_scaled_hitboxes_match_absolute_matrices_after_crop_and_rotation() {
        let scaled =
            b"q 2 0 0 .5 20 150 cm 1 0 0 1 10 20 cm BT /F1 12 Tf 3 0 0 4 5 40 Tm (TEXT) Tj ET Q";
        let absolute = b"BT /F1 12 Tf 6 0 0 2 50 180 Tm (TEXT) Tj ET";
        for rotation in [0, 90, 180, 270] {
            let mut results = Vec::new();
            for bytes in [scaled.as_slice(), absolute.as_slice()] {
                let mut doc = with_content(bytes);
                let page = crate::pagetree::ordered_pages(&doc)[0];
                let page = doc.get_dictionary_mut(page).unwrap();
                page.set("Rotate", rotation);
                page.set(
                    "CropBox",
                    vec![10.into(), 20.into(), 290.into(), 220.into()],
                );
                let run = scan(&doc, 0).unwrap().runs.remove(0);
                results.push((run.matrix, run.display_rect, run.advance));
            }
            assert_eq!(results[0], results[1], "rotation {rotation}");
        }
    }

    #[test]
    fn textedit_page_scales_bound_composed_scales_and_positions() {
        for prefix in [
            "1000000 0 0 1 0 0 cm 2 0 0 1 0 0 cm",
            "1 0 0 1000000 0 0 cm 1 0 0 2 0 0 cm",
            "1000000 0 0 1 0 0 cm 1 0 0 1 2 0 cm",
            "1 0 0 1000000 0 0 cm 1 0 0 1 0 -2 cm",
        ] {
            let bytes = format!("{prefix} BT /F1 12 Tf 0 0 Td (TEXT) Tj ET");
            assert!(
                scan(&with_content(bytes.as_bytes()), 0).is_err(),
                "accepted {prefix}"
            );
        }
        // Each authored value is bounded, but the composed text scale is not.
        let doc = with_content(b"1000 0 0 1000 0 0 cm BT /F1 12 Tf 1001 0 0 1 0 0 Tm (TEXT) Tj ET");
        assert!(scan(&doc, 0).is_err());
        // Repeated positive scales must not underflow to a collapsed matrix.
        let bytes = format!(
            "{}BT /F1 12 Tf 0 0 Td (TEXT) Tj ET",
            "0.000001 0 0 1 0 0 cm ".repeat(60)
        );
        assert!(scan(&with_content(bytes.as_bytes()), 0).is_err());
        let at_limit =
            with_content(b"1000 0 0 1000 0 0 cm BT /F1 12 Tf 1000 0 0 1000 0 0 Tm (TEXT) Tj ET");
        assert_eq!(scan(&at_limit, 0).unwrap().runs[0].matrix[0], 1_000_000.);
    }

    #[test]
    fn textedit_page_transforms_refuse_unbounded_or_nondiagonal_matrices() {
        for prefix in [
            "1 0 0 1 1000000 0 cm 1 0 0 1 1 0 cm",
            "1 0 0 1 0 -1000000 cm 1 0 0 1 0 -1 cm",
            "1 0 0 1 1000000 0 cm",
            "1 0 0 1 (bad) 0 cm",
            "1 0 0 1 0 cm",
            "1 0 0 1 0 0 0 cm",
            "1 0 0 1 20 30 cm 0 0 0 1 0 0 cm",
            "1 0.1 0 1 0 0 cm",
            "1 0 0.1 1 0 0 cm",
            "1 0 0 -1 0 0 cm",
        ] {
            let bytes = format!("{prefix} BT /F1 12 Tf 40 180 Td (TEXT) Tj ET");
            assert!(
                scan(&with_content(bytes.as_bytes()), 0).is_err(),
                "accepted {prefix}"
            );
        }
        let doc = with_content(b"BT /F1 12 Tf 1 0 0 1 20 30 cm 40 180 Td (TEXT) Tj ET");
        assert!(scan(&doc, 0).is_err());
    }

    #[test]
    fn textedit_accepts_single_flate_array_and_rejects_invalid_encodings() {
        let mut doc = fixture();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let stream = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        doc.get_object_mut(stream)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .compress()
            .unwrap();
        for (filters, accepted) in [
            (vec![Object::Name(b"FlateDecode".to_vec())], true),
            (vec![], false),
            (vec![Object::Name(b"ASCII85Decode".to_vec())], false),
            (vec![Object::Name(b"FlateDecode".to_vec()); 2], false),
        ] {
            doc.get_object_mut(stream)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .dict
                .set("Filter", Object::Array(filters));
            assert_eq!(scan(&doc, 0).is_ok(), accepted);
        }
    }

    #[test]
    fn textedit_maps_operators_and_clones_shared_streams() {
        let mut doc = fixture();
        let Inspection {
            content: before,
            runs,
            ..
        } = inspect(&doc, 0).unwrap();
        assert_eq!(
            runs.runs.iter().map(|r| r.operator).collect::<Vec<_>>(),
            [3, 8]
        );
        assert_eq!(runs.runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
        let first = change(&doc);
        let second = Change {
            operator: 8,
            original: "SYNTHETIC SECOND".into(),
            replacement: "EDITED SECOND".into(),
            ..first.clone()
        };
        write(&mut doc, &[first, second]).unwrap();
        let Inspection {
            content: after,
            runs: saved,
            ..
        } = inspect(&doc, 0).unwrap();
        assert_eq!(saved.runs[0].text, "EDITED FIRST");
        assert_eq!(saved.runs[1].text, "EDITED SECOND");
        assert_eq!(scan(&doc, 1).unwrap().runs[0].text, "SYNTHETIC FIRST");
        for (a, b) in before.operations.iter().zip(after.operations.iter()) {
            if a.operator != "Tj" {
                assert_eq!(a.operator, b.operator);
                assert_eq!(a.operands, b.operands);
            }
        }
    }

    #[test]
    fn textedit_rejects_invalid_batches_without_mutating_the_document() {
        let doc = fixture();
        let valid = change(&doc);
        let variants = [
            (
                Change {
                    revision: vec![0; 32],
                    ..valid.clone()
                },
                "changed since",
            ),
            (
                Change {
                    original: "WRONG".into(),
                    ..valid.clone()
                },
                "changed since",
            ),
            (
                Change {
                    operator: 4,
                    ..valid.clone()
                },
                "no longer exists",
            ),
            (
                Change {
                    page: 9,
                    ..valid.clone()
                },
                "not in this document",
            ),
            (
                Change {
                    replacement: "Z".repeat(80),
                    ..valid.clone()
                },
                "exceed the original",
            ),
            (
                Change {
                    replacement: "\u{03b1}".into(),
                    ..valid.clone()
                },
                "Latin-1 only",
            ),
            (
                Change {
                    replacement: valid.original.clone(),
                    ..valid.clone()
                },
                "unchanged",
            ),
        ];
        for (bad, expected) in variants {
            let mut copy = doc.clone();
            // Put a valid edit on the other page first, so a late refusal also
            // proves validation does not partially apply a batch.
            let prior = Change {
                page: 1,
                ..valid.clone()
            };
            assert!(write(&mut copy, &[prior, bad])
                .unwrap_err()
                .contains(expected));
            assert_eq!(copy.objects, doc.objects);
            assert_eq!(copy.max_id, doc.max_id);
        }
        let mut copy = doc.clone();
        assert!(write(&mut copy, &[valid.clone(), valid])
            .unwrap_err()
            .contains("duplicate"));
        assert_eq!(copy.objects, doc.objects);
    }

    #[test]
    fn textedit_refuses_unsupported_state_and_font_semantics() {
        for bytes in [
            "BT /F1 12 Tf 40 180 Td (ONE) Tj (TWO) Tj ET",
            "3 Tr BT /F1 12 Tf 40 180 Td (HIDDEN) Tj ET",
            "-2 0 0 2 0 0 cm BT /F1 12 Tf 40 180 Td (REFLECTED) Tj ET",
            "BT /F1 12 Tf 0 1 -1 0 40 180 Tm (ROTATED) Tj ET",
            "/Span << /ActualText (OTHER) >> BDC BT /F1 12 Tf 40 180 Td (TEXT) Tj ET EMC",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj",
            "BT BT /F1 12 Tf 40 180 Td (TEXT) Tj ET ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj ET ET",
            "BT 40 180 Td (TEXT) Tj ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj 4 Tc T* (MORE) Tj ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj /F1 10 Tf (MORE) Tj ET",
            "BT /F1 12 Tf (TEXT) Tj ET",
            "BT /F1 12 Tf 1000000 0 0 1 40 180 Tm 2 0 Td (TEXT) Tj ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj 1 T* ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj ET (OUTSIDE) Tj",
        ] {
            let mut doc = fixture();
            let id = crate::pagetree::ordered_pages(&doc)[0];
            let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.as_bytes().to_vec()));
            doc.get_dictionary_mut(id).unwrap().set("Contents", stream);
            assert!(scan(&doc, 0).is_err(), "accepted {bytes}");
        }
        for key in ["Widths", "ToUnicode", "FontDescriptor"] {
            let mut doc = fixture();
            let font = doc
                .objects
                .values_mut()
                .find_map(|o| o.as_dict_mut().ok().filter(|d| d.has(b"BaseFont")))
                .unwrap();
            font.set(key, Object::Null);
            assert!(scan(&doc, 0)
                .unwrap_err()
                .contains(if key == "FontDescriptor" {
                    "invalid text resources"
                } else {
                    "custom font"
                }));
        }
    }

    #[test]
    fn textedit_validates_empty_replacement_positioning_and_limits() {
        let mut doc = fixture();
        let mut edit = change(&doc);
        edit.replacement.clear();
        write(&mut doc, &[edit]).unwrap();
        assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "");
        assert_eq!(scan(&doc, 0).unwrap().runs[1].matrix[5], 140.);
        assert!(
            write(&mut fixture(), &vec![change(&fixture()); MAX_CHANGES + 1])
                .unwrap_err()
                .contains("too many")
        );
        assert!(decode_text(&vec![b'A'; MAX_TEXT + 1]).is_err());
    }

    #[test]
    fn textedit_rejects_partial_or_undecodable_content() {
        for kind in [
            "bad-reference",
            "bad-filter",
            "invalid-flate",
            "trailing-junk",
            "too-large",
        ] {
            let mut doc = fixture();
            let id = crate::pagetree::ordered_pages(&doc)[0];
            let original = doc
                .get_dictionary(id)
                .unwrap()
                .get(b"Contents")
                .unwrap()
                .clone();
            match kind {
                "bad-reference" => doc
                    .get_dictionary_mut(id)
                    .unwrap()
                    .set("Contents", vec![original, Object::Reference((999999, 0))]),
                _ => {
                    let stream = doc
                        .get_object_mut(original.as_reference().unwrap())
                        .unwrap()
                        .as_stream_mut()
                        .unwrap();
                    match kind {
                        "bad-filter" => stream.dict.set("Filter", 7),
                        "invalid-flate" => stream.dict.set("Filter", "FlateDecode"),
                        "trailing-junk" => stream.content.extend(b"\n(unfinished"),
                        "too-large" => stream.content = vec![b' '; MAX_CONTENT + 1],
                        _ => unreachable!(),
                    }
                }
            }
            assert!(scan(&doc, 0).is_err(), "accepted {kind}");
        }
        let mut compressed = fixture();
        compressed.compress();
        assert_eq!(
            scan(&compressed, 0).unwrap().runs[0].text,
            "SYNTHETIC FIRST"
        );
    }
}
