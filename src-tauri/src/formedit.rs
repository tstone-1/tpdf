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
//! Nothing here adds a script, an action or a calculation, and none is
//! touched: a field that has one keeps it under its new name or place.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Document, Object, ObjectId};
use serde::{Deserialize, Serialize};

use crate::forms::{self, Change, Control, Widget};

/// The most fields one call changes. A bound on what a caller can ask.
pub const MAX_EDITS: usize = 1000;

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
    if geometry.turns != 0 {
        return Err(format!(
            "`{name}`: page {} is turned by the document, and a field on a turned page cannot \
             be moved yet",
            widget.page + 1
        ));
    }
    let (width, height) = (f64::from(geometry.width), f64::from(geometry.height));
    if left < -0.01 || top < -0.01 || right > width + 0.01 || bottom > height + 0.01 {
        return Err(format!(
            "`{name}`: its rectangle is not inside page {}, which is {width} by {height} points",
            widget.page + 1
        ));
    }
    let x = f64::from(geometry.origin.0) + left;
    let y = f64::from(geometry.origin.1) + height - bottom;
    Ok([x, y, x + (right - left), y + (bottom - top)])
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

/// Makes every change, or none: each is checked against the document before
/// the first is made.
///
/// # Errors
///
/// A widget the document does not have or that is named twice; a signature
/// field, which is not edited; a name a field may not have or that a field
/// beside it has; a rectangle that is too small, off its page or on a turned
/// page; a resize of a text field whose appearance tpdf cannot redraw, or
/// whose answer no longer fits; and a form [`forms::scan`] refuses.
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
        if edit.remove {
            removals.push(widget);
            continue;
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
    forms::write(doc, &changes)
}

#[cfg(test)]
mod tests;
