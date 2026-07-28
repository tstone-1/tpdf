//! Mark-and-sweep over a `lopdf` object graph.
//!
//! Written out rather than reached for from `lopdf`, and the reason is measured.
//! `prune_objects()` and `renumber_objects()` both walk the graph through
//! `traverse_objects()`, which accumulates seen ids in a `Vec` and calls
//! `contains` before every push --- quadratic. Spike 0.4 measured 3.7 ms at
//! 2,445 objects, 83 ms at 7,758 and **1,414 ms at 25,583**: a 3.3x larger graph
//! costing 17x more, on a document size that is unremarkable. The `HashSet`
//! version below produces byte-identical output at a cost indistinguishable from
//! not collecting at all.
//!
//! It lived in `bin/sanitize_rewrite.rs` first, where the measurement was taken.
//! It is here because printing a page range needs exactly the same sweep, and
//! two copies of a graph walk is two things to keep in step.
//!
//! **`max_id` has to be lowered by hand.** `/Size` is written from it and
//! sweeping does not touch it, so a swept document otherwise claims more objects
//! than it contains. `qpdf --check` rejects that and PDFium does not notice ---
//! which is the whole argument for verifying a rewrite with a second parser, and
//! it caught this on the sweep's first run when every other check passed.

use std::collections::HashSet;

use lopdf::{Document, Object, ObjectId};

/// Drops every object unreachable from the trailer, and repairs `max_id`.
///
/// Returns how many were collected. Object numbers are deliberately **not**
/// made contiguous: that is cosmetic, and costs a second quadratic pass.
pub fn collect(doc: &mut Document) -> usize {
    let before = doc.objects.len();
    let reachable = reachable(doc);
    doc.objects.retain(|id, _| reachable.contains(id));
    doc.max_id = doc.objects.keys().map(|id| id.0).max().unwrap_or(0);
    before - doc.objects.len()
}

/// Every object reachable from the trailer, by breadth-first mark.
#[must_use]
pub fn reachable(doc: &Document) -> HashSet<ObjectId> {
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut queue: Vec<ObjectId> = Vec::new();

    let trailer = Object::Dictionary(doc.trailer.clone());
    let mut roots = Vec::new();
    references(&trailer, &mut roots);
    for id in roots {
        if seen.insert(id) {
            queue.push(id);
        }
    }

    while let Some(id) = queue.pop() {
        let Some(object) = doc.objects.get(&id) else {
            // A dangling reference. Not this pass's problem: it names no object,
            // so it carries no content.
            continue;
        };
        let mut referenced = Vec::new();
        references(object, &mut referenced);
        for id in referenced {
            if seen.insert(id) {
                queue.push(id);
            }
        }
    }
    seen
}

/// Every object id named directly by `object`.
pub fn references(object: &Object, out: &mut Vec<ObjectId>) {
    match object {
        Object::Reference(id) => out.push(*id),
        Object::Array(items) => items.iter().for_each(|i| references(i, out)),
        Object::Dictionary(dictionary) => dictionary.iter().for_each(|(_, v)| references(v, out)),
        Object::Stream(stream) => stream.dict.iter().for_each(|(_, v)| references(v, out)),
        _ => {}
    }
}
