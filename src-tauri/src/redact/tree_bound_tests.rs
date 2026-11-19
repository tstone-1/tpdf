//! Walks over document-shaped trees that the document can make expensive: the
//! parent tree's number tree, and the outline's `/Count`.
//!
//! A `/Kids` or `/First` naming a node already reached doubles the walk at
//! every level it repeats on, so a few dozen levels are more visits than a save
//! finishes. A node is walked once, and the walk stops after its bound.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId};

use super::{number_tree_lookup, recount, TreeWalk, MAX_TREE_NODES};

/// Twenty levels of number-tree nodes, each naming the next twice and holding
/// no key, as the first kid of a root whose second kid holds `key`.
fn doubled_tree(key: i64) -> (Document, Dictionary) {
    let mut doc = Document::with_version("1.7");
    let mut below = doc.add_object(dictionary! { "Nums" => Vec::<Object>::new() });
    for _ in 0..20 {
        below = doc.add_object(dictionary! { "Kids" => vec![below.into(), below.into()] });
    }
    let answer = doc.add_object(dictionary! {
        "Nums" => vec![Object::Integer(key), Object::string_literal("FOUND")],
    });
    let root = dictionary! { "Kids" => vec![below.into(), answer.into()] };
    (doc, root)
}

/// A node named twice is searched once, so the key behind it is still found.
///
/// Without that, the doubled branch is 2^21 visits and the bound ends the
/// lookup before the second kid is reached: the key would be answered as not
/// there, and a tagged page's structure element would keep its copy.
#[test]
fn a_number_tree_naming_one_node_twice_is_searched_once() {
    let (doc, root) = doubled_tree(5);
    let mut walk = TreeWalk::default();

    let found = number_tree_lookup(&doc, &root, 5, 0, &mut walk);

    assert_eq!(found, Some(Object::string_literal("FOUND")));
    assert!(walk.visits < 100, "{} visits", walk.visits);
}

/// A tree wide enough to pass the node bound stops at it.
#[test]
fn a_number_tree_larger_than_the_bound_stops_at_it() {
    let mut doc = Document::with_version("1.7");
    let kids: Vec<Object> = (0..MAX_TREE_NODES + 50)
        .map(|_| {
            doc.add_object(dictionary! { "Nums" => Vec::<Object>::new() })
                .into()
        })
        .collect();
    let root = dictionary! { "Kids" => kids };
    let mut walk = TreeWalk::default();

    let found = number_tree_lookup(&doc, &root, 5, 0, &mut walk);

    assert_eq!(found, None);
    assert!(
        walk.visits <= MAX_TREE_NODES + 1,
        "{} visits against a bound of {MAX_TREE_NODES}",
        walk.visits
    );
}

/// An outline whose entries at each of twenty levels both lead to one entry
/// below, then a second top-level entry `tail` holding one child, with a
/// `/Count` that is wrong on purpose.
fn doubled_outline() -> (Document, ObjectId, ObjectId) {
    let mut doc = Document::with_version("1.7");
    let mut below = doc.add_object(dictionary! { "Title" => Object::string_literal("deep") });
    for _ in 0..20 {
        let right = doc.add_object(dictionary! { "First" => below, "Last" => below });
        let left = doc.add_object(dictionary! {
            "First" => below, "Last" => below, "Next" => right,
        });
        below = doc.add_object(dictionary! { "First" => left, "Last" => right });
    }
    let leaf = doc.add_object(dictionary! { "Title" => Object::string_literal("leaf") });
    let tail = doc.add_object(dictionary! { "First" => leaf, "Last" => leaf, "Count" => 9 });
    if let Ok(Object::Dictionary(top)) = doc.get_object_mut(below) {
        top.set("Next", tail);
    }
    let root = doc.add_object(dictionary! { "First" => below, "Last" => tail });
    (doc, root, tail)
}

/// An entry reached twice is counted once, so the walk gets to the entries
/// after it and rewrites their `/Count`.
///
/// Without that the doubled branch is past [`MAX_OUTLINE_ITEMS`] visits, the
/// walk stops inside it, and the entry after it keeps the count it had.
#[test]
fn an_outline_entry_reached_twice_is_counted_once() {
    let (mut doc, root, tail) = doubled_outline();

    recount(&mut doc, root, 0);

    assert_eq!(
        doc.get_dictionary(tail)
            .and_then(|item| item.get(b"Count"))
            .and_then(Object::as_i64)
            .ok(),
        Some(1),
        "the entry after the doubled branch was reached"
    );
}

/// A chain of `n` new entries, each holding `children` new entries.
fn chain_of(doc: &mut Document, n: usize, children: usize) -> Vec<ObjectId> {
    let items: Vec<ObjectId> = (0..n)
        .map(|_| {
            let kids: Vec<ObjectId> = (0..children)
                .map(|_| doc.add_object(dictionary! { "Title" => Object::string_literal("x") }))
                .collect();
            for pair in kids.windows(2) {
                if let Ok(Object::Dictionary(item)) = doc.get_object_mut(pair[0]) {
                    item.set("Next", pair[1]);
                }
            }
            doc.add_object(dictionary! { "First" => kids[0], "Last" => kids[children - 1] })
        })
        .collect();
    for pair in items.windows(2) {
        if let Ok(Object::Dictionary(item)) = doc.get_object_mut(pair[0]) {
            item.set("Next", pair[1]);
        }
    }
    items
}

/// The visits are bounded even where every entry is new: past
/// [`MAX_OUTLINE_ITEMS`] the walk stops, and the entries after that point are
/// not rewritten.
#[test]
fn an_outline_larger_than_the_bound_stops_at_it() {
    let mut doc = Document::with_version("1.7");
    // 300 entries of 100 children each: 30,300 entries, none named twice.
    let items = chain_of(&mut doc, 300, 100);
    let root = doc.add_object(dictionary! { "First" => items[0], "Last" => items[299] });

    recount(&mut doc, root, 0);

    let has_count = |id: ObjectId| doc.get_dictionary(id).is_ok_and(|item| item.has(b"Count"));
    assert!(has_count(items[0]), "the control: the walk did run");
    assert!(
        !has_count(items[299]),
        "an entry past the bound was not reached"
    );
}
