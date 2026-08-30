//! Bringing another document's pages into this one.
//!
//! Two operations, and the second is the general one. [`import`] takes the pages
//! of a second [`Document`] that a caller names and adds them to the first as
//! objects, answering where each one landed; [`append`] asks for all of them and
//! hangs the answer off the page tree, which is what *Merge documents* is made
//! of. It is the piece nothing else here does --- every write path before it
//! produces a subset or a permutation of **one** file's pages, so `save.rs`'s
//! whole vocabulary is positions into a single object graph and `pagetree.rs`'s
//! is surgery within one tree.
//!
//! **The selection is what inserting pages from another file needs**, and it is
//! built here, under merge, for the reason `docs/PLAN.md` gives for building the
//! blank page before the imported one: an increment carrying a model change, a
//! second worker pool, undo identity and a graph import at once says very little
//! about which of the four is wrong.
//!
//! Three things make it more than a copy of a `BTreeMap`, and each is a way the
//! obvious version is silently wrong rather than a way it fails.
//!
//! **Object numbers collide.** Both documents number from 1, so every id of the
//! incoming one is shifted past the highest the destination holds, and every
//! reference inside every incoming object is shifted with it. A reference that
//! is missed does not dangle --- it resolves, to whatever the destination
//! happens to have at that number, which is a page's font becoming another
//! document's content stream.
//!
//! **A page inherits from the node it hangs under**, and it is about to hang
//! somewhere else. [`pagetree::reorder_pages`] states the same problem for a
//! permutation within one file; across files it is worse, because the two trees
//! are unrelated and there is no value the destination's root could be carrying
//! that would happen to be right. [`pagetree::detached_page`] is that half.
//!
//! **`/Parent` points up**, so a walk that collects what a page needs by
//! following its references reaches the tree above it, then the catalog, then
//! every other page, the outline and the form fields --- the whole file, by
//! definition, for any page of it. The walk here starts from page dictionaries
//! whose `/Parent` has already been removed, which is what bounds it: the only
//! way out of an orphaned page is downward.
//!
//! ⚠ **That bound was free while every page came across, and it is a rule the
//! moment some of them do not.** A `/Dest` or a `/Link` from a page you asked
//! for to a page you did not reaches that page *with its `/Parent` still on
//! it*, and the walk then climbs out through the tree node into the rest of the
//! file --- so asking for fewer pages makes the walk reach more. [`import`]
//! states it as `left`, the pages staying behind, which the walk steps over
//! rather than into. `append` passes an empty one, which is why its own tests
//! are the control for everything here and say nothing at all about this.
//!
//! ## What is left behind, and why it is not a defect to be fixed later
//!
//! The incoming document's catalog, page tree nodes, outline, named
//! destinations, `/AcroForm`, attachments and metadata are not imported. A
//! merged file keeps the **destination's** outline, which still names its own
//! pages, and gains none from the files merged into it.
//!
//! That is a real loss and the command's own description says so rather than
//! leaving a reader to find out. It is also the honest boundary of what "merge"
//! can mean without a name-resolution pass: an outline entry, a link and a
//! named destination all address a page through one of four shapes
//! (`links.rs`'s resolver enumerates them), and carrying them across would mean
//! rewriting each into the merged file's own name space --- with two files
//! free to use the same name for different pages. `pagetree::drop_outline`
//! takes the same position for a deletion and for the same reason.
//!
//! **Intra-document links survive**, which is the part that is not obvious: a
//! `/Link` annotation whose `/Dest` names a page object keeps working, because
//! every page of the incoming file comes across and the reference is shifted
//! with everything else. What breaks is a destination reached *by name*.

use std::collections::{BTreeMap, HashSet};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::pagetree;
use crate::sweep;

/// Adds every page of `from` to the end of `into`, and says how many.
///
/// `into` is left holding both documents' pages in one tree; `from` is not
/// touched. Nothing about the destination's own pages changes --- not their
/// object numbers, not their order, not what they inherit --- which is what
/// makes this safe to run against bytes that some other part of the write path
/// has already produced.
///
/// The incoming pages are grafted directly onto the destination's root, beside
/// whatever nodes are already there. A tree whose `/Kids` mixes `/Page` and
/// `/Pages` entries is ordinary structure, so there is nothing to flatten and
/// the destination's own pages keep inheriting exactly what they did.
///
/// **`/Count` is set to the tree's own answer** rather than incremented. The
/// count a reader acts on is the number of leaves under the root, and adding to
/// a value that was already wrong preserves the error into a file that has just
/// been rewritten. `pagetree::drop_pages` decrements instead, and correctly: it
/// walks per page *number*, so the counts it adjusts are the ones it followed.
///
/// # Errors
///
/// `into` has no catalog or no page tree; `from` has no pages; an object nests
/// deeper than [`sweep::MAX_NESTING`]; or an object number would overflow.
pub fn append(into: &mut Document, from: &Document) -> Result<usize, String> {
    let root = into
        .catalog()
        .map_err(|e| format!("no document catalog: {e}"))?
        .get(b"Pages")
        .and_then(Object::as_reference)
        .map_err(|e| format!("no page tree: {e}"))?;

    // Asked here rather than left to [`import`], which asks the same question,
    // so that a document with nothing in it is refused in those words. `import`
    // would answer "no pages were asked for" for the empty range this builds,
    // which is true and describes the caller rather than the file.
    let pages = pagetree::ordered_pages(from);
    if pages.is_empty() {
        return Err("it has no pages".into());
    }

    let want: Vec<usize> = (0..pages.len()).collect();
    let kids = import(into, from, &want)?;
    graft(into, root, &kids)?;
    Ok(kids.len())
}

/// Brings the named pages of `from` into `into`, and says where they landed.
///
/// `want` is positions in `from`'s own page order, zero-based; the answer is the
/// object id each one has in `into`, in the order asked for. `from` is not
/// touched, and neither is anything the destination already held --- object
/// numbers, order, inheritance --- which is what makes this safe to run against
/// bytes some other part of the write path has produced.
///
/// **Nothing is put into the page tree.** The pages are objects of `into` and
/// are not in any `/Kids`, so a document is not readable until the caller places
/// them: [`append`] grafts them onto the root, and `save.rs`'s rewrite hands the
/// ids to `pagetree::materialise`, which is the only step that decides an order.
/// `/Parent` is written to the destination's root here even so --- not as a
/// placement, but because the alternative is an imported page pointing up into
/// a document it is no longer part of, and a tree rebuild overwrites it anyway.
///
/// **A page you did not ask for is a wall, not a door.** The walk that decides
/// what an incoming page needs cannot enter a page left behind, so a `/Dest` or
/// a `/Link` naming one is shifted with everything else and left dangling. That
/// is the module's existing position --- a destination reached by name does not
/// survive a merge --- arriving for a page reached by *reference*, and it is the
/// only shape available: importing the page would bring content the caller did
/// not ask for, which is the leak `docs/TRAPS.md` records under a shipped
/// extract carrying all eight pages' streams.
///
/// # Errors
///
/// `into` has no catalog or no page tree; `from` has no pages; `want` is empty,
/// names a position `from` does not have, or names one twice; an object nests
/// deeper than [`sweep::MAX_NESTING`]; or an object number would overflow.
pub fn import(
    into: &mut Document,
    from: &Document,
    want: &[usize],
) -> Result<Vec<ObjectId>, String> {
    let root = into
        .catalog()
        .map_err(|e| format!("no document catalog: {e}"))?
        .get(b"Pages")
        .and_then(Object::as_reference)
        .map_err(|e| format!("no page tree: {e}"))?;

    let pages = pagetree::ordered_pages(from);
    if pages.is_empty() {
        return Err("it has no pages".into());
    }
    if want.is_empty() {
        return Err("no pages were asked for".into());
    }

    // The pages asked for, in the order asked for, before anything is walked.
    //
    // **A repeat is refused rather than served.** Two positions holding one page
    // *object* is the hazard `save.rs` refuses a mark on and `pagetree.rs`
    // refuses a deletion of --- a change to either position shows up at both.
    // Lifting this means copying the objects, which is a different operation
    // from importing them, so it is refused here rather than half-done.
    let mut taking: Vec<ObjectId> = Vec::with_capacity(want.len());
    for &at in want {
        let id = *pages.get(at).ok_or_else(|| {
            format!(
                "it has {} pages, so there is no page {}",
                pages.len(),
                at + 1
            )
        })?;
        if taking.contains(&id) {
            return Err(format!("page {} was asked for twice", at + 1));
        }
        taking.push(id);
    }

    // Orphaned before anything is walked or written. `detached` is what the walk
    // below reads instead of `from.objects` for these ids, and that substitution
    // *is* the bound on the walk --- see the module note.
    let mut detached: BTreeMap<ObjectId, Dictionary> = BTreeMap::new();
    for &page in &taking {
        detached.insert(page, pagetree::detached_page(from, page)?);
    }

    // Every page of `from` that is staying behind. The walk stops at these; see
    // the note above about a wall.
    let left: HashSet<ObjectId> = pages
        .iter()
        .copied()
        .filter(|id| !detached.contains_key(id))
        .collect();

    // Read off the objects rather than taken from `max_id`, and the two can
    // differ: `lopdf` sets `max_id` from the cross-reference table's `/Size`,
    // which a producer is free to understate. A shift that is one too small
    // does not fail --- it overwrites a destination object with an incoming
    // one, which is the collision this whole function is arranged to avoid.
    let by = highest(into);

    // Sorted, so that a document with two objects that both refuse produces the
    // same message twice running. A `HashSet`'s order is not a fact about the
    // input, and a failure that moves is one nobody can reproduce.
    let mut needed: Vec<ObjectId> = needed(from, &detached, &left)?.into_iter().collect();
    needed.sort_unstable();

    for id in needed {
        let object = match detached.get(&id) {
            Some(dictionary) => {
                let mut dictionary = shifted_dictionary(dictionary, by, 0)?;
                // The destination's root, not the shifted source's: the tree
                // above this page is not coming with it.
                dictionary.set("Parent", Object::Reference(root));
                Object::Dictionary(dictionary)
            }
            None => match from.objects.get(&id) {
                Some(object) => shifted(object, by, 0)?,
                // A dangling reference, which `sweep::reachable` also steps
                // over: it names no object, so it carries no content. Left
                // dangling rather than repaired --- shifting it would be a
                // guess, and dropping the reference that names it would mean
                // editing an object the reader did not ask to change.
                None => continue,
            },
        };
        into.objects.insert(shifted_id(id, by)?, object);
    }

    into.max_id = highest(into);

    taking
        .iter()
        .map(|&id| shifted_id(id, by))
        .collect::<Result<_, _>>()
}

/// The highest object number `doc` holds, or claims to.
///
/// Both, deliberately: the objects are what a collision would be with, and
/// `max_id` is what `/Size` is written from, so a shift that respects only one
/// of them is wrong in one of the two directions.
fn highest(doc: &Document) -> u32 {
    doc.objects
        .keys()
        .map(|id| id.0)
        .max()
        .unwrap_or(0)
        .max(doc.max_id)
}

/// Every object of `from` that the detached pages reach.
///
/// [`sweep::reachable`]'s shape, with two substitutions that change what it
/// means. An id in `detached` is walked as the **orphaned** dictionary rather
/// than as the one in the document, so the `/Parent` that would lead up out of
/// the page is not there to follow; every page taken is a seed, so a `/Dest`
/// naming another page that was taken finds it --- and finds the orphan, so it
/// cannot escape that way either.
///
/// `left` is every page of `from` that is **not** being taken, and the walk
/// steps over one rather than into it. Without that, one `/Dest` from a page
/// somebody asked for to a page they did not would import that page as an
/// object, with its resources and its content stream behind it: a file
/// reporting the pages that were asked for while carrying the text of one that
/// was not. When [`append`] takes the whole document `left` is empty and this
/// is the walk it always was, which is what makes the existing merge tests the
/// control for it.
///
/// # Errors
///
/// As [`sweep::references`]: an object nesting deeper than
/// [`sweep::MAX_NESTING`].
fn needed(
    from: &Document,
    detached: &BTreeMap<ObjectId, Dictionary>,
    left: &HashSet<ObjectId>,
) -> Result<HashSet<ObjectId>, String> {
    let mut seen: HashSet<ObjectId> = detached.keys().copied().collect();
    let mut queue: Vec<ObjectId> = seen.iter().copied().collect();
    while let Some(id) = queue.pop() {
        let mut referenced = Vec::new();
        match detached.get(&id) {
            // Cloned into an `Object` rather than iterated, so that the depth
            // this walk counts is the depth `sweep::reachable` counts for the
            // trailer --- which it clones for the same reason. A dictionary
            // walked value by value is one level shallower, and a bound that
            // differs between two walks of the same graph is a bound nobody can
            // reason about.
            Some(dictionary) => {
                sweep::references(&Object::Dictionary(dictionary.clone()), &mut referenced)?;
            }
            None => match from.objects.get(&id) {
                Some(object) => sweep::references(object, &mut referenced)?,
                None => continue,
            },
        }
        for id in referenced {
            // Before `seen`, so that a page left behind is never added to the
            // answer at all --- it is not merely un-walked, it is not imported.
            if left.contains(&id) {
                continue;
            }
            if seen.insert(id) {
                queue.push(id);
            }
        }
    }
    Ok(seen)
}

/// An object number moved past everything the destination holds.
///
/// The generation is kept rather than zeroed. Two objects may share a number and
/// differ in generation, and collapsing them would silently make one of them the
/// other --- the same collision this shift exists to prevent, arriving from
/// inside.
///
/// # Errors
///
/// The number would overflow `u32`, which means one of the two documents is
/// numbered near the top of the range. Refused rather than wrapped: a wrapped
/// number is a collision with a low-numbered object, i.e. the catalog.
fn shifted_id(id: ObjectId, by: u32) -> Result<ObjectId, String> {
    match id.0.checked_add(by) {
        Some(number) => Ok((number, id.1)),
        None => Err(format!(
            "object {} cannot be renumbered past {by} without overflowing",
            id.0
        )),
    }
}

/// `object` with every reference in it shifted by `by`.
///
/// Depth-bounded for [`sweep::descend`]'s reason and against the same constant:
/// this runs in the app process on a graph we did not write, and it is
/// recursive. A document `lopdf` will load cannot reach the bound --- its own
/// parser stops at 100 levels --- so this guards a change to that constant
/// rather than any input.
fn shifted(object: &Object, by: u32, depth: usize) -> Result<Object, String> {
    if depth > sweep::MAX_NESTING {
        return Err(format!(
            "an object nests deeper than {} levels",
            sweep::MAX_NESTING
        ));
    }
    Ok(match object {
        Object::Reference(id) => Object::Reference(shifted_id(*id, by)?),
        Object::Array(items) => Object::Array(
            items
                .iter()
                .map(|item| shifted(item, by, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        Object::Dictionary(dictionary) => {
            Object::Dictionary(shifted_dictionary(dictionary, by, depth + 1)?)
        }
        Object::Stream(stream) => {
            // Cloned whole and then rewritten, so that everything a stream
            // carries beside its dictionary comes with it --- the bytes, and
            // `allows_compression`, which decides whether a font stream survives
            // a later `compress()`.
            let mut stream = stream.clone();
            stream.dict = shifted_dictionary(&stream.dict, by, depth + 1)?;
            // The position it was parsed from, which is a fact about the file it
            // came out of and is about to be false. `lopdf` reads it back when
            // it decodes a stream whose `/Length` is an indirect reference.
            stream.start_position = None;
            Object::Stream(stream)
        }
        other => other.clone(),
    })
}

/// [`shifted`] for a dictionary, which is the only shape with two callers.
fn shifted_dictionary(
    dictionary: &Dictionary,
    by: u32,
    depth: usize,
) -> Result<Dictionary, String> {
    let mut out = Dictionary::new();
    for (key, value) in dictionary.iter() {
        out.set(key.to_vec(), shifted(value, by, depth + 1)?);
    }
    Ok(out)
}

/// Hangs `kids` off the end of the page tree root.
///
/// # Errors
///
/// The root is not a dictionary, or its `/Kids` is missing or not an array ---
/// which is a document with no pages, and the destination always has some.
fn graft(into: &mut Document, root: ObjectId, kids: &[ObjectId]) -> Result<(), String> {
    let tree = into
        .get_object_mut(root)
        .and_then(Object::as_dict_mut)
        .map_err(|e| format!("the page tree root is not a dictionary: {e}"))?;
    let mut order = tree
        .get(b"Kids")
        .and_then(Object::as_array)
        .map_err(|e| format!("the page tree root has no kids: {e}"))?
        .clone();
    order.extend(kids.iter().map(|&id| Object::Reference(id)));
    tree.set("Kids", order);
    // After the graft, and from the tree rather than from the old value --- see
    // the note on `append`.
    let count = into.get_pages().len() as i64;
    into.get_object_mut(root)
        .and_then(Object::as_dict_mut)
        .map_err(|e| format!("the page tree root is not a dictionary: {e}"))?
        .set("Count", count);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{append, highest, import, needed, shifted, shifted_id};
    use lopdf::{dictionary, Document, Object, Stream};
    use std::collections::{BTreeMap, HashSet};

    /// A one-page document whose page states everything itself.
    ///
    /// Built rather than parsed, for the reason `sweep.rs`'s nesting test gives:
    /// what is under test is the graph arithmetic, and a fixture read off disk
    /// would put a parser between the assertion and the thing it is about.
    fn flat(text: &str) -> Document {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
        let content = doc.add_object(Stream::new(dictionary! {}, text.as_bytes().to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        doc
    }

    /// A three-page document whose first page links to its **third**.
    ///
    /// The discriminating fixture for a selection, and it needs all three pages
    /// to be one: page 2 is the page nobody asks for and nothing points at, so
    /// its absence says the walk took only what was seeded; page 3 is the page
    /// nobody asks for and page 1 *does* point at, so its absence says the walk
    /// stopped at a page rather than stepping through it. A two-page fixture
    /// collapses those into one subject and either rule alone would pass.
    ///
    /// Each page carries its own content stream, and the text is what the
    /// assertions look for --- an object id would be a fact about this
    /// document's numbering, which is exactly what an import changes.
    fn linked() -> Document {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });

        let mut pages = Vec::new();
        for text in ["one", "two", "three"] {
            let content = doc.add_object(Stream::new(dictionary! {}, text.as_bytes().to_vec()));
            pages.push(doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }));
        }

        // First page to last, by reference: the door the walk is allowed to
        // open, leading somewhere it is not allowed to go.
        let link = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Link",
            "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "Dest" => vec![pages[2].into(), "Fit".into()],
        });
        doc.get_object_mut(pages[0])
            .and_then(Object::as_dict_mut)
            .expect("the first page")
            .set("Annots", vec![link.into()]);

        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => pages.iter().map(|&id| id.into()).collect::<Vec<Object>>(),
                "Count" => 3,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        doc
    }

    /// The bytes of every stream in `doc`, which is what a page's text is.
    ///
    /// The instrument for "did this page's content come across", and it reads
    /// the **objects** rather than the page tree on purpose: a leaked stream is
    /// one no page names, so anything that walked the tree to find it would be
    /// unable to see the defect it exists to catch. `docs/TRAPS.md` records that
    /// exact shape under an extract that reported one page and carried eight.
    fn streams(doc: &Document) -> Vec<Vec<u8>> {
        doc.objects
            .values()
            .filter_map(|object| object.as_stream().ok())
            .map(|stream| stream.content.clone())
            .collect()
    }

    /// What the page at `id` draws, read through its `/Contents`.
    fn text_of(doc: &Document, id: lopdf::ObjectId) -> Vec<u8> {
        let contents = doc
            .get_object(id)
            .and_then(Object::as_dict)
            .expect("an imported page")
            .get(b"Contents")
            .and_then(Object::as_reference)
            .expect("its contents");
        doc.get_object(contents)
            .and_then(Object::as_stream)
            .expect("the stream")
            .content
            .clone()
    }

    /// A one-page document whose page inherits its box and resources.
    ///
    /// The discriminating fixture: everything the page needs to lay out is
    /// stated on the tree node above it, so a copy that takes the page alone
    /// produces a page with no size. Nothing else in this module can tell the
    /// two apart.
    fn inheriting() -> Document {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
        });
        let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F9" => font } });
        let content = doc.add_object(Stream::new(dictionary! {}, b"inherited".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 200.into(), 400.into()],
                "Rotate" => 90,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        doc
    }

    #[test]
    fn a_merge_holds_both_documents_pages_in_order() {
        let mut into = flat("first");
        let added = append(&mut into, &flat("second")).expect("append");
        assert_eq!(added, 1);
        let pages: Vec<_> = into.get_pages().into_values().collect();
        assert_eq!(pages.len(), 2, "both documents' pages");
        let text = |at: usize| {
            let page = into.get_dictionary(pages[at]).expect("page");
            let stream = page
                .get(b"Contents")
                .and_then(Object::as_reference)
                .expect("contents");
            String::from_utf8(
                into.get_object(stream)
                    .and_then(Object::as_stream)
                    .expect("stream")
                    .content
                    .clone(),
            )
            .expect("utf-8")
        };
        assert_eq!(text(0), "first", "the destination's page stays first");
        assert_eq!(text(1), "second", "and the incoming one lands after it");
    }

    #[test]
    fn the_destinations_own_objects_are_untouched() {
        // The property that lets this run against bytes another path produced:
        // a merge adds, and a reference in the destination that moved would be
        // a defect no page count could see.
        let mut into = flat("first");
        let before = into.objects.clone();
        append(&mut into, &flat("second")).expect("append");
        for (id, object) in &before {
            // The page tree root is the one object a graft edits, by design.
            let root = into
                .catalog()
                .expect("catalog")
                .get(b"Pages")
                .and_then(Object::as_reference)
                .expect("tree");
            if *id == root {
                continue;
            }
            assert_eq!(
                into.objects.get(id),
                Some(object),
                "object {id:?} changed under the merge"
            );
        }
    }

    #[test]
    fn an_incoming_page_takes_what_it_inherited_with_it() {
        // The whole reason `detached_page` exists. Without it the merged page
        // has no `/MediaBox`, no `/Resources` and no `/Rotate` --- and inherits
        // the *destination's*, so it lays out at A4 with the wrong fonts rather
        // than failing.
        let mut into = flat("first");
        append(&mut into, &inheriting()).expect("append");
        let pages: Vec<_> = into.get_pages().into_values().collect();
        let page = into.get_dictionary(pages[1]).expect("page");
        assert_eq!(
            page.get(b"MediaBox")
                .and_then(Object::as_array)
                .expect("box")
                .len(),
            4,
            "the incoming page states its own box"
        );
        let box_pt: Vec<i64> = page
            .get(b"MediaBox")
            .and_then(Object::as_array)
            .expect("box")
            .iter()
            .map(|value| value.as_i64().expect("number"))
            .collect();
        assert_eq!(
            box_pt,
            vec![0, 0, 200, 400],
            "its own, not the destination's"
        );
        assert_eq!(
            page.get(b"Rotate")
                .and_then(Object::as_i64)
                .expect("rotate"),
            90
        );
        assert!(
            page.has(b"Resources"),
            "and the resources it was laid out with"
        );
    }

    #[test]
    fn an_incoming_page_hangs_off_the_destinations_root() {
        let mut into = flat("first");
        append(&mut into, &flat("second")).expect("append");
        let root = into
            .catalog()
            .expect("catalog")
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("tree");
        let pages: Vec<_> = into.get_pages().into_values().collect();
        // Before the loop, or the loop has nothing to look at. A merge that
        // grafts nothing leaves one page whose parent is already the root, so
        // this passed with the graft deleted --- a check whose domain is "the
        // pages the tree yields" cannot see a page that never joined it.
        assert_eq!(
            pages.len(),
            2,
            "both pages are in the tree to be asked about"
        );
        for page in pages {
            let parent = into
                .get_dictionary(page)
                .expect("page")
                .get(b"Parent")
                .and_then(Object::as_reference)
                .expect("parent");
            assert_eq!(parent, root, "every page hangs off the one root");
        }
    }

    #[test]
    fn the_count_is_the_trees_own_answer() {
        let mut into = flat("first");
        // A destination whose count is already wrong. Incrementing would carry
        // the error into a file that has just been rewritten.
        let root = into
            .catalog()
            .expect("catalog")
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("tree");
        into.get_object_mut(root)
            .and_then(Object::as_dict_mut)
            .expect("tree")
            .set("Count", 7);
        append(&mut into, &flat("second")).expect("append");
        let count = into
            .get_dictionary(root)
            .expect("tree")
            .get(b"Count")
            .and_then(Object::as_i64)
            .expect("count");
        assert_eq!(count, 2, "the leaves under the root, not 7 plus one");
    }

    #[test]
    fn a_document_with_no_pages_is_refused() {
        let mut into = flat("first");
        let empty = Document::with_version("1.7");
        let why = append(&mut into, &empty).expect_err("nothing to merge");
        assert!(why.contains("no pages"), "{why}");
    }

    #[test]
    fn shifting_moves_every_reference_by_the_same_amount() {
        let object = Object::Array(vec![
            Object::Reference((1, 0)),
            Object::Dictionary(dictionary! { "K" => Object::Reference((2, 5)) }),
        ]);
        let moved = shifted(&object, 40, 0).expect("shift");
        let items = moved.as_array().expect("array");
        assert_eq!(items[0].as_reference().expect("reference"), (41, 0));
        assert_eq!(
            items[1]
                .as_dict()
                .expect("dictionary")
                .get(b"K")
                .and_then(Object::as_reference)
                .expect("reference"),
            (42, 5),
            "the generation is kept, so two objects sharing a number stay two"
        );
    }

    #[test]
    fn a_number_that_would_overflow_is_refused_rather_than_wrapped() {
        // Wrapping lands on a low number, which is the catalog.
        let why = shifted_id((u32::MAX, 0), 1).expect_err("overflow");
        assert!(why.contains("overflow"), "{why}");
    }

    #[test]
    fn the_shift_clears_a_streams_recorded_position() {
        let stream = Object::Stream(Stream::new(dictionary! { "Length" => 4 }, b"data".to_vec()));
        let moved = shifted(&stream, 10, 0).expect("shift");
        assert!(
            moved.as_stream().expect("stream").start_position.is_none(),
            "a position in the file it came from is about to be false"
        );
    }

    #[test]
    fn the_walk_does_not_leave_the_page_it_started_from() {
        // The bound, stated as what it excludes. `from`'s catalog and tree node
        // are reachable from the page through `/Parent` and are not in the set
        // --- if they were, every page of every merged file would come with its
        // whole document behind it.
        let from = inheriting();
        let pages = crate::pagetree::ordered_pages(&from);
        let mut detached = BTreeMap::new();
        for &page in &pages {
            detached.insert(
                page,
                crate::pagetree::detached_page(&from, page).expect("detach"),
            );
        }
        let reached = needed(&from, &detached, &HashSet::new()).expect("walk");
        let catalog = from
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .expect("root");
        let tree = from
            .catalog()
            .expect("catalog")
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("tree");
        assert!(!reached.contains(&catalog), "the catalog is not the page's");
        assert!(!reached.contains(&tree), "nor is the node it hung under");
        assert!(reached.contains(&pages[0]), "the page itself is");
        // And the control: the walk is not simply empty. The font is three
        // references down --- page, resources, font --- so reaching it says the
        // descent works rather than that it refused everything.
        let resources = crate::pagetree::detached_page(&from, pages[0])
            .expect("detach")
            .get(b"Resources")
            .and_then(Object::as_reference)
            .expect("resources");
        assert!(reached.contains(&resources), "what it does need is reached");
    }

    #[test]
    fn the_shift_clears_the_destination_by_the_number_it_actually_holds() {
        // `max_id` understated, which a producer is free to do. A shift that
        // trusted it would overwrite live objects rather than fail.
        let mut into = flat("first");
        let real = into.objects.keys().map(|id| id.0).max().expect("objects");
        into.max_id = 1;
        assert_eq!(highest(&into), real, "the objects decide, not the claim");
        let before = into.objects.len();
        append(&mut into, &flat("second")).expect("append");
        assert!(
            into.objects.len() > before,
            "nothing was overwritten on the way in"
        );
        assert_eq!(into.get_pages().len(), 2);
    }

    #[test]
    fn only_the_pages_asked_for_come_across() {
        let mut into = flat("first");
        let from = linked();

        let ids = import(&mut into, &from, &[1]).expect("the import");
        assert_eq!(ids.len(), 1, "one page was asked for");
        assert_eq!(text_of(&into, ids[0]), b"two".to_vec());

        // The assertion that matters, and it is about the objects rather than
        // about the pages: a page nobody asked for arrives as a stream no page
        // names, which reads as a correct one-page file and carries the text of
        // two others.
        let carried = streams(&into);
        assert!(
            carried.contains(&b"two".to_vec()),
            "the page asked for is here"
        );
        assert!(
            !carried.contains(&b"one".to_vec()),
            "the page before it is not, in any object"
        );
        assert!(
            !carried.contains(&b"three".to_vec()),
            "nor the page after it"
        );
        assert!(
            carried.contains(&b"first".to_vec()),
            "and the destination still has its own"
        );
    }

    #[test]
    fn the_answer_is_in_the_order_asked_for() {
        // Reversed, which is the only order this can be wrong in and still be a
        // permutation of the right ids --- an answer built from the document's
        // own page order rather than from `want` passes every other assertion
        // in this module.
        let mut into = flat("first");
        let from = linked();

        let ids = import(&mut into, &from, &[2, 0]).expect("the import");
        assert_eq!(ids.len(), 2);
        assert_eq!(text_of(&into, ids[0]), b"three".to_vec());
        assert_eq!(text_of(&into, ids[1]), b"one".to_vec());
    }

    #[test]
    fn a_page_left_behind_is_not_followed_into() {
        // Page 1 links to page 3. Taking page 1 alone must bring the annotation
        // --- it is part of the page --- and must not bring the page it names.
        let mut into = flat("first");
        let from = linked();

        let ids = import(&mut into, &from, &[0]).expect("the import");

        let annots = into
            .get_object(ids[0])
            .and_then(Object::as_dict)
            .expect("the imported page")
            .get(b"Annots")
            .and_then(Object::as_array)
            .expect("its annotations")
            .clone();
        assert_eq!(annots.len(), 1, "the link came with the page");
        let link = annots[0].as_reference().expect("a reference");
        let dest = into
            .get_object(link)
            .and_then(Object::as_dict)
            .expect("the link itself, so the walk did descend")
            .get(b"Dest")
            .and_then(Object::as_array)
            .expect("its destination")
            .clone();

        // The wall: the destination still names a page, and that page is not in
        // this document. Dangling rather than repaired --- see `import`.
        let named = dest[0].as_reference().expect("a page reference");
        assert!(
            into.get_object(named).is_err(),
            "the page it names did not come across"
        );
        assert!(
            !streams(&into).contains(&b"three".to_vec()),
            "and neither did its content stream"
        );
    }

    #[test]
    fn without_the_wall_one_page_reaches_the_whole_file() {
        // The measurement the barrier exists for, kept as an assertion because
        // the sentence in the module note --- asking for fewer pages makes the
        // walk reach more --- is the kind of claim that reads as plausible and
        // is worth a number. The walk is run with an empty `left`, which is
        // exactly what `append` passes, on a seed set of ONE page.
        let from = linked();
        let pages = crate::pagetree::ordered_pages(&from);
        let mut detached = BTreeMap::new();
        detached.insert(
            pages[0],
            crate::pagetree::detached_page(&from, pages[0]).expect("detach"),
        );

        let unwalled = needed(&from, &detached, &HashSet::new()).expect("walk");
        assert!(
            unwalled.contains(&pages[2]),
            "the page it links to comes across, which is the door"
        );
        assert!(
            unwalled.contains(&pages[1]),
            "and so does the page nothing points at, because the walk climbed \
             out through the tree node above the one it reached"
        );

        // With the wall, the same seed reaches neither.
        let left: HashSet<lopdf::ObjectId> = pages[1..].iter().copied().collect();
        let walled = needed(&from, &detached, &left).expect("walk");
        assert!(!walled.contains(&pages[1]));
        assert!(!walled.contains(&pages[2]));
        assert!(
            walled.len() < unwalled.len(),
            "and it is strictly smaller: {} against {}",
            walled.len(),
            unwalled.len()
        );
    }

    #[test]
    fn a_destination_naming_a_page_you_did_take_still_resolves() {
        // The other side of the wall, and without it the barrier is decoration:
        // a rule that stops at *every* page passes
        // `a_page_left_behind_is_not_followed_into` exactly as the right one
        // does, and quietly breaks the intra-document link the module note
        // promises survives a merge.
        let mut into = flat("first");
        let from = linked();

        let ids = import(&mut into, &from, &[0, 2]).expect("the import");
        let link = into
            .get_object(ids[0])
            .and_then(Object::as_dict)
            .expect("the first page")
            .get(b"Annots")
            .and_then(Object::as_array)
            .expect("its annotations")[0]
            .as_reference()
            .expect("a reference");
        let named = into
            .get_object(link)
            .and_then(Object::as_dict)
            .expect("the link")
            .get(b"Dest")
            .and_then(Object::as_array)
            .expect("its destination")[0]
            .as_reference()
            .expect("a page reference");

        assert_eq!(
            named, ids[1],
            "the destination names the page that came with it, at its new number"
        );
        assert_eq!(text_of(&into, named), b"three".to_vec());
    }

    #[test]
    fn an_imported_page_is_in_no_kids_until_someone_places_it() {
        // The control for the sentence in `import`'s doc comment, and the
        // counterpart of `an_incoming_page_hangs_off_the_destinations_root`,
        // which asserts the opposite for `append`. Without one of the two,
        // grafting and not grafting are indistinguishable.
        let mut into = flat("first");
        let from = linked();

        let ids = import(&mut into, &from, &[1]).expect("the import");
        assert_eq!(
            into.get_pages().len(),
            1,
            "the destination\u{2019}s own page"
        );

        let root = into
            .catalog()
            .expect("catalog")
            .get(b"Pages")
            .and_then(Object::as_reference)
            .expect("tree");
        let kids = into
            .get_object(root)
            .and_then(Object::as_dict)
            .expect("the root")
            .get(b"Kids")
            .and_then(Object::as_array)
            .expect("its kids")
            .clone();
        assert!(
            !kids
                .iter()
                .any(|kid| kid.as_reference().ok() == Some(ids[0])),
            "the imported page is an object of this document and in no tree"
        );

        // And it is genuinely there, or the assertion above holds for a page
        // that was never imported at all.
        assert_eq!(text_of(&into, ids[0]), b"two".to_vec());
    }

    #[test]
    fn a_position_the_document_does_not_have_is_refused() {
        let mut into = flat("first");
        let from = linked();
        let why = import(&mut into, &from, &[3]).expect_err("there is no page 4");
        assert!(why.contains("3 pages"), "{why}");
        assert!(why.contains("page 4"), "{why}");
    }

    #[test]
    fn the_same_page_asked_for_twice_is_refused() {
        let mut into = flat("first");
        let from = linked();
        let why = import(&mut into, &from, &[1, 1]).expect_err("one object, two places");
        assert!(why.contains("twice"), "{why}");
    }

    #[test]
    fn asking_for_no_pages_is_refused() {
        let mut into = flat("first");
        let from = linked();
        let why = import(&mut into, &from, &[]).expect_err("nothing was asked for");
        assert!(why.contains("no pages were asked for"), "{why}");
        // And it is not the message for a document with nothing in it, which is
        // a different fact and is `append`\u{2019}s.
        assert!(!why.contains("it has no pages"), "{why}");
    }
}
