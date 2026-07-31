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
//! It lived in `examples/sanitize_rewrite.rs` first, where the measurement was taken.
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

/// How deep a single object may nest before the walk refuses to descend.
///
/// This runs on input we did not write, in the **app process** rather than in a
/// worker (see `docs/THREAT-MODEL.md` §3), and the walk below is recursive --- so
/// without a bound a document nesting arrays a few hundred thousand deep
/// overflows the stack and takes the application with it. Real structure is
/// nowhere near: a page tree is a handful of levels and an outline is bounded at
/// 32 by `outline.rs`.
///
/// **No document `lopdf` will load can reach this bound**, and saying so is the
/// point rather than an aside. `lopdf` 0.44's own parser refuses past
/// `reader::MAX_NESTING_DEPTH`, which is **100** --- it counts down through
/// arrays and dictionaries and returns a parse error at zero --- so every object
/// in a `Document` arrived through a stricter check than this one. The guard is
/// therefore defence against **that constant changing**, not against any input:
/// a `lopdf` bump that raised it past 256, or a caller that hands this a graph
/// it built rather than parsed, and this walk is the only thing between either
/// and a stack overflow in the process holding the user's filesystem authority.
///
/// It is deliberately not written as a compile-time comparison against `lopdf`'s
/// constant, which would be the stronger form: `reader` is a private module and
/// `MAX_NESTING_DEPTH` is not re-exported, so there is nothing to compare
/// against. A version bump is the moment to re-read it.
///
/// What happens at the bound is the part that matters. **Refusing is not
/// optional here**, and truncating would be worse than the overflow it avoids: a
/// mark-and-sweep that stops descending has not found every reference, so it
/// would then delete objects that are genuinely reachable and hand back a
/// document with holes in it --- silently, since the result still parses. So the
/// depth is reported and every caller turns it into an error.
pub const MAX_NESTING: usize = 256;

/// Drops every object unreachable from the trailer, and repairs `max_id`.
///
/// Returns how many were collected. Object numbers are deliberately **not**
/// made contiguous: that is cosmetic, and costs a second quadratic pass.
///
/// # Errors
///
/// An object nesting deeper than [`MAX_NESTING`], which makes the reachable set
/// incomplete and so makes collecting unsafe --- see that constant.
pub fn collect(doc: &mut Document) -> Result<usize, String> {
    let before = doc.objects.len();
    let reachable = reachable(doc)?;
    doc.objects.retain(|id, _| reachable.contains(id));
    doc.max_id = doc.objects.keys().map(|id| id.0).max().unwrap_or(0);
    Ok(before - doc.objects.len())
}

/// Every object reachable from the trailer, by breadth-first mark.
///
/// # Errors
///
/// As [`collect`].
pub fn reachable(doc: &Document) -> Result<HashSet<ObjectId>, String> {
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut queue: Vec<ObjectId> = Vec::new();

    let trailer = Object::Dictionary(doc.trailer.clone());
    let mut roots = Vec::new();
    references(&trailer, &mut roots)?;
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
        references(object, &mut referenced)?;
        for id in referenced {
            if seen.insert(id) {
                queue.push(id);
            }
        }
    }
    Ok(seen)
}

/// Every object id named directly by `object`.
///
/// # Errors
///
/// Nesting deeper than [`MAX_NESTING`].
pub fn references(object: &Object, out: &mut Vec<ObjectId>) -> Result<(), String> {
    descend(object, out, 0)
}

/// [`references`], carrying how deep it already is.
fn descend(object: &Object, out: &mut Vec<ObjectId>, depth: usize) -> Result<(), String> {
    if depth > MAX_NESTING {
        return Err(format!("an object nests deeper than {MAX_NESTING} levels"));
    }
    match object {
        Object::Reference(id) => out.push(*id),
        Object::Array(items) => {
            for item in items {
                descend(item, out, depth + 1)?;
            }
        }
        Object::Dictionary(dictionary) => {
            for (_, value) in dictionary.iter() {
                descend(value, out, depth + 1)?;
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter() {
                descend(value, out, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{references, MAX_NESTING};
    use lopdf::Object;

    /// An array nested `depth` levels, with a reference at the bottom.
    fn nested(depth: usize) -> Object {
        let mut object = Object::Reference((7, 0));
        for _ in 0..depth {
            object = Object::Array(vec![object]);
        }
        object
    }

    #[test]
    fn ordinary_nesting_is_walked_to_the_bottom() {
        // The control. Without it every assertion below is satisfied by a walk
        // that refuses everything, which is how a bound fails.
        let mut out = Vec::new();
        references(&nested(8), &mut out).expect("eight levels is ordinary structure");
        assert_eq!(out, vec![(7, 0)]);
    }

    #[test]
    fn nesting_at_the_bound_is_still_walked() {
        // The boundary from the permitted side, or the check is off by one in
        // the direction that refuses a document nobody would call hostile.
        let mut out = Vec::new();
        references(&nested(MAX_NESTING), &mut out).expect("the bound itself must be allowed");
        assert_eq!(out, vec![(7, 0)]);
    }

    #[test]
    fn nesting_past_the_bound_is_refused_rather_than_truncated() {
        // Refused, not "walked as far as it got": a partial reachable set makes
        // `collect` delete live objects, which produces a document that still
        // parses and has holes in it.
        //
        // Note the object is *built* rather than parsed, and it has to be:
        // `lopdf`'s parser stops at 100 levels, so no loaded document reaches
        // 257 and there is no fixture that could stand in for one. That is the
        // same fact `MAX_NESTING` records as its reason for existing.
        //
        // One honest limit, worth stating rather than leaving to be discovered.
        // This asserts the *bound fires*, not that an unbounded walk would have
        // overflowed --- provoking that needs an object nested hundreds of
        // thousands deep, and `lopdf::Object`'s derived `Drop` is itself
        // recursive, so building one to prove the point would overflow the test
        // thread on the way out rather than on the way in. The bound is the
        // reason no such object is ever walked; that it is reached at 257 levels
        // is what can be checked here.
        let mut out = Vec::new();
        let error = references(&nested(MAX_NESTING + 1), &mut out)
            .expect_err("nesting past the bound must be refused");
        assert!(error.contains(&MAX_NESTING.to_string()), "{error}");
    }
}
