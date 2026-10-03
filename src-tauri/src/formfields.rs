//! Adding fields to a document's form, or giving it one.
//!
//! `forms.rs` reads a form and answers it; nothing there makes a field exist.
//! This does, for the two kinds a form is mostly made of: a text field, on one
//! line or several, and a checkbox.
//!
//! A field written here is one object that is both the field and its widget,
//! which is what the specification allows for a field with a single widget and
//! what most producers write. It carries its own appearance, so a reader shows
//! it without regenerating anything, and it is added to the page's annotations
//! and to the form's field list together --- a widget in one and not the other
//! is a field `forms::scan` refuses.
//!
//! What is deliberately not here: calculations and formatting, which are
//! JavaScript in the document and tpdf runs none; and a field on a page the
//! document turns with `/Rotate`, whose appearance would have to be turned to
//! match and which `forms::write` does not turn either.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::forms;

/// What kind of field to add.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// One line of text.
    Text,
    /// Text that wraps over several lines.
    Multiline,
    /// A box that is ticked or not.
    Checkbox,
}

/// One field to add.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewField {
    /// The field's name, which is what an answer is addressed to.
    pub name: String,
    pub kind: Kind,
    /// Zero-based page of the document as it is on disk.
    pub page: u32,
    /// `[left, top, width, height]` in points, from the top-left corner of the
    /// page as it is displayed. The convention `tpdf crop --rect` has.
    pub rect: [f64; 4],
    /// What a reader shows when the pointer rests on the field.
    #[serde(default)]
    pub tooltip: Option<String>,
    /// Whether a form that submits itself insists on an answer.
    #[serde(default)]
    pub required: bool,
    /// The most characters a text field takes.
    #[serde(default)]
    pub max_length: Option<u32>,
    /// Whether a text field draws a thin black line round itself.
    ///
    /// An empty text field otherwise draws nothing, which is right on a page
    /// that already shows a line or a box to write on and leaves a field on a
    /// blank area invisible. A checkbox always draws its box and ignores this.
    #[serde(default)]
    pub border: bool,
}

/// What a field placed in the window is: its kind, and whether it is framed.
///
/// The payload of a `MarkKind::Field` mark. Its name is the mark's note and its
/// rectangle the mark's quad, so these two are all that is left to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    pub kind: Kind,
    /// [`NewField::border`].
    #[serde(default)]
    pub border: bool,
}

impl From<Kind> for Placed {
    /// A field of this kind with no border, which is what `tpdf form` adds
    /// unless it is asked for one.
    fn from(kind: Kind) -> Self {
        Self {
            kind,
            border: false,
        }
    }
}

/// How thick a field's border is, in points.
pub const BORDER_WIDTH: f64 = 1.0;

/// Fields one call may add. A form of a thousand fields is a large one.
pub const MAX_NEW: usize = 1000;
/// The longest name, in characters. `forms::scan` refuses one past 1024 bytes,
/// and a name is written as UTF-16.
pub const MAX_NAME: usize = 255;
/// The smallest side of a text field, in points: below it `forms::write` has
/// no size of type that fits.
pub const MIN_TEXT: f64 = 8.0;
/// The smallest side of a checkbox, in points.
pub const MIN_BOX: f64 = 6.0;

/// The default appearance every field written here names: Helvetica, sized by
/// the reader, black. `Helv` is the name producers use for it.
const DEFAULT_APPEARANCE: &str = "/Helv 0 Tf 0 g";

fn text(value: &str) -> Object {
    Object::String(value.as_bytes().to_vec(), lopdf::StringFormat::Literal)
}

/// Whether `name` can be a field's name, and why not.
pub fn name_problem(name: &str) -> Option<String> {
    if name.trim().is_empty() {
        return Some("a field needs a name".into());
    }
    if name != name.trim() {
        return Some(format!("the name `{name}` begins or ends with a space"));
    }
    if name.contains('.') {
        // §12.7.4.2: a period separates a field from its parent in a full name.
        return Some(format!(
            "the name `{name}` contains a period, which a form uses to separate a field from \
             the group it is in"
        ));
    }
    if name.chars().any(char::is_control) {
        return Some("a field's name cannot contain a control character".into());
    }
    if name.chars().count() > MAX_NAME {
        return Some(format!("a field's name is at most {MAX_NAME} characters"));
    }
    None
}

/// The names the form's top-level fields have, which a new one must not repeat.
///
/// Read through the catalog, so a form written into it directly is seen as
/// well as one that is an object of its own.
fn taken(doc: &Document) -> HashSet<String> {
    let mut names = HashSet::new();
    let Some(fields) = doc
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"AcroForm").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .and_then(|form| form.get(b"Fields").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())
    else {
        return names;
    };
    for field in fields {
        let name = doc
            .dereference(field)
            .ok()
            .and_then(|(_, o)| o.as_dict().ok())
            .and_then(|d| d.get(b"T").ok())
            .and_then(|t| t.as_str().ok())
            .map(crate::annots::decode_text_string);
        if let Some(name) = name {
            names.insert(name);
        }
    }
    names
}

/// A field's rectangle in the page's own space, or why it cannot be placed.
fn placed(doc: &Document, page: ObjectId, field: &NewField) -> Result<[f64; 4], String> {
    let name = &field.name;
    let [left, top, width, height] = field.rect;
    if !field.rect.iter().all(|v| v.is_finite()) {
        return Err(format!("`{name}`: its rectangle is not four numbers"));
    }
    let least = least_side(field.kind);
    if width < least || height < least {
        return Err(format!(
            "`{name}`: {width} by {height} points is too small --- a {} needs at least {least} \
             by {least}",
            if field.kind == Kind::Checkbox {
                "checkbox"
            } else {
                "text field"
            }
        ));
    }
    let geometry = crate::pagetree::displayed_page(doc, page);
    if geometry.turns != 0 {
        return Err(format!(
            "`{name}`: page {} is turned by the document, and a field cannot be added to a \
             turned page yet",
            field.page + 1
        ));
    }
    let (page_width, page_height) = (f64::from(geometry.width), f64::from(geometry.height));
    // A hundredth of a point of slack, so a rectangle computed to the page's
    // edge is not refused over the last digit.
    if left < -0.01
        || top < -0.01
        || left + width > page_width + 0.01
        || top + height > page_height + 0.01
    {
        return Err(format!(
            "`{name}`: its rectangle is not inside page {}, which is {page_width} by \
             {page_height} points",
            field.page + 1
        ));
    }
    let x = f64::from(geometry.origin.0) + left;
    let y = f64::from(geometry.origin.1) + page_height - top - height;
    Ok([x, y, x + width, y + height])
}

/// Every reason `fields` cannot be added, or their rectangles in page space.
///
/// All of them are checked before anything is written, for `tpdf fill`'s
/// reason: one problem means nothing is written, and a caller told about the
/// first of five has to run five times.
pub fn check(
    doc: &Document,
    fields: &[NewField],
) -> Result<Vec<(ObjectId, [f64; 4])>, Vec<String>> {
    let mut problems = Vec::new();
    if fields.len() > MAX_NEW {
        return Err(vec![format!(
            "{} fields is more than the {MAX_NEW} one call adds",
            fields.len()
        )]);
    }
    // Refuses an XFA form and one past the scan's limits, which are documents
    // whose fields tpdf could not read back.
    if let Err(why) = forms::scan(doc) {
        return Err(vec![why]);
    }
    let pages = crate::pagetree::ordered_pages(doc);
    let existing = taken(doc);
    let mut seen = HashSet::new();
    let mut placed_at = Vec::new();
    for field in fields {
        if let Some(why) = name_problem(&field.name) {
            problems.push(why);
        } else if existing.contains(&field.name) {
            problems.push(format!(
                "`{}`: the form already has a field of this name",
                field.name
            ));
        } else if !seen.insert(field.name.as_str()) {
            problems.push(format!("`{}` is named more than once", field.name));
        }
        if field.kind == Kind::Checkbox && field.max_length.is_some() {
            problems.push(format!(
                "`{}`: a checkbox takes no characters, so it has no most",
                field.name
            ));
        }
        if field
            .max_length
            .is_some_and(|most| !(1..=16384).contains(&most))
        {
            problems.push(format!(
                "`{}`: the most characters is a number from 1 to 16384",
                field.name
            ));
        }
        if field
            .tooltip
            .as_ref()
            .is_some_and(|tip| tip.chars().count() > 1024)
        {
            problems.push(format!(
                "`{}`: a tooltip is at most 1024 characters",
                field.name
            ));
        }
        let Some(page) = pages.get(field.page as usize) else {
            problems.push(format!(
                "`{}`: there is no page {}; the document has {}",
                field.name,
                u64::from(field.page) + 1,
                pages.len()
            ));
            continue;
        };
        match placed(doc, *page, field) {
            // Kept whether or not this field has another problem: the list is
            // only handed back when no field has any.
            Ok(rect) => placed_at.push((*page, rect)),
            Err(why) => problems.push(why),
        }
    }
    if problems.is_empty() {
        Ok(placed_at)
    } else {
        Err(problems)
    }
}

/// The form dictionary's object, when it is one.
fn form_id(doc: &Document) -> Option<ObjectId> {
    doc.catalog()
        .ok()?
        .get(b"AcroForm")
        .ok()?
        .as_reference()
        .ok()
}

/// The form dictionary as an object of its own, made if the document has none.
///
/// A form written directly into the catalog is moved into an object, so that
/// everything below addresses it one way.
fn ensure_form(doc: &mut Document) -> Result<ObjectId, String> {
    if let Some(id) = form_id(doc) {
        doc.get_dictionary(id)
            .map_err(|_| "the document's form is not a dictionary")?;
        return Ok(id);
    }
    let direct = doc
        .catalog()
        .map_err(|e| e.to_string())?
        .get(b"AcroForm")
        .ok()
        .map(|o| o.as_dict().cloned());
    let form = match direct {
        Some(Ok(form)) => form,
        Some(Err(_)) => return Err("the document's form is not a dictionary".into()),
        None => dictionary! { "Fields" => Vec::<Object>::new() },
    };
    let id = doc.add_object(form);
    doc.catalog_mut()
        .map_err(|e| e.to_string())?
        .set("AcroForm", id);
    Ok(id)
}

/// Makes sure the form names Helvetica as `Helv` and has a default appearance.
///
/// A reader that redraws a field looks the font up in the form's `/DR`, and a
/// field whose `/DA` names a font that is not there is one Acrobat refuses to
/// type into.
fn ensure_font(doc: &mut Document, form: ObjectId) -> Result<ObjectId, String> {
    let resolved = |doc: &Document, object: Option<Object>| -> Dictionary {
        object
            .and_then(|o| doc.dereference(&o).ok().map(|(_, o)| o.clone()))
            .and_then(|o| o.as_dict().ok().cloned())
            .unwrap_or_default()
    };
    let dict = doc.get_dictionary(form).map_err(|e| e.to_string())?;
    let mut resources = resolved(doc, dict.get(b"DR").ok().cloned());
    let mut fonts = resolved(doc, resources.get(b"Font").ok().cloned());
    let has_appearance = dict.has(b"DA");
    let font = match fonts.get(b"Helv").and_then(Object::as_reference) {
        Ok(id) => id,
        Err(_) => {
            let id = doc.add_object(dictionary! {
                "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
                "Encoding" => "WinAnsiEncoding",
            });
            fonts.set("Helv", id);
            id
        }
    };
    resources.set("Font", fonts);
    let dict = doc.get_dictionary_mut(form).map_err(|e| e.to_string())?;
    dict.set("DR", resources);
    if !has_appearance {
        dict.set("DA", text(DEFAULT_APPEARANCE));
    }
    Ok(font)
}

/// Appends `item` to the array `key` names in `owner`, wherever the array is.
fn append(doc: &mut Document, owner: ObjectId, key: &[u8], item: ObjectId) -> Result<(), String> {
    let held = doc
        .get_dictionary(owner)
        .map_err(|e| e.to_string())?
        .get(key)
        .ok()
        .cloned();
    match held {
        Some(Object::Reference(array)) => doc
            .get_object_mut(array)
            .and_then(Object::as_array_mut)
            .map_err(|_| format!("/{} is not a list", String::from_utf8_lossy(key)))?
            .push(item.into()),
        Some(Object::Array(mut items)) => {
            items.push(item.into());
            doc.get_dictionary_mut(owner)
                .map_err(|e| e.to_string())?
                .set(key, items);
        }
        Some(_) => return Err(format!("/{} is not a list", String::from_utf8_lossy(key))),
        None => doc
            .get_dictionary_mut(owner)
            .map_err(|e| e.to_string())?
            .set(key, vec![Object::Reference(item)]),
    }
    Ok(())
}

/// Writes one field's object: field and widget in one, with its appearance.
///
/// `rect` is in the page's own space. The object is not yet in the page's
/// annotations or the form's field list; a field in one and not the other is
/// one `forms::scan` refuses, so every caller does both.
fn widget(
    doc: &mut Document,
    font: ObjectId,
    page: ObjectId,
    rect: [f64; 4],
    field: &NewField,
) -> ObjectId {
    let (width, height) = (rect[2] - rect[0], rect[3] - rect[1]);
    let mut flags = 0_i64;
    if field.required {
        flags |= 1 << 1;
    }
    let mut widget = dictionary! {
        "Type" => "Annot", "Subtype" => "Widget",
        "T" => forms::pdf_string(&field.name),
        "Rect" => rect.iter().map(|v| Object::Real(*v as f32)).collect::<Vec<_>>(),
        "P" => page,
        // Print, which is what makes a filled field appear on paper.
        "F" => 4,
        "DA" => text(DEFAULT_APPEARANCE),
    };
    if let Some(tip) = field.tooltip.as_deref().filter(|tip| !tip.is_empty()) {
        widget.set("TU", forms::pdf_string(tip));
    }
    match field.kind {
        Kind::Text | Kind::Multiline => {
            if field.kind == Kind::Multiline {
                flags |= 1 << 12;
            }
            widget.set("FT", "Tx");
            if let Some(most) = field.max_length {
                widget.set("MaxLen", i64::from(most));
            }
            // The empty appearance §12.7.4.3 describes: marked as the
            // field's text and holding none. A border is drawn before it, and
            // declared in `/MK` and `/BS` as well: a reader that redraws the
            // field, `forms::write` among them, reads it from there.
            let mut body = Vec::new();
            if field.border {
                widget.set("MK", dictionary! { "BC" => vec![Object::Integer(0)] });
                widget.set(
                    "BS",
                    dictionary! { "W" => Object::Real(BORDER_WIDTH as f32), "S" => "S" },
                );
                body.extend_from_slice(
                    forms::border_path(width, height, "0 G", BORDER_WIDTH).as_bytes(),
                );
            }
            body.extend_from_slice(b"/Tx BMC EMC");
            let empty = forms::appearance(
                doc,
                width,
                height,
                body,
                dictionary! { "Font" => dictionary! { "Helv" => font } },
            );
            widget.set("AP", dictionary! { "N" => empty });
        }
        Kind::Checkbox => {
            widget.set("FT", "Btn");
            widget.set("V", "Off");
            widget.set("AS", "Off");
            let (off, on) = forms::checkbox_appearances(doc, width, height);
            widget.set(
                "AP",
                dictionary! { "N" => dictionary! { "Off" => off, "Yes" => on } },
            );
        }
    }
    widget.set("Ff", flags);
    doc.add_object(widget)
}

/// Adds `fields` to the document, all of them or none.
pub fn add(doc: &mut Document, fields: &[NewField]) -> Result<(), String> {
    if fields.is_empty() {
        return Ok(());
    }
    let placed = check(doc, fields).map_err(|problems| problems.join("; "))?;
    let form = ensure_form(doc)?;
    let font = ensure_font(doc, form)?;
    for (field, (page, rect)) in fields.iter().zip(placed) {
        let id = widget(doc, font, page, rect, field);
        append(doc, page, b"Annots", id)?;
        append(doc, form, b"Fields", id)?;
    }
    Ok(())
}

/// The least a side of a field of this kind may be, in points.
#[must_use]
pub fn least_side(kind: Kind) -> f64 {
    if kind == Kind::Checkbox {
        MIN_BOX
    } else {
        MIN_TEXT
    }
}

/// Makes the field a reader placed in the window, and lists it in the form.
///
/// The other door into [`widget`]. `add` is handed pages of the file and
/// rectangles as the page is displayed; a field placed in the window is a mark
/// in the edit journal, and by the time it is written the save has already put
/// the pages in their final order and mapped the rectangle into the page's own
/// space. So this takes both as they are, checks what `check` checks of a name
/// and a size, and leaves attaching the widget to its page to the caller,
/// which attaches every other mark the same way.
///
/// # Errors
///
/// A name that cannot be a field's or that the form already has, a rectangle
/// under the least size, or a form tpdf cannot read.
pub fn place(
    doc: &mut Document,
    page: ObjectId,
    rect: [f64; 4],
    name: &str,
    placed: Placed,
) -> Result<ObjectId, String> {
    let kind = placed.kind;
    forms::scan(doc)?;
    if let Some(why) = name_problem(name) {
        return Err(why);
    }
    if taken(doc).contains(name) {
        return Err(format!(
            "`{name}`: the form already has a field of this name"
        ));
    }
    let least = least_side(kind);
    if rect[2] - rect[0] < least || rect[3] - rect[1] < least {
        return Err(format!(
            "`{name}`: a field of this kind needs at least {least} by {least} points"
        ));
    }
    let form = ensure_form(doc)?;
    let font = ensure_font(doc, form)?;
    let field = NewField {
        name: name.to_string(),
        kind,
        page: 0,
        rect,
        tooltip: None,
        required: false,
        max_length: None,
        border: placed.border,
    };
    let id = widget(doc, font, page, rect, &field);
    append(doc, form, b"Fields", id)?;
    Ok(id)
}

#[cfg(test)]
mod tests;
