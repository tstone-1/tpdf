//! The links in a document --- the rectangles a reader clicks to go somewhere.
//!
//! ## They are not decoration, and today they do nothing
//!
//! Measured on this machine before any of this was written: **16 of the 39 PDFs
//! in `~/Downloads` carry link annotations**, one of them 7,694 of them with
//! 6,617 pointing inside itself, and the EU packaging regulation 284. In those
//! documents a cross-reference, a table of contents entry and a footnote marker
//! are the navigation --- and in tpdf, before this module, clicking one did
//! nothing at all. That is a *reading* defect, which is why it lands in Phase 1
//! beside [`crate::annots`] rather than with the editing work.
//!
//! ## Why `lopdf`, when `outline.rs` resolves the same destinations via PDFium
//!
//! The same reason [`crate::annots`] gives: PDFium's `FPDFLink_*` accessors all
//! take an `FPDF_PAGE`, `FPDF_LoadPage` re-parses on every call at up to 44 ms
//! on a complex page (`docs/PLAN.md` §4), and the question here is about the
//! whole document at once. One `lopdf` parse answers it --- links live in the
//! same `/Annots` array the comment scan already walks.
//!
//! It costs a **second destination resolver**, and this repository has a trap
//! titled *"Two copies of a distinction drift, and a mutation of one survives"*
//! about exactly that. Two things hold it down. The refusal policy is not copied:
//! both produce [`crate::outline::Target`], so there is one type, one set of
//! variants and one test proving none of them can carry a URL. And the two
//! resolvers are compared against each other on a fixture built for it ---
//! `links.pdf` gives its outline entries the same destinations as its links, so
//! `links-probe` can put PDFium's answer beside this one and a disagreement is a
//! finding rather than something nobody would notice.
//!
//! ## A link carries no text, and that is structural rather than careful
//!
//! [`Link`] has a rectangle, a page and a [`Target`]. There is no field a string
//! from the document could occupy --- not the URL, not a title, not the action's
//! name, which is one of five literals chosen in `outline.rs`. So
//! `docs/THREAT-MODEL.md` T8 holds here by the shape of the type rather than by
//! a rule someone has to keep following, and `no_link_field_may_carry_a_url`
//! below is what says so.
//!
//! **A refused link deliberately does not show where it pointed.** That is a
//! decision, not an omission: a URL is text a stranger wrote, and a refusal
//! reading *"open https://your-bank.example/verify?"* is a better phishing
//! surface than no refusal at all. The reader is told what kind of action it was
//! and that tpdf declines it. Whether tpdf should ever open a web link --- and
//! how it would have to display one to do that safely --- is an open product
//! decision recorded in `docs/PLAN.md` §10.
//!
//! ## Every bound reports what it cut
//!
//! [`Limits`] again, for the reason `outline.rs` and `annots.rs` both state: an
//! answer truncated silently is indistinguishable from a complete one. Here it
//! bites harder than in either --- the 7,694-link document is real, and it is
//! over the per-document budget by itself.

use std::collections::{HashMap, HashSet};

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId};

use crate::encoding::resolve;
use crate::outline::Target;

use crate::encoding::MAX_DECODE;

/// Most links the scan will report for one page.
///
/// Higher than the comment budget because links are legitimately dense: an index
/// page or a table of authorities is one link per line, and there is no
/// per-link DOM row --- hit-testing walks an array.
const MAX_PER_PAGE: usize = 4_000;

/// Most links the scan will report for the whole document.
///
/// The 7,694-link thesis measured above is over this, and that is the intended
/// behaviour rather than a limit chosen not to bite: past it the reader is told
/// the list is incomplete instead of being handed a silent half of it. Raising
/// it is cheap; pretending the cut did not happen is not.
const MAX_TOTAL: usize = 20_000;

/// Deepest a `/Names` name tree is walked looking for a destination.
const MAX_TREE_DEPTH: usize = 32;

/// Most name-tree nodes visited resolving a single named destination.
///
/// A bound on the work, not on the tree: a hostile document can build a tree
/// whose nodes fan out far more than they narrow, and the walk would otherwise
/// be the document's to schedule.
const MAX_TREE_NODES: usize = 4_096;

/// Most outline entries [`outline_targets`] will resolve.
///
/// Matches `outline.rs`'s own item budget, so the two walks cut at the same
/// place --- a differential whose sides stop at different points reports the
/// difference between two bounds as a disagreement about destinations.
const MAX_OUTLINE_ITEMS: usize = 10_000;

/// One clickable rectangle, and where it goes.
///
/// No field here can hold a string from the file --- see the module note on T8.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Link {
    /// Index in [`Links::items`], stable for as long as the document is open.
    pub id: u32,
    /// Zero-based page.
    pub page: u32,
    /// `[left, top, right, bottom]` in points, from the displayed page's
    /// top-left --- the same space [`crate::annots::Comment::rect`] uses.
    pub rect: [f32; 4],
    /// Where it points, or why it points nowhere.
    pub target: Target,
}

/// What the bounds cut off, so the UI can say the list is incomplete.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Limits {
    /// Pages that had more links than [`MAX_PER_PAGE`].
    pub crowded_pages: usize,
    /// The document had more links than [`MAX_TOTAL`].
    pub over_budget: bool,
    /// `/Annots` entries that could not be read as an annotation dictionary.
    pub unreadable: usize,
    /// Named destinations the name tree could not resolve within its bounds.
    pub unresolved_names: usize,
    /// Pages PDFium has that `lopdf` could not account for.
    ///
    /// **The case this exists for is encryption**, and it is the quietest
    /// failure either scan has. A document encrypted with an empty user
    /// password opens with no prompt in any reader --- PDFium paginates it
    /// normally and the pages render --- while `lopdf` may parse the file and
    /// report **zero pages**. Every loop here then runs zero times and returns
    /// an empty list with no bound tripped, so a reader is told the document has
    /// no links when what happened is that nothing could look.
    ///
    /// `encoding.rs` already draws this distinction per page ("a page `lopdf`
    /// cannot account for is unknown, not clean"). This is the same distinction
    /// for a whole-document scan, and its absence is a truncated answer
    /// displayed as a complete one --- the failure every bound in this file is
    /// arranged to avoid.
    pub pages_missed: usize,
}

impl Limits {
    /// Whether anything was cut, which is what the UI asks.
    pub fn any(&self) -> bool {
        self.crowded_pages > 0
            || self.over_budget
            || self.unreadable > 0
            || self.unresolved_names > 0
            || self.pages_missed > 0
    }
}

/// Every link in a document, with what the scan could not do.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Links {
    pub items: Vec<Link>,
    pub limits: Limits,
    /// Time spent scanning, in milliseconds.
    pub scan_ms: f64,
}

/// Reads every link in a document.
///
/// # Errors
///
/// The bytes not parsing as a PDF, or a stream exceeding [`MAX_DECODE`]. A
/// failure is reported rather than answered with an empty list, for the reason
/// [`crate::annots::scan`] gives: "no links" and "could not be read" are
/// different things, and only one of them is reassuring.
///
/// **`password` is the reader's, when the document needed one, and it decides
/// whether this reads anything at all.** `lopdf` tries the empty password by
/// itself, so a permission-restricted document needs nothing here --- but one
/// behind a real password parses to a `Document` with **no objects in it**,
/// which loads cleanly and reports zero pages. Without the key that is
/// indistinguishable from a document that simply has none of what is being
/// looked for. See [`crate::progressive::RawDocument::password`].
pub fn scan(bytes: &[u8], page_count: usize, password: Option<&str>) -> Result<Links, String> {
    let started = std::time::Instant::now();
    let document = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            password: password.map(str::to_string),
            ..Default::default()
        },
    )
    .map_err(|e| format!("could not parse the document: {e}"))?;

    let mut limits = Limits::default();
    let mut items: Vec<Link> = Vec::new();

    // Page object to index, built once. Resolving a destination means turning a
    // page *reference* into the number the viewer scrolls to, and doing that by
    // searching the page list per link would be quadratic on exactly the
    // documents that have the most links.
    let pages = document.get_pages();
    // What PDFium can see and this cannot. Counted before anything is walked,
    // because the walk's own emptiness is exactly what it cannot distinguish.
    limits.pages_missed = page_count.saturating_sub(pages.len());
    let numbers: HashMap<ObjectId, u32> = pages
        .iter()
        .take(page_count)
        .map(|(number, id)| (*id, number.saturating_sub(1)))
        .collect();

    // Each page's displayed geometry, computed once and indexed by page number.
    // Resolving a destination needs the *destination* page's height to flip its
    // offset, which is rarely the page the link sits on --- looking that up by
    // searching the page list per link is quadratic, on precisely the documents
    // that have the most links.
    let geometry: Vec<PageBox> = pages
        .values()
        .take(page_count)
        .map(|page| page_geometry(&document, *page))
        .collect();

    for (index, page) in pages.values().take(page_count).enumerate() {
        if items.len() >= MAX_TOTAL {
            limits.over_budget = true;
            break;
        }
        read_page(
            &document,
            *page,
            index as u32,
            &numbers,
            &geometry,
            &mut items,
            &mut limits,
        );
    }

    Ok(Links {
        items,
        limits,
        scan_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

/// Reads one page's `/Annots`, keeping the links.
fn read_page(
    document: &Document,
    page: ObjectId,
    number: u32,
    numbers: &HashMap<ObjectId, u32>,
    geometry: &[PageBox],
    items: &mut Vec<Link>,
    limits: &mut Limits,
) {
    let Ok(dict) = document.get_dictionary(page) else {
        limits.unreadable += 1;
        return;
    };
    let Ok(annots) = dict.get(b"Annots") else {
        return;
    };
    // An indirect `/Annots` is ordinary, and a scan that did not resolve it
    // would report a page of links as having none.
    let Object::Array(entries) = resolve(document, annots) else {
        limits.unreadable += 1;
        return;
    };

    let shown = geometry.get(number as usize).copied().unwrap_or(PageBox {
        width: 612.0,
        height: 792.0,
        turns: 0,
        origin: (0.0, 0.0),
    });
    let mut on_this_page = 0usize;

    for entry in entries {
        if items.len() >= MAX_TOTAL {
            limits.over_budget = true;
            return;
        }
        if on_this_page >= MAX_PER_PAGE {
            limits.crowded_pages += 1;
            return;
        }

        let object = match entry {
            Object::Reference(id) => match document.get_object(*id) {
                Ok(object) => object,
                Err(_) => {
                    limits.unreadable += 1;
                    continue;
                }
            },
            other => other,
        };
        let Ok(annot) = object.as_dict() else {
            limits.unreadable += 1;
            continue;
        };

        match annot.get(b"Subtype").and_then(Object::as_name) {
            Ok(b"Link") => {}
            // Not a link, and not a defect: this array is shared with every
            // other annotation on the page.
            Ok(_) => continue,
            Err(_) => {
                limits.unreadable += 1;
                continue;
            }
        }

        // `/F` bit 2 is Hidden: the page does not show this annotation, so
        // there is nothing under the pointer to have been clicked. Dropped here
        // rather than carried with a flag --- which is what `annots.rs` does ---
        // because a hidden *comment* is still listed in the panel a reader
        // opened deliberately, and a hidden link has no panel to survive in.
        let hidden = annot
            .get(b"F")
            .ok()
            .and_then(|flags| resolve(document, flags).as_i64().ok())
            .is_some_and(|flags| flags & 0b10 != 0);
        if hidden {
            continue;
        }

        let rect = rect_of(annot, document, shown);
        // A zero-area rectangle is unclickable, so keeping it would put a link
        // in the list that no reader can ever reach and every hit test has to
        // walk past.
        if rect[2] - rect[0] <= 0.0 || rect[3] - rect[1] <= 0.0 {
            continue;
        }

        items.push(Link {
            id: items.len() as u32,
            page: number,
            rect,
            target: target_of(annot, document, numbers, geometry, limits),
        });
        on_this_page += 1;
    }
}

/// Resolves a link's destination, or records why it has none.
///
/// **The action is read first, and that ordering is the same one `outline.rs`
/// had to learn.** §12.3.3 says `/Dest` shall not be present when `/A` is, and a
/// document that writes both is telling a reader two things at once; taking the
/// action means a `/GoToR` is refused rather than having its `/D` array quietly
/// resolved against *this* document, which is the concrete bug PDFium's
/// `FPDFBookmark_GetDest` has and which was measured on the hostile outline
/// fixture. The failure is not a crash --- it is a jump to a plausible page of
/// the wrong file's numbering.
fn target_of(
    annot: &Dictionary,
    document: &Document,
    numbers: &HashMap<ObjectId, u32>,
    geometry: &[PageBox],
    limits: &mut Limits,
) -> Target {
    if let Ok(action) = annot.get(b"A") {
        let Ok(action) = resolve(document, action).as_dict() else {
            return Target::Broken;
        };
        let Ok(kind) = action.get(b"S").and_then(Object::as_name) else {
            return Target::Broken;
        };
        return match kind {
            b"GoTo" => match action.get(b"D") {
                Ok(dest) => destination(dest, document, numbers, geometry, limits),
                Err(_) => Target::Broken,
            },
            b"GoToR" => refused("remote"),
            b"URI" => refused("uri"),
            b"Launch" => refused("launch"),
            b"GoToE" => refused("embedded"),
            // Named actions (`/Named /NextPage`), JavaScript, form submission,
            // sound, movie, and anything a later specification adds. Declined by
            // default rather than by enumeration: an action tpdf has never heard
            // of is exactly the one not to follow.
            _ => refused("unsupported"),
        };
    }

    match annot.get(b"Dest") {
        Ok(dest) => destination(dest, document, numbers, geometry, limits),
        // A link rectangle with no action and no destination is legal and inert
        // --- it is how a producer marks a region it decided not to link.
        Err(_) => Target::None,
    }
}

/// Resolves a destination in any of the three forms a document may write it.
///
/// An array is the destination itself; a name or a byte string is a key into the
/// document's named destinations, and PDF has **two** of those, added in
/// different versions and both still in the wild --- `/Root /Dests`, whose keys
/// are names, and `/Root /Names /Dests`, whose keys are strings in a balanced
/// tree. A reader that knows only one of them silently fails to follow every
/// link in whichever half of the corpus uses the other.
fn destination(
    dest: &Object,
    document: &Document,
    numbers: &HashMap<ObjectId, u32>,
    geometry: &[PageBox],
    limits: &mut Limits,
) -> Target {
    match resolve(document, dest) {
        Object::Array(array) => place(array, document, numbers, geometry),
        Object::Name(name) => named(name, document, numbers, geometry, limits),
        Object::String(bytes, _) => named(bytes, document, numbers, geometry, limits),
        _ => Target::Broken,
    }
}

/// Looks a named destination up in both of the places one can live.
fn named(
    key: &[u8],
    document: &Document,
    numbers: &HashMap<ObjectId, u32>,
    geometry: &[PageBox],
    limits: &mut Limits,
) -> Target {
    let Ok(catalog) = document.catalog() else {
        return Target::Broken;
    };

    // The PDF 1.1 form first: a flat dictionary keyed by name. Cheap, and a
    // document carrying both should agree.
    if let Ok(dests) = catalog.get(b"Dests") {
        if let Ok(dict) = resolve(document, dests).as_dict() {
            if let Ok(found) = dict.get(key) {
                return follow(found, document, numbers, geometry);
            }
        }
    }

    // The 1.2 form: a name tree under `/Names`.
    let tree = catalog
        .get(b"Names")
        .ok()
        .and_then(|names| resolve(document, names).as_dict().ok())
        .and_then(|names| names.get(b"Dests").ok());
    let Some(tree) = tree else {
        // No named destinations anywhere. Broken rather than unresolved: the
        // document names a destination and then does not define the mechanism.
        return Target::Broken;
    };

    let mut budget = MAX_TREE_NODES;
    match walk_tree(tree, key, document, MAX_TREE_DEPTH, &mut budget) {
        Found::Value(object) => follow(&object, document, numbers, geometry),
        Found::Missing => Target::Broken,
        Found::Exhausted => {
            limits.unresolved_names += 1;
            Target::Broken
        }
    }
}

/// What a name-tree walk found, keeping "not there" apart from "gave up".
///
/// They produce the same [`Target`] and they are not the same event: one is the
/// document being wrong, the other is this scan declining to keep looking, and
/// only the second belongs in [`Limits`]. Collapsing them would make a bound
/// that fired look exactly like a broken link.
enum Found {
    Value(Object),
    Missing,
    Exhausted,
}

/// Walks a `/Names` tree for `key`.
///
/// The tree is sorted and the specification permits a binary search on `/Limits`;
/// this walks it linearly instead, because `/Limits` is written by the producer
/// and a hostile or merely buggy one that lies about a subtree's range would
/// make a binary search skip entries that are really there. The bound on nodes
/// is what keeps the linear walk from being the document's to schedule.
fn walk_tree(
    node: &Object,
    key: &[u8],
    document: &Document,
    depth: usize,
    budget: &mut usize,
) -> Found {
    if depth == 0 || *budget == 0 {
        return Found::Exhausted;
    }
    *budget -= 1;

    let Ok(dict) = resolve(document, node).as_dict() else {
        return Found::Missing;
    };

    if let Ok(names) = dict.get(b"Names") {
        if let Object::Array(pairs) = resolve(document, names) {
            // Alternating key, value. An odd-length array is a malformed node,
            // and `chunks_exact` drops the trailing key rather than pairing it
            // with whatever follows.
            for pair in pairs.chunks_exact(2) {
                let matches = match resolve(document, &pair[0]) {
                    Object::String(bytes, _) => bytes == key,
                    Object::Name(name) => name == key,
                    _ => false,
                };
                if matches {
                    return Found::Value(pair[1].clone());
                }
            }
        }
    }

    if let Ok(kids) = dict.get(b"Kids") {
        if let Object::Array(kids) = resolve(document, kids) {
            for kid in kids {
                match walk_tree(kid, key, document, depth - 1, budget) {
                    Found::Value(value) => return Found::Value(value),
                    Found::Exhausted => return Found::Exhausted,
                    Found::Missing => {}
                }
            }
        }
    }

    Found::Missing
}

/// A named destination's value, which is either the array or a dictionary
/// holding it under `/D`.
fn follow(
    value: &Object,
    document: &Document,
    numbers: &HashMap<ObjectId, u32>,
    geometry: &[PageBox],
) -> Target {
    match resolve(document, value) {
        Object::Array(array) => place(array, document, numbers, geometry),
        Object::Dictionary(dict) => match dict.get(b"D") {
            Ok(inner) => match resolve(document, inner) {
                Object::Array(array) => place(array, document, numbers, geometry),
                _ => Target::Broken,
            },
            Err(_) => Target::Broken,
        },
        _ => Target::Broken,
    }
}

/// Turns a destination array into a page and an offset down it.
///
/// The first element is the page: a reference in a same-document destination,
/// or --- in the remote form, which a file sometimes writes anyway --- a plain
/// integer index. The rest is the fit, and only some fits name a vertical
/// coordinate at all.
fn place(
    array: &[Object],
    document: &Document,
    numbers: &HashMap<ObjectId, u32>,
    geometry: &[PageBox],
) -> Target {
    let Some(first) = array.first() else {
        return Target::Broken;
    };

    // The page count is `geometry.len()` rather than a value passed beside it,
    // so a page this scan has no geometry for cannot also be a page it reports
    // as reachable --- the two facts come from one place and cannot disagree.
    let page_count = geometry.len() as u32;
    let page = match first {
        Object::Reference(id) => match numbers.get(id) {
            Some(number) => *number,
            None => return Target::Broken,
        },
        Object::Integer(index) => match u32::try_from(*index) {
            Ok(index) => index,
            _ => return Target::Broken,
        },
        _ => return Target::Broken,
    };
    if page >= page_count {
        return Target::Broken;
    }

    Target::Page {
        page,
        top_pt: top_of(array, document, geometry, page),
    }
}

/// The destination's y coordinate, flipped to points from the page's top.
///
/// PDF measures it upwards from the page's bottom-left and every consumer
/// downstream works from the top edge. Getting that backwards does not look like
/// a bug --- it still lands on the right *page*, at the mirror image of the
/// right place on it, which reads as a document whose links are slightly off.
///
/// **A rotated page returns `None`, and that is deliberate.** The destination is
/// written in the page's own unrotated space; under a quarter turn the display's
/// vertical axis is the page's horizontal one, so there is no offset to scroll
/// to. `outline.rs` reaches the same answer through the same reasoning, and the
/// honest interpretation of a destination with no coordinate is the page's top.
fn top_of(array: &[Object], document: &Document, geometry: &[PageBox], page: u32) -> Option<f32> {
    let fit = array.get(1).and_then(|object| match object {
        Object::Name(name) => Some(name.as_slice()),
        _ => None,
    })?;

    // Which element carries the top, per §12.3.2.2. `/Fit`, `/FitB` and the two
    // that name only an x carry none.
    let index = match fit {
        b"XYZ" => 3,
        b"FitH" | b"FitBH" => 2,
        b"FitR" => 5,
        _ => return None,
    };

    let raw = array
        .get(index)
        .map(|object| resolve(document, object))
        .and_then(|object| object.as_float().ok())
        .filter(|value| value.is_finite())?;

    // The *destination* page's height, which is rarely the page the link is on:
    // a document may mix sizes, and flipping an offset against the wrong one
    // lands somewhere plausible and wrong on the right page.
    let shown = *geometry.get(page as usize)?;
    if shown.turns != 0 {
        return None;
    }

    // Into crop space first: a destination is written in the page's own
    // coordinates, and the page is displayed from its crop box's corner.
    Some((shown.height - (raw - shown.origin.1)).clamp(0.0, shown.height))
}

/// Every outline entry's destination, resolved through `lopdf`, in tree order.
///
/// **This is an oracle, not a feature.** Nothing in the application calls it:
/// the outline a reader sees comes from `outline.rs` through PDFium, and this
/// exists so that `links-probe --mode agree` can put a second, independent
/// answer beside it.
///
/// The differential it enables already found one defect --- PDFium's
/// `FPDFDest_GetLocationInPage` answers only for `/XYZ`, so every `/FitH`
/// outline entry had been landing at the top of its page since `outline.rs` was
/// written --- but only on the one fixture whose manifest states what its
/// destinations should be. With both sides resolved from the file itself there
/// is nothing to state, so **any document with an outline becomes a test**: 421
/// entries across the PDFs on one machine, where the fixture offers six.
///
/// Order is pre-order --- an entry, then its children --- because that is what
/// `outline::read` produces when flattened, and a comparison of two lists in
/// different orders is a comparison of nothing.
///
/// The bounds match `outline.rs`'s and exist for the reason its own note gives:
/// PDFium's documentation says the caller must handle circular bookmark
/// references, and a `/Next` chain that loops is a hostile document's cheapest
/// trick. Here the visited set is what terminates it; the count is the backstop
/// for a producer that hands back a fresh object for a node already seen.
///
/// # Errors
///
/// The bytes not parsing as a PDF, or a stream exceeding [`MAX_DECODE`].
pub fn outline_targets(bytes: &[u8], page_count: usize) -> Result<Vec<Target>, String> {
    let document = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            ..Default::default()
        },
    )
    .map_err(|e| format!("could not parse the document: {e}"))?;

    let pages = document.get_pages();
    let numbers: HashMap<ObjectId, u32> = pages
        .iter()
        .take(page_count)
        .map(|(number, id)| (*id, number.saturating_sub(1)))
        .collect();
    let geometry: Vec<PageBox> = pages
        .values()
        .take(page_count)
        .map(|page| page_geometry(&document, *page))
        .collect();

    let Ok(catalog) = document.catalog() else {
        return Ok(Vec::new());
    };
    let Ok(Object::Reference(root)) = catalog.get(b"Outlines") else {
        return Ok(Vec::new());
    };
    let Ok(root_dict) = document.get_dictionary(*root) else {
        return Ok(Vec::new());
    };
    let Ok(Object::Reference(first)) = root_dict.get(b"First") else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut limits = Limits::default();
    walk_outline(
        &document,
        *first,
        &numbers,
        &geometry,
        &mut seen,
        &mut out,
        &mut limits,
        MAX_TREE_DEPTH,
    );
    Ok(out)
}

/// Walks a sibling chain and everything under it, pre-order.
#[allow(clippy::too_many_arguments)]
fn walk_outline(
    document: &Document,
    first: ObjectId,
    numbers: &HashMap<ObjectId, u32>,
    geometry: &[PageBox],
    seen: &mut HashSet<ObjectId>,
    out: &mut Vec<Target>,
    limits: &mut Limits,
    depth: usize,
) {
    if depth == 0 {
        return;
    }
    let mut node = Some(first);
    while let Some(id) = node {
        if out.len() >= MAX_OUTLINE_ITEMS || !seen.insert(id) {
            return;
        }
        let Ok(dict) = document.get_dictionary(id) else {
            return;
        };

        // The same precedence the link path uses, and for the same reason:
        // §12.3.3 says `/Dest` shall not be present when `/A` is, and taking the
        // action is what refuses a `/GoToR` instead of resolving its `/D`
        // against this document.
        out.push(target_of(dict, document, numbers, geometry, limits));

        if let Ok(Object::Reference(child)) = dict.get(b"First") {
            walk_outline(
                document,
                *child,
                numbers,
                geometry,
                seen,
                out,
                limits,
                depth - 1,
            );
        }
        node = match dict.get(b"Next") {
            Ok(Object::Reference(next)) => Some(*next),
            _ => None,
        };
    }
}

/// Builds a refusal, spelling out which action kind was declined.
///
/// The strings match `outline.rs`'s exactly, because the frontend has one
/// wording table for both and a sixth spelling here would render as the
/// fallback --- which reads as tpdf not knowing what it declined.
fn refused(kind: &str) -> Target {
    Target::Refused {
        action: kind.to_string(),
    }
}

/// A page's displayed size, its rotation, and where its own space starts.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PageBox {
    /// Displayed width in points, after `/Rotate`.
    width: f32,
    /// Displayed height in points, after `/Rotate`.
    height: f32,
    turns: u8,
    /// The lower-left corner of the box the page is displayed from.
    ///
    /// Zero for most documents. Not zero for one whose `/CropBox` is inset, and
    /// then it is the difference between a rectangle landing on its text and
    /// landing somewhere else entirely --- see [`page_geometry`].
    origin: (f32, f32),
}

/// The displayed page box, its rotation and its origin.
///
/// **The arithmetic moved to [`crate::pagetree::displayed_page`]** when the save
/// path needed it in order to place a highlight: it was here, duplicated in
/// [`crate::annots`], and a third copy is the drift `docs/TRAPS.md` records.
/// This wrapper stays so that the shape the rest of this module reads ---
/// [`PageBox`] --- is defined where it is used, and so that the change to a
/// shared implementation is one line rather than a rename across the file.
///
/// `annots.rs` deliberately keeps its own copy: `both_scans_agree_about_a_rotated_page`
/// compares the two answers on one document, and collapsing them would leave a
/// test that cannot fail.
fn page_geometry(document: &Document, page: ObjectId) -> PageBox {
    let shown = crate::pagetree::displayed_page(document, page);
    PageBox {
        width: shown.width,
        height: shown.height,
        turns: shown.turns,
        origin: shown.origin,
    }
}

/// A link's `/Rect`, normalised and mapped into display space.
///
/// The corners are sorted (§12.5.2 requires a consumer to normalise), non-finite
/// values collapse the rectangle rather than poisoning the layout, and the
/// result is clamped to the page. Same three reasons as
/// [`crate::annots`]'s, and through `text::to_device` for the same one: a page
/// carrying `/Rotate` is described in its own unrotated space, and a second
/// implementation of that turn is a second place to get it wrong.
fn rect_of(annot: &Dictionary, document: &Document, shown: PageBox) -> [f32; 4] {
    let Some(values) = annot
        .get(b"Rect")
        .ok()
        .and_then(|object| crate::pagetree::numbers_of(document, object, 4))
    else {
        return [0.0, 0.0, 0.0, 0.0];
    };
    if values.iter().any(|value| !value.is_finite()) {
        return [0.0, 0.0, 0.0, 0.0];
    }

    // Shifted into crop space before the turn: `to_device` works in the
    // displayed page's coordinates, and the displayed page starts at the crop
    // box's corner rather than at the media box's.
    let (ox, oy) = (shown.origin.0 as f64, shown.origin.1 as f64);
    let placed = crate::text::to_device(
        shown.turns,
        shown.width,
        shown.height,
        [
            values[0].min(values[2]) as f64 - ox,
            values[1].min(values[3]) as f64 - oy,
            values[0].max(values[2]) as f64 - ox,
            values[1].max(values[3]) as f64 - oy,
        ],
    );

    [
        placed[0].clamp(0.0, shown.width),
        placed[1].clamp(0.0, shown.height),
        placed[2].clamp(0.0, shown.width),
        placed[3].clamp(0.0, shown.height),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    /// A document with `pages` pages, each carrying the annotations given for it.
    ///
    /// Synthetic rather than a fixture on disk, because what is under test is
    /// how this reads an object graph --- and a graph built here can hold the
    /// shapes a producer would never write, which is where the defects are.
    fn build(pages: usize, annots: &[(usize, Vec<Dictionary>)]) -> (Document, Vec<ObjectId>) {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let mut ids = Vec::new();

        for index in 0..pages {
            let mut page = dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            };
            if let Some((_, list)) = annots.iter().find(|(at, _)| *at == index) {
                let refs: Vec<Object> = list
                    .iter()
                    .map(|annot| document.add_object(annot.clone()).into())
                    .collect();
                page.set("Annots", refs);
            }
            ids.push(document.add_object(page));
        }

        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Count" => pages as i64,
                "Kids" => ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            }),
        );
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog);
        (document, ids)
    }

    /// Runs the scan over a built document, through its serialized bytes.
    ///
    /// Through the bytes rather than against the `Document` in hand, so the test
    /// exercises the same path the application does --- including the load
    /// options, which is where a bound lives.
    fn scan_of(document: &mut Document, pages: usize) -> Links {
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("the fixture must save");
        scan(&bytes, pages, None).expect("the fixture must parse")
    }

    fn link(rect: Vec<Object>) -> Dictionary {
        dictionary! {
            "Type" => "Annot",
            "Subtype" => "Link",
            "Rect" => rect,
        }
    }

    fn rect() -> Vec<Object> {
        vec![100.into(), 700.into(), 200.into(), 720.into()]
    }

    #[test]
    fn a_goto_action_resolves_to_a_page_and_an_offset() {
        let (mut document, ids) = build(3, &[]);
        let mut annot = link(rect());
        annot.set(
            "A",
            dictionary! {
                "S" => "GoTo",
                "D" => vec![
                    Object::Reference(ids[2]),
                    "XYZ".into(),
                    Object::Null,
                    500.into(),
                    Object::Null,
                ],
            },
        );
        // Attached after the fact, since `build` needs the annotation before the
        // page and the destination needs the page before the annotation.
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let links = scan_of(&mut document, 3);
        assert_eq!(links.items.len(), 1);
        // 792 - 500 = 292 points from the top.
        assert_eq!(
            links.items[0].target,
            Target::Page {
                page: 2,
                top_pt: Some(292.0)
            }
        );
    }

    /// The offset is flipped against the *destination* page's height, not the
    /// page the link sits on.
    ///
    /// A document that mixes sizes is the only one that can tell those apart,
    /// which is why this needs a fixture of its own: on a document of uniform
    /// pages the wrong height is the right height, and every other test here
    /// would pass with the lookup pointing at either page.
    #[test]
    fn the_offset_is_flipped_against_the_page_it_lands_on() {
        let (mut document, ids) = build(2, &[]);
        // A tall second page. 1000 - 500 = 500 from its top; flipped against
        // page 1's 792 it would be 292, which is what the naive version returns.
        document
            .get_object_mut(ids[1])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(
                "MediaBox",
                vec![0.into(), 0.into(), 612.into(), 1000.into()],
            );

        let mut annot = link(rect());
        annot.set(
            "A",
            dictionary! {
                "S" => "GoTo",
                "D" => vec![
                    Object::Reference(ids[1]),
                    "XYZ".into(),
                    Object::Null,
                    500.into(),
                    Object::Null,
                ],
            },
        );
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let links = scan_of(&mut document, 2);
        assert_eq!(
            links.items[0].target,
            Target::Page {
                page: 1,
                top_pt: Some(500.0)
            },
            "292.0 here means the flip used the link's own page height"
        );
    }

    /// `/XYZ null null null` means "keep the reader where they are", and a null
    /// is absent rather than zero. Reading it as zero scrolls to the top of the
    /// page, which looks exactly like a link that works.
    #[test]
    fn an_xyz_destination_with_no_coordinate_names_no_offset() {
        let (mut document, ids) = build(2, &[]);
        let mut annot = link(rect());
        annot.set(
            "A",
            dictionary! {
                "S" => "GoTo",
                "D" => vec![
                    Object::Reference(ids[1]),
                    "XYZ".into(),
                    Object::Null,
                    Object::Null,
                    Object::Null,
                ],
            },
        );
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let links = scan_of(&mut document, 2);
        assert_eq!(
            links.items[0].target,
            Target::Page {
                page: 1,
                top_pt: None
            }
        );
    }

    /// `/FitH` carries its top one element earlier than `/XYZ` does, and `/Fit`
    /// carries none at all. Reading the wrong index is not a crash: it picks up
    /// whatever number happens to sit there.
    #[test]
    fn each_fit_takes_its_top_from_its_own_position() {
        for (fit, rest, expected) in [
            ("FitH", vec![Object::Real(700.0)], Some(92.0)),
            ("FitBH", vec![Object::Real(700.0)], Some(92.0)),
            (
                "FitR",
                vec![
                    Object::Real(0.0),
                    Object::Real(0.0),
                    Object::Real(612.0),
                    Object::Real(700.0),
                ],
                Some(92.0),
            ),
            ("Fit", vec![], None),
            ("FitB", vec![], None),
            ("FitV", vec![Object::Real(100.0)], None),
        ] {
            let (mut document, ids) = build(2, &[]);
            let mut destination = vec![Object::Reference(ids[1]), Object::Name(fit.into())];
            destination.extend(rest);

            let mut annot = link(rect());
            annot.set("A", dictionary! { "S" => "GoTo", "D" => destination });
            let annot_id = document.add_object(annot);
            document
                .get_object_mut(ids[0])
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Annots", vec![Object::Reference(annot_id)]);

            let links = scan_of(&mut document, 2);
            assert_eq!(
                links.items[0].target,
                Target::Page {
                    page: 1,
                    top_pt: expected
                },
                "/{fit} placed its top wrongly"
            );
        }
    }

    #[test]
    fn a_uri_action_is_refused_and_names_itself() {
        let (mut document, ids) = build(1, &[]);
        let mut annot = link(rect());
        annot.set(
            "A",
            dictionary! {
                "S" => "URI",
                "URI" => Object::string_literal("https://example.invalid/"),
            },
        );
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let links = scan_of(&mut document, 1);
        assert_eq!(
            links.items[0].target,
            Target::Refused {
                action: "uri".into()
            }
        );
    }

    /// The control for the entry above: an action tpdf has never heard of is
    /// declined too, rather than falling through to a destination lookup.
    #[test]
    fn an_unknown_action_is_refused_rather_than_followed() {
        let (mut document, ids) = build(2, &[]);
        let mut annot = link(rect());
        annot.set(
            "A",
            dictionary! {
                "S" => "JavaScript",
                // A destination sitting beside it, which a resolver that read
                // `/D` without checking `/S` would happily follow.
                "D" => vec![Object::Reference(ids[1]), "Fit".into()],
            },
        );
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let links = scan_of(&mut document, 2);
        assert_eq!(
            links.items[0].target,
            Target::Refused {
                action: "unsupported".into()
            }
        );
    }

    /// `/GoToR` carries a `/D` that looks exactly like a local destination, and
    /// resolving it would jump to a page of *this* document using another
    /// file's numbering --- the concrete defect `outline.rs` measured in
    /// PDFium's bookmark accessor.
    #[test]
    fn a_remote_goto_is_refused_even_though_its_destination_would_resolve() {
        let (mut document, ids) = build(3, &[]);
        let mut annot = link(rect());
        annot.set(
            "A",
            dictionary! {
                "S" => "GoToR",
                "F" => Object::string_literal("other.pdf"),
                "D" => vec![Object::Reference(ids[2]), "Fit".into()],
            },
        );
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let links = scan_of(&mut document, 3);
        assert_eq!(
            links.items[0].target,
            Target::Refused {
                action: "remote".into()
            }
        );
    }

    /// `/Dest` and `/A` together is malformed per §12.3.3, and the action wins.
    /// The two point at different pages so the assertion can tell which was
    /// taken --- with both at the same page it could not fail.
    #[test]
    fn an_action_beats_a_dest_sitting_beside_it() {
        let (mut document, ids) = build(3, &[]);
        let mut annot = link(rect());
        annot.set("Dest", vec![Object::Reference(ids[1]), "Fit".into()]);
        annot.set(
            "A",
            dictionary! {
                "S" => "GoTo",
                "D" => vec![Object::Reference(ids[2]), "Fit".into()],
            },
        );
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let links = scan_of(&mut document, 3);
        assert_eq!(
            links.items[0].target,
            Target::Page {
                page: 2,
                top_pt: None
            }
        );
    }

    #[test]
    fn a_named_destination_resolves_through_the_flat_dictionary() {
        let (mut document, ids) = build(3, &[]);
        let mut annot = link(rect());
        annot.set("Dest", Object::Name(b"chapter2".to_vec()));
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let dests = document.add_object(dictionary! {
            "chapter2" => vec![Object::Reference(ids[2]), "Fit".into()],
        });
        let catalog_id = document
            .trailer
            .get(b"Root")
            .unwrap()
            .as_reference()
            .unwrap();
        document
            .get_object_mut(catalog_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Dests", dests);

        let links = scan_of(&mut document, 3);
        assert_eq!(
            links.items[0].target,
            Target::Page {
                page: 2,
                top_pt: None
            }
        );
    }

    /// The other half of the same feature, and the reason both are implemented:
    /// a reader that knows only one form silently drops every link in the half
    /// of the corpus that uses the other.
    #[test]
    fn a_named_destination_resolves_through_the_name_tree() {
        let (mut document, ids) = build(3, &[]);
        let mut annot = link(rect());
        annot.set("Dest", Object::string_literal("chapter2"));
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        // A two-level tree, so the walk has to descend rather than read the
        // root's own `/Names`.
        let leaf = document.add_object(dictionary! {
            "Names" => vec![
                Object::string_literal("chapter2"),
                Object::Array(vec![Object::Reference(ids[2]), "XYZ".into(), Object::Null, 700.into(), Object::Null]),
            ],
        });
        let root = document.add_object(dictionary! {
            "Kids" => vec![Object::Reference(leaf)],
        });
        let names = document.add_object(dictionary! { "Dests" => root });
        let catalog_id = document
            .trailer
            .get(b"Root")
            .unwrap()
            .as_reference()
            .unwrap();
        document
            .get_object_mut(catalog_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Names", names);

        let links = scan_of(&mut document, 3);
        assert_eq!(
            links.items[0].target,
            Target::Page {
                page: 2,
                top_pt: Some(92.0)
            }
        );
    }

    /// A name tree that points at itself. Without the node budget this does not
    /// return a wrong answer --- it does not return.
    #[test]
    fn a_cyclic_name_tree_is_given_up_on_and_counted() {
        let (mut document, ids) = build(1, &[]);
        let mut annot = link(rect());
        annot.set("Dest", Object::string_literal("nowhere"));
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let root = document.new_object_id();
        document.objects.insert(
            root,
            Object::Dictionary(dictionary! {
                "Kids" => vec![Object::Reference(root)],
            }),
        );
        let names = document.add_object(dictionary! { "Dests" => root });
        let catalog_id = document
            .trailer
            .get(b"Root")
            .unwrap()
            .as_reference()
            .unwrap();
        document
            .get_object_mut(catalog_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Names", names);

        let links = scan_of(&mut document, 1);
        assert_eq!(links.items[0].target, Target::Broken);
        assert_eq!(links.limits.unresolved_names, 1);
        assert!(links.limits.any());
    }

    /// The control for the entry above: a name that is simply not in a healthy
    /// tree is broken and is *not* counted as a bound firing. Collapsing the two
    /// would make every ordinary broken link look like a truncated scan.
    #[test]
    fn a_missing_name_is_broken_without_charging_a_limit() {
        let (mut document, ids) = build(1, &[]);
        let mut annot = link(rect());
        annot.set("Dest", Object::string_literal("nowhere"));
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let root = document.add_object(dictionary! {
            "Names" => vec![
                Object::string_literal("somewhere"),
                Object::Array(vec![Object::Reference(ids[0]), "Fit".into()]),
            ],
        });
        let names = document.add_object(dictionary! { "Dests" => root });
        let catalog_id = document
            .trailer
            .get(b"Root")
            .unwrap()
            .as_reference()
            .unwrap();
        document
            .get_object_mut(catalog_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Names", names);

        let links = scan_of(&mut document, 1);
        assert_eq!(links.items[0].target, Target::Broken);
        assert_eq!(links.limits.unresolved_names, 0);
        assert!(!links.limits.any());
    }

    #[test]
    fn a_destination_past_the_last_page_is_broken() {
        let (mut document, ids) = build(2, &[]);
        let mut annot = link(rect());
        annot.set(
            "A",
            dictionary! { "S" => "GoTo", "D" => vec![99.into(), "Fit".into()] },
        );
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let links = scan_of(&mut document, 2);
        assert_eq!(links.items[0].target, Target::Broken);
    }

    #[test]
    fn a_link_with_neither_action_nor_destination_points_nowhere() {
        let (mut document, _) = build(1, &[(0, vec![link(rect())])]);
        let links = scan_of(&mut document, 1);
        assert_eq!(links.items[0].target, Target::None);
    }

    #[test]
    fn a_zero_area_rectangle_is_left_out() {
        let flat = link(vec![100.into(), 700.into(), 100.into(), 720.into()]);
        let real = link(rect());
        let (mut document, _) = build(1, &[(0, vec![flat, real])]);
        let links = scan_of(&mut document, 1);
        assert_eq!(
            links.items.len(),
            1,
            "the flat rectangle must not be listed"
        );
        assert_eq!(links.items[0].rect[0], 100.0);
    }

    /// `/F` bit 2 is Hidden. The control is a link carrying *other* flags ---
    /// bit 3 is Print, which every real link sets --- so the assertion is about
    /// that one bit rather than about `/F` being present at all.
    #[test]
    fn a_hidden_link_is_not_clickable_and_a_printing_one_is() {
        let mut invisible = link(rect());
        invisible.set("F", 2);
        let mut printing = link(vec![300.into(), 700.into(), 400.into(), 720.into()]);
        printing.set("F", 4);

        let (mut document, _) = build(1, &[(0, vec![invisible, printing])]);
        let links = scan_of(&mut document, 1);
        assert_eq!(links.items.len(), 1, "only the hidden one is dropped");
        assert_eq!(links.items[0].rect[0], 300.0);
        assert!(
            !links.limits.any(),
            "a hidden link is the document's intent, not a bound firing"
        );
    }

    #[test]
    fn annotations_that_are_not_links_are_skipped_without_complaint() {
        let note = dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => rect(),
            "Contents" => Object::string_literal("a note"),
        };
        let (mut document, _) = build(1, &[(0, vec![note, link(rect())])]);
        let links = scan_of(&mut document, 1);
        assert_eq!(links.items.len(), 1);
        assert_eq!(
            links.limits.unreadable, 0,
            "a comment sharing the array is not an unreadable entry"
        );
    }

    #[test]
    fn a_rectangle_written_backwards_is_normalised() {
        // Corners swapped on both axes, which §12.5.2 requires a consumer to fix.
        let backwards = link(vec![200.into(), 720.into(), 100.into(), 700.into()]);
        let (mut document, _) = build(1, &[(0, vec![backwards])]);
        let links = scan_of(&mut document, 1);
        let [left, top, right, bottom] = links.items[0].rect;
        assert!(
            left < right && top < bottom,
            "got {:?}",
            links.items[0].rect
        );
        assert_eq!([left, right], [100.0, 200.0]);
        // 792 - 720 = 72 from the top, 792 - 700 = 92.
        assert_eq!([top, bottom], [72.0, 92.0]);
    }

    #[test]
    fn an_unreadable_entry_is_counted_rather_than_dropped_silently() {
        let (mut document, ids) = build(1, &[(0, vec![link(rect())])]);
        // A reference to an object that does not exist, plus a bare integer.
        let annots = document
            .get_dictionary(ids[0])
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        let mut entries = vec![Object::Reference((9999, 0)), Object::Integer(42)];
        entries.extend(annots);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", entries);

        let links = scan_of(&mut document, 1);
        assert_eq!(links.items.len(), 1);
        assert_eq!(links.limits.unreadable, 2);
        assert!(links.limits.any());
    }

    #[test]
    fn a_crowded_page_is_cut_and_says_so() {
        let many: Vec<Dictionary> = (0..MAX_PER_PAGE + 10).map(|_| link(rect())).collect();
        let (mut document, _) = build(1, &[(0, many)]);
        let links = scan_of(&mut document, 1);
        assert_eq!(links.items.len(), MAX_PER_PAGE);
        assert_eq!(links.limits.crowded_pages, 1);
        assert!(links.limits.any());
    }

    /// A `/Rotate 90` page: the destination offset becomes unplaceable, and the
    /// rectangle turns with the page rather than staying where the file wrote it.
    #[test]
    fn a_rotated_page_turns_the_rectangle_and_drops_the_offset() {
        let (mut document, ids) = build(2, &[]);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Rotate", 90);

        let mut annot = link(rect());
        annot.set(
            "A",
            dictionary! {
                "S" => "GoTo",
                "D" => vec![Object::Reference(ids[0]), "XYZ".into(), Object::Null, 500.into(), Object::Null],
            },
        );
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let links = scan_of(&mut document, 2);
        assert_eq!(
            links.items[0].target,
            Target::Page {
                page: 0,
                top_pt: None
            },
            "a quarter turn has no vertical axis for the destination to name"
        );
        // The page is displayed 792 wide by 612 tall, and the rectangle turned
        // with it: what was 100..200 across is now measured down the display.
        let [left, top, right, bottom] = links.items[0].rect;
        assert!(
            right <= 792.0 && bottom <= 612.0,
            "got {:?}",
            links.items[0].rect
        );
        assert!(left < right && top < bottom);
        assert_ne!([left, top, right, bottom], [100.0, 72.0, 200.0, 92.0]);
    }

    /// The two scans must agree about a page's displayed geometry, because each
    /// computes it. A disagreement would put a comment and a link that share a
    /// rectangle in the file in two different places on screen.
    #[test]
    fn both_scans_agree_about_a_rotated_page() {
        let shared = vec![100.into(), 700.into(), 200.into(), 720.into()];
        let mut note = dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => shared.clone(),
            "Contents" => Object::string_literal("beside the link"),
        };
        note.set("T", Object::string_literal("Timo"));
        let (mut document, ids) = build(1, &[(0, vec![note, link(shared)])]);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Rotate", 270);

        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        let links = scan(&bytes, 1, None).unwrap();
        let comments = crate::annots::scan(&bytes, 1, None).unwrap();

        assert_eq!(links.items.len(), 1);
        assert_eq!(comments.items.len(), 1);
        assert_eq!(
            links.items[0].rect, comments.items[0].rect,
            "one rectangle in the file must be one rectangle on screen"
        );
    }

    /// A document PDFium pages and `lopdf` cannot is reported, not answered "none".
    ///
    /// **Reproduced synthetically here, and there is a real instance since
    /// 2026-08-23.** The scan was run across every `testdata/*.pdf` on
    /// 2026-08-16 and `pages_missed` was 0 on all of them, and the one document
    /// where the parsers famously disagree --- `incr-encrypted-pw.pdf`, where
    /// `lopdf` reports zero pages --- was one PDFium would not open at all, so it
    /// never reached this scan. That stopped being true when tpdf learned to ask
    /// for a password: PDFium opens it now, and a `lopdf` parse without the key
    /// still reports zero. `examples/password_probe.rs` asserts `pages_missed`
    /// is 0 once the key reaches this module, and its mutation drives it to 2.
    ///
    /// The synthetic case stays, because a guard against two independent parsers
    /// disagreeing must not depend on a fixture that happens to make them.
    #[test]
    fn a_page_lopdf_cannot_account_for_is_reported() {
        let (mut document, _) = build(2, &[(0, vec![link(rect())])]);
        // Five, as PDFium would say for a document whose page tree it repaired.
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("the fixture must save");
        let links = scan(&bytes, 5, None).expect("the fixture must parse");

        assert_eq!(links.limits.pages_missed, 3, "5 claimed, 2 readable");
        assert!(
            links.limits.any(),
            "a scan that could not see three pages must not look complete"
        );
        // And the links it *could* read are still returned: this is a notice,
        // not a refusal, and dropping the readable half would be worse than the
        // silence it replaces.
        assert_eq!(links.items.len(), 1);
    }

    /// The control: agreement charges nothing.
    ///
    /// Without it, a scan that reported every document as short would pass the
    /// test above and put a warning on every file tpdf opens --- which trains a
    /// reader to ignore the one that matters.
    #[test]
    fn a_document_both_parsers_agree_about_reports_nothing_missing() {
        let (mut document, _) = build(2, &[(0, vec![link(rect())])]);
        let links = scan_of(&mut document, 2);
        assert_eq!(links.limits.pages_missed, 0);
        assert!(!links.limits.any());
    }

    /// More pages than PDFium claims is not a deficit.
    ///
    /// `saturating_sub` rather than a signed difference: a document `lopdf` reads
    /// further into than PDFium paginates is odd and is not a *short* answer, and
    /// an underflow here would report the largest number the type can hold.
    #[test]
    fn seeing_more_pages_than_claimed_is_not_a_deficit() {
        let (mut document, _) = build(4, &[(0, vec![link(rect())])]);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save");
        let links = scan(&bytes, 2, None).expect("parse");
        assert_eq!(links.limits.pages_missed, 0);
    }

    /// Builds an outline of `entries`, each `(dest-or-none, children)`.
    ///
    /// Returns the catalog id so a caller can attach named destinations.
    fn with_outline(document: &mut Document, entries: Vec<(Option<Object>, Vec<Option<Object>>)>) {
        let root = document.new_object_id();
        let mut tops: Vec<ObjectId> = Vec::new();

        for (dest, kids) in entries {
            let id = document.new_object_id();
            let mut node = dictionary! { "Parent" => root };
            if let Some(dest) = dest {
                node.set("Dest", dest);
            }
            if !kids.is_empty() {
                let ids: Vec<ObjectId> = kids.iter().map(|_| document.new_object_id()).collect();
                for (at, kid) in kids.into_iter().enumerate() {
                    let mut child = dictionary! { "Parent" => id };
                    if let Some(dest) = kid {
                        child.set("Dest", dest);
                    }
                    if let Some(next) = ids.get(at + 1) {
                        child.set("Next", *next);
                    }
                    document.objects.insert(ids[at], Object::Dictionary(child));
                }
                node.set("First", ids[0]);
                node.set("Last", *ids.last().expect("kids"));
            }
            document.objects.insert(id, Object::Dictionary(node));
            tops.push(id);
        }
        for (at, id) in tops.iter().enumerate() {
            if let Some(next) = tops.get(at + 1) {
                document
                    .get_object_mut(*id)
                    .unwrap()
                    .as_dict_mut()
                    .unwrap()
                    .set("Next", *next);
            }
        }
        document.objects.insert(
            root,
            Object::Dictionary(dictionary! {
                "Type" => "Outlines",
                "First" => tops[0],
                "Last" => *tops.last().expect("tops"),
            }),
        );
        let catalog_id = document
            .trailer
            .get(b"Root")
            .unwrap()
            .as_reference()
            .unwrap();
        document
            .get_object_mut(catalog_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Outlines", root);
    }

    fn targets_of(document: &mut Document, pages: usize) -> Vec<Target> {
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save");
        outline_targets(&bytes, pages).expect("parse")
    }

    /// Pre-order: an entry, then its children, then the next sibling.
    ///
    /// The order is the whole of the comparison `links-probe --mode agree`
    /// makes --- `outline::read` flattens pre-order, and two lists in different
    /// orders compare nothing. A fixture of one level could not tell the two
    /// apart, so this one nests.
    #[test]
    fn the_outline_walk_is_pre_order() {
        let (mut document, ids) = build(4, &[]);
        let to = |page: usize| {
            Some(Object::Array(vec![
                Object::Reference(ids[page]),
                "Fit".into(),
            ]))
        };
        with_outline(
            &mut document,
            vec![(to(0), vec![to(1), to(2)]), (to(3), vec![])],
        );

        let pages: Vec<u32> = targets_of(&mut document, 4)
            .into_iter()
            .map(|target| match target {
                Target::Page { page, .. } => page,
                other => panic!("every entry here has a destination: {other:?}"),
            })
            .collect();
        assert_eq!(
            pages,
            vec![0, 1, 2, 3],
            "parent, its children, then the sibling"
        );
    }

    /// A `/Next` chain that loops terminates, and does not repeat the loop.
    ///
    /// PDFium's own documentation says a caller must handle circular bookmark
    /// references, so this is an input we are told to expect. Without the
    /// visited set it does not return a wrong answer --- it does not return.
    #[test]
    fn a_looping_outline_chain_terminates() {
        let (mut document, ids) = build(2, &[]);
        let to = |page: usize| {
            Some(Object::Array(vec![
                Object::Reference(ids[page]),
                "Fit".into(),
            ]))
        };
        with_outline(&mut document, vec![(to(0), vec![]), (to(1), vec![])]);

        // Point the second entry's `/Next` back at the first.
        let root = document
            .catalog()
            .unwrap()
            .get(b"Outlines")
            .unwrap()
            .as_reference()
            .unwrap();
        let first = document
            .get_dictionary(root)
            .unwrap()
            .get(b"First")
            .unwrap()
            .as_reference()
            .unwrap();
        let last = document
            .get_dictionary(root)
            .unwrap()
            .get(b"Last")
            .unwrap()
            .as_reference()
            .unwrap();
        document
            .get_object_mut(last)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Next", first);

        let targets = targets_of(&mut document, 2);
        assert_eq!(targets.len(), 2, "each entry once, and the loop broken");
    }

    /// A document with no outline is an empty list, not an error.
    #[test]
    fn a_document_with_no_outline_has_no_targets() {
        let (mut document, _) = build(1, &[(0, vec![link(rect())])]);
        assert!(targets_of(&mut document, 1).is_empty());
    }

    /// The walk resolves through the same rules the links do.
    ///
    /// Which is the point of sharing `target_of`: an oracle that resolved
    /// destinations by its own rules would be comparing `outline.rs` against a
    /// third implementation rather than against this one.
    #[test]
    fn the_outline_walk_refuses_what_a_link_would() {
        let (mut document, ids) = build(2, &[]);
        with_outline(&mut document, vec![(None, vec![])]);
        let root = document
            .catalog()
            .unwrap()
            .get(b"Outlines")
            .unwrap()
            .as_reference()
            .unwrap();
        let first = document
            .get_dictionary(root)
            .unwrap()
            .get(b"First")
            .unwrap()
            .as_reference()
            .unwrap();
        document
            .get_object_mut(first)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(
                "A",
                dictionary! {
                    "S" => "GoToR",
                    "F" => Object::string_literal("other.pdf"),
                    "D" => vec![Object::Reference(ids[1]), "Fit".into()],
                },
            );

        assert_eq!(
            targets_of(&mut document, 2),
            vec![Target::Refused {
                action: "remote".into()
            }],
        );
    }

    /// An entry naming a destination that does not resolve is broken, not absent.
    ///
    /// This is the case a real document produced and PDFium cannot see: a
    /// `/Dest` written as a name string that resolves nowhere. The control is
    /// the entry beside it with no `/Dest` at all, which *is* absent --- without
    /// it the assertion could not tell the two apart, which is exactly the
    /// confusion being pinned.
    #[test]
    fn a_destination_that_resolves_nowhere_is_broken_not_absent() {
        let (mut document, _) = build(2, &[]);
        with_outline(
            &mut document,
            vec![
                (Some(Object::string_literal("nowhere")), vec![]),
                (None, vec![]),
            ],
        );
        assert_eq!(
            targets_of(&mut document, 2),
            vec![Target::Broken, Target::None],
        );
    }

    /// A page displayed from an inset `/CropBox` places rectangles in *its*
    /// space, not the media box's.
    ///
    /// **PDFium lays a page out from its crop box**, so the viewer's
    /// coordinates start at that box's corner. Reading `/MediaBox` alone put
    /// every rectangle out by the difference --- silently, on a page that looks
    /// entirely normal. Measured before the fix on a fixture shaped like this
    /// one: character boxes landed on ink 0% of the time against 100% uncropped.
    ///
    /// The control is the same page with **no** crop box, because a rule that
    /// ignored the origin gives the right answer there and the wrong one here,
    /// and only the pair can tell them apart.
    #[test]
    fn a_cropped_page_places_a_rectangle_in_the_crop_box_s_space() {
        let make = |crop: Option<Vec<Object>>| {
            let mut document = Document::with_version("1.7");
            let pages_id = document.new_object_id();
            let annot =
                document.add_object(link(vec![100.into(), 690.into(), 300.into(), 720.into()]));
            let mut page = dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Annots" => vec![Object::Reference(annot)],
            };
            if let Some(crop) = crop {
                page.set("CropBox", crop);
            }
            let page_id = document.add_object(page);
            document.objects.insert(
                pages_id,
                Object::Dictionary(dictionary! {
                    "Type" => "Pages",
                    "Kids" => vec![page_id.into()],
                    "Count" => 1,
                }),
            );
            let catalog = document.add_object(dictionary! {
                "Type" => "Catalog",
                "Pages" => pages_id,
            });
            document.trailer.set("Root", catalog);
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).expect("save");
            scan(&bytes, 1, None).expect("parse").items[0].rect
        };

        // No crop box: the page is 595x842 from (0, 0), and 842 - 720 = 122.
        assert_eq!(make(None), [100.0, 122.0, 300.0, 152.0]);

        // Inset by (50, 50) and 495x692 tall. The same rectangle is 50 to the
        // left of where it was, and 742 - 720 = 22 from the top.
        let cropped = make(Some(vec![50.into(), 50.into(), 545.into(), 742.into()]));
        assert_eq!(cropped, [50.0, 22.0, 250.0, 52.0]);
    }

    /// A `/CropBox` larger than the sheet is intersected with it, per §14.11.2.
    ///
    /// A producer can write one, and a page displayed bigger than its own paper
    /// is not a space to map coordinates into --- every rectangle would be
    /// scaled against a size the renderer never uses.
    #[test]
    fn a_crop_box_larger_than_the_page_is_intersected_with_it() {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let annot = document.add_object(link(vec![100.into(), 690.into(), 300.into(), 720.into()]));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "CropBox" => vec![(-100).into(), (-100).into(), 900.into(), 1200.into()],
            "Annots" => vec![Object::Reference(annot)],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save");

        // Identical to the uncropped answer: the intersection is the media box.
        assert_eq!(
            scan(&bytes, 1, None).expect("parse").items[0].rect,
            [100.0, 122.0, 300.0, 152.0]
        );
    }

    /// No field of a [`Link`] may carry a string the document chose.
    ///
    /// The same standing check `annots.rs` and `outline.rs` keep, and here it is
    /// nearly free because the type has no string field at all --- which is the
    /// point. It is written as an exhaustive destructure so that *adding* one
    /// fails to compile rather than failing this test, and a `Target` variant
    /// carrying a URL is already a compile error next door.
    #[test]
    fn no_link_field_may_carry_a_url() {
        let (mut document, ids) = build(1, &[]);
        let mut annot = link(rect());
        annot.set(
            "A",
            dictionary! {
                "S" => "URI",
                "URI" => Object::string_literal("https://tracker.invalid/beacon?id=42"),
            },
        );
        // A title and a subject too, in case a later change starts reading them.
        annot.set("T", Object::string_literal("https://also.invalid/"));
        annot.set("Contents", Object::string_literal("javascript:alert(1)"));
        let annot_id = document.add_object(annot);
        document
            .get_object_mut(ids[0])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", vec![Object::Reference(annot_id)]);

        let links = scan_of(&mut document, 1);
        let Link {
            id,
            page,
            rect,
            target,
        } = &links.items[0];

        let mut strings: Vec<String> = vec![id.to_string(), page.to_string()];
        strings.extend(rect.iter().map(|value| value.to_string()));
        if let Target::Refused { action } = target {
            strings.push(action.clone());
        }

        for value in strings {
            let lowered = value.to_ascii_lowercase();
            assert!(
                !lowered.contains("://")
                    && !lowered.contains("invalid")
                    && !lowered.contains("javascript"),
                "a link field reached the frontend looking like a URL: {value:?}"
            );
        }
    }
}
