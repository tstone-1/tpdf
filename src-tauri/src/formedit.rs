//! Changing form fields a document already has: where a field's widget sits,
//! how large it is, what the field is called, and whether it is there at all.
//!
//! [`crate::formfields`] adds fields and [`crate::forms`] answers them. Until
//! this module a field, once saved, could be filled and nothing else: a form
//! laid out a few points wrong had to be made again from the start.
//!
//! **What is changed, and what is left.** A move rewrites the widget's
//! `/Rect`. A resize does the same and, for a text or choice field, redraws
//! its appearance at the new size through [`forms::write`], with the answer
//! it holds; for a checkbox, a radio button or a button the appearance is
//! left as the document's author drew it, and a reader scales it to the new
//! rectangle as §12.5.5 says to. A rename rewrites the field's own `/T`. A
//! removal takes the widget off its page and out of the field tree; the
//! objects go when the rewrite sweeps what nothing refers to.
//!
//! **A field's properties.** Its tooltip (`/TU`), whether it is required and
//! whether it is read-only (`/Ff` bits 2 and 1), the most characters a text
//! field takes (`/MaxLen`), how its text is aligned (`/Q`) and what a choice
//! field offers (`/Opt`) are each written on the field itself. Alignment and
//! choices change what the field looks like, so the field is drawn again,
//! with the answer it holds; an answer that is no longer among the choices
//! is taken off the field rather than left naming something it cannot show.
//!
//! Nothing here adds a script, an action or a calculation, and none is
//! touched: a field that has one keeps it under its new name or place.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Document, Object, ObjectId};
use serde::{Deserialize, Serialize};

use crate::forms::{self, Align, Change, Control, Value, Widget};

/// The most fields one call changes. A bound on what a caller can ask.
pub const MAX_EDITS: usize = 1000;

/// What is changed about a field apart from its place and its name. Each
/// part that is `None` is left as the document has it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Props {
    /// What a reader shows when the pointer rests on the field. Empty takes
    /// the tooltip off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// Whether a form that submits itself insists on an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Whether the field refuses an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    /// The most characters a text field takes. Nought takes the limit off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    /// Where a text or choice field's text sits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    /// What a choice field offers, in order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    /// The size a text or choice field's text is drawn at, in points, where
    /// it fits; a longer answer is drawn smaller. Nought is a size that
    /// follows the field's height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_size: Option<f32>,
    /// What a text field holds once a reader resets the form, and what it
    /// holds now if it holds nothing. Empty takes the default off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

/// The most characters a text field can be limited to, which is the most an
/// answer holds.
pub const MAX_LENGTH: u32 = 16384;
/// The most characters a tooltip has.
pub const MAX_TOOLTIP: usize = 1024;

impl Props {
    /// Whether nothing is changed.
    pub fn is_empty(&self) -> bool {
        *self == Props::default()
    }

    /// Lays a later change over this one: each part the later one names
    /// replaces the part here.
    pub fn merge(&mut self, later: &Props) {
        if later.tooltip.is_some() {
            self.tooltip.clone_from(&later.tooltip);
        }
        self.required = later.required.or(self.required);
        self.read_only = later.read_only.or(self.read_only);
        self.max_length = later.max_length.or(self.max_length);
        self.align = later.align.or(self.align);
        if later.options.is_some() {
            self.options.clone_from(&later.options);
        }
        self.text_size = later.text_size.or(self.text_size);
        if later.default_value.is_some() {
            self.default_value.clone_from(&later.default_value);
        }
    }

    /// What is wrong with these that can be said without the document, for a
    /// field called `name`.
    pub fn problem(&self, name: &str) -> Option<String> {
        if let Some(tip) = &self.tooltip {
            if tip.chars().count() > MAX_TOOLTIP {
                return Some(format!(
                    "`{name}`: a tooltip is at most {MAX_TOOLTIP} characters"
                ));
            }
            if tip.chars().any(|ch| ch.is_control() && ch != '\n') {
                return Some(format!(
                    "`{name}`: a tooltip cannot contain a control character"
                ));
            }
        }
        if self.max_length.is_some_and(|most| most > MAX_LENGTH) {
            return Some(format!(
                "`{name}`: the most characters is a number up to {MAX_LENGTH}"
            ));
        }
        if let Some(size) = self.text_size {
            let (least, most) = (forms::MIN_TEXT_SIZE, forms::MAX_TEXT_SIZE);
            if size != 0.0 && !(least..=most).contains(&size) {
                return Some(format!(
                    "`{name}`: a text size is {least} to {most} points, or nought for one that follows the field"
                ));
            }
        }
        if let Some(default) = &self.default_value {
            if default.chars().count() > MAX_LENGTH as usize {
                return Some(format!(
                    "`{name}`: a default value is at most {MAX_LENGTH} characters"
                ));
            }
            if default.chars().any(|ch| ch.is_control() && ch != '\n') {
                return Some(format!(
                    "`{name}`: a default value cannot contain a control character"
                ));
            }
        }
        self.options.as_ref().and_then(|options| {
            crate::formfields::options_problem(name, crate::formfields::Kind::Dropdown, options)
        })
    }
}

/// One field's widget, and what becomes of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldEdit {
    /// The widget annotation, as [`Widget::widget`] names it.
    pub widget: ObjectId,
    /// Its new rectangle, `[left, top, right, bottom]` in the page's display
    /// space, the space [`Widget::display_rect`] is in. `None` leaves it.
    #[serde(default)]
    pub rect: Option<[f32; 4]>,
    /// The field's new name. `None` leaves it. A field with several widgets
    /// has one name, so this renames all of them.
    #[serde(default)]
    pub name: Option<String>,
    /// Take the widget out of the document.
    #[serde(default)]
    pub remove: bool,
    /// The field's properties. A field with several widgets has one set, so
    /// this changes all of them.
    #[serde(default)]
    pub props: Props,
    /// The value a radio button gives its group: the name of the state it
    /// has when it is the one chosen. `None` leaves it. This is the button's
    /// own and not the field's, so it is not among the properties.
    #[serde(default)]
    pub value: Option<String>,
}

/// What is wrong with `value` as the value of a radio button called `name`,
/// as far as can be said without the document.
pub fn button_problem(name: &str, value: &str) -> Option<String> {
    crate::formfields::options_problem(name, crate::formfields::Kind::Radio, &[value.to_string()])
}

/// A radio button, the state it has when chosen, and the name that state is
/// to have.
struct Revalued<'a> {
    widget: &'a Widget,
    was: Vec<u8>,
    to: Vec<u8>,
}

/// A radio button and the value it is to give its group, checked against the
/// group, or `None` where the button has the value already.
fn revalued<'a>(
    doc: &Document,
    widget: &'a Widget,
    value: &str,
) -> Result<Option<Revalued<'a>>, String> {
    let name = &widget.name;
    let Control::Radio { index, states, .. } = &widget.control else {
        return Err(format!(
            "`{name}`: only a radio button has a value it gives its group"
        ));
    };
    if let Some(why) = button_problem(name, value) {
        return Err(why);
    }
    let unanswerable = widget
        .reason
        .as_deref()
        .filter(|why| *why != forms::READ_ONLY);
    if let Some(reason) = unanswerable {
        return Err(format!("`{name}` keeps its value: {reason}"));
    }
    // With `/Opt` the group exports what that list says and the state names
    // are only places in it, so a new name would change nothing a recipient reads.
    if forms::inherited(doc, widget.object, b"Opt").is_some() {
        return Err(format!(
            "`{name}` keeps its value: the group lists what its buttons export, which tpdf does not rewrite"
        ));
    }
    let was = states
        .get(*index)
        .ok_or("a radio button is not among its group's states")?;
    let to = value.as_bytes().to_vec();
    if *was == to {
        return Ok(None);
    }
    let others = || {
        states
            .iter()
            .enumerate()
            .filter(|(at, _)| at != index)
            .map(|(_, state)| state)
    };
    if others().any(|state| *state == to) {
        return Err(format!(
            "`{name}`: the group already has a button with the value `{value}`"
        ));
    }
    // Buttons that share a value are chosen together. Renaming one would
    // take it out of that, which is a different change from the one asked for.
    if others().any(|state| state == was) {
        return Err(format!(
            "`{name}` keeps its value: another button of the group has the same one, and they are chosen together"
        ));
    }
    Ok(Some(Revalued {
        widget,
        was: was.clone(),
        to,
    }))
}

/// Renames the state a radio button has when chosen: in each of its
/// appearances, in what it is showing now, and in what the group holds and
/// holds after a reset where that is this button.
fn revalue(doc: &mut Document, widget: &Widget, was: &[u8], to: &[u8]) -> Result<(), String> {
    let appearances = match doc
        .get_dictionary(widget.widget)
        .map_err(|e| e.to_string())?
        .get(b"AP")
    {
        Ok(Object::Reference(id)) => Some(*id),
        _ => None,
    };
    for look in [b"N".as_slice(), b"D", b"R"] {
        // The appearances and each look are a dictionary in place or an object of their own.
        let held = |doc: &Document| -> Option<Object> {
            let ap = match appearances {
                Some(id) => doc.get_dictionary(id).ok()?,
                None => doc
                    .get_dictionary(widget.widget)
                    .ok()?
                    .get(b"AP")
                    .ok()?
                    .as_dict()
                    .ok()?,
            };
            ap.get(look).ok().cloned()
        };
        let Some(states) = held(doc) else { continue };
        let rename = |states: &mut lopdf::Dictionary| {
            if let Some(drawing) = states.remove(was) {
                states.set(to.to_vec(), drawing);
            }
        };
        match states {
            Object::Reference(id) => {
                if let Ok(states) = doc.get_dictionary_mut(id) {
                    rename(states);
                }
            }
            Object::Dictionary(_) => {
                let ap = match appearances {
                    Some(id) => doc.get_dictionary_mut(id).map_err(|e| e.to_string())?,
                    None => doc
                        .get_dictionary_mut(widget.widget)
                        .map_err(|e| e.to_string())?
                        .get_mut(b"AP")
                        .and_then(Object::as_dict_mut)
                        .map_err(|e| e.to_string())?,
                };
                if let Ok(states) = ap.get_mut(look).and_then(Object::as_dict_mut) {
                    rename(states);
                }
            }
            _ => {}
        }
    }
    let renamed = |dict: &mut lopdf::Dictionary, key: &[u8]| {
        if dict
            .get(key)
            .and_then(Object::as_name)
            .is_ok_and(|now| now == was)
        {
            dict.set(key.to_vec(), Object::Name(to.to_vec()));
        }
    };
    renamed(
        doc.get_dictionary_mut(widget.widget)
            .map_err(|e| e.to_string())?,
        b"AS",
    );
    let group = doc
        .get_dictionary_mut(widget.object)
        .map_err(|e| e.to_string())?;
    renamed(group, b"V");
    renamed(group, b"DV");
    Ok(())
}

/// The least side a widget of this kind may be given.
fn least_side(control: &Control) -> f64 {
    match control {
        Control::Checkbox | Control::Radio { .. } => crate::formfields::MIN_BOX,
        _ => crate::formfields::MIN_TEXT,
    }
}

fn is_signature(doc: &Document, field: ObjectId) -> bool {
    let mut node = Some(field);
    for _ in 0..32 {
        let Some(id) = node else { break };
        let Ok(dict) = doc.get_dictionary(id) else {
            break;
        };
        if let Ok(kind) = dict.get(b"FT").and_then(Object::as_name) {
            return kind == b"Sig";
        }
        node = dict.get(b"Parent").and_then(Object::as_reference).ok();
    }
    false
}

/// A display-space rectangle on the widget's page, in the page's own space.
fn placed(doc: &Document, widget: &Widget, rect: [f32; 4]) -> Result<[f64; 4], String> {
    let name = &widget.name;
    if !rect.iter().all(|v| v.is_finite()) {
        return Err(format!("`{name}`: its rectangle is not four numbers"));
    }
    let [left, top, right, bottom] = rect.map(f64::from);
    let least = least_side(&widget.control);
    if right - left < least || bottom - top < least {
        return Err(format!(
            "`{name}`: a field of this kind is at least {least} by {least} points"
        ));
    }
    let page = *crate::pagetree::ordered_pages(doc)
        .get(widget.page as usize)
        .ok_or_else(|| format!("`{name}`: its page is no longer in this document"))?;
    let geometry = crate::pagetree::displayed_page(doc, page);
    let (width, height) = (f64::from(geometry.width), f64::from(geometry.height));
    if left < -0.01 || top < -0.01 || right > width + 0.01 || bottom > height + 0.01 {
        return Err(format!(
            "`{name}`: its rectangle is not inside page {}, which is {width} by {height} points",
            widget.page + 1
        ));
    }
    Ok(crate::pagetree::from_displayed(
        &geometry,
        [left, top, right, bottom],
    ))
}

/// Where a list of references is written: an array object, or a key of a
/// dictionary object, or the form's `/Fields` inside a catalog that holds the
/// form directly.
enum List {
    Array(ObjectId),
    Key(ObjectId, &'static [u8]),
    CatalogForm(ObjectId),
}

fn list_at(doc: &Document, owner: ObjectId, key: &'static [u8]) -> Option<List> {
    match doc.get_dictionary(owner).ok()?.get(key).ok()? {
        Object::Reference(id) => Some(List::Array(*id)),
        Object::Array(_) => Some(List::Key(owner, key)),
        _ => None,
    }
}

/// The form's top-level field list.
fn top_fields(doc: &Document) -> Option<List> {
    let root = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .ok()?;
    match doc.get_dictionary(root).ok()?.get(b"AcroForm").ok()? {
        Object::Reference(form) => list_at(doc, *form, b"Fields"),
        Object::Dictionary(form) => match form.get(b"Fields").ok()? {
            Object::Reference(id) => Some(List::Array(*id)),
            Object::Array(_) => Some(List::CatalogForm(root)),
            _ => None,
        },
        _ => None,
    }
}

fn items_mut<'a>(doc: &'a mut Document, list: &List) -> Option<&'a mut Vec<Object>> {
    match list {
        List::Array(id) => doc.get_object_mut(*id).ok()?.as_array_mut().ok(),
        List::Key(owner, key) => doc
            .get_dictionary_mut(*owner)
            .ok()?
            .get_mut(key)
            .ok()?
            .as_array_mut()
            .ok(),
        List::CatalogForm(root) => doc
            .get_dictionary_mut(*root)
            .ok()?
            .get_mut(b"AcroForm")
            .ok()?
            .as_dict_mut()
            .ok()?
            .get_mut(b"Fields")
            .ok()?
            .as_array_mut()
            .ok(),
    }
}

/// Takes `item` out of a list, and says how many entries are left.
fn take(doc: &mut Document, list: &List, item: ObjectId) -> Option<usize> {
    let items = items_mut(doc, list)?;
    items.retain(|entry| entry.as_reference().ok() != Some(item));
    Some(items.len())
}

/// The list a field is a member of: its parent's `/Kids`, or the form's.
fn siblings(doc: &Document, field: ObjectId) -> Option<List> {
    match doc
        .get_dictionary(field)
        .ok()?
        .get(b"Parent")
        .and_then(Object::as_reference)
    {
        Ok(parent) => list_at(doc, parent, b"Kids"),
        Err(_) => top_fields(doc),
    }
}

/// The fields beside `field` in its list, each with its own name; `field`
/// itself left out.
fn fields_beside(doc: &Document, field: ObjectId) -> Vec<(ObjectId, String)> {
    let entries: Vec<Object> = match siblings(doc, field) {
        Some(List::Array(id)) => doc.get_object(id).and_then(Object::as_array).ok().cloned(),
        Some(List::Key(owner, key)) => doc
            .get_dictionary(owner)
            .ok()
            .and_then(|dict| dict.get(key).and_then(Object::as_array).ok().cloned()),
        Some(List::CatalogForm(root)) => doc
            .get_dictionary(root)
            .ok()
            .and_then(|dict| dict.get(b"AcroForm").and_then(Object::as_dict).ok())
            .and_then(|form| form.get(b"Fields").and_then(Object::as_array).ok().cloned()),
        None => None,
    }
    .unwrap_or_default();
    entries
        .iter()
        .filter_map(|entry| entry.as_reference().ok())
        .filter(|id| *id != field)
        .filter_map(|id| {
            let raw = doc.get_dictionary(id).ok()?.get(b"T").ok()?.as_str().ok()?;
            Some((id, crate::annots::decode_text_string(raw)))
        })
        .collect()
}

/// Removes one widget: off its page, and out of the field tree, taking the
/// field with it when this was its last widget.
fn remove(doc: &mut Document, widget: &Widget) -> Result<(), String> {
    let page = *crate::pagetree::ordered_pages(doc)
        .get(widget.page as usize)
        .ok_or("the field's page is no longer in this document")?;
    let annots = list_at(doc, page, b"Annots").ok_or("the field's page lists no annotations")?;
    take(doc, &annots, widget.widget).ok_or("the page's annotations could not be changed")?;

    let mut gone = widget.widget;
    if widget.widget != widget.object {
        // One of several widgets under a field: out of that field's kids, and
        // the field goes too only when none is left.
        let kids = list_at(doc, widget.object, b"Kids").ok_or("the field lists no widgets")?;
        let left =
            take(doc, &kids, widget.widget).ok_or("the field's widgets could not be changed")?;
        if left > 0 {
            return Ok(());
        }
        gone = widget.object;
    }
    let beside = siblings(doc, gone).ok_or("the form's field list could not be found")?;
    take(doc, &beside, gone).ok_or("the form's field list could not be changed")?;
    // The calculation order names fields too, and a name left there is a
    // reference that keeps the removed field in the file.
    let root = doc.trailer.get(b"Root").and_then(Object::as_reference).ok();
    let form = root.and_then(|root| {
        doc.get_dictionary(root)
            .ok()?
            .get(b"AcroForm")
            .and_then(Object::as_reference)
            .ok()
    });
    if let Some(order) = form.and_then(|form| list_at(doc, form, b"CO")) {
        take(doc, &order, gone);
    }
    Ok(())
}

/// What a field's properties are to become, as [`apply`] has checked them.
struct Propertied<'a> {
    widget: &'a Widget,
    props: &'a Props,
    /// Whether the field is drawn again.
    redrawn: bool,
    /// The default value the field is given as its answer, because it holds
    /// none.
    answered: Option<&'a str>,
}

/// Checks one field's new properties against the field.
fn propertied<'a>(widget: &'a Widget, props: &'a Props) -> Result<Propertied<'a>, String> {
    let name = &widget.name;
    if let Some(why) = props.problem(name) {
        return Err(why);
    }
    if let Some(most) = props.max_length {
        if !matches!(widget.control, Control::Text) {
            return Err(format!("`{name}`: only a text field has a most characters"));
        }
        let holds = match &widget.value {
            Value::Text(text) => text.chars().count(),
            _ => 0,
        };
        if most > 0 && holds > most as usize {
            return Err(format!(
                "`{name}` holds {holds} characters, which is more than the {most} it would take"
            ));
        }
    }
    let drawn_by_tpdf = matches!(widget.control, Control::Text | Control::Choice { .. });
    if props.align.is_some() && !drawn_by_tpdf {
        return Err(format!(
            "`{name}`: only a text field or a field of choices has text to align"
        ));
    }
    if props.options.is_some() && !matches!(widget.control, Control::Choice { .. }) {
        return Err(format!("`{name}`: only a field of choices has choices"));
    }
    if props.text_size.is_some() && !drawn_by_tpdf {
        return Err(format!(
            "`{name}`: only a text field or a field of choices has a text size"
        ));
    }
    let mut answered = None;
    if let Some(default) = &props.default_value {
        if !matches!(widget.control, Control::Text) {
            return Err(format!("`{name}`: only a text field has a default value"));
        }
        if !widget.multiline && default.contains('\n') {
            return Err(format!(
                "`{name}` takes one line, and its default value has several"
            ));
        }
        let most = match props.max_length {
            Some(0) => None,
            Some(most) => Some(most as usize),
            None => widget.max_length,
        };
        let holds = default.chars().count();
        if most.is_some_and(|most| holds > most) {
            return Err(format!(
                "`{name}`: the default value has {holds} characters, which is more than the field takes"
            ));
        }
        let empty = matches!(&widget.value, Value::Text(text) if text.is_empty());
        if empty && !default.is_empty() {
            answered = Some(default.as_str());
        }
    }
    // Nought and none are the same size: the one that follows the field.
    let resized = props
        .text_size
        .is_some_and(|to| Some(to).filter(|size| *size != 0.0) != widget.text_size);
    let redrawn = props.options.is_some()
        || props.align.is_some_and(|to| to != widget.align)
        || resized
        || answered.is_some();
    if redrawn {
        // Read-only is no obstacle: the flag is lifted while the field is
        // drawn and put back after. Any other reason is one tpdf cannot draw.
        if let Some(reason) = widget.reason.as_deref().filter(|r| *r != forms::READ_ONLY) {
            return Err(format!(
                "`{name}` keeps its alignment, text size, default value and choices: {reason}, so tpdf cannot redraw it"
            ));
        }
    }
    // What a new size or a default value draws is checked here, before
    // anything is written: the drawing itself comes last and would refuse it
    // with the rest of the change already made.
    if (resized || answered.is_some()) && props.options.is_none() {
        let mut as_changed = widget.clone();
        as_changed.reason = None;
        if let Some(size) = props.text_size {
            as_changed.text_size = Some(size).filter(|size| *size != 0.0);
        }
        match props.max_length {
            Some(0) => as_changed.max_length = None,
            Some(most) => as_changed.max_length = Some(most as usize),
            None => {}
        }
        let value = answered.map_or_else(
            || widget.value.clone(),
            |default| Value::Text(default.to_string()),
        );
        forms::validate(&as_changed, &value).map_err(|why| format!("`{name}`: {why}"))?;
    }
    Ok(Propertied {
        widget,
        props,
        redrawn,
        answered,
    })
}

/// Writes a choice field's new choices, and says what it then holds.
///
/// A choice the field already offered keeps the value it exports, which a
/// form's recipient may be reading. What was chosen stays chosen where the
/// new choices still have it; otherwise the field holds nothing.
fn set_options(doc: &mut Document, widget: &Widget, to: &[String]) -> Result<Value, String> {
    let Control::Choice { options: was, .. } = &widget.control else {
        return Ok(widget.value.clone());
    };
    let exported = |label: &String| -> String {
        was.iter()
            .find(|old| &old.label == label)
            .map_or_else(|| label.clone(), |old| old.export.clone())
    };
    let entries: Vec<Object> = to
        .iter()
        .map(|label| {
            let export = exported(label);
            if &export == label {
                forms::pdf_string(label)
            } else {
                Object::Array(vec![forms::pdf_string(&export), forms::pdf_string(label)])
            }
        })
        .collect();
    let value = match &widget.value {
        Value::Selection(chosen) => {
            let labels: Vec<&String> = chosen
                .iter()
                .filter_map(|at| was.get(*at).map(|old| &old.label))
                .collect();
            Value::Selection(
                (0..to.len())
                    .filter(|at| labels.contains(&&to[*at]))
                    .collect(),
            )
        }
        other => other.clone(),
    };
    let field = doc
        .get_dictionary_mut(widget.object)
        .map_err(|e| e.to_string())?;
    field.set("Opt", entries);
    // The scan reads `/V` against `/Opt` and refuses a form whose answer is
    // not among its choices, so the answer is written here as it will stand;
    // the redraw that follows writes it again with its appearance.
    field.remove(b"I");
    match &value {
        Value::Selection(chosen) if chosen.is_empty() => {
            field.remove(b"V");
        }
        Value::Selection(chosen) => {
            let values: Vec<Object> = chosen
                .iter()
                .map(|at| forms::pdf_string(&exported(&to[*at])))
                .collect();
            field.set(
                "V",
                if values.len() == 1 {
                    values[0].clone()
                } else {
                    Object::Array(values)
                },
            );
        }
        _ => {}
    }
    Ok(value)
}

/// Makes every change, or none: each is checked against the document before
/// the first is made.
///
/// # Errors
///
/// A widget the document does not have or that is named twice; a signature
/// field, which is not edited; a name a field may not have or that a field
/// beside it has; a rectangle that is too small, off its page or on a turned
/// page; a resize of a text field whose appearance tpdf cannot redraw, or
/// whose answer no longer fits; a property the field's kind does not have, a
/// limit shorter than the answer the field holds, or two sets of properties
/// for one field; and a form [`forms::scan`] refuses.
pub fn apply(doc: &mut Document, edits: &[FieldEdit]) -> Result<(), String> {
    if edits.is_empty() {
        return Ok(());
    }
    if edits.len() > MAX_EDITS {
        return Err(format!(
            "{} field changes is more than the {MAX_EDITS} one save makes",
            edits.len()
        ));
    }
    let form = forms::scan(doc)?;
    let mut seen = BTreeSet::new();
    let mut rects: Vec<(ObjectId, [f64; 4])> = Vec::new();
    let mut redraw: BTreeMap<ObjectId, forms::Value> = BTreeMap::new();
    let mut names: BTreeMap<ObjectId, String> = BTreeMap::new();
    let mut removals: Vec<&Widget> = Vec::new();
    let mut properties: BTreeMap<ObjectId, Propertied> = BTreeMap::new();
    let mut values: Vec<Revalued> = Vec::new();
    for edit in edits {
        if !seen.insert(edit.widget) {
            return Err("a form field is changed twice".into());
        }
        let widget = form
            .widgets
            .iter()
            .find(|w| w.widget == edit.widget)
            .ok_or("The form field is no longer in this document")?;
        if is_signature(doc, widget.object) {
            return Err(format!(
                "`{}` is a signature field, which tpdf does not move, rename or remove",
                widget.name
            ));
        }
        // Before a removal is set aside: a field shown in several places
        // keeps its properties when the widget they were set under goes.
        if !edit.props.is_empty() {
            let to = propertied(widget, &edit.props)?;
            if properties
                .get(&widget.object)
                .is_some_and(|other| other.props != to.props)
            {
                return Err(format!("`{}` is given two sets of properties", widget.name));
            }
            properties.insert(widget.object, to);
        }
        if edit.remove {
            removals.push(widget);
            continue;
        }
        if let Some(value) = &edit.value {
            values.extend(revalued(doc, widget, value)?);
        }
        if let Some(name) = &edit.name {
            if let Some(why) = crate::formfields::name_problem(name) {
                return Err(why);
            }
            if names.get(&widget.object).is_some_and(|other| other != name) {
                return Err(format!("`{}` is given two names", widget.name));
            }
            names.insert(widget.object, name.clone());
        }
        if let Some(rect) = edit.rect {
            let to = placed(doc, widget, rect)?;
            let was = (
                widget.rect[2] - widget.rect[0],
                widget.rect[3] - widget.rect[1],
            );
            let resized =
                ((to[2] - to[0]) - was.0).abs() > 0.01 || ((to[3] - to[1]) - was.1).abs() > 0.01;
            let drawn_by_tpdf = matches!(widget.control, Control::Text | Control::Choice { .. });
            if resized && drawn_by_tpdf {
                if let Some(reason) = &widget.reason {
                    return Err(format!(
                        "`{}` can be moved and not resized: {reason}, so tpdf cannot redraw it",
                        widget.name
                    ));
                }
                redraw.insert(widget.object, widget.value.clone());
            }
            rects.push((widget.widget, to));
        }
    }
    // A field goes when every one of its widgets does.
    let removed_fields: BTreeSet<ObjectId> = removals
        .iter()
        .map(|w| w.object)
        .filter(|field| {
            let all = form.widgets.iter().filter(|w| w.object == *field).count();
            let going = removals.iter().filter(|w| w.object == *field).count();
            going == all
        })
        .collect();
    // A name is taken when a field beside this one will have it once every
    // change here is made: its own name if it keeps it, its new one if not,
    // and none if it is removed. So two fields can swap names in one call.
    for (field, name) in &names {
        let taken = fields_beside(doc, *field).into_iter().any(|(other, has)| {
            !removed_fields.contains(&other) && names.get(&other).unwrap_or(&has) == name
        });
        if taken {
            return Err(format!("`{name}`: another field has this name"));
        }
    }

    for (widget, rect) in &rects {
        doc.get_dictionary_mut(*widget)
            .map_err(|e| e.to_string())?
            .set(
                "Rect",
                rect.iter()
                    .map(|v| Object::Real(*v as f32))
                    .collect::<Vec<_>>(),
            );
    }
    for (field, name) in &names {
        doc.get_dictionary_mut(*field)
            .map_err(|e| e.to_string())?
            .set("T", crate::formfields::text(name));
    }
    // A field that is drawn again is drawn as one that takes an answer, and
    // is made read-only afterwards if that is what it is to be.
    let mut read_only_after: Vec<(ObjectId, i64)> = Vec::new();
    for (field, to) in &properties {
        let Propertied {
            widget,
            props,
            redrawn,
            answered,
        } = to;
        // Before the answer is drawn again, which reads the size back.
        let appearance = props.text_size.map(|size| {
            forms::sized_appearance(forms::default_appearance(doc, *field).as_deref(), size)
        });
        let was = forms::integer(doc, *field, b"Ff");
        let mut flags = was;
        for (bit, on) in [(1_i64, props.read_only), (1 << 1, props.required)] {
            match on {
                Some(true) => flags |= bit,
                Some(false) => flags &= !bit,
                None => {}
            }
        }
        if *redrawn {
            let value = match (&props.options, answered) {
                (Some(options), _) => set_options(doc, widget, options)?,
                (None, Some(default)) => Value::Text((*default).to_string()),
                (None, None) => widget.value.clone(),
            };
            redraw.insert(*field, value);
        }
        let dict = doc.get_dictionary_mut(*field).map_err(|e| e.to_string())?;
        if let Some(tip) = &props.tooltip {
            if tip.is_empty() {
                dict.remove(b"TU");
            } else {
                dict.set("TU", forms::pdf_string(tip));
            }
        }
        match props.max_length {
            Some(0) => {
                dict.remove(b"MaxLen");
            }
            Some(most) => dict.set("MaxLen", i64::from(most)),
            None => {}
        }
        if let Some(align) = props.align {
            dict.set("Q", align.quadding());
        }
        if let Some(appearance) = appearance {
            dict.set(
                "DA",
                Object::String(appearance, lopdf::StringFormat::Literal),
            );
        }
        match props.default_value.as_deref() {
            Some("") => {
                dict.remove(b"DV");
            }
            Some(default) => dict.set("DV", forms::pdf_string(default)),
            None => {}
        }
        if *redrawn && flags & 1 != 0 {
            dict.set("Ff", flags & !1);
            read_only_after.push((*field, flags));
        } else if flags != was {
            dict.set("Ff", flags);
        }
    }
    for Revalued { widget, was, to } in &values {
        revalue(doc, widget, was, to)?;
    }
    for widget in removals {
        remove(doc, widget)?;
    }
    // Last, on the rectangles as they now are: a text or choice field's
    // appearance is drawn for its size, so a resized one is drawn again.
    let changes: Vec<Change> = redraw
        .into_iter()
        .filter(|(field, _)| !removed_fields.contains(field))
        .map(|(object, value)| Change { object, value })
        .collect();
    forms::write(doc, &changes)?;
    for (field, flags) in read_only_after {
        doc.get_dictionary_mut(field)
            .map_err(|e| e.to_string())?
            .set("Ff", flags);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
