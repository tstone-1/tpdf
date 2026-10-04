//! Adding fields to a document's form, or giving it one.
//!
//! `forms.rs` reads a form and answers it; nothing there makes a field exist.
//! This does, for the kinds a form is mostly made of: a text field, on one
//! line or several, a checkbox, a dropdown and a group of radio buttons.
//!
//! A field written here is one object that is both the field and its widget,
//! which is what the specification allows for a field with a single widget and
//! what most producers write. A group of radio buttons is the exception: it
//! is one field with a widget for each button, and each button is added to
//! the group of its name, which is made when its first button is. It carries its own appearance, so a reader shows
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
    /// One choice from a list that drops down. The list is the field's
    /// `options`.
    Dropdown,
    /// One button of a group of which one is chosen. The field's name is the
    /// group's, and its `options` hold one entry, the value this button gives
    /// the group when it is the one chosen.
    Radio,
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
    /// What a dropdown offers, in order. Empty for every other kind.
    #[serde(default)]
    pub options: Vec<String>,
    /// The size a text field's or a dropdown's text is drawn at where it
    /// fits, in points. `None` is a size that follows the field's height.
    #[serde(default)]
    pub text_size: Option<f32>,
    /// What a text field holds after a reader resets the form, and what it
    /// is made holding.
    #[serde(default)]
    pub default_value: Option<String>,
}

/// What is wrong with a text size or a default value for a field of this
/// kind, of what can be said without the document.
fn text_problem(
    name: &str,
    kind: Kind,
    text_size: Option<f32>,
    default_value: Option<&str>,
    max_length: Option<u32>,
) -> Option<String> {
    let props = crate::formedit::Props {
        text_size,
        default_value: default_value.map(str::to_string),
        ..Default::default()
    };
    if let Some(why) = props.problem(name) {
        return Some(why);
    }
    if text_size == Some(0.0) {
        return Some(format!(
            "`{name}`: a text size is {} to {} points, and is left out for one that follows the field",
            forms::MIN_TEXT_SIZE,
            forms::MAX_TEXT_SIZE
        ));
    }
    let text = matches!(kind, Kind::Text | Kind::Multiline);
    if text_size.is_some() && !text && kind != Kind::Dropdown {
        return Some(format!("`{name}`: a box or a button has no text to size"));
    }
    let default = default_value.filter(|default| !default.is_empty())?;
    if !text {
        return Some(format!("`{name}`: only a text field has a default value"));
    }
    if kind == Kind::Text && default.contains('\n') {
        return Some(format!(
            "`{name}` takes one line, and its default value has several"
        ));
    }
    let holds = default.chars().count();
    if max_length.is_some_and(|most| holds > most as usize) {
        return Some(format!(
            "`{name}`: the default value has {holds} characters, which is more than the field takes"
        ));
    }
    None
}

/// Why a field's default value would not be drawn in a field of this
/// rectangle, as the answer it is.
fn default_problem(field: &NewField, rect: [f64; 4]) -> Option<String> {
    let default = field.default_value.as_deref().filter(|d| !d.is_empty())?;
    let would_be = forms::Widget {
        object: (0, 0),
        widget: (0, 0),
        page: 0,
        rect,
        display_rect: [0.0; 4],
        name: field.name.clone(),
        value: forms::Value::Text(String::new()),
        control: forms::Control::Text,
        multiline: field.kind == Kind::Multiline,
        max_length: field.max_length.map(|most| most as usize),
        reason: None,
        tooltip: String::new(),
        required: false,
        read_only: false,
        align: forms::Align::Left,
        text_size: field.text_size,
        default_value: String::new(),
    };
    forms::validate(&would_be, &forms::Value::Text(default.to_string()))
        .err()
        .map(|why| format!("`{}`: its default value: {why}", field.name))
}

impl Placed {
    /// Why this field's default value would not be drawn in a rectangle of
    /// this size, or `None` when it would or there is none.
    ///
    /// Asked when the default is set, with the rectangle the mark then has,
    /// so that a reader is told in the panel. The save asks again, of the
    /// rectangle it writes.
    pub fn default_problem(&self, name: &str, width: f64, height: f64) -> Option<String> {
        let field = NewField {
            name: name.to_string(),
            kind: self.kind,
            page: 0,
            rect: [0.0; 4],
            tooltip: None,
            required: false,
            max_length: self.max_length,
            border: self.border,
            options: Vec::new(),
            text_size: self.text_size,
            default_value: Some(self.default_value.clone()),
        };
        default_problem(&field, [0.0, 0.0, width, height])
    }
}

/// Answers a field just made with its default value, which draws it.
fn answer_default(doc: &mut Document, id: ObjectId, field: &NewField) -> Result<(), String> {
    let Some(default) = field.default_value.as_deref().filter(|d| !d.is_empty()) else {
        return Ok(());
    };
    forms::write(
        doc,
        &[forms::Change {
            object: id,
            value: forms::Value::Text(default.to_string()),
        }],
    )
}

/// What a field placed in the window is: its kind, and whether it is framed.
///
/// The payload of a `MarkKind::Field` mark. Its name is the mark's note and its
/// rectangle the mark's quad, so these two are all that is left to say.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub kind: Kind,
    /// [`NewField::border`].
    #[serde(default)]
    pub border: bool,
    /// [`NewField::options`]. Left out of a reply when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// [`NewField::tooltip`]. Empty for none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tooltip: String,
    /// [`NewField::required`].
    #[serde(default)]
    pub required: bool,
    /// Whether the field refuses an answer.
    #[serde(default)]
    pub read_only: bool,
    /// [`NewField::max_length`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    /// Where a text field's or a dropdown's text sits.
    #[serde(default)]
    pub align: forms::Align,
    /// [`NewField::text_size`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_size: Option<f32>,
    /// [`NewField::default_value`]. Empty for none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_value: String,
}

impl Placed {
    /// What is wrong with this as a field called `name`, of what can be said
    /// without the document: its choices, its tooltip, and a limit or an
    /// alignment its kind does not have.
    pub fn problem(&self, name: &str) -> Option<String> {
        if let Some(why) = options_problem(name, self.kind, &self.options) {
            return Some(why);
        }
        let props = crate::formedit::Props {
            tooltip: Some(self.tooltip.clone()),
            max_length: self.max_length,
            ..Default::default()
        };
        if let Some(why) = props.problem(name) {
            return Some(why);
        }
        if let Some(most) = self.max_length {
            if !matches!(self.kind, Kind::Text | Kind::Multiline) {
                return Some(format!("`{name}`: only a text field has a most characters"));
            }
            if most == 0 {
                return Some(format!(
                    "`{name}`: the most characters is a number from 1 to 16384"
                ));
            }
        }
        if matches!(self.kind, Kind::Checkbox | Kind::Radio) && self.align != forms::Align::Left {
            return Some(format!("`{name}`: a box or a button has no text to align"));
        }
        text_problem(
            name,
            self.kind,
            self.text_size,
            Some(self.default_value.as_str()),
            self.max_length,
        )
    }
}

impl From<Kind> for Placed {
    /// A field of this kind with no border, which is what `tpdf form` adds
    /// unless it is asked for one.
    fn from(kind: Kind) -> Self {
        Self {
            kind,
            border: false,
            options: Vec::new(),
            tooltip: String::new(),
            required: false,
            read_only: false,
            max_length: None,
            align: forms::Align::Left,
            text_size: None,
            default_value: String::new(),
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

pub(crate) fn text(value: &str) -> Object {
    Object::String(value.as_bytes().to_vec(), lopdf::StringFormat::Literal)
}

/// The most choices one dropdown offers.
pub const MAX_OPTIONS: usize = 1000;
/// The most characters one choice has.
pub const MAX_OPTION: usize = 255;

/// Whether these can be a field's choices, and why not.
///
/// A dropdown needs at least one; no other kind takes any. Each is text a
/// reader will see, so it is not empty, not padded, has no control character
/// and is not there twice: two choices that read the same cannot be told
/// apart in the list, and a filled form would not say which was meant.
pub fn options_problem(name: &str, kind: Kind, options: &[String]) -> Option<String> {
    if kind == Kind::Radio {
        if options.len() != 1 {
            return Some(format!("`{name}`: a radio button has one value"));
        }
        // `/Off` is the state every button has for not being the one chosen.
        if options[0] == "Off" {
            return Some(format!(
                "`{name}`: `Off` is what a group holds when nothing is chosen, so no button can have it as its value"
            ));
        }
    } else if kind != Kind::Dropdown {
        return (!options.is_empty()).then(|| format!("`{name}`: only a dropdown has choices"));
    }
    if options.is_empty() {
        return Some(format!("`{name}`: a dropdown needs at least one choice"));
    }
    if options.len() > MAX_OPTIONS {
        return Some(format!(
            "`{name}`: {} choices is more than the {MAX_OPTIONS} a dropdown offers",
            options.len()
        ));
    }
    let mut seen = HashSet::new();
    for option in options {
        if option.trim().is_empty() {
            return Some(format!("`{name}`: a choice cannot be empty"));
        }
        if option != option.trim() {
            return Some(format!(
                "`{name}`: the choice `{option}` begins or ends with a space"
            ));
        }
        if option.chars().any(char::is_control) {
            return Some(format!(
                "`{name}`: a choice cannot contain a control character"
            ));
        }
        if option.chars().count() > MAX_OPTION {
            return Some(format!(
                "`{name}`: a choice is at most {MAX_OPTION} characters"
            ));
        }
        if !seen.insert(option.as_str()) {
            return Some(format!("`{name}`: the choice `{option}` is there twice"));
        }
    }
    None
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
            match field.kind {
                Kind::Checkbox => "checkbox",
                Kind::Radio => "radio button",
                _ => "text field",
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
    let mut seen: std::collections::HashMap<&str, Kind> = std::collections::HashMap::new();
    let mut values: HashSet<(&str, Vec<u8>)> = HashSet::new();
    let mut placed_at = Vec::new();
    for field in fields {
        // A radio button shares its name with the other buttons of its group,
        // in this call and in the form; nothing else shares a name.
        let radio = field.kind == Kind::Radio;
        let group = radio.then(|| radio_group(doc, &field.name)).flatten();
        if let Some(why) = name_problem(&field.name) {
            problems.push(why);
        } else if existing.contains(&field.name) && group.is_none() {
            problems.push(format!(
                "`{}`: the form already has a field of this name",
                field.name
            ));
        } else if seen
            .insert(field.name.as_str(), field.kind)
            .is_some_and(|before| !(radio && before == Kind::Radio))
        {
            problems.push(format!("`{}` is named more than once", field.name));
        }
        if let (true, Some(value)) = (radio, field.options.first()) {
            let held = group
                .is_some_and(|group| radio_values(doc, group).contains(&value.as_bytes().to_vec()));
            if held || !values.insert((field.name.as_str(), value.as_bytes().to_vec())) {
                problems.push(format!(
                    "`{}`: the group already has a button with the value `{value}`",
                    field.name
                ));
            }
        }
        if field.kind == Kind::Radio && field.max_length.is_some() {
            problems.push(format!(
                "`{}`: a radio button takes no characters, so it has no most",
                field.name
            ));
        }
        if field.kind == Kind::Checkbox && field.max_length.is_some() {
            problems.push(format!(
                "`{}`: a checkbox takes no characters, so it has no most",
                field.name
            ));
        }
        if field.kind == Kind::Dropdown && field.max_length.is_some() {
            problems.push(format!(
                "`{}`: a dropdown takes one of its choices, so it has no most characters",
                field.name
            ));
        }
        if let Some(why) = options_problem(&field.name, field.kind, &field.options) {
            problems.push(why);
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
        if let Some(why) = text_problem(
            &field.name,
            field.kind,
            field.text_size,
            field.default_value.as_deref(),
            field.max_length,
        ) {
            problems.push(why);
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
            Ok(rect) => {
                if let Some(why) = default_problem(field, rect) {
                    problems.push(why);
                }
                placed_at.push((*page, rect));
            }
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
        "DA" => match field.text_size {
            Some(size) => Object::String(
                forms::sized_appearance(None, size),
                lopdf::StringFormat::Literal,
            ),
            None => text(DEFAULT_APPEARANCE),
        },
    };
    if let Some(default) = field.default_value.as_deref().filter(|d| !d.is_empty()) {
        widget.set("DV", forms::pdf_string(default));
    }
    if let Some(tip) = field.tooltip.as_deref().filter(|tip| !tip.is_empty()) {
        widget.set("TU", forms::pdf_string(tip));
    }
    match field.kind {
        Kind::Text | Kind::Multiline | Kind::Dropdown => {
            if field.kind == Kind::Multiline {
                flags |= 1 << 12;
            }
            if field.kind == Kind::Dropdown {
                // A choice field shown as a box that drops its list down
                // (`Combo`, bit 18), with nothing chosen: no `/V`.
                flags |= 1 << 17;
                widget.set("FT", "Ch");
                widget.set(
                    "Opt",
                    field
                        .options
                        .iter()
                        .map(|option| forms::pdf_string(option))
                        .collect::<Vec<_>>(),
                );
            } else {
                widget.set("FT", "Tx");
            }
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
        // A radio button is a widget under its group and is made by `radio`.
        Kind::Radio => unreachable!("a radio button is not a field of its own"),
    }
    widget.set("Ff", flags);
    doc.add_object(widget)
}

/// `/Ff` of a group of radio buttons: bit 16 says radio, and bit 15 that a
/// press on the chosen button does not leave the group with none chosen.
const RADIO_FLAGS: i64 = (1 << 15) | (1 << 14);

/// The top-level field called `name`, when it is a group of radio buttons.
fn radio_group(doc: &Document, name: &str) -> Option<ObjectId> {
    let fields = doc
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"AcroForm").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .and_then(|form| form.get(b"Fields").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())?;
    fields
        .iter()
        .filter_map(|field| field.as_reference().ok())
        .find(|id| {
            doc.get_dictionary(*id).is_ok_and(|dict| {
                let named = dict
                    .get(b"T")
                    .and_then(Object::as_str)
                    .is_ok_and(|raw| crate::annots::decode_text_string(raw) == name);
                let flags = forms::integer(doc, *id, b"Ff");
                named
                    && dict.get(b"FT").and_then(Object::as_name).ok() == Some(b"Btn")
                    && flags & (1 << 15) != 0
                    && flags & (1 << 16) == 0
            })
        })
}

/// The names of the states a group's buttons have: the value each gives the
/// group, and `Off`, which no button may have as its value.
fn radio_values(doc: &Document, group: ObjectId) -> Vec<Vec<u8>> {
    let kids = doc
        .get_dictionary(group)
        .ok()
        .and_then(|dict| dict.get(b"Kids").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok());
    kids.into_iter()
        .flatten()
        .filter_map(|kid| doc.dereference(kid).ok()?.1.as_dict().ok())
        .filter_map(|kid| doc.dereference(kid.get(b"AP").ok()?).ok()?.1.as_dict().ok())
        .filter_map(|ap| doc.dereference(ap.get(b"N").ok()?).ok()?.1.as_dict().ok())
        .flat_map(|normal| {
            normal
                .iter()
                .map(|(state, _)| state.clone())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A circle of radius `r` about `(x, y)`, as four curves.
fn circle(x: f64, y: f64, r: f64) -> String {
    // The distance of a control point that makes a quarter circle of a curve.
    let k = r * 0.552_284_75;
    format!(
        "{} {y} m {} {} {} {} {x} {} c {} {} {} {} {} {y} c {} {} {} {} {x} {} c {} {} {} {} {} {y} c h ",
        x + r,
        x + r, y + k, x + k, y + r, y + r,
        x - k, y + r, x - r, y + k, x - r,
        x - r, y - k, x - k, y - r, y - r,
        x + k, y - r, x + r, y - k, x + r,
    )
}

/// A radio button's two looks: a ring, and the ring with a dot in it.
fn radio_appearances(doc: &mut Document, width: f64, height: f64) -> (ObjectId, ObjectId) {
    let (x, y) = (width / 2.0, height / 2.0);
    let r = (width.min(height) / 2.0 - 0.5).max(0.5);
    let ring = format!(
        "q 1 1 1 rg {}f 0 0 0 RG 1 w {}S ",
        circle(x, y, r),
        circle(x, y, r)
    );
    let off = forms::appearance(
        doc,
        width,
        height,
        format!("{ring}Q").into_bytes(),
        Dictionary::new(),
    );
    let on = forms::appearance(
        doc,
        width,
        height,
        format!("{ring}0 0 0 rg {}f Q", circle(x, y, r * 0.5)).into_bytes(),
        Dictionary::new(),
    );
    (off, on)
}

/// What a group of radio buttons is given by a button added to it.
struct Grouped<'a> {
    name: &'a str,
    value: &'a str,
    tooltip: Option<&'a str>,
    required: bool,
    read_only: bool,
}

/// Adds one radio button, to the group of its name or to a new one.
///
/// The group's tooltip and its two flags are the group's and not a button's:
/// a button that is required or read-only makes the group so, and a tooltip
/// is taken by a group that has none. `rect` is in the page's own space, and
/// the button is not yet in its page's annotations.
///
/// # Errors
///
/// The group already has a button with this value.
fn radio(
    doc: &mut Document,
    form: ObjectId,
    page: ObjectId,
    rect: [f64; 4],
    to: &Grouped,
) -> Result<ObjectId, String> {
    let group = match radio_group(doc, to.name) {
        Some(group) => group,
        None => {
            let group = doc.add_object(dictionary! {
                "FT" => "Btn", "T" => forms::pdf_string(to.name),
                "Ff" => RADIO_FLAGS, "Kids" => Vec::<Object>::new(),
            });
            append(doc, form, b"Fields", group)?;
            group
        }
    };
    if radio_values(doc, group).contains(&to.value.as_bytes().to_vec()) {
        return Err(format!(
            "`{}`: the group already has a button with the value `{}`",
            to.name, to.value
        ));
    }
    let mut flags = forms::integer(doc, group, b"Ff");
    if to.required {
        flags |= 1 << 1;
    }
    if to.read_only {
        flags |= 1;
    }
    let dict = doc.get_dictionary_mut(group).map_err(|e| e.to_string())?;
    dict.set("Ff", flags);
    if let Some(tip) = to.tooltip.filter(|tip| !tip.is_empty() && !dict.has(b"TU")) {
        dict.set("TU", forms::pdf_string(tip));
    }
    let (width, height) = (rect[2] - rect[0], rect[3] - rect[1]);
    let (off, on) = radio_appearances(doc, width, height);
    let mut normal = dictionary! { "Off" => off };
    normal.set(to.value.as_bytes().to_vec(), on);
    let button = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "Parent" => group,
        "Rect" => rect.iter().map(|v| Object::Real(*v as f32)).collect::<Vec<_>>(),
        "P" => page, "F" => 4, "AS" => "Off",
        "AP" => dictionary! { "N" => normal },
    });
    append(doc, group, b"Kids", button)?;
    Ok(button)
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
        if field.kind == Kind::Radio {
            let to = Grouped {
                name: &field.name,
                value: field.options.first().map_or("", String::as_str),
                tooltip: field.tooltip.as_deref(),
                required: field.required,
                read_only: false,
            };
            let id = radio(doc, form, page, rect, &to)?;
            append(doc, page, b"Annots", id)?;
            continue;
        }
        let id = widget(doc, font, page, rect, field);
        append(doc, page, b"Annots", id)?;
        append(doc, form, b"Fields", id)?;
        answer_default(doc, id, field)?;
    }
    Ok(())
}

/// The least a side of a field of this kind may be, in points.
#[must_use]
pub fn least_side(kind: Kind) -> f64 {
    if matches!(kind, Kind::Checkbox | Kind::Radio) {
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
    placed: &Placed,
) -> Result<ObjectId, String> {
    let kind = placed.kind;
    forms::scan(doc)?;
    if let Some(why) = name_problem(name) {
        return Err(why);
    }
    if let Some(why) = placed.problem(name) {
        return Err(why);
    }
    // A radio button joins the group of its name; anything else needs a
    // name the form does not have.
    if taken(doc).contains(name) && !(kind == Kind::Radio && radio_group(doc, name).is_some()) {
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
    if kind == Kind::Radio {
        let to = Grouped {
            name,
            value: placed.options.first().map_or("", String::as_str),
            tooltip: Some(placed.tooltip.as_str()),
            required: placed.required,
            read_only: placed.read_only,
        };
        return radio(doc, form, page, rect, &to);
    }
    let font = ensure_font(doc, form)?;
    let field = NewField {
        name: name.to_string(),
        kind,
        page: 0,
        rect,
        tooltip: (!placed.tooltip.is_empty()).then(|| placed.tooltip.clone()),
        required: placed.required,
        max_length: placed.max_length,
        border: placed.border,
        options: placed.options.clone(),
        text_size: placed.text_size,
        default_value: (!placed.default_value.is_empty()).then(|| placed.default_value.clone()),
    };
    if let Some(why) = default_problem(&field, rect) {
        return Err(why);
    }
    let id = widget(doc, font, page, rect, &field);
    // The two a field made by `tpdf form` does not have. The empty appearance
    // holds no text, so an alignment changes nothing that is drawn yet.
    let made = doc.get_dictionary_mut(id).map_err(|e| e.to_string())?;
    if placed.align != forms::Align::Left {
        made.set("Q", placed.align.quadding());
    }
    if placed.read_only {
        let flags = made.get(b"Ff").and_then(Object::as_i64).unwrap_or(0);
        made.set("Ff", flags | 1);
    }
    append(doc, form, b"Fields", id)?;
    Ok(id)
}

/// Answers a field [`place`] made with its default value, once the caller
/// has attached it to its page: an answer is drawn into a field the form and
/// a page both list, and `place` leaves the second to its caller.
///
/// A read-only field is answered too. It refuses a reader's answer, and this
/// is its author's.
///
/// # Errors
///
/// What `forms::write` refuses the answer for.
pub fn answer_placed(doc: &mut Document, id: ObjectId, placed: &Placed) -> Result<(), String> {
    if placed.default_value.is_empty() || !matches!(placed.kind, Kind::Text | Kind::Multiline) {
        return Ok(());
    }
    let flags = forms::integer(doc, id, b"Ff");
    let set = |doc: &mut Document, flags: i64| -> Result<(), String> {
        doc.get_dictionary_mut(id)
            .map_err(|e| e.to_string())?
            .set("Ff", flags);
        Ok(())
    };
    set(doc, flags & !1)?;
    forms::write(
        doc,
        &[forms::Change {
            object: id,
            value: forms::Value::Text(placed.default_value.clone()),
        }],
    )?;
    set(doc, flags)
}

#[cfg(test)]
mod tests;
