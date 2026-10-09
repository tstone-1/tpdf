//! Keeps a document's bookmarks when pages are deleted.
//!
//! Until 2026-10-07 a deletion dropped the outline whole
//! ([`pagetree::drop_outline`]): its destinations name page objects, some of
//! those objects are about to go, and an entry left pointing at nothing is worse
//! than no entry. That was safe and it cost every bookmark in a 300-page manual
//! for the sake of one deleted page --- `docs/DEMAND.md` ranks it first among
//! what tpdf lacks, because it is the exception to "a file loses nothing by
//! being saved".
//!
//! ## The rule
//!
//! Each entry is asked which page object it leads to, **before** any page is
//! removed, through every shape a destination can be written in: a `/Dest`
//! array, a `/Dest` name or string looked up in `/Dests` or in the `/Names`
//! tree, and the same three inside a `/GoTo` action. Then, from the leaves up:
//!
//!   - An entry leading to a page that is **staying** is not touched.
//!   - An entry leading to a page that is **going** is removed. The entries
//!     under it that stay **move up into its place**, in their order.
//!   - A heading that leads nowhere is removed when it had entries under it and
//!     none is left, and kept otherwise.
//!   - Anything else --- a web link, a remote destination, a name that
//!     resolves to nothing --- is not this function's to judge and is kept.
//!
//! **Moved up, not kept under a heading.** The first version of this module
//! kept a chapter whose page went as a heading with no destination, so that
//! section 2.3 stayed where it was. It was wrong for a reason that has nothing
//! to do with outlines: the heading is the *title of a deleted page*. Somebody
//! who extracts pages 6 to 8 to send them on would have sent the name of the
//! chapter that begins on page 5, and the old whole-drop never did that. So no
//! title **that leads to a page of this document** survives unless that page
//! does, and the price is a flatter table of contents.
//!
//! That is narrower than "no title survives unless its own page does", which
//! this paragraph said until 2026-10-09 and the list above never did. An
//! entry with no destination stays as long as it has no entries under it, or
//! one of them is left; a web link, a remote destination and a name that
//! resolves to nothing stay whatever pages go. None of those is known to be
//! the title of a page that went, and a heading that only groups what is
//! under it can still stand over entries whose pages stayed.
//!
//! ## When the old rule still applies
//!
//! The outline is dropped whole, as before, when repairing it would mean
//! trusting something that cannot be checked:
//!
//!   - **It is not a tree**: an entry reached twice, a `/Next` that loops, a
//!     sibling that is not a dictionary, or more entries than [`MAX_ITEMS`].
//!   - **An entry names its page by number**, the remote form, which a file
//!     sometimes writes for itself. A number is a position, every position after
//!     a deleted page shifts, and the entry would lead to the wrong page ---
//!     which is worse than leading nowhere.
//!   - **A name could not be looked up to the end**, because the `/Names` tree
//!     is deeper or wider than `links.rs` walks. The name may lead to a page
//!     that is going.
//!
//! The whole-drop is the one answer that cannot write a wrong outline, so it is
//! what an outline this module cannot vouch for gets.
//!
//! **A merge still brings no bookmarks across**; `merge.rs` says why, and this
//! module does not change it.

use std::collections::{HashMap, HashSet};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::encoding::resolve;
use crate::links;
use crate::pagetree;
use crate::redact;

/// Most outline entries a repair will walk before giving the outline up.
///
/// The figure `redact.rs` uses for the same tree and for the same reason: it
/// bounds a malformed file, not a long table of contents.
const MAX_ITEMS: usize = 20_000;

/// What a repair did to the outline.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Kept {
    /// Entries removed: they led to a deleted page, or were headings with
    /// nothing left under them.
    pub removed: usize,
    /// The outline could not be vouched for and went whole.
    pub whole: bool,
}

/// Where one outline entry leads.
enum Aim {
    /// A page object of this document.
    Page(ObjectId),
    /// Nowhere: the entry has neither `/Dest` nor `/A`.
    Nothing,
    /// Somewhere this module does not judge.
    Other,
    /// Somewhere a deletion would make wrong, or that could not be found out.
    Unsafe,
}

/// Removes the outline entries that lead to `doomed` pages and keeps the rest.
///
/// Call it **before** the pages are dropped. [`pagetree::drop_pages`] takes the
/// page reference out of every array that holds one, so afterwards a
/// destination to a deleted page reads `[/XYZ 0 792 0]` and cannot be told from
/// a damaged one.
///
/// # Errors
///
/// What [`pagetree::forget`] and [`pagetree::drop_outline`] refuse.
pub fn keep_for(doc: &mut Document, doomed: &HashSet<ObjectId>) -> Result<Kept, String> {
    let Some(root) = redact::outline_root(doc) else {
        return Ok(Kept::default());
    };
    let Some(Tree { order, children }) = tree_of(doc, root) else {
        return whole(doc);
    };

    // `order` lists a parent before its children, so walking it backwards
    // settles every entry after the entries under it. `under` ends up holding,
    // for an entry that stays, its children afterwards; for one that goes, the
    // entries that move up into its place.
    let mut removed: HashSet<ObjectId> = HashSet::new();
    let mut under: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for &id in order.iter().rev() {
        let was = children.get(&id).map_or(&[][..], Vec::as_slice);
        let left = lifted(was, &removed, &under);
        let aim = doc
            .get_dictionary(id)
            .map_or(Aim::Other, |item| aim_of(doc, item));
        let goes = match aim {
            Aim::Unsafe => return whole(doc),
            Aim::Page(page) => doomed.contains(&page),
            Aim::Nothing => !was.is_empty() && left.is_empty(),
            Aim::Other => false,
        };
        if goes {
            removed.insert(id);
        }
        under.insert(id, left);
    }
    if removed.is_empty() {
        return Ok(Kept::default());
    }

    let top = lifted(
        children.get(&root).map_or(&[][..], Vec::as_slice),
        &removed,
        &under,
    );
    chain(doc, root, &top);
    for &id in order.iter().filter(|id| !removed.contains(id)) {
        chain(doc, id, under.get(&id).map_or(&[][..], Vec::as_slice));
    }
    pagetree::forget(doc, &removed)?;
    if top.is_empty() {
        // A root with nothing under it is legal and draws an empty panel.
        pagetree::drop_outline(doc)?;
    } else {
        redact::recount(doc, root, 0);
    }
    Ok(Kept {
        removed: removed.len(),
        whole: false,
    })
}

/// Drops the outline whole and says so.
fn whole(doc: &mut Document) -> Result<Kept, String> {
    pagetree::drop_outline(doc)?;
    Ok(Kept {
        removed: 0,
        whole: true,
    })
}

/// `was` with every removed entry replaced by the entries that take its place.
fn lifted(
    was: &[ObjectId],
    removed: &HashSet<ObjectId>,
    under: &HashMap<ObjectId, Vec<ObjectId>>,
) -> Vec<ObjectId> {
    let mut left = Vec::with_capacity(was.len());
    for id in was {
        if removed.contains(id) {
            left.extend(under.get(id).into_iter().flatten());
        } else {
            left.push(*id);
        }
    }
    left
}

/// Writes `entries` as the children of `parent`: `/First` and `/Last` on the
/// parent, `/Parent`, `/Prev` and `/Next` on each entry.
///
/// Every link is written rather than patched. A splice has to get four
/// neighbours right for each entry that goes, and an entry that moves up a
/// level changes its parent as well; writing the chain from the list is one
/// rule with no cases. `/Count` is left to [`redact::recount`].
fn chain(doc: &mut Document, parent: ObjectId, entries: &[ObjectId]) {
    if let Ok(Object::Dictionary(node)) = doc.get_object_mut(parent) {
        match (entries.first(), entries.last()) {
            (Some(first), Some(last)) => {
                node.set("First", *first);
                node.set("Last", *last);
            }
            _ => {
                node.remove(b"First");
                node.remove(b"Last");
            }
        }
    }
    for (at, id) in entries.iter().enumerate() {
        let Ok(Object::Dictionary(item)) = doc.get_object_mut(*id) else {
            continue;
        };
        item.set("Parent", parent);
        match at.checked_sub(1).and_then(|before| entries.get(before)) {
            Some(prev) => item.set("Prev", *prev),
            None => {
                item.remove(b"Prev");
            }
        }
        match entries.get(at + 1) {
            Some(next) => item.set("Next", *next),
            None => {
                item.remove(b"Next");
            }
        }
    }
}

/// An outline that was walked and found to be a tree.
struct Tree {
    /// Every entry, a parent before its children.
    order: Vec<ObjectId>,
    /// The children of the root and of each entry, in sibling order.
    children: HashMap<ObjectId, Vec<ObjectId>>,
}

/// The outline under `root`. `None` when it is not a tree.
fn tree_of(doc: &Document, root: ObjectId) -> Option<Tree> {
    let mut seen: HashSet<ObjectId> = HashSet::from([root]);
    let mut order: Vec<ObjectId> = Vec::new();
    let mut children: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    let mut queue: Vec<ObjectId> = vec![root];
    let mut at = 0;
    while let Some(&node) = queue.get(at) {
        at += 1;
        let mut under: Vec<ObjectId> = Vec::new();
        let mut next = redact::first_child(doc, node);
        while let Some(id) = next {
            if !seen.insert(id) || seen.len() > MAX_ITEMS {
                return None;
            }
            let item = doc.get_dictionary(id).ok()?;
            under.push(id);
            next = item.get(b"Next").and_then(Object::as_reference).ok();
        }
        order.extend(&under);
        queue.extend(&under);
        children.insert(node, under);
    }
    Some(Tree { order, children })
}

/// Where an entry leads. An action wins over a `/Dest` beside it, which is the
/// order `links.rs` reads them in and records the reason for.
fn aim_of(doc: &Document, item: &Dictionary) -> Aim {
    if let Ok(action) = item.get(b"A") {
        let Ok(action) = resolve(doc, action).as_dict() else {
            return Aim::Other;
        };
        return match action.get(b"S").and_then(Object::as_name) {
            Ok(b"GoTo") => action
                .get(b"D")
                .map_or(Aim::Other, |dest| destination(doc, dest)),
            _ => Aim::Other,
        };
    }
    item.get(b"Dest")
        .map_or(Aim::Nothing, |dest| destination(doc, dest))
}

/// A destination in any of its three written forms.
fn destination(doc: &Document, dest: &Object) -> Aim {
    match resolve(doc, dest) {
        Object::Array(array) => page_of(array),
        Object::Name(name) => named(doc, name),
        Object::String(bytes, _) => named(doc, bytes),
        _ => Aim::Other,
    }
}

/// A named destination, from `/Dests` or from the `/Names` tree.
fn named(doc: &Document, key: &[u8]) -> Aim {
    let Ok(catalog) = doc.catalog() else {
        return Aim::Other;
    };
    if let Ok(dests) = catalog.get(b"Dests") {
        if let Ok(found) = resolve(doc, dests).as_dict().and_then(|dict| dict.get(key)) {
            return follow(doc, found);
        }
    }
    let tree = catalog
        .get(b"Names")
        .ok()
        .and_then(|names| resolve(doc, names).as_dict().ok())
        .and_then(|names| names.get(b"Dests").ok());
    let Some(tree) = tree else {
        return Aim::Other;
    };
    match links::find_named(tree, key, doc) {
        Ok(Some(value)) => follow(doc, &value),
        Ok(None) => Aim::Other,
        Err(links::GaveUp) => Aim::Unsafe,
    }
}

/// A named destination's value: the array, or a dictionary holding it as `/D`.
fn follow(doc: &Document, value: &Object) -> Aim {
    match resolve(doc, value) {
        Object::Array(array) => page_of(array),
        Object::Dictionary(dict) => match dict.get(b"D").map(|inner| resolve(doc, inner)) {
            Ok(Object::Array(array)) => page_of(array),
            _ => Aim::Other,
        },
        _ => Aim::Other,
    }
}

/// The page a destination array names.
fn page_of(array: &[Object]) -> Aim {
    match array.first() {
        Some(Object::Reference(id)) => Aim::Page(*id),
        // A page number. Right until a page before it is deleted.
        Some(Object::Integer(_)) => Aim::Unsafe,
        _ => Aim::Other,
    }
}

#[cfg(test)]
mod tests {
    use lopdf::dictionary;

    use super::*;

    /// A document of `count` pages and nothing else.
    fn pages(count: usize) -> (Document, Vec<ObjectId>) {
        let mut doc = Document::with_version("1.7");
        let tree = doc.new_object_id();
        let kids: Vec<ObjectId> = (0..count)
            .map(|_| doc.add_object(dictionary! { "Type" => "Page", "Parent" => tree }))
            .collect();
        doc.objects.insert(
            tree,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids.iter().map(|id| (*id).into()).collect::<Vec<Object>>(),
                "Count" => i64::try_from(count).expect("a small count"),
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
        doc.trailer.set("Root", catalog);
        (doc, kids)
    }

    /// One entry of an outline under construction.
    struct Entry {
        title: &'static str,
        /// Extra keys: `/Dest` or `/A`.
        keys: Dictionary,
        under: Vec<Entry>,
    }

    fn entry(title: &'static str, keys: Dictionary, under: Vec<Entry>) -> Entry {
        Entry { title, keys, under }
    }

    fn to(page: ObjectId) -> Dictionary {
        dictionary! { "Dest" => vec![Object::Reference(page), "Fit".into()] }
    }

    /// Writes `entries` under `parent` as a linked chain and returns their ids.
    fn link(doc: &mut Document, parent: ObjectId, entries: Vec<Entry>) -> Vec<ObjectId> {
        let ids: Vec<ObjectId> = entries.iter().map(|_| doc.new_object_id()).collect();
        for (at, entry) in entries.into_iter().enumerate() {
            let mut item = entry.keys;
            item.set("Title", Object::string_literal(entry.title));
            item.set("Parent", parent);
            if at > 0 {
                item.set("Prev", ids[at - 1]);
            }
            if let Some(next) = ids.get(at + 1) {
                item.set("Next", *next);
            }
            let count = i64::try_from(entry.under.len()).expect("a small count");
            doc.objects.insert(ids[at], Object::Dictionary(item));
            let under = link(doc, ids[at], entry.under);
            if let (Some(first), Some(last)) = (under.first(), under.last()) {
                let Ok(Object::Dictionary(item)) = doc.get_object_mut(ids[at]) else {
                    unreachable!("just inserted");
                };
                item.set("First", *first);
                item.set("Last", *last);
                item.set("Count", count);
            }
        }
        ids
    }

    fn outline(doc: &mut Document, entries: Vec<Entry>) -> ObjectId {
        let root = doc.new_object_id();
        let count = i64::try_from(entries.len()).expect("a small count");
        let top = link(doc, root, entries);
        let mut dict = dictionary! { "Type" => "Outlines", "Count" => count };
        if let (Some(first), Some(last)) = (top.first(), top.last()) {
            dict.set("First", *first);
            dict.set("Last", *last);
        }
        doc.objects.insert(root, Object::Dictionary(dict));
        doc.catalog_mut().expect("catalog").set("Outlines", root);
        root
    }

    /// The outline as a reader walks it: `title`, `title>child`, with a `*` on
    /// an entry that leads nowhere. Walked forwards by `/First` and `/Next`
    /// **and** backwards by `/Last` and `/Prev`, and the two must agree, so a
    /// chain spliced on one side only fails here.
    fn shape(doc: &Document) -> Vec<String> {
        fn walk(doc: &Document, parent: ObjectId, prefix: &str, out: &mut Vec<String>) {
            let dict = doc.get_dictionary(parent).expect("a node");
            let mut forwards = Vec::new();
            let mut at = dict.get(b"First").and_then(Object::as_reference).ok();
            while let Some(id) = at {
                forwards.push(id);
                at = doc
                    .get_dictionary(id)
                    .expect("an entry")
                    .get(b"Next")
                    .and_then(Object::as_reference)
                    .ok();
            }
            let mut backwards = Vec::new();
            let mut at = dict.get(b"Last").and_then(Object::as_reference).ok();
            while let Some(id) = at {
                backwards.push(id);
                at = doc
                    .get_dictionary(id)
                    .expect("an entry")
                    .get(b"Prev")
                    .and_then(Object::as_reference)
                    .ok();
            }
            backwards.reverse();
            assert_eq!(
                forwards, backwards,
                "the chain under {parent:?} reads the same both ways"
            );
            for id in forwards {
                let item = doc.get_dictionary(id).expect("an entry");
                assert_eq!(
                    item.get(b"Parent").and_then(Object::as_reference).ok(),
                    Some(parent),
                    "an entry names the node it hangs under"
                );
                let title = String::from_utf8_lossy(
                    item.get(b"Title")
                        .and_then(Object::as_str)
                        .expect("a title"),
                )
                .into_owned();
                let leads = item.has(b"Dest") || item.has(b"A");
                let name = format!("{prefix}{title}{}", if leads { "" } else { "*" });
                out.push(name);
                walk(doc, id, &format!("{prefix}{title}>"), out);
            }
        }
        let mut out = Vec::new();
        if let Some(root) = redact::outline_root(doc) {
            walk(doc, root, "", &mut out);
        }
        out
    }

    fn doomed(ids: &[ObjectId]) -> HashSet<ObjectId> {
        ids.iter().copied().collect()
    }

    /// The rule, one entry per case, with page 2 deleted.
    #[test]
    fn deleting_a_page_removes_only_the_entries_that_led_to_it() {
        let (mut doc, kids) = pages(3);
        doc.catalog_mut().expect("catalog").set(
            "Dests",
            dictionary! {
                "end" => vec![Object::Reference(kids[2]), "Fit".into()],
                "middle" => vec![Object::Reference(kids[1]), "Fit".into()],
            },
        );
        let go_to = dictionary! {
            "A" => dictionary! {
                "S" => "GoTo",
                "D" => vec![Object::Reference(kids[1]), "Fit".into()],
            },
        };
        let go_to_parent = go_to.clone();
        let web = dictionary! {
            "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://example.org/") },
        };
        outline(
            &mut doc,
            vec![
                entry("one", to(kids[0]), vec![]),
                entry(
                    "two",
                    to(kids[1]),
                    vec![
                        entry("two-a", to(kids[1]), vec![]),
                        entry("two-b", to(kids[2]), vec![]),
                    ],
                ),
                entry("named", dictionary! { "Dest" => "end" }, vec![]),
                entry("named-gone", dictionary! { "Dest" => "middle" }, vec![]),
                entry("action", go_to, vec![]),
                entry(
                    "act",
                    go_to_parent,
                    vec![entry("act-a", to(kids[0]), vec![])],
                ),
                entry(
                    "part",
                    Dictionary::new(),
                    vec![entry("part-a", to(kids[1]), vec![])],
                ),
                entry("web", web, vec![]),
                entry("bare", Dictionary::new(), vec![]),
            ],
        );
        assert_eq!(shape(&doc).len(), 13, "the fixture has thirteen entries");

        let kept = keep_for(&mut doc, &doomed(&[kids[1]])).expect("repair");

        assert_eq!(
            shape(&doc),
            ["one", "two-b", "named", "act-a", "web", "bare*"],
            "what led to page 2 went, and what was under it and stays moved up"
        );
        assert_eq!(
            kept,
            Kept {
                removed: 7,
                whole: false
            }
        );
        let root = redact::outline_root(&doc).expect("still an outline");
        assert_eq!(
            doc.get_dictionary(root)
                .expect("root")
                .get(b"Count")
                .and_then(Object::as_i64)
                .ok(),
            Some(6),
            "the count is what a reader now sees open"
        );
    }

    /// The control for the test above: with a page deleted that nothing leads
    /// to, the outline is the one that went in. Without it, a repair that
    /// removed entries at random would pass.
    #[test]
    fn deleting_a_page_no_entry_leads_to_changes_nothing() {
        let (mut doc, kids) = pages(3);
        outline(
            &mut doc,
            vec![
                entry(
                    "one",
                    to(kids[0]),
                    vec![entry("one-a", to(kids[0]), vec![])],
                ),
                entry("three", to(kids[2]), vec![]),
            ],
        );
        let before = shape(&doc);
        let kept = keep_for(&mut doc, &doomed(&[kids[1]])).expect("repair");
        assert_eq!(shape(&doc), before);
        assert_eq!(kept, Kept::default());
    }

    /// A name in the `/Names` tree is followed like one in `/Dests`, in both of
    /// the forms a value may take.
    #[test]
    fn a_name_in_the_name_tree_is_followed_to_its_page() {
        let (mut doc, kids) = pages(2);
        let leaf = doc.add_object(dictionary! {
            "Names" => vec![
                Object::string_literal("gone"),
                Object::Dictionary(dictionary! { "D" => vec![Object::Reference(kids[1]), "Fit".into()] }),
                Object::string_literal("kept"),
                Object::Array(vec![Object::Reference(kids[0]), "Fit".into()]),
            ],
        });
        let dests = doc.add_object(dictionary! { "Kids" => vec![Object::Reference(leaf)] });
        doc.catalog_mut()
            .expect("catalog")
            .set("Names", dictionary! { "Dests" => dests });
        outline(
            &mut doc,
            vec![
                entry(
                    "gone",
                    dictionary! { "Dest" => Object::string_literal("gone") },
                    vec![],
                ),
                entry(
                    "kept",
                    dictionary! { "Dest" => Object::string_literal("kept") },
                    vec![],
                ),
                entry(
                    "unknown",
                    dictionary! { "Dest" => Object::string_literal("nowhere") },
                    vec![],
                ),
            ],
        );
        keep_for(&mut doc, &doomed(&[kids[1]])).expect("repair");
        assert_eq!(shape(&doc), ["kept", "unknown"]);
    }

    /// Moving up works through more than one level, and a heading that leads
    /// nowhere stays while something is left under it. No title of a deleted
    /// page is left anywhere in the outline.
    #[test]
    fn what_stays_moves_up_past_every_entry_that_goes() {
        let (mut doc, kids) = pages(3);
        outline(
            &mut doc,
            vec![entry(
                "part",
                Dictionary::new(),
                vec![
                    entry(
                        "gone",
                        to(kids[1]),
                        vec![
                            entry(
                                "gone too",
                                to(kids[1]),
                                vec![
                                    entry("deep one", to(kids[0]), vec![]),
                                    entry("deep two", to(kids[2]), vec![]),
                                ],
                            ),
                            entry("after", to(kids[2]), vec![]),
                        ],
                    ),
                    entry("last", to(kids[0]), vec![]),
                ],
            )],
        );
        keep_for(&mut doc, &doomed(&[kids[1]])).expect("repair");
        assert_eq!(
            shape(&doc),
            [
                "part*",
                "part>deep one",
                "part>deep two",
                "part>after",
                "part>last"
            ]
        );
        let titles: Vec<String> = doc
            .objects
            .values()
            .filter_map(|object| object.as_dict().ok())
            .filter_map(|dict| dict.get(b"Title").and_then(Object::as_str).ok())
            .map(|title| String::from_utf8_lossy(title).into_owned())
            .collect();
        assert!(
            !titles.iter().any(|title| title.starts_with("gone")),
            "the removed entries are out of the file, not only out of the chain: {titles:?}"
        );
    }

    /// An entry that names its page by number makes the whole outline
    /// unrepairable: after a deletion the number is another page.
    #[test]
    fn an_entry_naming_its_page_by_number_drops_the_outline_whole() {
        let (mut doc, kids) = pages(3);
        outline(
            &mut doc,
            vec![
                entry("one", to(kids[0]), vec![]),
                entry(
                    "by number",
                    dictionary! { "Dest" => vec![2.into(), "Fit".into()] },
                    vec![],
                ),
            ],
        );
        let kept = keep_for(&mut doc, &doomed(&[kids[1]])).expect("repair");
        assert!(kept.whole);
        assert!(doc.catalog().expect("catalog").get(b"Outlines").is_err());
    }

    /// A name the bounded walk cannot reach may lead to a page that is going,
    /// so the outline is not vouched for. The control is the same tree one
    /// level shallower, where the name is found and the entry simply goes.
    #[test]
    fn a_name_the_walk_gives_up_on_drops_the_outline_whole() {
        for (depth, expect_whole) in [(40, true), (3, false)] {
            let (mut doc, kids) = pages(2);
            let mut node = doc.add_object(dictionary! {
                "Names" => vec![
                    Object::string_literal("deep"),
                    Object::Array(vec![Object::Reference(kids[1]), "Fit".into()]),
                ],
            });
            for _ in 0..depth {
                node = doc.add_object(dictionary! { "Kids" => vec![Object::Reference(node)] });
            }
            doc.catalog_mut()
                .expect("catalog")
                .set("Names", dictionary! { "Dests" => node });
            outline(
                &mut doc,
                vec![
                    entry("one", to(kids[0]), vec![]),
                    entry(
                        "deep",
                        dictionary! { "Dest" => Object::string_literal("deep") },
                        vec![],
                    ),
                ],
            );
            let kept = keep_for(&mut doc, &doomed(&[kids[1]])).expect("repair");
            assert_eq!(kept.whole, expect_whole, "a name {depth} levels down");
            if !expect_whole {
                assert_eq!(shape(&doc), ["one"]);
            }
        }
    }

    /// When every entry goes, so does the outline: a root with nothing under it
    /// draws an empty panel.
    #[test]
    fn an_outline_with_nothing_left_is_dropped() {
        let (mut doc, kids) = pages(2);
        outline(&mut doc, vec![entry("two", to(kids[1]), vec![])]);
        let kept = keep_for(&mut doc, &doomed(&[kids[1]])).expect("repair");
        assert_eq!(kept.removed, 1);
        assert!(doc.catalog().expect("catalog").get(b"Outlines").is_err());
    }

    /// An outline that loops is not repaired. It goes whole, as every outline
    /// did before this module.
    #[test]
    fn an_outline_that_is_not_a_tree_is_dropped_whole() {
        let (mut doc, kids) = pages(2);
        let root = outline(
            &mut doc,
            vec![
                entry("one", to(kids[0]), vec![]),
                entry("two", to(kids[1]), vec![]),
            ],
        );
        let first = redact::first_child(&doc, root).expect("an entry");
        let last = doc
            .get_dictionary(root)
            .expect("root")
            .get(b"Last")
            .and_then(Object::as_reference)
            .expect("a last entry");
        let Ok(Object::Dictionary(item)) = doc.get_object_mut(last) else {
            unreachable!("an entry is a dictionary");
        };
        item.set("Next", first);

        let kept = keep_for(&mut doc, &doomed(&[kids[1]])).expect("repair");
        assert!(kept.whole);
        assert!(doc.catalog().expect("catalog").get(b"Outlines").is_err());
    }

    /// The same for an entry two chains reach: `/Prev` and `/Next` cannot be
    /// spliced for both.
    #[test]
    fn an_entry_reached_twice_drops_the_outline_whole() {
        let (mut doc, kids) = pages(2);
        let root = outline(
            &mut doc,
            vec![
                entry(
                    "one",
                    to(kids[0]),
                    vec![entry("one-a", to(kids[0]), vec![])],
                ),
                entry("two", to(kids[1]), vec![]),
            ],
        );
        let first = redact::first_child(&doc, root).expect("an entry");
        let shared = redact::first_child(&doc, first).expect("its child");
        let second = doc
            .get_dictionary(first)
            .expect("entry")
            .get(b"Next")
            .and_then(Object::as_reference)
            .expect("a second entry");
        let Ok(Object::Dictionary(item)) = doc.get_object_mut(second) else {
            unreachable!("an entry is a dictionary");
        };
        item.set("First", shared);
        item.set("Last", shared);

        let kept = keep_for(&mut doc, &doomed(&[kids[1]])).expect("repair");
        assert!(kept.whole);
    }

    /// A document with no outline is left without one, and nothing is reported.
    #[test]
    fn a_document_with_no_outline_is_left_alone() {
        let (mut doc, kids) = pages(2);
        assert_eq!(keep_for(&mut doc, &doomed(&[kids[1]])), Ok(Kept::default()));
        assert!(doc.catalog().expect("catalog").get(b"Outlines").is_err());
    }
}
