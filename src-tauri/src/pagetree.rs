//! Surgery on a document's page tree, shared by the two things that write one.
//!
//! `print.rs` builds a job and `save.rs` writes a copy, and they are the same
//! operation with different endings: take a file, keep some of its pages, turn
//! some of them, serialise. Everything below had one caller until deleting a page
//! landed --- `drop_pages` was print's and `agreed_turns` was save's --- and a
//! second copy of either is the failure `docs/TRAPS.md` records under *"two
//! copies of a distinction drift, and a mutation of one survives"*. They are one
//! copy here.
//!
//! **What makes this a module rather than a `pub(crate)` on each of them** is
//! that the sharing goes both ways. Left where they were, `save.rs` would import
//! the page-tree walk from `print.rs` and `print.rs` would import the turn
//! reconciliation from `save.rs`, which reads as though one of them is a library
//! for the other.
//!
//! The two properties the callers depend on and cannot see:
//!
//!  - **A page object can answer to more than one page number.** `/Kids` may name
//!    it twice and `lopdf`'s page walk keeps no visited set, so a loop over page
//!    numbers visits one object twice --- and the second visit reads what the
//!    first wrote. Every function here counts in whichever unit its answer is in,
//!    and says which.
//!  - **`/Rotate` is inheritable**, so the value composed against must come up the
//!    `/Parent` chain rather than out of the page's own dictionary. It is one of
//!    four, and the other three are why [`reorder_pages`] is more than a rewrite
//!    of one array.

use std::collections::{HashMap, HashSet};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::sweep;

mod labels;
pub(crate) use labels::remap as remap_labels;

/// How far up a `/Parent` chain anything here will walk.
///
/// A cycle in a malformed file would otherwise spin, and every walk in this
/// module runs on a document we did not write. One constant rather than one per
/// walk, because the answer they give when they give up has to be the same.
const MAX_PARENTS: usize = 64;

/// The four page attributes a page may inherit from an ancestor.
///
/// PDF 32000-1 table 29. They are the reason [`reorder_pages`] cannot simply
/// rewrite the root's `/Kids`: a page that inherits its size from the tree node
/// it hangs under loses that size the moment it hangs somewhere else.
const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// The value a page has for an inheritable key, its own or an ancestor's.
///
/// Absent all the way up means the page really has none. A value that is present
/// and malformed *is* the answer --- the lookup stops at the first ancestor
/// stating the key, which is what the specification says and is not what this
/// walk used to do for `/Rotate`: it stepped over a non-integer and inherited the
/// grandparent's. The difference is only reachable on a document that states
/// `/Rotate (ninety)`, and answering upright is the safer of the two.
pub(crate) fn inherited(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    inherited_value(doc, page, key).ok().flatten().cloned()
}

/// Borrow the effective resources without cloning a dictionary (which may
/// contain direct streams). A missing resource dictionary is valid on a page
/// drawn entirely with the built-in graphics operators.
pub(crate) fn resources(doc: &Document, page: ObjectId) -> lopdf::Result<Option<&Dictionary>> {
    inherited_value(doc, page, b"Resources")?
        .map(|value| doc.dereference(value)?.1.as_dict())
        .transpose()
}

fn inherited_value<'a>(
    doc: &'a Document,
    page: ObjectId,
    key: &[u8],
) -> lopdf::Result<Option<&'a Object>> {
    Ok(inherited_entry(doc, page, key)?.map(|(_, value)| value))
}

/// Keep the owning node so a writer need only copy values inherited from a parent.
fn inherited_entry<'a>(
    doc: &'a Document,
    page: ObjectId,
    key: &[u8],
) -> lopdf::Result<Option<(ObjectId, &'a Object)>> {
    let mut at = page;
    for _ in 0..MAX_PARENTS {
        let node = doc.get_dictionary(at)?;
        if let Ok(value) = node.get(key) {
            if !matches!(doc.dereference(value)?.1, Object::Null) {
                return Ok(Some((at, value)));
            }
        }
        let Ok(parent) = node.get(b"Parent") else {
            return Ok(None);
        };
        if matches!(doc.dereference(parent)?.1, Object::Null) {
            return Ok(None);
        }
        at = parent.as_reference()?;
    }
    Err(lopdf::Error::ReferenceLimit)
}

/// Read before reparenting: an unreadable reference is not an absent key, and a
/// missing media box has no default that can safely travel with the page.
fn moving_attribute<'a>(
    doc: &'a Document,
    page: ObjectId,
    key: &[u8],
) -> Result<Option<(ObjectId, &'a Object)>, String> {
    let entry = inherited_entry(doc, page, key).map_err(|e| {
        said!(
            "Cannot read the page attribute /{key}: {e}",
            key = String::from_utf8_lossy(key)
        )
    })?;
    if key == b"MediaBox" && entry.and_then(|(_, media)| box_value(doc, media)).is_none() {
        return Err(said!("The page has no usable media box"));
    }
    Ok(entry)
}

/// Every page's quarter-turns, in page order.
///
/// **The same table [`displayed_page`] builds, exposed for a caller that wants
/// only the turn**, which is not a second reading of `/Rotate`: the formula in
/// there is PDFium's own --- truncating division by 90, then modulo 4, then the
/// wrap for a negative --- and the inheritance walk is the one this module
/// already does for `/MediaBox`.
///
/// It exists because the outline walk had no way to ask. `FPDFPage_GetRotation`
/// needs an `FPDF_PAGE`, so a bookmark naming coordinates cost an
/// `FPDF_LoadPage` --- 44 ms on a complex page --- and an outline over four
/// hundred pages paid it four hundred times, at open, before a reader had
/// clicked anything. Read through `docgraph::DocumentGraph`, which parses the
/// document once for this and four other questions.
///
/// # Errors
///
/// The page tree and the renderer disagreeing about how many pages there are,
/// which makes every index below mean something different from what the caller
/// means by it. Refused rather than answered short: a caller reading turn `n`
/// from a table that is missing page 3 gets a plausible wrong answer.
pub fn rotations_from(document: &Document, page_count: usize) -> Result<Vec<u8>, String> {
    Ok(pages_of(document, page_count)?
        .into_iter()
        .map(|id| displayed_page(document, id).turns)
        .collect())
}

/// A page's `/Rotate` including anything it inherits.
///
/// Absent means "ask the parent", and only a page with no ancestor carrying one
/// is really at zero. Composing against the literal value instead would be
/// correct on every document that states it and wrong on every document that
/// does not --- which is the half nobody has a fixture for.
pub fn effective_rotation(doc: &Document, page: ObjectId) -> i64 {
    inherited(doc, page, b"Rotate")
        .and_then(|value| crate::encoding::resolve(doc, &value).as_i64().ok())
        .unwrap_or(0)
}

/// Quarter turns clockwise for a `/Rotate` value in degrees.
///
/// The division comes **before** the modulus, which is what decides a value
/// §7.7.3.3 does not allow: `/Rotate 135` truncates towards zero to one quarter
/// turn and `/Rotate 45` to none. The specification requires a multiple of 90
/// and a hostile file is under no obligation to write one, so this answers for
/// every `i64` rather than refusing --- a page whose rotation cannot be read is
/// a page shown the way it was written, not a document that will not open.
///
/// The `+ 4` before the second modulus is load-bearing. Rust's `%` takes the
/// dividend's sign, so `-90 / 90 % 4` is `-1`, and a page the document says to
/// show turned three quarters clockwise would come back as a turn count that
/// does not exist. `/Rotate -90` is ordinary; scanners write it.
///
/// One function rather than the expression twice. `annots.rs` and
/// [`displayed_page`] both need it and had a copy each, which is the drift this
/// module's own note is about --- and note this collapses only the arithmetic:
/// the two page-box readings stay separate on purpose, so
/// `links::tests::both_scans_agree_about_a_rotated_page` still compares two
/// answers rather than one answer with itself.
#[must_use]
pub fn turns_of_degrees(degrees: i64) -> u8 {
    (((degrees / 90) % 4 + 4) % 4) as u8
}

/// A page's displayed size, its rotation, and where its own space starts.
///
/// "Displayed" is the page as the *document* says to show it --- `/Rotate`
/// applied, laid out from the crop box --- and not as any reader is currently
/// looking at it. A view rotation and the turns a reader has added live in the
/// model, not here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayedPage {
    /// Displayed width in points, after `/Rotate`.
    pub width: f32,
    /// Displayed height in points, after `/Rotate`.
    pub height: f32,
    /// Quarter turns clockwise the document asks for, 0 to 3.
    pub turns: u8,
    /// The lower-left corner of the box the page is displayed from.
    ///
    /// Zero for most documents. Not zero for one whose `/CropBox` is inset, and
    /// then it is the difference between a rectangle landing on its text and
    /// landing somewhere else entirely.
    pub origin: (f32, f32),
}

/// First pair of slots sharing each repeated page object.
pub(crate) fn shared_pages(pages: &[ObjectId]) -> HashMap<ObjectId, (usize, usize)> {
    let mut first = HashMap::new();
    let mut shared = HashMap::new();
    for (slot, &id) in pages.iter().enumerate() {
        let original = *first.entry(id).or_insert(slot);
        if original != slot {
            shared.entry(id).or_insert((original, slot));
        }
    }
    shared
}

/// Physical points per default user-space unit. UserUnit is a page entry.
pub(crate) fn user_unit(doc: &Document, page: ObjectId) -> Option<f64> {
    let dict = doc.get_dictionary(page).ok()?;
    let Ok(value) = dict.get(b"UserUnit") else {
        return Some(1.0);
    };
    let (_, value) = doc.dereference(value).ok()?;
    let unit = match value {
        Object::Null => 1.0,
        Object::Integer(n) => *n as f64,
        Object::Real(n) => f64::from(*n),
        _ => return None,
    };
    (unit.is_finite() && unit > 0.0).then_some(unit)
}

impl DisplayedPage {
    /// The same rectangle in the page's **own** space, before `/Rotate`.
    ///
    /// [`width`](Self::width) and [`height`](Self::height) are what a reader
    /// sees, so they are already transposed on a quarter turn. A `/CropBox` is
    /// not: §14.11.2 puts it in the page's coordinate system, and so does every
    /// PDFium box API. Anything handing this rectangle back to either wants
    /// this rather than those two.
    ///
    /// A method rather than four lines at the call site, because the transpose
    /// is the part that is easy to get backwards and this module's own note is
    /// about second copies of exactly that kind of rule.
    pub fn box_pt(&self) -> [f32; 4] {
        let (width, height) = if self.turns % 2 == 1 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        [
            self.origin.0,
            self.origin.1,
            self.origin.0 + width,
            self.origin.1 + height,
        ]
    }
}

/// One of a page's boxes, inherited, normalised, and refused if it is degenerate.
///
/// `[llx, lly, urx, ury]`, corners in ascending order whichever way round the
/// producer wrote them. `None` for a box that is absent, unreadable, non-finite
/// or encloses nothing --- four ways of saying "there is no usable box here",
/// which every caller wants to treat the same way.
///
/// Extracted from [`displayed_page`]'s own closure when [`apply_crops`] needed
/// the media box: a second reader of the same four numbers is the drift this
/// module's note is about, and it would disagree in exactly the case that makes
/// the difference --- a box written corner-first.
pub(crate) fn box_on(doc: &Document, page: ObjectId, key: &[u8]) -> Option<[f32; 4]> {
    box_value(doc, inherited_value(doc, page, key).ok()??)
}

fn box_value(doc: &Document, value: &Object) -> Option<[f32; 4]> {
    numbers_of(doc, value, 4)
        .filter(|values| values.iter().all(|value| value.is_finite()))
        .map(|v| {
            [
                v[0].min(v[2]),
                v[1].min(v[3]),
                v[0].max(v[2]),
                v[1].max(v[3]),
            ]
        })
        // Four finite corners can still be an infinite page: `f32::MAX` less
        // `-f32::MAX` overflows, and every rectangle mapped through that
        // extent is infinity, which a JSON reply carries as `null`.
        .filter(|b| b[2] > b[0] && b[3] > b[1])
        .filter(|b| (b[2] - b[0]).is_finite() && (b[3] - b[1]).is_finite())
}

/// The displayed page box: `/CropBox` where there is one, else `/MediaBox`.
///
/// **Reading `/MediaBox` alone was wrong, and the failure is silent.** PDFium
/// lays a page out from its crop box, so the viewer's coordinates start at that
/// box's corner --- while the media box's corner may be somewhere else. Every
/// rectangle mapped through the wrong one is offset by the difference, on a page
/// that looks entirely normal. Measured when `links.rs` had it wrong: a fixture
/// with `/CropBox [50 50 545 742]` on `/MediaBox [0 0 595 842]` renders 495x692
/// and put its character boxes on ink **0%** of the time, against 100% for the
/// same page uncropped. One of the 43 PDFs on the machine this was written on
/// carries an off-origin crop box on all ten pages.
///
/// §14.11.2 says the crop box is intersected with the media box, and that is
/// done here rather than trusted: a producer may write a crop box larger than
/// the sheet, and a page displayed bigger than its own paper is not something to
/// map coordinates into.
///
/// **This lives here because there are three consumers and there were nearly
/// three copies.** It was `links.rs`'s, duplicated in `annots.rs`; when the save
/// path needed it in order to place a highlight, a third copy would have been
/// the drift this module's own note is about. `links.rs` calls this now and
/// `annots.rs` keeps its own --- deliberately, because
/// `links::tests::both_scans_agree_about_a_rotated_page` compares the two
/// answers on one document, and collapsing them would turn that test into one
/// that cannot fail.
pub fn displayed_page(doc: &Document, page: ObjectId) -> DisplayedPage {
    let box_of = |key: &[u8]| box_on(doc, page, key);

    let media = box_of(b"MediaBox").unwrap_or([0.0, 0.0, 612.0, 792.0]);
    page_geometry(media, box_of(b"CropBox"), effective_rotation(doc, page))
}

/// A writer must know the actual page geometry before placing visible content.
pub(crate) fn writable_page(doc: &Document, page: ObjectId) -> Option<DisplayedPage> {
    let media = box_value(doc, inherited_value(doc, page, b"MediaBox").ok()??)?;
    let crop = match inherited_value(doc, page, b"CropBox").ok()? {
        Some(value) => Some(box_value(doc, value)?),
        None => None,
    };
    let rotation = match inherited_value(doc, page, b"Rotate").ok()? {
        Some(value) => doc.dereference(value).ok()?.1.as_i64().ok()?,
        None => 0,
    };
    Some(page_geometry(media, crop, rotation))
}

fn page_geometry(media: [f32; 4], crop: Option<[f32; 4]>, rotation: i64) -> DisplayedPage {
    let shown = match crop {
        Some(crop) => [
            crop[0].max(media[0]),
            crop[1].max(media[1]),
            crop[2].min(media[2]),
            crop[3].min(media[3]),
        ],
        None => media,
    };
    // An intersection can be empty if the two boxes do not overlap, which is a
    // malformed document rather than a page of no size. The media box is the
    // honest fallback: it is the sheet, and §14.11.2 makes the crop box the
    // thing being questioned.
    let shown = if shown[2] > shown[0] && shown[3] > shown[1] {
        shown
    } else {
        media
    };

    let turns = turns_of_degrees(rotation);
    let (width, height) = (shown[2] - shown[0], shown[3] - shown[1]);
    DisplayedPage {
        width: if turns % 2 == 1 { height } else { width },
        height: if turns % 2 == 1 { width } else { height },
        turns,
        origin: (shown[0], shown[1]),
    }
}

/// A rectangle of the page as it is displayed, `[left, top, right, bottom]`
/// in points from its top-left corner, in the page's own space:
/// `[left, bottom, right, top]` with y upwards, where an annotation's `/Rect`
/// is.
///
/// `text::from_device` with the box's origin added, and in `f64`: a field's
/// rectangle is written with the digits it was asked for.
#[must_use]
pub fn from_displayed(shown: &DisplayedPage, rect: [f64; 4]) -> [f64; 4] {
    let [left, top, right, bottom] = rect;
    let (width, height) = (f64::from(shown.width), f64::from(shown.height));
    // The page's own size, before the turn.
    let (w0, h0) = if shown.turns % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    let [l, b, r, t] = match shown.turns % 4 {
        0 => [left, h0 - bottom, right, h0 - top],
        1 => [top, left, bottom, right],
        2 => [w0 - right, top, w0 - left, bottom],
        _ => [w0 - bottom, h0 - right, w0 - top, h0 - left],
    };
    let (x, y) = (f64::from(shown.origin.0), f64::from(shown.origin.1));
    [x + l, y + b, x + r, y + t]
}

/// The box every page is displayed from, in page order.
///
/// **Why this exists at all**: `FPDFPage_GetMediaBox` does not walk `/Parent`,
/// so a page that inherits its box from an ancestor gets no answer from PDFium
/// --- and `FPDF_GetPageWidthF` then reports `width x width` for one that also
/// carries a quarter turn. `docs/TRAPS.md` has the crossed measurements. This is
/// the answer PDFium cannot give, in the space its own box API uses, so that
/// `RawDocument::page_cropped` can hand it back.
///
/// Boxes come out in PDFium's page-index order, and a document `lopdf` and
/// PDFium disagree about the *length* of is refused outright rather than
/// returned short. Applying page 5's box to page 4 is worse than applying none:
/// the failure would be a plausible page at a plausible size, on a document
/// where nothing looks wrong.
///
/// # Errors
///
/// The bytes not parsing, or the two parsers disagreeing about the page count.
pub fn displayed_boxes(
    bytes: &[u8],
    page_count: usize,
    password: Option<&str>,
) -> Result<Vec<[f32; 4]>, String> {
    displayed_boxes_from(&crate::encoding::load(bytes, password)?, page_count)
}

/// [`displayed_boxes`] over a document somebody else parsed.
///
/// See [`crate::annots::scan_from`]. This one is on the path that loads a page,
/// so the parse it no longer makes for itself is a parse a reader was waiting on.
///
/// # Errors
///
/// As [`displayed_boxes`], less the parse it no longer does.
pub fn displayed_boxes_from(
    document: &Document,
    page_count: usize,
) -> Result<Vec<[f32; 4]>, String> {
    Ok(pages_of(document, page_count)?
        .into_iter()
        .map(|id| displayed_page(document, id).box_pt())
        .collect())
}

/// The page objects in page order, refusing a tree the renderer disagrees with.
///
/// **One guard for both tables**, and not only to avoid the copy: the two
/// answers are indexed positionally by the same caller, so a rule that held for
/// the boxes and not for the rotations would be a document where page 5's box
/// and page 4's turn describe one page --- each plausible, and nothing anywhere
/// disagreeing. Refused rather than answered short, for the same reason.
///
/// # Errors
///
/// The page tree and the renderer counting different numbers of pages.
pub(crate) fn pages_of(document: &Document, page_count: usize) -> Result<Vec<ObjectId>, String> {
    let pages = document.get_pages();
    if pages.len() != page_count {
        return Err(said!(
            "the page tree holds {count} pages and the renderer sees {page_count}",
            count = pages.len()
        ));
    }
    Ok(pages.values().copied().collect())
}

/// `count` numbers out of an object that may be an array or a reference to one.
///
/// A short array, a non-numeric entry, or anything that is not an array at all
/// is `None` rather than a partial answer: a `/MediaBox` of three numbers is not
/// a box, and taking three of the four would place every rectangle on the page
/// somewhere plausible and wrong.
///
/// `pub(crate)` for `links.rs`, which reads a `/Rect` with it. It came from
/// there, and leaving a copy behind to avoid one shared helper would be the
/// duplication this whole move was about.
pub(crate) fn numbers_of(doc: &Document, object: &Object, count: usize) -> Option<Vec<f32>> {
    let Object::Array(array) = resolved(doc, object) else {
        return None;
    };
    if array.len() < count {
        return None;
    }
    let values: Vec<f32> = array
        .iter()
        .take(count)
        .filter_map(|item| resolved(doc, item).as_float().ok())
        .collect();
    (values.len() == count).then_some(values)
}

/// One step through a reference, or the object itself.
///
/// One step rather than a loop: a reference to a reference is not something a
/// well-formed file produces, and following a chain is how a malformed one gets
/// a walk to spin.
fn resolved(doc: &Document, object: &Object) -> Object {
    match object {
        Object::Reference(id) => doc.get_object(*id).cloned().unwrap_or(Object::Null),
        other => other.clone(),
    }
}

/// The document's pages as object ids, in page-number order.
///
/// `get_pages` answers a `BTreeMap` keyed by page number, so the order is the
/// map's and no sort is needed --- named here rather than left as a `.values()`
/// at each caller, because "the plan's *n*th entry is the document's *n*th page"
/// is the whole contract between the model and these two writers, and it rests
/// on which container that map is.
pub fn ordered_pages(doc: &Document) -> Vec<ObjectId> {
    doc.get_pages().values().copied().collect()
}

/// One turn per distinct page *object*, or a refusal naming the pages that disagree.
///
/// `plan[i]` is the page the reader sees as page `i + 1`: the object that supplies
/// it, and the quarter turns to add. A well-formed document gives every page an
/// object of its own; a malformed one need not, and then two entries share one
/// `/Rotate`. Composing the turn once per *entry* would turn that page twice,
/// because the second visit reads what the first wrote. See the trap; the same
/// shape was live in `print.rs` in two places, one of them since printing landed.
///
/// **Refused only where the plan genuinely cannot be honoured.** Turns that agree
/// can be: the object turns once and both pages show it, which is what was asked
/// for. Turns that differ cannot be by any output --- page 3 cannot be at 90 and
/// page 7 at 180 when they are one object --- so that is refused rather than
/// resolved by picking one and handing back a file the reader would have to check.
/// A blanket refusal was the obvious move and is wrong for the case that
/// dominates: a document nobody edited, where every turn is zero and there is
/// nothing to reconcile.
///
/// # Errors
///
/// Two entries name one object and ask it for different turns.
pub fn agreed_turns(plan: &[(ObjectId, u8)]) -> Result<Vec<(ObjectId, u8)>, String> {
    let mut order: Vec<ObjectId> = Vec::with_capacity(plan.len());
    let mut chosen: HashMap<ObjectId, (u8, usize)> = HashMap::new();
    for (at, (id, extra)) in plan.iter().enumerate() {
        let extra = extra % 4;
        match chosen.get(id) {
            None => {
                chosen.insert(*id, (extra, at));
                order.push(*id);
            }
            Some(&(first, first_at)) if first != extra => {
                return Err(said!(
                    "pages {first} and {second} are the same page in this file, so they cannot be turned \
                     differently. Turn them the same way, or leave both as they are.", first = first_at + 1, second = at + 1));
            }
            Some(_) => {}
        }
    }
    Ok(order.into_iter().map(|id| (id, chosen[&id].0)).collect())
}

/// Writes each page's composed rotation, once per object.
///
/// Takes what [`agreed_turns`] produced, so the caller cannot hand this a list
/// that names one object twice.
///
/// **A page whose turn is zero is not written to at all.** Not tidiness: the
/// value that would be written is [`effective_rotation`]'s answer, and that walk
/// is bounded at 64 hops and answers **0** when it gives up or meets a cycle ---
/// so writing it onto every page silently flattens the rotation of any page whose
/// `/Parent` chain is longer than the bound, pages nobody asked to change. The
/// skip also leaves an untouched page byte-identical, which is what "a copy"
/// should mean.
///
/// # Errors
///
/// A page id that is not a dictionary.
pub fn apply_turns(doc: &mut Document, turns: &[(ObjectId, u8)]) -> Result<(), String> {
    for (id, extra) in turns {
        let extra = i64::from(*extra);
        if extra == 0 {
            continue;
        }
        let composed = (i64::from(turns_of_degrees(effective_rotation(doc, *id))) + extra) % 4 * 90;
        doc.get_object_mut(*id)
            .and_then(Object::as_dict_mut)
            .map_err(|e| said!("page {page:?} is not a dictionary: {e}", page = id))?
            .set("Rotate", composed);
    }
    Ok(())
}

/// Writes a `/CropBox` on each page that carries one in the plan.
///
/// `crops` is `(page object, [llx, lly, urx, ury])` in the page's own space,
/// y upwards, which is the space `/CropBox` is defined in.
///
/// **Written on the page itself, never on an ancestor**, even where the page
/// inherited its box from one. `/CropBox` is inheritable ([`INHERITABLE`]), so
/// setting it on a `/Pages` node would crop every page hanging under it ---
/// which for a document whose pages share one node is every page in the file,
/// from a reader who cropped one.
///
/// **Intersected with the media box**, because §14.11.2 says a reader does that
/// and a box outside the sheet otherwise produces a page that renders as nothing
/// in tpdf and as the whole sheet in something else. The model already refused a
/// degenerate rectangle (`docmodel::Rect::is_proper`); this refuses one the
/// intersection empties, which is a different question and can only be asked
/// here, where the media box is known.
///
/// # Errors
///
/// A page object that is not a dictionary, or a crop that shares no area with
/// the page's media box.
pub fn apply_crops(doc: &mut Document, crops: &[(ObjectId, [f64; 4])]) -> Result<(), String> {
    for (id, want) in crops {
        let media = box_on(doc, *id, b"MediaBox").map(|b| b.map(f64::from));
        // The default US Letter sheet, which is what `displayed_page` falls back
        // to and has to be the same number: a crop clamped against one sheet and
        // displayed against another is the disagreement this whole module exists
        // to prevent.
        let media = media.unwrap_or([0.0, 0.0, 612.0, 792.0]);
        let box_pt = [
            want[0].max(media[0]),
            want[1].max(media[1]),
            want[2].min(media[2]),
            want[3].min(media[3]),
        ];
        if box_pt[2] <= box_pt[0] || box_pt[3] <= box_pt[1] {
            return Err(said!(
                "page {id:?}: the crop {want:?} is outside the page {media:?}"
            ));
        }
        doc.get_object_mut(*id)
            .and_then(Object::as_dict_mut)
            .map_err(|e| said!("page {page:?} is not a dictionary: {e}", page = id))?
            .set(
                "CropBox",
                Object::Array(box_pt.iter().map(|v| Object::Real(*v as f32)).collect()),
            );
    }
    Ok(())
}

/// The page objects that removing `numbers` removes.
///
/// A page object can answer to more than one page number: `/Kids` may list it
/// twice, and `lopdf`'s page walk keeps no visited set. An object that a KEPT
/// number also names must survive --- otherwise printing "page 1" of such a
/// document deletes the object page 1 *is*, and prints a blank sheet. This is
/// the damaging member of the family in the trap: the other two turn a page
/// twice, this one removes the page that was asked for.
///
/// One spelling, because [`materialise`] asks the same question for the outline
/// before [`drop_pages`] answers it for the page tree, and two answers would
/// leave a bookmark to a page that stayed removed, or one to a page that went
/// kept.
#[must_use]
pub fn doomed_pages(
    pages: &std::collections::BTreeMap<u32, ObjectId>,
    numbers: &[u32],
) -> HashSet<ObjectId> {
    let kept: HashSet<ObjectId> = pages
        .iter()
        .filter(|(number, _)| !numbers.contains(number))
        .map(|(_, id)| *id)
        .collect();
    numbers
        .iter()
        .filter_map(|number| pages.get(number).copied())
        .filter(|id| !kept.contains(id))
        .collect()
}

/// Removes pages, and every reference to them, in a single pass.
///
/// `numbers` are page numbers, one-based, as `lopdf`'s page table keys them.
///
/// `lopdf::delete_pages` does exactly this and does not scale: it calls
/// `delete_object` per page, and `delete_object` calls `traverse_objects` ---
/// the quadratic walk AGENTS.md already records for `prune_objects`, here run
/// once *per deleted page*. Measured release-profile on the 775-page corpus,
/// keeping two pages: **620 ms** against **1.2 ms** here, and the two produce
/// byte-identical output on every fixture and corpus
/// (`control_page_deletion_matches_lopdf_byte_for_byte`).
///
/// Same shape as the mark-and-sweep, and the same conclusion --- use `lopdf` for
/// the object model, write the graph walks ourselves.
///
/// # Errors
///
/// An object nesting deeper than [`sweep::MAX_NESTING`]. Refused rather than
/// walked as far as it goes, for the same reason the sweep refuses: a pass that
/// stopped early would leave a reference to a deleted page in the file, and a
/// page tree naming an object that is gone is a document that opens and prints
/// blank pages.
pub fn drop_pages(doc: &mut Document, numbers: &[u32]) -> Result<(), String> {
    if numbers.is_empty() {
        return Ok(());
    }
    let pages = doc.get_pages();
    let doomed = doomed_pages(&pages, numbers);
    if doomed.is_empty() {
        return Ok(());
    }

    // Every ancestor of every doomed page, collected before anything moves, so
    // that a `/Count` is decremented once per page beneath it. A page tree is
    // usually two levels deep and may be many.
    //
    // Walked per page NUMBER rather than per object, because `/Count` counts
    // entries in the tree: two numbers naming one doomed object remove two of
    // them. Iterating the object set decremented once per object, which is right
    // only while objects and page numbers are the same thing.
    let mut decrements = Vec::new();
    for number in numbers {
        let Some(id) = pages.get(number).copied() else {
            continue;
        };
        if !doomed.contains(&id) {
            continue;
        }
        let mut at = parent_of(doc, id);
        // Same `/Parent`-cycle bound as `inherited`, same reason: this runs on
        // input we did not write.
        //
        // **The bound alone stops the walk and not the damage**, which is why
        // there is a visited set beside it. A `/Parent` chain that loops --- two
        // `Pages` nodes naming each other, which is four lines of a file we did
        // not write --- yields the same two nodes thirty-two times each, and
        // every one of them is pushed here and decrements a `/Count` below. A
        // three-page tree came out claiming -61 pages, from deleting one page.
        // Each ancestor is counted once per page NUMBER, which is what the
        // paragraph above is about; a cycle is not a second ancestor.
        let mut seen: HashSet<ObjectId> = HashSet::new();
        for _ in 0..MAX_PARENTS {
            let Some(parent) = at else { break };
            if !seen.insert(parent) {
                break;
            }
            decrements.push(parent);
            at = parent_of(doc, parent);
        }
    }
    for parent in decrements {
        let count = doc
            .get_dictionary(parent)
            .and_then(|tree| tree.get_deref(b"Count", doc))
            .and_then(Object::as_i64);
        if let (Ok(count), Ok(tree)) = (count, doc.get_dictionary_mut(parent)) {
            // Saturating, because the count is the document's number and
            // `/Count -9223372036854775808` is as writable as any other:
            // `count - 1` on it panics under debug assertions and wraps to
            // the largest positive count there is in the shipped build,
            // which is a document that reports 9.2 quintillion pages after
            // one was deleted.
            tree.set("Count", count.saturating_sub(1));
        }
    }

    // One pass over the whole graph, dropping array entries and dictionary keys
    // that name a doomed page --- the `/Kids` entry that removes it from the
    // tree, and anything else pointing at it. The trailer is not in `objects`
    // and has to be walked in its own right.
    forget(doc, &doomed)
}

/// Removes objects and every reference to them, in a single pass.
///
/// The half of [`drop_pages`] that is not about the page tree, extracted when
/// the redaction path needed the same guarantee for annotations. **Every
/// reference, not only the obvious one**, is the property that matters and the
/// reason this is not `objects.remove` at the call site: an annotation is named
/// by its page's `/Annots` and can be named again by a structure element's
/// `/OBJR`, by an AcroForm's `/Fields`, or by another annotation's `/IRT`.
/// Pruning the one list a caller has in mind leaves the object alive in the
/// file --- reachable, written out, and still carrying whatever it carries.
///
/// The trailer is walked in its own right because it is not in `objects`.
///
/// # Errors
///
/// An object nesting deeper than [`sweep::MAX_NESTING`], for [`drop_pages`]'s
/// reason: a pass that stopped early leaves a reference to an object that is
/// gone.
pub fn forget(doc: &mut Document, doomed: &HashSet<ObjectId>) -> Result<(), String> {
    forget_in_dictionary(&mut doc.trailer, doomed, 0)?;
    for object in doc.objects.values_mut() {
        forget_in_object(object, doomed, 0)?;
    }
    for id in doomed {
        doc.objects.remove(id);
    }
    Ok(())
}

/// Rewrites the page tree so the document's pages are `order`, in that order.
///
/// `order` is object ids, which is what makes this expressible at all: page
/// *numbers* are positions in the tree being replaced, so a plan spelled in them
/// would name its own output.
///
/// **The tree comes out one level deep**, every page hanging directly off the
/// catalog's `/Pages` node. A permutation cannot be done by shuffling entries
/// between the nodes they already hang under --- what a page inherits belongs to
/// the *slot*, not to the page, so a page moving from a node that states
/// `/MediaBox [0 0 595 842]` to one that does not would silently change size.
/// Flattening removes the question, and the four attributes a page may inherit
/// are written onto it first, so that it takes its own size, box, resources and
/// rotation with it.
///
/// The intermediate `/Pages` nodes are left unreachable rather than removed, and
/// the caller collects. `print::build` and `save::rewrite` both run
/// `sweep::collect` after this --- the save path since 2026-08-26, which is when
/// measuring found that a plan dropping pages left every dropped page's content
/// stream in the file. This function does not sweep itself because a mark-and-
/// sweep is a statement about the whole document and a reorder is a statement
/// about the page tree; the caller is the one that knows whether anything else
/// it did also orphaned something.
///
/// **Call it only when the order really differs.** A plan in document order that
/// went through here would flatten a nested tree, write inherited attributes onto
/// pages nobody touched, and produce a copy that differs from its source
/// everywhere. Both callers check first.
///
/// # Errors
///
/// The document has no catalog or no page tree, or a page id that is not a
/// dictionary.
pub fn reorder_pages(doc: &mut Document, order: &[ObjectId]) -> Result<(), String> {
    let root = doc
        .catalog()
        .map_err(|e| said!("no document catalog: {e}"))?
        .get(b"Pages")
        .and_then(Object::as_reference)
        .map_err(|e| said!("no page tree: {e}"))?;

    // What each page would inherit *after* the move: the root is about to be its
    // only ancestor. Read before anything is written, so that a page pushed down
    // to below cannot change the answer for the page after it.
    let from_root: Vec<Option<Object>> = INHERITABLE
        .iter()
        .map(|key| inherited(doc, root, key))
        .collect();

    let mut pushes: Vec<(ObjectId, &[u8], Object)> = Vec::new();
    for &page in order {
        for (at, key) in INHERITABLE.iter().enumerate() {
            let now = match moving_attribute(doc, page, key)? {
                Some((owner, _)) if owner == page => continue,
                Some((_, value)) => Some(value.clone()),
                None => None,
            };
            // Equal means the move changes nothing, so writing it would only
            // make an untouched page differ from its source. A page gaining
            // a root value had a disconnected parent chain.
            match now {
                Some(value) if Some(&value) != from_root[at].as_ref() => {
                    pushes.push((page, key, value));
                }
                None if from_root[at].is_some() => {
                    return Err(said!(
                        "The page would inherit a different value after moving"
                    ));
                }
                _ => {}
            }
        }
    }
    for (page, key, value) in pushes {
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .map_err(|e| said!("page {page:?} is not a dictionary: {e}"))?
            .set(key.to_vec(), value);
    }

    let kids: Vec<Object> = order.iter().map(|&id| Object::Reference(id)).collect();
    let tree = doc
        .get_object_mut(root)
        .and_then(Object::as_dict_mut)
        .map_err(|e| said!("the page tree root is not a dictionary: {e}"))?;
    tree.set("Kids", kids);
    // Entries in the tree, not distinct objects: a page reached twice is two
    // pages to every reader, which is the same unit `drop_pages` decrements in.
    tree.set("Count", order.len() as i64);

    for &page in order {
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .map_err(|e| said!("page {page:?} is not a dictionary: {e}"))?
            .set("Parent", Object::Reference(root));
    }
    Ok(())
}

/// A page dictionary that no longer needs the tree it hangs under.
///
/// The four attributes it may be inheriting are written onto it, and `/Parent`
/// is removed. What comes back is a dictionary, not an edit: nothing in `doc`
/// changes, because the caller for this is copying the page into a *different*
/// document and the source is read-only there.
///
/// **Removing `/Parent` is the half that is not cosmetic.** A walk that collects
/// what a page needs by following its references reaches the tree above it, then
/// the catalog, then every other page, the outline and the form fields --- so a
/// page that has been orphaned is the only kind whose reachable set is the page.
/// [`reorder_pages`] does not need that because it moves a page within the
/// document that already holds it; an import does, and it is the difference
/// between copying a page and copying a file.
///
/// **The four attributes are written unconditionally where an ancestor supplies
/// them**, which is where this differs from [`reorder_pages`]'s otherwise
/// identical loop. That one compares against what the destination root would
/// supply and leaves the key off when they agree, so an untouched page comes out
/// byte-identical to its source. Here the destination is another document
/// entirely: agreeing with its root would be a coincidence between two unrelated
/// trees, and one that stops holding the moment either is edited. The page takes
/// its size, box, resources and rotation with it.
///
/// Missing rotation, crop and resources take their PDF defaults here, before
/// the destination can supply a different value.
///
/// # Errors
///
/// The page dictionary or an inherited attribute cannot be read, or the page
/// has no usable media box.
pub fn detached_page(doc: &Document, page: ObjectId) -> Result<Dictionary, String> {
    let mut dictionary = doc
        .get_object(page)
        .and_then(Object::as_dict)
        .map_err(|e| said!("page {page:?} is not a dictionary: {e}"))?
        .clone();
    for key in INHERITABLE {
        match moving_attribute(doc, page, key)? {
            Some((owner, _)) if owner == page => {}
            Some((_, value)) => dictionary.set(key.to_vec(), value.clone()),
            None => {
                dictionary.remove(key);
            }
        }
    }
    page_defaults(&mut dictionary);
    dictionary.remove(b"Parent");
    Ok(dictionary)
}

/// Materialize the defaults a new parent could override.
pub(crate) fn page_defaults(page: &mut Dictionary) {
    if !page.has(b"Rotate") {
        page.set("Rotate", 0);
    }
    if !page.has(b"CropBox") {
        if let Ok(media) = page.get(b"MediaBox") {
            page.set("CropBox", media.clone());
        }
    }
    if !page.has(b"Resources") {
        page.set("Resources", Dictionary::new());
    }
}

/// Drops the outline of a document, whole.
///
/// What a page deletion did to every outline until 2026-10-07, and what it
/// still does to one `outline_repair` cannot vouch for: an outline that is not
/// a tree, one that names a page by its number, one whose names cannot be
/// looked up to the end. A table of contents that points at nothing, or at the
/// wrong page, is worse than none --- the same reason a bounded outline walk
/// reports what it cut rather than presenting a partial tree as whole --- and
/// this is the one answer that cannot write a wrong one.
///
/// It is also what is left when every entry has gone: a root with nothing
/// under it draws an empty panel.
///
/// # Errors
///
/// The document has no catalog, which means it is not a document.
pub fn drop_outline(doc: &mut Document) -> Result<(), String> {
    doc.catalog_mut()
        .map_err(|e| said!("no document catalog: {e}"))?
        .remove(b"Outlines");
    Ok(())
}

/// The `/Parent` of an object, if it names one.
fn parent_of(doc: &Document, id: ObjectId) -> Option<ObjectId> {
    doc.get_object(id)
        .and_then(Object::as_dict)
        .and_then(|dict| dict.get(b"Parent"))
        .and_then(Object::as_reference)
        .ok()
}

/// Drops references to any doomed object, recursively.
///
/// Depth-bounded by [`sweep::MAX_NESTING`], for the reason recorded there: this
/// runs on a document we did not write, in the app process, and the recursion is
/// otherwise unbounded.
fn forget_in_object(
    object: &mut Object,
    doomed: &HashSet<ObjectId>,
    depth: usize,
) -> Result<(), String> {
    if depth > sweep::MAX_NESTING {
        return Err(said!(
            "an object nests deeper than {limit} levels",
            limit = sweep::MAX_NESTING
        ));
    }
    match object {
        Object::Array(items) => {
            items.retain(|item| !matches!(item, Object::Reference(id) if doomed.contains(id)));
            for item in items.iter_mut() {
                forget_in_object(item, doomed, depth + 1)?;
            }
        }
        Object::Dictionary(dictionary) => forget_in_dictionary(dictionary, doomed, depth + 1)?,
        Object::Stream(stream) => forget_in_dictionary(&mut stream.dict, doomed, depth + 1)?,
        _ => {}
    }
    Ok(())
}

/// Drops keys whose value names a doomed object, then recurses.
fn forget_in_dictionary(
    dictionary: &mut Dictionary,
    doomed: &HashSet<ObjectId>,
    depth: usize,
) -> Result<(), String> {
    if depth > sweep::MAX_NESTING {
        return Err(said!(
            "an object nests deeper than {limit} levels",
            limit = sweep::MAX_NESTING
        ));
    }
    let dead: Vec<Vec<u8>> = dictionary
        .iter()
        .filter(|(_, value)| matches!(value, Object::Reference(id) if doomed.contains(id)))
        .map(|(key, _)| key.clone())
        .collect();
    for key in dead {
        dictionary.remove(&key);
    }
    for (_, value) in dictionary.iter_mut() {
        forget_in_object(value, doomed, depth + 1)?;
    }
    Ok(())
}

/// Applies a plan's **page selection** to a document: what goes, and in what order.
///
/// **The two steps every writer here performs before it does anything of its
/// own, in the one order that is correct.** `save::rewrite` and `print::build`
/// both did this inline, spelled slightly differently, and `docs/TRAPS.md`
/// records two shipped defects from that split --- *"Two writers for one
/// document, and the printer got the older one"* and *"The same silent
/// decryption, on the path whose output a reader keeps"*. The primitives were
/// already shared; the *sequence* was not, so the next rule added to one of them
/// had nothing forcing it into the other.
///
/// Three orderings are load-bearing rather than tidy:
///
///   - **Drop before reorder.** `reorder_pages` flattens the tree, and doing
///     that to pages about to be deleted rewrites ancestry for nothing.
///   - **Check destinations before positions change.** Object destinations
///     follow their pages. Numeric destinations depend on the old order.
///   - **The outline is read before the pages are dropped.** `drop_pages`
///     takes a deleted page out of every destination array, after which a
///     bookmark to it cannot be told from a damaged one.
///
/// `reorder_to` is `None` when the reader's order is the file's, and the caller
/// decides that: `save` carries it as a flag from the model, `print` compares
/// the numbers it resolved. Those are genuinely different questions, so the
/// predicate stays at the call site and only the action is shared.
///
/// `source_pages` gives the zero-based original position of each output page
/// (None for an inserted page). Labels belong to positions, even when two
/// positions share a page object.
///
/// **Turning pages is deliberately not here.** `print` turns immediately after
/// this; `save` cannot, because a mark was made against the rotation the file
/// had when it was opened and the mapping that places it reads the rotation the
/// file has *now* --- so `save` maps its marks first and turns afterwards. A
/// third step in this function would force the order that defect comes from.
///
/// # Errors
///
/// Whatever [`drop_pages`], `outline_repair::keep_for` and [`reorder_pages`]
/// refuse.
pub fn materialise(
    doc: &mut Document,
    dropped: &[u32],
    reorder_to: Option<&[ObjectId]>,
    source_pages: impl IntoIterator<Item = Option<u32>>,
) -> Result<(), String> {
    if !dropped.is_empty() || reorder_to.is_some() {
        labels::remap(doc, source_pages)?;
    }
    let mut surviving_slots = None;
    if !dropped.is_empty()
        || reorder_to.is_some_and(|order| !order.starts_with(&ordered_pages(doc)))
    {
        // Before the pages go: `drop_pages` takes the page out of every
        // destination array, and the repair reads those arrays.
        let table = doc.get_pages();
        let doomed = doomed_pages(&table, dropped);
        if reorder_to.is_none()
            && dropped
                .iter()
                .filter_map(|n| table.get(n))
                .any(|id| !doomed.contains(id))
        {
            let removed: HashSet<_> = dropped.iter().collect();
            surviving_slots = Some(
                table
                    .iter()
                    .filter(|(n, _)| !removed.contains(n))
                    .map(|(_, id)| *id)
                    .collect::<Vec<_>>(),
            );
        }
        crate::links::bind_numeric_links(doc, &doomed)?;
        crate::outline_repair::keep_for(doc, &doomed)?;
        drop_pages(doc, dropped)?;
    }
    if let Some(order) = reorder_to.or(surviving_slots.as_deref()) {
        reorder_pages(doc, order)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    /// A three-page document with an outline, for [`materialise`].
    ///
    /// The outline is the discriminating part: a deletion must take one
    /// bookmark and a move none, and a fixture without one cannot tell those
    /// apart.
    fn three_pages_with_outline() -> (Document, Vec<ObjectId>) {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let kids: Vec<ObjectId> = (0..3)
            .map(|_| doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id }))
            .collect();
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids.iter().map(|id| (*id).into()).collect::<Vec<Object>>(),
                "Count" => 3,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        // One bookmark per page, chained, so a deletion has one to remove and
        // two to keep.
        let outline = doc.new_object_id();
        let items: Vec<ObjectId> = kids.iter().map(|_| doc.new_object_id()).collect();
        for (at, page) in kids.iter().enumerate() {
            let mut item = dictionary! {
                "Title" => Object::string_literal(format!("page {}", at + 1)),
                "Parent" => outline,
                "Dest" => vec![Object::Reference(*page), "Fit".into()],
            };
            if at > 0 {
                item.set("Prev", items[at - 1]);
            }
            if let Some(next) = items.get(at + 1) {
                item.set("Next", *next);
            }
            doc.objects.insert(items[at], Object::Dictionary(item));
        }
        doc.objects.insert(
            outline,
            Object::Dictionary(dictionary! {
                "Type" => "Outlines",
                "First" => items[0],
                "Last" => items[2],
                "Count" => 3,
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "Outlines" => outline,
        });
        doc.trailer.set("Root", catalog);
        (doc, kids)
    }

    fn has_outline(doc: &Document) -> bool {
        doc.catalog().expect("a catalog").get(b"Outlines").is_ok()
    }

    /// The bookmarks' titles in reading order, each checked to lead to a page
    /// that is in the document.
    fn bookmarks(doc: &Document) -> Vec<String> {
        let pages: HashSet<ObjectId> = doc.get_pages().into_values().collect();
        let root = doc
            .catalog()
            .expect("a catalog")
            .get(b"Outlines")
            .and_then(Object::as_reference)
            .expect("an outline");
        let mut titles = Vec::new();
        let mut at = doc
            .get_dictionary(root)
            .expect("the outline root")
            .get(b"First")
            .and_then(Object::as_reference)
            .ok();
        while let Some(id) = at {
            let item = doc.get_dictionary(id).expect("a bookmark");
            let dest = item
                .get(b"Dest")
                .and_then(Object::as_array)
                .expect("a destination");
            let page = dest
                .first()
                .and_then(|first| first.as_reference().ok())
                .expect("a destination begins with its page");
            assert!(
                pages.contains(&page),
                "a bookmark leads to a page that is in the file"
            );
            titles.push(
                String::from_utf8_lossy(
                    item.get(b"Title")
                        .and_then(Object::as_str)
                        .expect("a title"),
                )
                .into_owned(),
            );
            at = item.get(b"Next").and_then(Object::as_reference).ok();
        }
        titles
    }

    #[test]
    fn a_displayed_rectangle_lands_in_the_page_where_the_text_mapping_puts_it() {
        // 200 by 300 as displayed, from a box whose corner is not the origin.
        let rect = [10.0, 20.0, 110.0, 45.0];
        for turns in 0..4_u8 {
            let (width, height) = if turns % 2 == 1 {
                (200.0, 300.0)
            } else {
                (300.0, 200.0)
            };
            let shown = DisplayedPage {
                width,
                height,
                turns,
                origin: (7.0, 11.0),
            };
            let page = from_displayed(&shown, rect);
            assert!(page[0] < page[2] && page[1] < page[3], "{turns}: {page:?}");
            // The table the text layer maps with, which is another one.
            let other = crate::text::from_device(turns, width, height, rect.map(|v| v as f32));
            assert_eq!(
                page,
                [
                    other[0] + 7.0,
                    other[1] + 11.0,
                    other[2] + 7.0,
                    other[3] + 11.0
                ],
                "{turns}"
            );
            // And back, the way the form's scan reads a widget.
            let back = crate::text::to_device(
                turns,
                width,
                height,
                [page[0] - 7.0, page[1] - 11.0, page[2] - 7.0, page[3] - 11.0],
            );
            assert_eq!(back.map(f64::from), rect, "{turns}");
        }
    }

    /// A deletion takes the bookmark of the page that went, and no other.
    ///
    /// The order inside [`materialise`] is what this holds: the outline is read
    /// before the pages are dropped. The other way round, `drop_pages` has
    /// already taken the page out of the destination, the repair cannot tell
    /// where the bookmark led, and "page 2" stays --- leading nowhere.
    #[test]
    fn materialising_a_deletion_keeps_the_bookmarks_of_the_pages_that_stay() {
        let (mut doc, _) = three_pages_with_outline();
        assert_eq!(bookmarks(&doc), ["page 1", "page 2", "page 3"]);
        materialise(&mut doc, &[2], None, [Some(0), Some(2)]).expect("materialise");
        assert_eq!(doc.get_pages().len(), 2, "the page went");
        assert_eq!(bookmarks(&doc), ["page 1", "page 3"]);
    }

    /// When every bookmark led to a deleted page, the outline goes.
    #[test]
    fn materialising_a_deletion_of_every_bookmarked_page_drops_the_outline() {
        let (mut doc, _) = three_pages_with_outline();
        let Ok(Object::Dictionary(root)) = doc
            .catalog()
            .expect("a catalog")
            .get(b"Outlines")
            .and_then(Object::as_reference)
            .map_err(|e| e.to_string())
            .and_then(|id| doc.get_object(id).cloned().map_err(|e| e.to_string()))
        else {
            unreachable!("the fixture has an outline");
        };
        let first = root
            .get(b"First")
            .and_then(Object::as_reference)
            .expect("a bookmark");
        // Leave only the bookmark of page 1.
        let outline = doc
            .catalog()
            .expect("a catalog")
            .get(b"Outlines")
            .and_then(Object::as_reference)
            .expect("an outline");
        if let Ok(Object::Dictionary(item)) = doc.get_object_mut(first) {
            item.remove(b"Next");
        }
        if let Ok(Object::Dictionary(root)) = doc.get_object_mut(outline) {
            root.set("Last", first);
            root.set("Count", 1);
        }
        assert_eq!(bookmarks(&doc), ["page 1"]);
        materialise(&mut doc, &[1], None, [Some(1), Some(2)]).expect("materialise");
        assert!(!has_outline(&doc), "nothing was left to list");
    }

    /// A move keeps it, and this is the assertion that makes the one above mean
    /// something.
    ///
    /// A destination names a page *object*, and a move does not remove any --- so
    /// a bookmark follows its page to wherever the reader put it. Without this
    /// test, thinning the outline on every save passes.
    #[test]
    fn materialising_a_move_keeps_the_outline() {
        let (mut doc, kids) = three_pages_with_outline();
        let order = vec![kids[2], kids[0], kids[1]];
        materialise(&mut doc, &[], Some(&order), [Some(2), Some(0), Some(1)]).expect("materialise");
        assert_eq!(doc.get_pages().len(), 3, "nothing was dropped");
        assert_eq!(
            bookmarks(&doc),
            ["page 1", "page 2", "page 3"],
            "a move removes no page object, so every destination still names one"
        );
    }

    #[test]
    fn numeric_local_links_keep_their_page_identity() {
        for remove in [false, true] {
            let (mut doc, kids) = three_pages_with_outline();
            let action = doc.add_object(
                dictionary! { "S"=>"GoTo", "D"=>vec![Object::Integer(2), "Fit".into()] },
            );
            let link = doc.add_object(dictionary! { "Subtype"=>"Link", "A"=>action });
            doc.get_dictionary_mut(kids[0])
                .unwrap()
                .set("Annots", vec![Object::Reference(link)]);
            if remove {
                materialise(&mut doc, &[3], None, [Some(0), Some(1)]).unwrap();
            } else {
                materialise(
                    &mut doc,
                    &[],
                    Some(&[kids[0], kids[2], kids[1]]),
                    [Some(0), Some(2), Some(1)],
                )
                .unwrap();
            }
            let mut saved = Vec::new();
            doc.save_to(&mut saved).unwrap();
            let reopened = Document::load_mem(&saved).unwrap();
            let annotation = reopened.get_dictionary(link).unwrap();
            if remove {
                assert!(!annotation.has(b"A") && !annotation.has(b"Dest"));
            } else {
                let action = annotation
                    .get_deref(b"A", &reopened)
                    .unwrap()
                    .as_dict()
                    .unwrap();
                assert_eq!(
                    action.get(b"D").unwrap().as_array().unwrap()[0]
                        .as_reference()
                        .unwrap(),
                    reopened.get_pages()[&2]
                );
            }
        }
    }

    #[test]
    fn a_move_cannot_leave_a_numeric_destination_pointing_at_another_page() {
        let (mut doc, kids) = three_pages_with_outline();
        let root = crate::redact::outline_root(&doc).unwrap();
        let first = crate::redact::first_child(&doc, root).unwrap();
        doc.get_dictionary_mut(first)
            .unwrap()
            .set("Dest", vec![Object::Integer(0), "Fit".into()]);
        materialise(
            &mut doc,
            &[],
            Some(&[kids[1], kids[0], kids[2]]),
            [Some(1), Some(0), Some(2)],
        )
        .unwrap();
        assert!(!has_outline(&doc));
    }

    /// A three-page tree whose root hangs under a node that hangs under it.
    ///
    /// Four lines of a file nobody sane writes, and nothing refuses it: `/Kids`
    /// is a tree and `/Parent` is a separate back-pointer, so a loop in the
    /// second is invisible to every walk of the first --- `get_pages` returns
    /// three pages from this document exactly as it does from a healthy one.
    fn three_pages_under_a_parent_loop(root_count: i64) -> (Document, ObjectId, ObjectId) {
        let mut doc = Document::with_version("1.7");
        let root_id = doc.new_object_id();
        let above_id = doc.new_object_id();
        let kids: Vec<ObjectId> = (0..3)
            .map(|_| doc.add_object(dictionary! { "Type" => "Page", "Parent" => root_id }))
            .collect();
        doc.objects.insert(
            root_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => above_id,
                "Kids" => kids.iter().map(|id| (*id).into()).collect::<Vec<Object>>(),
                "Count" => root_count,
            }),
        );
        doc.objects.insert(
            above_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => root_id,
                "Kids" => vec![Object::Reference(root_id)],
                "Count" => 3,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root_id });
        doc.trailer.set("Root", catalog);
        (doc, root_id, above_id)
    }

    fn count_of(doc: &Document, node: ObjectId) -> i64 {
        doc.get_object(node)
            .and_then(Object::as_dict)
            .expect("a node")
            .get(b"Count")
            .and_then(Object::as_i64)
            .expect("a count")
    }

    /// One deleted page is one decrement per ancestor, cycle or no cycle.
    ///
    /// The `/Parent` walk was bounded and had no visited set, so a loop handed
    /// it the same two nodes until the bound ran out --- thirty-two decrements
    /// each for one deleted page. The bound stopped the walk and did nothing at
    /// all about the damage, which is the shape worth remembering: a loop guard
    /// written as a trip count is not a loop guard on what the trips do.
    #[test]
    fn a_cyclic_parent_chain_decrements_each_ancestor_once() {
        let (mut doc, root, above) = three_pages_under_a_parent_loop(3);
        assert_eq!(doc.get_pages().len(), 3, "the fixture reads as three pages");

        drop_pages(&mut doc, &[2]).expect("the deletion itself is unremarkable");

        assert_eq!(doc.get_pages().len(), 2, "one page went");
        assert_eq!(
            count_of(&doc, root),
            2,
            "and the root lost exactly one from its count"
        );
        assert_eq!(
            count_of(&doc, above),
            2,
            "as did the node above it, once rather than once per lap"
        );
    }

    /// A count that cannot be decremented saturates rather than wrapping.
    ///
    /// `/Count -9223372036854775808` is as writable as any other number, and
    /// `count - 1` on it is a panic under debug assertions --- so a document
    /// crafted this way took down whichever process was doing the deleting ---
    /// and in the shipped build wraps to the largest count there is, which is a
    /// document reporting 9.2 quintillion pages after one was removed.
    #[test]
    fn a_count_at_the_bottom_of_its_range_saturates_rather_than_wrapping() {
        let (mut doc, root, _) = three_pages_under_a_parent_loop(i64::MIN);

        drop_pages(&mut doc, &[2]).expect("the deletion itself is unremarkable");

        assert_eq!(
            count_of(&doc, root),
            i64::MIN,
            "there is nowhere below this to go, and the answer must not be positive"
        );
    }

    /// Neither step runs when there is nothing to do.
    ///
    /// The emptiness control. `reorder_pages` **flattens** the page tree, so
    /// calling it for a job that changed nothing rewrites the ancestry of every
    /// page for no reason --- and `docs/TRAPS.md` records what that costs: a page
    /// loses whatever it inherited from the node it hung under.
    ///
    /// **The tree has to be nested or this cannot fail.** Flattening a tree that
    /// is already flat is unobservable, and a `materialise` that reordered
    /// unconditionally passed this test while the fixture was one `/Pages` node
    /// with three kids. The intermediate node is the discriminating property.
    #[test]
    fn materialising_nothing_leaves_the_tree_alone() {
        let mut doc = Document::with_version("1.7");
        let root_id = doc.new_object_id();
        let branch_id = doc.new_object_id();
        let under_branch: Vec<ObjectId> = (0..2)
            .map(|_| doc.add_object(dictionary! { "Type" => "Page", "Parent" => branch_id }))
            .collect();
        // The inheritance the flattening would have to carry, on the node that
        // would disappear.
        doc.objects.insert(
            branch_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => root_id,
                "Kids" => under_branch.iter().map(|id| (*id).into()).collect::<Vec<Object>>(),
                "Count" => 2,
                "Rotate" => 90,
            }),
        );
        let loose = doc.add_object(dictionary! { "Type" => "Page", "Parent" => root_id });
        doc.objects.insert(
            root_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![branch_id.into(), loose.into()],
                "Count" => 3,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root_id });
        doc.trailer.set("Root", catalog);
        let order: Vec<ObjectId> = doc.get_pages().into_values().collect();

        materialise(&mut doc, &[], None, [Some(0), Some(1), Some(2)]).expect("materialise");

        assert_eq!(doc.get_pages().len(), 3, "every page is still there");
        assert_eq!(
            doc.get_pages().into_values().collect::<Vec<_>>(),
            order,
            "and in the same order"
        );
        let kids = doc
            .get_object(root_id)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Kids"))
            .and_then(Object::as_array)
            .expect("the root's kids");
        assert_eq!(
            kids.len(),
            2,
            "the tree must not have been flattened: the root still has a branch and a page, \
             not three pages"
        );
        assert!(
            doc.get_object(branch_id).is_ok(),
            "the intermediate node, and the /Rotate every page under it inherits, survive"
        );
    }

    use super::*;
    use lopdf::dictionary;

    /// A document whose page tree is `/Kids [a b]` under one `/Pages` node.
    ///
    /// Built by hand rather than taken from the corpus: every fixture states its
    /// own `/Rotate`, and the inheritance these functions are about is only
    /// visible on a document that does not.
    fn two_pages(parent_rotate: Option<i64>) -> (Document, ObjectId, ObjectId) {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let a = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
        let b = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
        let mut tree = dictionary! {
            "Type" => "Pages",
            "Kids" => vec![a.into(), b.into()],
            "Count" => 2,
        };
        if let Some(rotate) = parent_rotate {
            tree.set("Rotate", rotate);
        }
        doc.objects.insert(pages_id, Object::Dictionary(tree));
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        (doc, a, b)
    }

    /// {@link two_pages}, with a US Letter sheet on the shared parent.
    ///
    /// The media box is on the *parent* deliberately: `apply_crops` reads it
    /// through `box_on`, which walks the inheritance, and a fixture stating it on
    /// the page would pass for an implementation that did not walk at all.
    fn two_pages_with_media(rotate: Option<i64>) -> (Document, ObjectId, ObjectId) {
        let (mut doc, a, b) = two_pages(rotate);
        let parent = doc
            .get_object(a)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Parent"))
            .and_then(Object::as_reference)
            .expect("a parent");
        doc.get_object_mut(parent)
            .and_then(Object::as_dict_mut)
            .expect("the page tree")
            .set("MediaBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
        (doc, a, b)
    }

    /// The box comes back in the page's own space at every quarter turn.
    ///
    /// `width` and `height` are what a reader sees and are transposed on an odd
    /// turn; a `/CropBox` is not, and neither is any PDFium box API. So the one
    /// thing this method has to do is undo that, and the check is that the
    /// rectangle is the **same** at all four turns while the displayed
    /// dimensions swap.
    ///
    /// A page that was square could not tell the two apart, which is why the
    /// fixture is 612 by 792.
    #[test]
    fn the_pages_own_box_is_the_same_rectangle_at_every_turn() {
        let sheet = [0.0, 0.0, 612.0, 792.0];
        for (rotate, displayed) in [
            (0, (612.0, 792.0)),
            (90, (792.0, 612.0)),
            (180, (612.0, 792.0)),
            (270, (792.0, 612.0)),
        ] {
            let (doc, page, _) = two_pages_with_media(Some(rotate));
            let shown = displayed_page(&doc, page);
            assert_eq!(
                (shown.width, shown.height),
                displayed,
                "the displayed size at {rotate} degrees"
            );
            assert_eq!(
                shown.box_pt(),
                sheet,
                "the page's own box at {rotate} degrees"
            );
        }
    }

    /// A document the two parsers count differently is refused, not returned short.
    ///
    /// The boxes come back positionally and the caller indexes them by PDFium's
    /// page number, so a `lopdf` walk that saw a different set of pages puts
    /// page 5's box on page 4. That is worse than putting none there: the
    /// failure is a plausible page at a plausible size, on a document where
    /// nothing else looks wrong, and the reader has no way to notice.
    #[test]
    fn a_page_count_the_two_parsers_disagree_about_is_refused() {
        let (doc, _, _) = two_pages_with_media(Some(90));
        let mut bytes = Vec::new();
        let mut doc = doc;
        doc.save_to(&mut bytes).expect("serialise");

        let refused = displayed_boxes(&bytes, 3, None).expect_err("three against two");
        assert!(
            refused.contains('2') && refused.contains('3'),
            "the refusal must name both counts, not merely decline: {refused}"
        );
        // The control: the same bytes with the count the file actually has come
        // back, and with the box the pages inherit rather than a default sheet.
        let boxes = displayed_boxes(&bytes, 2, None).expect("two against two");
        assert_eq!(boxes, vec![[0.0, 0.0, 612.0, 792.0]; 2]);
    }

    /// A crop is written on the page, never on the `/Pages` node it inherits
    /// from.
    ///
    /// `/CropBox` is inheritable, so a crop set on an ancestor crops every page
    /// under it --- which for a document whose pages share one node is the whole
    /// file, from a reader who cropped one page.
    #[test]
    fn a_crop_lands_on_the_page_and_not_on_its_parent() {
        let (mut doc, first, second) = two_pages_with_media(None);
        apply_crops(&mut doc, &[(first, [10.0, 20.0, 300.0, 400.0])]).expect("crop");

        assert_eq!(
            box_on(&doc, first, b"CropBox"),
            Some([10.0, 20.0, 300.0, 400.0])
        );
        // The control: the other page is untouched. Without it, a write onto the
        // shared parent would satisfy the assertion above exactly.
        assert_eq!(box_on(&doc, second, b"CropBox"), None);
    }

    /// A crop larger than the sheet is clamped to it, per §14.11.2.
    #[test]
    fn a_crop_outside_the_page_is_brought_back_onto_it() {
        let (mut doc, page, _) = two_pages_with_media(None);
        apply_crops(&mut doc, &[(page, [-50.0, -50.0, 10_000.0, 10_000.0])]).expect("crop");
        assert_eq!(
            box_on(&doc, page, b"CropBox"),
            box_on(&doc, page, b"MediaBox")
        );
    }

    /// A crop sharing no area with the sheet is refused rather than written.
    ///
    /// The model already refused a degenerate rectangle; this is the different
    /// question only this layer can ask, because only this layer knows the sheet.
    #[test]
    fn a_crop_that_misses_the_page_is_refused() {
        let (mut doc, page, _) = two_pages_with_media(None);
        let why = apply_crops(&mut doc, &[(page, [5000.0, 5000.0, 6000.0, 6000.0])])
            .expect_err("a crop off the sheet");
        assert!(why.contains("outside the page"), "{why}");
        // Refused *before* writing, so the page is as it was.
        assert_eq!(box_on(&doc, page, b"CropBox"), None);
    }

    #[test]
    fn a_rotation_is_read_off_the_parent_when_the_page_does_not_state_one() {
        let (doc, a, _) = two_pages(Some(90));
        assert_eq!(effective_rotation(&doc, a), 90);
        let (doc, a, _) = two_pages(None);
        assert_eq!(
            effective_rotation(&doc, a),
            0,
            "no ancestor carries one, so the page really is upright"
        );
    }

    #[test]
    fn pages_come_back_in_page_number_order() {
        let (doc, a, b) = two_pages(None);
        assert_eq!(ordered_pages(&doc), vec![a, b]);
    }

    #[test]
    fn turns_that_agree_are_applied_once_and_turns_that_differ_are_refused() {
        let (_, a, b) = two_pages(None);
        assert_eq!(
            agreed_turns(&[(a, 1), (b, 2)]).expect("distinct pages"),
            vec![(a, 1), (b, 2)]
        );
        assert_eq!(
            agreed_turns(&[(a, 1), (a, 1)]).expect("one page, one turn"),
            vec![(a, 1)],
            "a shared page asked for the same turn twice turns once"
        );
        let why = agreed_turns(&[(a, 1), (b, 0), (a, 2)]).expect_err("conflicting turns");
        assert!(
            why.contains("pages 1 and 3"),
            "the message names the pages the reader sees: {why}"
        );
    }

    /// Four pages under two intermediate nodes, only one of which states a size.
    ///
    /// Nothing in the corpus is shaped this way. `text-heavy.pdf` is three levels
    /// deep --- the only nested fixture there is --- and its `/MediaBox` sits on
    /// the *root*, so flattening it onto the root preserves everything and it
    /// cannot tell a reorder that carries inherited attributes from one that
    /// drops them. Whatever a fixture is meant to discriminate, it needs two of:
    /// here that is two tree nodes that disagree.
    ///
    /// Returns the document, the root, and the four pages in tree order.
    fn nested_pages() -> (Document, ObjectId, [ObjectId; 4]) {
        let mut doc = Document::with_version("1.7");
        let root_id = doc.new_object_id();
        let left_id = doc.new_object_id();
        let right_id = doc.new_object_id();

        let page = |doc: &mut Document, parent: ObjectId| {
            doc.add_object(dictionary! { "Type" => "Page", "Parent" => parent })
        };
        let a = page(&mut doc, left_id);
        let b = page(&mut doc, left_id);
        let c = page(&mut doc, right_id);
        let d = page(&mut doc, right_id);

        doc.objects.insert(
            left_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => root_id,
                "Kids" => vec![a.into(), b.into()],
                "Count" => 2,
            }),
        );
        doc.objects.insert(
            right_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Parent" => root_id,
                "Kids" => vec![c.into(), d.into()],
                "Count" => 2,
                // The two attributes this node has and the root does not. A page
                // under it that moves to the root loses both unless they travel.
                "MediaBox" => vec![0.into(), 0.into(), 200.into(), 400.into()],
                "Rotate" => 90,
            }),
        );
        doc.objects.insert(
            root_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![left_id.into(), right_id.into()],
                "Count" => 4,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root_id });
        doc.trailer.set("Root", catalog);
        (doc, root_id, [a, b, c, d])
    }

    fn media_box(doc: &Document, page: ObjectId) -> Vec<i64> {
        inherited(doc, page, b"MediaBox")
            .and_then(|value| value.as_array().ok().cloned())
            .expect("a page has a media box")
            .iter()
            .map(|n| n.as_i64().expect("an integer"))
            .collect()
    }

    #[test]
    fn the_nested_fixture_really_does_hang_two_sizes_off_two_nodes() {
        // The precondition every check below rests on. Without it a reorder that
        // silently flattened everything to one size would pass them all.
        let (doc, _, [a, _, _, d]) = nested_pages();
        assert_eq!(
            media_box(&doc, a),
            vec![0, 0, 612, 792],
            "inherited from root"
        );
        assert_eq!(
            media_box(&doc, d),
            vec![0, 0, 200, 400],
            "from its own node"
        );
        assert_eq!(effective_rotation(&doc, a), 0);
        assert_eq!(effective_rotation(&doc, d), 90);
    }

    #[test]
    fn a_reordered_page_takes_what_it_inherited_with_it() {
        let (mut doc, _, [a, b, c, d]) = nested_pages();
        reorder_pages(&mut doc, &[d, a, c, b]).expect("reorder");

        assert_eq!(ordered_pages(&doc), vec![d, a, c, b], "the new order");
        assert_eq!(
            media_box(&doc, d),
            vec![0, 0, 200, 400],
            "the page that hung under the node stating a size keeps that size, \
             which is the whole of why this flattens rather than shuffles"
        );
        assert_eq!(effective_rotation(&doc, d), 90, "and its rotation");
        assert_eq!(media_box(&doc, c), vec![0, 0, 200, 400]);
        assert_eq!(
            media_box(&doc, a),
            vec![0, 0, 612, 792],
            "and a page that inherited from the root still does"
        );
        assert_eq!(effective_rotation(&doc, a), 0);
    }

    #[test]
    fn reordering_preserves_resources_behind_local_nulls() {
        let (mut doc, _, [a, b, c, d]) = nested_pages();
        let parent = doc
            .get_dictionary(c)
            .unwrap()
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        let resources =
            doc.add_object(dictionary! { "Font" => dictionary! { "F9" => dictionary! {} } });
        doc.get_dictionary_mut(parent)
            .unwrap()
            .set("Resources", resources);
        let null = doc.add_object(Object::Null);
        doc.get_dictionary_mut(c).unwrap().set("Resources", null);
        doc.get_dictionary_mut(d)
            .unwrap()
            .set("Resources", Object::Null);
        reorder_pages(&mut doc, &[d, c, b, a]).unwrap();
        for page in [c, d] {
            assert_eq!(
                doc.get_dictionary(page)
                    .unwrap()
                    .get(b"Resources")
                    .unwrap()
                    .as_reference()
                    .unwrap(),
                resources
            );
        }
    }

    #[test]
    fn missing_media_or_unreadable_resources_refuse_before_reparenting() {
        for key in [b"MediaBox".as_slice(), b"Resources"] {
            let (mut doc, root, [a, b, c, d]) = nested_pages();
            if key == b"MediaBox" {
                doc.get_dictionary_mut(root).unwrap().remove(key);
            } else {
                let missing = doc.new_object_id();
                doc.get_dictionary_mut(a).unwrap().set(key, missing);
            }
            let before = doc.objects.clone();
            assert!(detached_page(&doc, a).is_err());
            assert!(reorder_pages(&mut doc, &[d, c, b, a]).is_err());
            assert_eq!(doc.objects, before);
        }
    }

    /// The control for the check above: a page that loses nothing gains nothing.
    ///
    /// Pushing every inherited attribute onto every page would pass that check
    /// and would rewrite pages nobody moved --- and for `/Rotate` it is the
    /// flattening `apply_turns` deliberately avoids, since a page whose `/Parent`
    /// chain is longer than the bound reads back as upright.
    #[test]
    fn a_page_that_inherits_from_the_root_is_not_written_to() {
        let (mut doc, _, [a, b, c, d]) = nested_pages();
        reorder_pages(&mut doc, &[d, a, c, b]).expect("reorder");

        let states = |doc: &Document, id: ObjectId, key: &[u8]| {
            doc.get_object(id)
                .and_then(Object::as_dict)
                .expect("a page")
                .has(key)
        };
        assert!(
            !states(&doc, a, b"MediaBox"),
            "the root supplies it either way, so the page says nothing"
        );
        assert!(!states(&doc, a, b"Rotate"), "and it is still upright");
        assert!(
            states(&doc, d, b"MediaBox") && states(&doc, d, b"Rotate"),
            "the control: the page that WOULD have lost them does state both"
        );
    }

    #[test]
    fn the_flattened_tree_counts_what_it_holds_and_owns_every_page() {
        let (mut doc, root, [_, b, c, _]) = nested_pages();
        reorder_pages(&mut doc, &[c, b]).expect("reorder");

        let tree = doc
            .get_object(root)
            .and_then(Object::as_dict)
            .expect("the root");
        assert_eq!(
            tree.get(b"Count")
                .and_then(Object::as_i64)
                .expect("a count"),
            2,
            "the count is what the tree holds, not what the file used to"
        );
        assert_eq!(ordered_pages(&doc), vec![c, b]);
        for page in [c, b] {
            assert_eq!(
                doc.get_object(page)
                    .and_then(Object::as_dict)
                    .expect("a page")
                    .get(b"Parent")
                    .and_then(Object::as_reference)
                    .expect("a parent"),
                root,
                "every page hangs off the root now --- a page still pointing at \
                 the node it came from would report a stale ancestry to anything \
                 that walks up, this module included"
            );
        }
    }

    #[test]
    fn a_page_reached_twice_can_be_reordered_and_stays_reached_twice() {
        let (mut doc, _, [a, b, _, d]) = nested_pages();
        reorder_pages(&mut doc, &[d, a, d, b]).expect("reorder");
        assert_eq!(
            ordered_pages(&doc),
            vec![d, a, d, b],
            "one object under two page numbers is expressible in a `/Kids` array \
             and stays expressible after a reorder"
        );
    }

    #[test]
    fn a_turn_uses_the_same_starting_orientation_as_the_reader() {
        for height in [612, 792] {
            let (mut doc, a, _) = two_pages(Some(-45));
            doc.get_dictionary_mut(a).unwrap().set(
                "MediaBox",
                vec![0.into(), 0.into(), 612.into(), height.into()],
            );
            apply_turns(&mut doc, &[(a, 1)]).unwrap();
            assert_eq!(effective_rotation(&doc, a), 90);
            let shown = displayed_page(&doc, a);
            assert_eq!((shown.width, shown.height), (height as f32, 612.0));
        }
    }

    #[test]
    fn a_null_user_unit_uses_default_points() {
        let (mut doc, page, _) = two_pages(None);
        let indirect = doc.add_object(Object::Null);
        assert_eq!(user_unit(&doc, page), Some(1.0));
        for value in [Object::Null, Object::Reference(indirect)] {
            doc.get_dictionary_mut(page).unwrap().set("UserUnit", value);
            assert_eq!(user_unit(&doc, page), Some(1.0));
        }
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("UserUnit", "wrong");
        assert_eq!(user_unit(&doc, page), None);
    }

    #[test]
    fn null_page_labels_and_optional_rule_entries_are_omitted() {
        let (mut doc, _) = three_pages_with_outline();
        let indirect = doc.add_object(Object::Null);
        labels::remap(&mut doc, [Some(1)]).unwrap();
        for null in [Object::Null, Object::Reference(indirect)] {
            doc.catalog_mut().unwrap().set("PageLabels", null.clone());
            labels::remap(&mut doc, [Some(1)]).unwrap();
            let rule =
                dictionary! { "P" => null.clone(), "S" => null.clone(), "St" => null.clone() };
            doc.catalog_mut().unwrap().set(
                "PageLabels",
                dictionary! {
                    "Kids" => null, "Nums" => vec![0.into(), Object::Dictionary(rule)]
                },
            );
            labels::remap(&mut doc, [Some(1)]).unwrap();
            let nums = doc
                .catalog()
                .unwrap()
                .get(b"PageLabels")
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"Nums")
                .unwrap()
                .as_array()
                .unwrap();
            assert_eq!(nums.len(), 2);
            assert!(nums[1].as_dict().unwrap().is_empty());
        }
    }

    #[test]
    fn a_page_with_no_turn_is_not_written_to_at_all() {
        let (mut doc, a, b) = two_pages(Some(90));
        apply_turns(&mut doc, &[(a, 0), (b, 1)]).expect("apply");

        let stated = |doc: &Document, id: ObjectId| {
            doc.get_object(id)
                .and_then(Object::as_dict)
                .ok()
                .and_then(|d| d.get(b"Rotate").ok().cloned())
        };
        assert!(
            stated(&doc, a).is_none(),
            "the untouched page states no rotation of its own; it still inherits 90"
        );
        assert_eq!(
            stated(&doc, b).and_then(|o| o.as_i64().ok()),
            Some(180),
            "the turned page states the composed value: the inherited 90 plus a quarter"
        );
    }

    /// Every quarter turn, both wraps, and the values the specification forbids.
    ///
    /// The table is the point: 360 and 0 are the same page and -90 and 270 are
    /// the same page, and an implementation that gets the second pair wrong is
    /// correct on every document that never writes a negative rotation --- which
    /// is most of them, and not the scanned ones.
    #[test]
    fn a_rotation_in_degrees_becomes_quarter_turns_clockwise() {
        for (degrees, want) in [
            (0, 0u8),
            (90, 1),
            (180, 2),
            (270, 3),
            (360, 0),
            (450, 1),
            (-90, 3),
            (-180, 2),
            (-360, 0),
        ] {
            assert_eq!(
                turns_of_degrees(degrees),
                want,
                "/Rotate {degrees} is {want} quarter turn(s) clockwise"
            );
        }
    }

    /// A rotation the specification does not allow still has to answer.
    ///
    /// Truncation towards zero, which is what the existing arithmetic did and
    /// what this pins: 135 is *one* quarter turn rather than two, and 45 is
    /// none. Nothing here rounds, so a document that writes 89 is shown
    /// upright rather than sideways.
    #[test]
    fn a_rotation_that_is_not_a_multiple_of_ninety_truncates_towards_zero() {
        assert_eq!(turns_of_degrees(45), 0, "45 does not reach a quarter turn");
        assert_eq!(turns_of_degrees(89), 0, "89 does not reach a quarter turn");
        assert_eq!(turns_of_degrees(135), 1, "135 truncates to one, not two");
        assert_eq!(turns_of_degrees(-45), 0, "-45 truncates towards zero");
        assert_eq!(
            turns_of_degrees(-135),
            3,
            "-135 is -1 quarter turn, wrapped"
        );
    }
}
