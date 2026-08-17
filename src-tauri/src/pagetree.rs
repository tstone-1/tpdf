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
//!    `/Parent` chain rather than out of the page's own dictionary.

use std::collections::{HashMap, HashSet};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::sweep;

/// A page's `/Rotate` including anything it inherits.
///
/// Absent means "ask the parent", and only a page with no ancestor carrying one
/// is really at zero. Composing against the literal value instead would be
/// correct on every document that states it and wrong on every document that
/// does not --- which is the half nobody has a fixture for.
pub fn effective_rotation(doc: &Document, page: ObjectId) -> i64 {
    let mut at = page;
    // Bounded: a `/Parent` cycle in a malformed file would otherwise spin here,
    // and this runs on input we did not write.
    for _ in 0..64 {
        let Ok(dictionary) = doc.get_object(at).and_then(Object::as_dict) else {
            break;
        };
        if let Ok(value) = dictionary.get(b"Rotate").and_then(Object::as_i64) {
            return value;
        }
        match dictionary.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent) => at = parent,
            Err(_) => break,
        }
    }
    0
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
        // Same `/Parent`-cycle bound as `effective_rotation`, same reason: this
        // runs on input we did not write.
        for _ in 0..64 {
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
