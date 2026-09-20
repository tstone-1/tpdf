//! Preserve bounded Form XObjects as read-only content. A form restores graphics
//! state on return and clips painting to its transformed BBox. If it contains
//! text, reserve that entire box against new text layouts; never expose its shows
//! as page operator addresses or rewrite its resources.
use super::{colors, dictionary, filters, images, number, MAX_CONTENT, MAX_OPERATIONS};
use lopdf::{content::Content, Dictionary, Document, Object, ObjectId};
use std::collections::BTreeSet;

#[cfg(test)]
mod tests;

const INVALID: &str = "unsupported preserved Form XObject";

// A form is only read, never rewritten, so it may be larger than the page
// content the editor patches: pdfTeX includes a plotted figure as one form,
// and one arXiv figure holds 4.7 MB and 288,594 operators. Each top-level form
// is parsed and dropped before the next, so the operator bound is what sets
// the worker's peak: that figure took it from 41 MB to 181 MB on Windows
// against the 1 GiB commit cap (sandbox_win::WORKER_MEMORY_CAP).
const MAX_FORM_CONTENT: usize = 8 * MAX_CONTENT;
const MAX_FORM_OPERATIONS: usize = 32 * MAX_OPERATIONS;

#[derive(Clone)]
pub(super) struct Form {
    pub bytes: usize,
    pub text_bounds: Option<[f64; 4]>,
    /// The form's own BBox under its Matrix, which is everything it may paint
    /// (ISO 32000-1 8.10.2 clips a form to it). `text_bounds` is the same
    /// rectangle and is `None` unless the form shows text; this one is always
    /// there, because a figure with no text is still something the editor must
    /// not push a line of text onto.
    pub bounds: [f64; 4],
}

pub(super) fn check(
    doc: &Document,
    resources: &Dictionary,
    name: &[u8],
    remaining: usize,
) -> Result<Option<Form>, String> {
    let objects = dictionary(doc, resources.get(b"XObject").map_err(|_| INVALID)?)?;
    let value = objects.get(name).map_err(|_| INVALID)?;
    let stream = crate::encoding::resolve(doc, value)
        .as_stream()
        .map_err(|_| INVALID)?;
    if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Form") {
        return Ok(None);
    }
    let mut budget = Budget {
        remaining,
        content: MAX_FORM_CONTENT,
        operations: 0,
        calls: 0,
        active: BTreeSet::new(),
    };
    let (text_bounds, bounds) = visit(doc, resources, value, 0, &mut budget)?;
    Ok(Some(Form {
        bytes: remaining - budget.remaining,
        text_bounds,
        bounds,
    }))
}

// Content streams and the images they draw are charged separately: content
// against MAX_FORM_CONTENT for the whole form tree, and
// both against the page's image budget passed in as `remaining`, as an image
// drawn by the page itself is.
struct Budget {
    remaining: usize,
    content: usize,
    operations: usize,
    calls: usize,
    active: BTreeSet<ObjectId>,
}

fn visit(
    doc: &Document,
    inherited: &Dictionary,
    value: &Object,
    depth: usize,
    budget: &mut Budget,
) -> Result<(Option<[f64; 4]>, [f64; 4]), String> {
    budget.calls += 1;
    let id = value.as_reference().map_err(|_| INVALID)?;
    if depth >= 8 || budget.calls > 32 || !budget.active.insert(id) {
        return Err(INVALID.into());
    }
    let form = doc
        .get_object(id)
        .and_then(Object::as_stream)
        .map_err(|_| INVALID)?;
    for (key, value) in &form.dict {
        match (key.as_slice(), value) {
            (b"Type", Object::Name(name)) if name == b"XObject" => {}
            (b"Subtype", Object::Name(name)) if name == b"Form" => {}
            (b"FormType", Object::Integer(1)) => {}
            (b"Name", Object::Name(name)) if name.len() <= 127 => {}
            // Producer identifier, with no rendering or extraction semantics.
            (b"StampId", Object::String(bytes, _)) if bytes.len() <= 127 => {}
            (b"StampId", Object::Integer(_)) => {}
            (b"StampId", Object::Null) => {}
            // ISO 32000-1 14.5 and 7.8.4: private application data and the date
            // it was written. Neither is painted; both are carried unchanged.
            (b"PieceInfo", _) => {
                dictionary(doc, value)?;
            }
            (b"LastModified", Object::String(bytes, _)) if bytes.len() <= 127 => {}
            // pdfTeX's record of an included figure: the file, its page and
            // that file's Info dictionary. None is painted.
            (b"PTEX.FileName", Object::String(bytes, _)) if bytes.len() <= 4096 => {}
            (b"PTEX.PageNumber", Object::Integer(page)) if *page >= 0 => {}
            (b"PTEX.InfoDict", _) => {
                dictionary(doc, value)?;
            }
            // ISO 32000-1 8.11.3.3: the layer this form belongs to. The editor
            // never resolves the layer state, and treats the form as painted
            // either way: reserving the box of a form that turns out to be
            // hidden refuses a layout that would have fitted, while the reverse
            // would let new text land on top of visible graphics.
            (b"OC", _) => {
                let group = dictionary(doc, value)?;
                if !matches!(
                    group.get(b"Type").and_then(Object::as_name).ok(),
                    Some(b"OCG") | Some(b"OCMD")
                ) {
                    return Err(INVALID.into());
                }
            }
            (b"Group", _) => group(doc, value)?,
            (b"BBox" | b"Matrix" | b"Resources" | b"Length" | b"Filter", _) => {}
            _ => return Err(INVALID.into()),
        }
    }
    if form.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Form") {
        return Err(INVALID.into());
    }
    let bounds = crate::encoding::resolve(doc, form.dict.get(b"BBox").map_err(|_| INVALID)?)
        .as_array()
        .map_err(|_| INVALID)?;
    if bounds.len() != 4 {
        return Err(INVALID.into());
    }
    let bounds = bounds.iter().map(number).collect::<Result<Vec<_>, _>>()?;
    if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
        return Err(INVALID.into());
    }
    let mut matrix = [1., 0., 0., 1., 0., 0.];
    if let Ok(value) = form.dict.get(b"Matrix") {
        let values = crate::encoding::resolve(doc, value)
            .as_array()
            .map_err(|_| INVALID)?;
        if values.len() != 6 {
            return Err(INVALID.into());
        }
        for (dest, value) in matrix.iter_mut().zip(values) {
            *dest = number(value)?;
        }
    }
    // Validate finite, invertible transforms with the same bounds as page CTMs.
    matrix = super::compose_affine([1., 0., 0., 1., 0., 0.], matrix)?;
    let resources = match form.dict.get(b"Resources") {
        Ok(value) => dictionary(doc, value)?,
        Err(_) => inherited,
    };
    let bytes = filters::decode(form, budget.content.min(budget.remaining))?;
    budget.content -= bytes.len();
    budget.remaining -= bytes.len();
    let content = Content::decode_strict(&bytes).map_err(|_| INVALID)?;
    budget.operations += content.operations.len();
    if budget.operations > MAX_FORM_OPERATIONS {
        return Err(INVALID.into());
    }
    let mut has_text = false;
    let mut inside = false;
    let mut stack = 0;
    for op in &content.operations {
        match (op.operator.as_str(), op.operands.as_slice()) {
            ("BT", []) if !inside => {
                inside = true;
                has_text = true;
            }
            ("ET", []) if inside => inside = false,
            ("q", []) if !inside && stack < 64 => stack += 1,
            ("Q", []) if !inside && stack > 0 => stack -= 1,
            ("gs", [Object::Name(name)]) => {
                super::graphics::normal(doc, resources, name)?;
            }
            ("Do", [Object::Name(name)]) if !inside => {
                let objects = dictionary(doc, resources.get(b"XObject").map_err(|_| INVALID)?)?;
                let child = objects.get(name).map_err(|_| INVALID)?;
                let stream = crate::encoding::resolve(doc, child)
                    .as_stream()
                    .map_err(|_| INVALID)?;
                if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Form") {
                    has_text |= visit(doc, resources, child, depth + 1, budget)?.0.is_some();
                } else {
                    let image = images::check(doc, resources, name, budget.remaining)?;
                    // A form's fill colour is not tracked, and a stencil paints it.
                    if image.stencil {
                        return Err(INVALID.into());
                    }
                    budget.remaining -= image.bytes;
                }
            }
            // These operations are confined to the form's automatic graphics
            // save/restore and bounding clip. They are preserved, not interpreted
            // as editable text; inline images and recursive/external carriers
            // remain outside this profile.
            (
                "cm" | "w" | "J" | "j" | "M" | "d" | "ri" | "i" | "m" | "l" | "c" | "v" | "y" | "h"
                | "re" | "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" | "W" | "W*"
                | "G" | "g" | "RG" | "rg" | "K" | "k",
                _,
            ) => {}
            // ISO 32000-1 Table 51: text state belongs to both the page
            // description and the text object level, and Acrobat's page-number
            // stamps set it before BT. Positioning and showing need the text
            // object, whose own state this form restores on return either way.
            ("Tc" | "Tw" | "Tz" | "TL" | "Tf" | "Tr" | "Ts", _) => {}
            ("Td" | "TD" | "Tm" | "T*" | "Tj" | "TJ" | "'" | "\"", _) if inside => {}
            _ => return Err(INVALID.into()),
        }
    }
    if inside || stack != 0 {
        return Err(INVALID.into());
    }
    budget.active.remove(&id);
    let box_bounds = super::text_bounds(matrix, [bounds[0], bounds[1], bounds[2], bounds[3]]);
    Ok((has_text.then_some(box_bounds), box_bounds))
}

// ISO 32000-1 11.6.6, Table 147: a transparency group composites the form's
// content as one unit before it meets the page (Inkscape writes one around the
// Creative Commons badge LaTeX papers include). The form is kept unchanged and
// nothing the editor writes lies inside it, so the group describes painting the
// editor never redoes; only its grammar and colour space are checked.
fn group(doc: &Document, value: &Object) -> Result<(), String> {
    let group = dictionary(doc, value)?;
    if group.get(b"S").and_then(Object::as_name).ok() != Some(b"Transparency") {
        return Err(INVALID.into());
    }
    for (key, value) in group {
        match (key.as_slice(), value) {
            (b"Type", Object::Name(name)) if name == b"Group" => {}
            (b"S", _) => {}
            (b"CS", _) => {
                colors::space(doc, value)?;
            }
            (b"I" | b"K", Object::Boolean(_)) => {}
            _ => return Err(INVALID.into()),
        }
    }
    Ok(())
}
