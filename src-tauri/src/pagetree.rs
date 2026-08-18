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
fn inherited(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let mut at = page;
    for _ in 0..MAX_PARENTS {
        let dictionary = doc.get_object(at).and_then(Object::as_dict).ok()?;
        if let Ok(value) = dictionary.get(key) {
            return Some(value.clone());
        }
        at = dictionary
            .get(b"Parent")
            .and_then(Object::as_reference)
            .ok()?;
    }
    None
}

/// A page's `/Rotate` including anything it inherits.
///
/// Absent means "ask the parent", and only a page with no ancestor carrying one
/// is really at zero. Composing against the literal value instead would be
/// correct on every document that states it and wrong on every document that
/// does not --- which is the half nobody has a fixture for.
pub fn effective_rotation(doc: &Document, page: ObjectId) -> i64 {
    inherited(doc, page, b"Rotate")
        .and_then(|value| value.as_i64().ok())
        .unwrap_or(0)
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
    let box_of = |key: &[u8]| {
        inherited(doc, page, key)
            .and_then(|object| numbers_of(doc, &object, 4))
            .filter(|values| values.iter().all(|value| value.is_finite()))
            .map(|v| {
                [
                    v[0].min(v[2]),
                    v[1].min(v[3]),
                    v[0].max(v[2]),
                    v[1].max(v[3]),
                ]
            })
            .filter(|b| b[2] > b[0] && b[3] > b[1])
    };

    let media = box_of(b"MediaBox").unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let shown = match box_of(b"CropBox") {
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

    let turns = (((effective_rotation(doc, page) / 90) % 4 + 4) % 4) as u8;
    let (width, height) = (shown[2] - shown[0], shown[3] - shown[1]);
    DisplayedPage {
        width: if turns % 2 == 1 { height } else { width },
        height: if turns % 2 == 1 { width } else { height },
        turns,
        origin: (shown[0], shown[1]),
    }
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
                return Err(format!(
                    "pages {} and {} are the same page in this file, so they cannot be turned \
                     differently. Turn them the same way, or leave both as they are.",
                    first_at + 1,
                    at + 1
                ));
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
        let composed = (effective_rotation(doc, *id) + extra * 90).rem_euclid(360);
        doc.get_object_mut(*id)
            .and_then(Object::as_dict_mut)
            .map_err(|e| format!("page {id:?} is not a dictionary: {e}"))?
            .set("Rotate", composed);
    }
    Ok(())
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
    let pages = doc.get_pages();
    // A page object can answer to more than one page number: `/Kids` may list it
    // twice, and `lopdf`'s page walk keeps no visited set. An object that a KEPT
    // number also names must survive --- otherwise printing "page 1" of such a
    // document deletes the object page 1 *is*, and prints a blank sheet. This is
    // the damaging member of the family in the trap: the other two turn a page
    // twice, this one removes the page that was asked for.
    let kept: HashSet<ObjectId> = pages
        .iter()
        .filter(|(number, _)| !numbers.contains(number))
        .map(|(_, id)| *id)
        .collect();
    let doomed: HashSet<ObjectId> = numbers
        .iter()
        .filter_map(|number| pages.get(number).copied())
        .filter(|id| !kept.contains(id))
        .collect();
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
        for _ in 0..MAX_PARENTS {
            let Some(parent) = at else { break };
            decrements.push(parent);
            at = parent_of(doc, parent);
        }
    }
    for parent in decrements {
        if let Ok(tree) = doc.get_object_mut(parent).and_then(Object::as_dict_mut) {
            if let Ok(count) = tree.get(b"Count").and_then(Object::as_i64) {
                tree.set("Count", count - 1);
            }
        }
    }

    // One pass over the whole graph, dropping array entries and dictionary keys
    // that name a doomed page --- the `/Kids` entry that removes it from the
    // tree, and anything else pointing at it. The trailer is not in `objects`
    // and has to be walked in its own right.
    forget_in_dictionary(&mut doc.trailer, &doomed, 0)?;
    for object in doc.objects.values_mut() {
        forget_in_object(object, &doomed, 0)?;
    }
    for id in &doomed {
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
/// The intermediate `/Pages` nodes are left in the file, unreachable. That is the
/// same position `save.rs` takes about a deleted page's content --- a copy is a
/// serialisation, not a sanitation --- and the print path's `sweep::collect`
/// removes them because a print job is rewritten anyway.
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
        .map_err(|e| format!("no document catalog: {e}"))?
        .get(b"Pages")
        .and_then(Object::as_reference)
        .map_err(|e| format!("no page tree: {e}"))?;

    // What each page would inherit *after* the move: the root is about to be its
    // only ancestor. Read before anything is written, so that a page pushed down
    // to below cannot change the answer for the page after it.
    let from_root: Vec<Option<Object>> = INHERITABLE
        .iter()
        .map(|key| inherited(doc, root, key))
        .collect();

    let mut pushes: Vec<(ObjectId, &[u8], Object)> = Vec::new();
    for &page in order {
        let Ok(dictionary) = doc.get_object(page).and_then(Object::as_dict) else {
            continue;
        };
        for (at, key) in INHERITABLE.iter().enumerate() {
            if dictionary.has(key) {
                continue;
            }
            let now = inherited(doc, page, key);
            // Equal means the move changes nothing, so writing it would only
            // make an untouched page differ from its source. `None` means the
            // page has no value for this key anywhere above it, and the root
            // supplying one is a page *gaining* an attribute --- only reachable
            // on a document whose `/Parent` chain does not reach its own root,
            // and not something to synthesise from here.
            match now {
                Some(value) if Some(&value) != from_root[at].as_ref() => {
                    pushes.push((page, key, value));
                }
                _ => {}
            }
        }
    }
    for (page, key, value) in pushes {
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?
            .set(key.to_vec(), value);
    }

    let kids: Vec<Object> = order.iter().map(|&id| Object::Reference(id)).collect();
    let tree = doc
        .get_object_mut(root)
        .and_then(Object::as_dict_mut)
        .map_err(|e| format!("the page tree root is not a dictionary: {e}"))?;
    tree.set("Kids", kids);
    // Entries in the tree, not distinct objects: a page reached twice is two
    // pages to every reader, which is the same unit `drop_pages` decrements in.
    tree.set("Count", order.len() as i64);

    for &page in order {
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?
            .set("Parent", Object::Reference(root));
    }
    Ok(())
}

/// Drops the outline of a document that has lost pages.
///
/// Its destinations name pages that are no longer in the file, and a table of
/// contents that points at nothing is worse than none --- the same reason a
/// bounded outline walk reports what it cut rather than presenting a partial tree
/// as whole. It is also the only option that cannot write a *malformed* one:
/// [`drop_pages`] drops the reference out of a `/Dest` array rather than dropping
/// the array, so an entry that survives carries `[/XYZ 0 792 0]` with no page in
/// front of it.
///
/// **Whole rather than entry by entry**, which is a real loss on a long document
/// and is stated in `CHANGELOG.md` rather than hidden here. Repairing an outline
/// means resolving every destination shape --- a direct `/Dest`, a `/Dest` inside
/// an `/A` action, a name into `/Dests` or into the `/Names` tree --- deciding
/// which of them landed on a page that is gone, and rewriting the surviving tree
/// around the entries that have to go. That is `links.rs`'s resolver, on the
/// write side, and it is its own piece of work.
///
/// # Errors
///
/// The document has no catalog, which means it is not a document.
pub fn drop_outline(doc: &mut Document) -> Result<(), String> {
    doc.catalog_mut()
        .map_err(|e| format!("no document catalog: {e}"))?
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
        return Err(format!(
            "an object nests deeper than {} levels",
            sweep::MAX_NESTING
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
        return Err(format!(
            "an object nests deeper than {} levels",
            sweep::MAX_NESTING
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

#[cfg(test)]
mod tests {
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
}
