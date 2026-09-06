//! One bounded walk of `/AcroForm /Fields`, for everything that reads a form.
//!
//! `/Fields` is a **tree**: an entry is a field, or a node whose `/Kids` hold
//! fields, and a fully qualified name is the `/T` values joined down the chain.
//! Two readers walk it --- `docinfo.rs` to list the signature fields, `redact.rs`
//! to decide which fields a removal has to take with it --- and until this module
//! existed each carried its own traversal, its own node budget and its own name
//! for that budget.
//!
//! ## What is shared, and what deliberately is not
//!
//! The *plumbing* is shared and the *policy* is not, which is worth saying
//! plainly rather than discovering: [`Bounds`] has five fields and the two
//! callers set every one of them differently. That is not a shared rule with
//! knobs on it. It is one place where the pop-with-a-budget, resolve, name and
//! enqueue-the-kids sequence is written --- the sequence that a hostile `/Kids`
//! is aimed at --- with each caller keeping the predicate that is its own.
//!
//! The differences the knobs record were invisible while the two walks lived in
//! different files, and each is a decision rather than an oversight:
//!
//! - **Depth.** `docinfo` bounds a chain at eight and reports what it cut.
//!   `redact` bounds nothing but the node count, because a field the walk
//!   declines to reach is a value left in a redacted document --- the direction
//!   that costs a reader their redaction rather than a line in a properties
//!   panel.
//! - **Node budget.** 4,096 against 20,000, for the same asymmetry.
//! - **Cycles.** `redact` refuses to visit an id twice; `docinfo` does not, and
//!   relies on its depth bound instead.
//! - **Names.** Only `docinfo` wants them, and building one costs a `/T` read
//!   and a string per node.
//! - **Order.** `docinfo` reports in document order, so it pushes kids reversed
//!   onto a stack that pops them back. `redact` produces a set of ids to remove
//!   and pushes them forward.
//!
//! ## Two entry points, one loop
//!
//! [`walk`] starts at the catalog's `/AcroForm /Fields`; [`descend`] starts at
//! whatever roots it is handed, which is what a caller wants once it has decided
//! a field must go and needs the widgets under it. Both run the same loop, so a
//! bound applies to both and there is one place to change when it moves.
//!
//! ## The budget is charged on the pop
//!
//! Before anything is read, so a refusal costs one pop rather than one parse ---
//! and what is left in the queue is reported through [`Cut::dropped`] rather
//! than dropped in silence. Stopping quietly reads as a form with nothing more
//! in it, which is the ordinary case and therefore the reassuring one.

use std::collections::HashSet;

use lopdf::{Dictionary, Document, Object, ObjectId};

/// Which way kids are pushed, and so the order nodes come back in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// Kids pushed reversed, so the stack pops them in the order the array
    /// lists them --- the order a reader sees the fields in every other
    /// application.
    Document,
    /// Kids pushed as they come, which pops the last one first.
    ///
    /// Not an aesthetic choice where it is used: [`crate::redact`] produces a
    /// set of object ids to remove, so the order it finds them in is not
    /// observable, and matching what its own loop did is what keeps this a move
    /// rather than a change.
    Queue,
}

/// How far a walk may go, and what it should build on the way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    /// Most entries popped before the walk gives up.
    ///
    /// A bound on the work, not on the tree, and it is the one a depth bound
    /// does not give: depth stops a chain and neither stops *fan-out*, so a
    /// node whose `/Kids` names itself sixty-four times costs 64^8 pops inside
    /// a bound of eight.
    pub nodes: usize,
    /// Deepest chain followed, or `None` for no depth bound at all.
    ///
    /// `Some(n)` means a node at depth `n` is read and its kids are not, which
    /// is counted in [`Cut::too_deep`].
    pub depth: Option<u32>,
    /// Whether an object id already visited is skipped.
    ///
    /// Only an id can be deduplicated. An entry written as a direct dictionary
    /// has no identity to compare, so it is always visited.
    pub dedup: bool,
    /// Whether to build each node's fully qualified name.
    pub names: bool,
    /// Which way kids are pushed.
    pub order: Order,
}

/// What a walk could not do.
///
/// Both fields are about the *tree*, never about one node's contents: a caller
/// that could not make sense of a dictionary it was handed counts that itself,
/// because only it knows what it was looking for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cut {
    /// Entries still queued when the node budget ran out, plus the one that
    /// exhausted it.
    pub dropped: usize,
    /// Nodes whose `/Kids` were not followed because the depth bound was
    /// reached.
    pub too_deep: usize,
}

/// One node of the field tree, as the walk found it.
pub struct Node<'a> {
    /// Its object id, when the entry was a reference rather than written out.
    pub id: Option<ObjectId>,
    /// Its dictionary, or `None` when the entry does not resolve to one.
    ///
    /// Handed over rather than counted here, so a caller decides in its own
    /// order whether an unreadable node is a limit it wants to report --- which
    /// is what keeps this a move rather than a rewrite of two callers' books.
    pub dict: Option<&'a Dictionary>,
    /// How many `/Kids` links were followed to reach it; zero at `/Fields`.
    pub depth: u32,
    /// Its fully qualified name, or empty when [`Bounds::names`] is off.
    pub name: String,
}

/// What the walk should do with a node's kids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Follow `/Kids`, subject to the depth bound.
    Descend,
    /// Stop at this node.
    Leaf,
}

/// A queued entry: an id to fetch, or an object written out in place.
enum Entry<'a> {
    At(ObjectId),
    Inline(&'a Object),
}

/// The catalog's `/AcroForm /Fields` array, or `None`.
///
/// `dereference` rather than one hop, because it is what both callers' own
/// lookups did between them and it is the stronger of the two: a reference
/// naming a reference is legal and pathological, and lopdf bounds the chase.
#[must_use]
pub fn fields_of(doc: &Document) -> Option<&Vec<Object>> {
    let form = doc
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"AcroForm").ok())
        .and_then(|object| doc.dereference(object).map(|(_, object)| object).ok())
        .and_then(|object| object.as_dict().ok())?;
    form.get(b"Fields")
        .and_then(|object| doc.dereference(object).map(|(_, object)| object))
        .and_then(Object::as_array)
        .ok()
}

/// Walks the form's field tree, calling `visit` for each node.
///
/// Answers an empty [`Cut`] and calls nothing when the document declares no
/// form: a document without one is not a document whose form could not be read.
pub fn walk<F>(doc: &Document, bounds: &Bounds, visit: F) -> Cut
where
    F: FnMut(Node<'_>) -> Flow,
{
    let Some(fields) = fields_of(doc) else {
        return Cut::default();
    };
    run(doc, fields.iter().map(Entry::Inline), bounds, visit)
}

/// Walks the subtrees under `roots`, calling `visit` for each node.
///
/// The roots themselves are visited. Used once a caller has decided a field
/// goes and needs everything hanging under it.
pub fn descend<F>(doc: &Document, roots: &[ObjectId], bounds: &Bounds, visit: F) -> Cut
where
    F: FnMut(Node<'_>) -> Flow,
{
    run(doc, roots.iter().copied().map(Entry::At), bounds, visit)
}

/// The loop both entry points are.
fn run<'a, I, F>(doc: &'a Document, roots: I, bounds: &Bounds, mut visit: F) -> Cut
where
    I: Iterator<Item = Entry<'a>>,
    F: FnMut(Node<'_>) -> Flow,
{
    let mut cut = Cut::default();
    let mut queue: Vec<(Entry<'a>, u32, String)> =
        roots.map(|entry| (entry, 0u32, String::new())).collect();
    if bounds.order == Order::Document {
        queue.reverse();
    }

    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut budget = bounds.nodes;

    while let Some((entry, depth, prefix)) = queue.pop() {
        let Some(left) = budget.checked_sub(1) else {
            cut.dropped += queue.len() + 1;
            break;
        };
        budget = left;

        let id = match &entry {
            Entry::At(id) => Some(*id),
            Entry::Inline(object) => object.as_reference().ok(),
        };
        if bounds.dedup {
            if let Some(id) = id {
                if !seen.insert(id) {
                    continue;
                }
            }
        }

        let dict = match &entry {
            Entry::At(id) => doc.get_dictionary(*id).ok(),
            Entry::Inline(object) => doc
                .dereference(object)
                .map(|(_, object)| object)
                .ok()
                .and_then(|object| object.as_dict().ok()),
        };

        let name = match (bounds.names, dict) {
            (true, Some(field)) => qualified_name(&prefix, &partial_name(doc, field)),
            _ => prefix,
        };

        let flow = visit(Node {
            id,
            dict,
            depth,
            name: name.clone(),
        });

        if flow == Flow::Leaf {
            continue;
        }
        let Some(field) = dict else { continue };
        let Ok(kids) = field
            .get(b"Kids")
            .and_then(|object| doc.dereference(object).map(|(_, object)| object))
            .and_then(Object::as_array)
        else {
            continue;
        };
        if bounds.depth.is_some_and(|max| depth >= max) {
            cut.too_deep += 1;
            continue;
        }
        let pushed: Vec<&Object> = match bounds.order {
            Order::Document => kids.iter().rev().collect(),
            Order::Queue => kids.iter().collect(),
        };
        for kid in pushed {
            queue.push((Entry::Inline(kid), depth + 1, name.clone()));
        }
    }

    cut
}

/// A field's own `/T`, decoded, or empty.
fn partial_name(doc: &Document, field: &Dictionary) -> String {
    field
        .get(b"T")
        .ok()
        .and_then(|object| crate::encoding::resolve(doc, object).as_str().ok())
        .map(crate::annots::decode_text_string)
        .unwrap_or_default()
}

/// A field's fully qualified name: its ancestors' partial names and its own,
/// joined with a period --- PDF 32000-1 §12.7.3.2.
///
/// A node with no `/T` contributes nothing and is **not** a level in the name,
/// which is what the specification says and is not merely tidier: a widget
/// annotation merged into its field is such a node, and so is the group a
/// document uses purely to hold kids together. Skipping them is what makes the
/// name Acrobat shows and the name reported here the same string.
#[must_use]
pub fn qualified_name(prefix: &str, partial: &str) -> String {
    match (prefix.is_empty(), partial.is_empty()) {
        (_, true) => prefix.to_string(),
        (true, false) => partial.to_string(),
        (false, false) => format!("{prefix}.{partial}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use lopdf::dictionary;

    /// Bounds that stop at nothing but the node count, for a test that is about
    /// the loop rather than about either caller's policy.
    fn loose(nodes: usize) -> Bounds {
        Bounds {
            nodes,
            depth: None,
            dedup: false,
            names: true,
            order: Order::Document,
        }
    }

    /// A document whose `/AcroForm /Fields` is `roots`.
    fn form(doc: &mut Document, roots: Vec<Object>) {
        let form = doc.add_object(dictionary! { "Fields" => roots });
        let pages =
            doc.add_object(dictionary! { "Type" => "Pages", "Kids" => vec![], "Count" => 0 });
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages,
            "AcroForm" => form,
        });
        doc.trailer.set("Root", catalog);
    }

    /// A `/Kids` naming an ancestor terminates, and says it ran out.
    ///
    /// **The bound is what terminates it, not a cycle check** --- these bounds
    /// have `dedup` off, which is `docinfo`'s setting, so the walk really does
    /// go round. What the test pins is that going round costs a fixed number of
    /// pops and is *reported*: a walk that stopped in silence would read as a
    /// form with nothing more in it, which is the ordinary case and therefore
    /// the one nobody questions.
    #[test]
    fn a_kids_naming_its_own_parent_terminates_and_the_walk_says_so() {
        let mut doc = Document::with_version("1.7");
        let parent = doc.new_object_id();
        doc.set_object(
            parent,
            Object::Dictionary(dictionary! {
                "T" => Object::string_literal("top"),
                "Kids" => vec![Object::Reference(parent)],
            }),
        );
        form(&mut doc, vec![Object::Reference(parent)]);

        let mut seen = 0usize;
        let cut = walk(&doc, &loose(16), |_| {
            seen += 1;
            Flow::Descend
        });

        assert_eq!(seen, 16, "the walk spends its whole budget going round");
        assert_eq!(
            cut.dropped, 1,
            "the pop that exhausted the budget is reported, and nothing was left queued behind it"
        );
    }

    /// A tree wider than the node bound stops, and reports what it did not walk.
    ///
    /// Fan-out rather than depth, which is the direction a depth bound does not
    /// cover: every node here is one link from the root.
    #[test]
    fn a_tree_over_the_node_bound_reports_what_it_did_not_reach() {
        let mut doc = Document::with_version("1.7");
        let kids: Vec<Object> = (0..12)
            .map(|n| {
                Object::Reference(doc.add_object(dictionary! {
                    "T" => Object::string_literal(format!("kid{n}")),
                }))
            })
            .collect();
        let parent = doc.add_object(dictionary! {
            "T" => Object::string_literal("top"),
            "Kids" => kids,
        });
        form(&mut doc, vec![Object::Reference(parent)]);

        let mut names: Vec<String> = Vec::new();
        let cut = walk(&doc, &loose(5), |node| {
            names.push(node.name);
            Flow::Descend
        });

        assert_eq!(
            names,
            vec!["top", "top.kid0", "top.kid1", "top.kid2", "top.kid3"],
            "document order, and the qualified name carries the chain"
        );
        // Five pops fit in the budget and the sixth did not. That sixth entry
        // is counted, and so are the seven still queued behind it.
        assert_eq!(
            cut.dropped, 8,
            "what is left in the queue is reported, not dropped in silence"
        );
    }

    /// The depth bound stops a chain and is counted separately from the budget.
    #[test]
    fn the_depth_bound_stops_a_chain_and_is_reported_on_its_own() {
        let mut doc = Document::with_version("1.7");
        let leaf = doc.add_object(dictionary! { "T" => Object::string_literal("c") });
        let middle = doc.add_object(dictionary! {
            "T" => Object::string_literal("b"),
            "Kids" => vec![Object::Reference(leaf)],
        });
        let top = doc.add_object(dictionary! {
            "T" => Object::string_literal("a"),
            "Kids" => vec![Object::Reference(middle)],
        });
        form(&mut doc, vec![Object::Reference(top)]);

        let mut names: Vec<String> = Vec::new();
        let cut = walk(
            &doc,
            &Bounds {
                depth: Some(1),
                ..loose(64)
            },
            |node| {
                names.push(node.name);
                Flow::Descend
            },
        );

        assert_eq!(names, vec!["a", "a.b"], "the node at the bound is read");
        assert_eq!(cut.too_deep, 1, "and its kids are reported as not walked");
        assert_eq!(cut.dropped, 0, "the node budget was never the limit here");
    }

    /// `dedup` visits a shared node once; without it the same node comes twice.
    ///
    /// Both directions, because a guard that is always on and a guard that is
    /// never on produce the same run on a tree with nothing shared in it.
    #[test]
    fn a_shared_node_is_visited_once_with_dedup_and_twice_without() {
        let mut doc = Document::with_version("1.7");
        let shared = doc.add_object(dictionary! { "T" => Object::string_literal("s") });
        let one = doc.add_object(dictionary! {
            "T" => Object::string_literal("a"),
            "Kids" => vec![Object::Reference(shared)],
        });
        let two = doc.add_object(dictionary! {
            "T" => Object::string_literal("b"),
            "Kids" => vec![Object::Reference(shared)],
        });
        form(
            &mut doc,
            vec![Object::Reference(one), Object::Reference(two)],
        );

        let count = |dedup: bool| {
            let mut n = 0usize;
            walk(&doc, &Bounds { dedup, ..loose(64) }, |node| {
                if node.id == Some(shared) {
                    n += 1;
                }
                Flow::Descend
            });
            n
        };

        assert_eq!(count(true), 1, "deduplicated");
        assert_eq!(count(false), 2, "and not, which is the other caller");
    }

    /// A document with no `/AcroForm` is not a form that could not be read.
    #[test]
    fn a_document_with_no_form_visits_nothing_and_reports_no_limit() {
        let mut doc = Document::with_version("1.7");
        let pages =
            doc.add_object(dictionary! { "Type" => "Pages", "Kids" => vec![], "Count" => 0 });
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);

        let mut seen = 0usize;
        let cut = walk(&doc, &loose(64), |_| {
            seen += 1;
            Flow::Descend
        });
        assert_eq!(seen, 0);
        assert_eq!(cut, Cut::default());
    }

    /// A node with no `/T` is not a level of the name.
    #[test]
    fn a_node_with_no_partial_name_contributes_nothing_to_the_chain() {
        let mut doc = Document::with_version("1.7");
        let leaf = doc.add_object(dictionary! { "T" => Object::string_literal("leaf") });
        let group = doc.add_object(dictionary! { "Kids" => vec![Object::Reference(leaf)] });
        let top = doc.add_object(dictionary! {
            "T" => Object::string_literal("top"),
            "Kids" => vec![Object::Reference(group)],
        });
        form(&mut doc, vec![Object::Reference(top)]);

        let mut names: Vec<String> = Vec::new();
        walk(&doc, &loose(64), |node| {
            names.push(node.name);
            Flow::Descend
        });
        assert_eq!(names, vec!["top", "top", "top.leaf"]);
    }

    /// `Flow::Leaf` stops the walk at that node rather than the whole walk.
    #[test]
    fn a_leaf_verdict_skips_the_kids_and_not_the_siblings() {
        let mut doc = Document::with_version("1.7");
        let hidden = doc.add_object(dictionary! { "T" => Object::string_literal("hidden") });
        let closed = doc.add_object(dictionary! {
            "T" => Object::string_literal("closed"),
            "Kids" => vec![Object::Reference(hidden)],
        });
        let after = doc.add_object(dictionary! { "T" => Object::string_literal("after") });
        form(
            &mut doc,
            vec![Object::Reference(closed), Object::Reference(after)],
        );

        let mut names: Vec<String> = Vec::new();
        walk(&doc, &loose(64), |node| {
            names.push(node.name.clone());
            if node.name == "closed" {
                Flow::Leaf
            } else {
                Flow::Descend
            }
        });
        assert_eq!(names, vec!["closed", "after"]);
    }

    /// [`descend`] runs the same loop from roots the caller names.
    #[test]
    fn descend_starts_where_it_is_told_and_takes_the_root_with_it() {
        let mut doc = Document::with_version("1.7");
        let leaf = doc.add_object(dictionary! { "T" => Object::string_literal("w") });
        let field = doc.add_object(dictionary! {
            "T" => Object::string_literal("f"),
            "Kids" => vec![Object::Reference(leaf)],
        });
        // Deliberately not reachable from any `/AcroForm`: `descend` answers
        // about the roots it is handed, which is the whole difference.
        let mut ids: Vec<ObjectId> = Vec::new();
        let cut = descend(&doc, &[field], &loose(64), |node| {
            if let Some(id) = node.id {
                ids.push(id);
            }
            Flow::Descend
        });
        assert_eq!(ids, vec![field, leaf]);
        assert_eq!(cut, Cut::default());
    }
}
