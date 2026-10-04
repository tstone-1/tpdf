//! The order a reader's Tab key takes a page's form fields in.
//!
//! A reader that has no instruction tabs through a page's fields in the order
//! the page lists its annotations, which is the order they were made in. A
//! form made in tpdf was therefore tabbed through in the order its fields
//! were placed, and a field added later was last wherever it sat.
//!
//! This puts a page's fields in the order a person reads them: rows from the
//! top, each row from the left, as the page is displayed. The fields are
//! reordered inside the page's `/Annots`, each widget taking one of the slots
//! a widget had, so that a reader which follows the list, tpdf among them, is
//! right; and the page is given `/Tabs /R`, which is the specification's way
//! of saying the same thing to a reader that asks (§12.5.1, table 30).
//!
//! Nothing but the order changes. A comment or a link keeps its slot, and no
//! field's answer depends on where its widget is listed.

use lopdf::{Document, Object, ObjectId};

/// One widget of a page: where it is listed, and its rectangle as the page is
/// displayed, `[left, top, right, bottom]` from the top-left corner.
struct Listed {
    slot: usize,
    widget: ObjectId,
    rect: [f32; 4],
}

/// Where the page's annotation list is held.
enum Held {
    /// Written into the page.
    Page,
    /// An array object of its own.
    Object(ObjectId),
}

fn held(doc: &Document, page: ObjectId) -> Option<Held> {
    match doc.get_dictionary(page).ok()?.get(b"Annots").ok()? {
        Object::Reference(id) => Some(Held::Object(*id)),
        Object::Array(_) => Some(Held::Page),
        _ => None,
    }
}

fn annots(doc: &Document, page: ObjectId) -> Option<&Vec<Object>> {
    match held(doc, page)? {
        Held::Object(id) => doc.get_object(id).ok()?.as_array().ok(),
        Held::Page => doc
            .get_dictionary(page)
            .ok()?
            .get(b"Annots")
            .ok()?
            .as_array()
            .ok(),
    }
}

/// The page's widgets with a rectangle tpdf can read, in the order listed.
fn widgets(doc: &Document, page: ObjectId) -> Vec<Listed> {
    let Some(entries) = annots(doc, page) else {
        return Vec::new();
    };
    let geometry = crate::pagetree::displayed_page(doc, page);
    entries
        .iter()
        .enumerate()
        .filter_map(|(slot, entry)| {
            let widget = entry.as_reference().ok()?;
            let dict = doc.get_dictionary(widget).ok()?;
            if dict.get(b"Subtype").and_then(Object::as_name).ok()? != b"Widget" {
                return None;
            }
            let numbers = dict.get(b"Rect").and_then(Object::as_array).ok()?;
            let [a, b, c, d] = numbers.as_slice() else {
                return None;
            };
            let mut in_page = [0.0_f64; 4];
            for (out, value) in in_page.iter_mut().zip([a, b, c, d]) {
                *out = f64::from(value.as_float().ok()?);
            }
            if !in_page.iter().all(|v| v.is_finite()) {
                return None;
            }
            let rect = crate::text::to_device(
                geometry.turns,
                geometry.width,
                geometry.height,
                [
                    in_page[0] - f64::from(geometry.origin.0),
                    in_page[1] - f64::from(geometry.origin.1),
                    in_page[2] - f64::from(geometry.origin.0),
                    in_page[3] - f64::from(geometry.origin.1),
                ],
            );
            Some(Listed { slot, widget, rect })
        })
        .collect()
}

/// The widgets in reading order: rows from the top, each from the left.
///
/// Two widgets are in one row when the lower one's middle is no lower than
/// the bottom of the row's first, so a checkbox beside a taller text field is
/// in that field's row. Widgets that tie keep the order they were listed in.
fn by_position(listed: &[Listed]) -> Vec<ObjectId> {
    let mut order: Vec<&Listed> = listed.iter().collect();
    order.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    let mut out = Vec::with_capacity(order.len());
    let mut at = 0;
    while at < order.len() {
        let bottom = order[at].rect[3];
        let mut row: Vec<&Listed> = order[at..]
            .iter()
            .take_while(|one| {
                (one.rect[1] + one.rect[3]) / 2.0 < bottom || one.slot == order[at].slot
            })
            .copied()
            .collect();
        at += row.len();
        row.sort_by(|a, b| a.rect[0].total_cmp(&b.rect[0]));
        out.extend(row.iter().map(|one| one.widget));
    }
    out
}

/// How many widgets the page lists.
#[must_use]
pub fn count(doc: &Document, page: ObjectId) -> usize {
    widgets(doc, page).len()
}

/// Whether the page's fields are already listed in reading order. A page
/// with none is.
#[must_use]
pub fn in_order(doc: &Document, page: ObjectId) -> bool {
    let listed = widgets(doc, page);
    by_position(&listed)
        .iter()
        .zip(&listed)
        .all(|(wanted, now)| *wanted == now.widget)
}

/// Lists the page's fields in reading order and says so in `/Tabs`. `true`
/// when the list changed.
///
/// # Errors
///
/// The page or its annotation list cannot be written.
pub fn sort(doc: &mut Document, page: ObjectId) -> Result<bool, String> {
    let listed = widgets(doc, page);
    if listed.is_empty() {
        return Ok(false);
    }
    let wanted = by_position(&listed);
    let changed = wanted
        .iter()
        .zip(&listed)
        .any(|(to, now)| *to != now.widget);
    if changed {
        let entries = match held(doc, page).ok_or("the page lists no annotations")? {
            Held::Object(id) => doc.get_object_mut(id).and_then(Object::as_array_mut),
            Held::Page => doc
                .get_dictionary_mut(page)
                .and_then(|dict| dict.get_mut(b"Annots"))
                .and_then(Object::as_array_mut),
        }
        .map_err(|e| e.to_string())?;
        for (to, now) in wanted.iter().zip(&listed) {
            entries[now.slot] = Object::Reference(*to);
        }
    }
    doc.get_dictionary_mut(page)
        .map_err(|e| e.to_string())?
        .set("Tabs", "R");
    Ok(changed)
}

#[cfg(test)]
mod tests;
