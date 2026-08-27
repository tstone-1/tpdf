//! Removing content from a page, rather than drawing over it.
//!
//! `docs/PLAN.md` §6's first rule: a redaction removes content, and an
//! implementation that leaves the bytes recoverable is a defect. This module is
//! the primitive that makes that literally true for text --- the instruction
//! that drew the glyphs is deleted from the content stream, so there is nothing
//! left to uncover.
//!
//! ## Route B, and why the destructive one is the shipped one
//!
//! §6 names two routes. **A** splits a text-showing operator at glyph
//! boundaries and re-emits the survivors with their original codes; **B**
//! removes the whole operation containing any redacted glyph. B eats
//! neighbouring words on the same line, which is visibly destructive and is
//! exactly why it is safe: there is no re-encoding step to get wrong. Spike 0.3
//! proved A is feasible and §6 keeps B as the shipped behaviour until a hostile
//! corpus proves A's fidelity. This is B.
//!
//! ## The correspondence this rests on, and the guard that makes it honest
//!
//! PDFium enumerates a page's *objects* and gives each a bounding box; `lopdf`
//! decodes the same page's *content stream* into operators. Nothing connects
//! them but order: the nth text object is assumed to be the nth show operator.
//! Spike 0.3 measured that holding 4:4 on all four of its fixtures and said
//! plainly it is not guaranteed --- a `TJ` split across objects, or a Form
//! XObject contributing objects from another stream, breaks it.
//!
//! So [`remove_shows`] takes the count the caller saw from PDFium and **refuses**
//! when it disagrees with the operators found. Addressing the wrong operator
//! deletes the wrong words while reporting success, which is the plausible wrong
//! answer this whole subsystem exists to prevent. A refusal is a redaction that
//! did not happen; a mis-addressed removal is one that says it happened.
//!
//! ## What it deliberately cannot do, reported rather than skipped
//!
//! Only text is removed. An image, a vector path or an object PDFium calls
//! unsupported that overlaps the region is **reported as unhandled** and no
//! amount of removing text changes that: §6's deny-by-default rule says an
//! object this does not understand is a verification failure, not a shrug. A
//! caller that applied [`Plan::shows`] and ignored [`Plan::unhandled`] would
//! produce a file with the words gone and the picture of the words still in it.
//! [`Plan::is_complete`] is the one question worth asking before acting.
//!
//! ## The words are also beside the glyphs
//!
//! Deleting the show operator takes the *drawing* away. It does not take the
//! words away, because a tagged document writes them a second time into the
//! marked-content span around them --- `/ActualText` is what a screen reader
//! speaks, and spike 0.3 measured it surviving a surgical removal on
//! `text-marked.pdf` while every pixel-based check passed. `docs/PLAN.md` §6
//! calls that a *carrier* and lists a table of them.
//!
//! [`clear_shadow_text`] clears the ones that live in the content stream this
//! module is already rewriting: the property list of every span a removal fell
//! inside. That is one of the carrier table's rows and one of that row's two
//! homes --- the same keys hang off a **structure element** in
//! `/StructTreeRoot`, reached by `/MCID` rather than by nesting, and this does
//! not touch them. Nor does it touch an annotation's `/Contents`, a form
//! field's value, `/Info`, or anything else outside the page's content. Those
//! are document-level objects and a different piece of work; `text-marked.pdf`
//! holds two of them on purpose, so `redact-probe` can go on measuring that they
//! are still there.

use std::collections::HashSet;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};

/// Ceiling on a decoded content stream.
///
/// `AGENTS.md` requires every `lopdf` decode to be bounded, and a content stream
/// is attacker-chosen like everything else in the file. The same value spike 0.3
/// used.
pub const MAX_CONTENT_BYTES: usize = 64 * 1024 * 1024;

/// A rectangle in PDF page space --- `[left, bottom, right, top]`, y upwards.
///
/// PDFium's own convention for object bounds, which is what the caller has when
/// it builds one of these, so nothing is converted on the way in. Note this is
/// **not** the display space `text::to_device` produces: a page with `/Rotate
/// 90` reports object bounds here unrotated, and a caller holding a reader's
/// selection has to map it back. `docs/TRAPS.md` has that entry more than once.
pub type Rect = [f32; 4];

/// One object PDFium found on the page, in the order it enumerated them.
#[derive(Debug, Clone, PartialEq)]
pub struct PageObject {
    /// Its bounding box, as PDFium reports it.
    pub bounds: Rect,
    /// What PDFium calls it: `text`, `image`, `path`, or anything else.
    ///
    /// A string rather than an enum because the only two questions asked of it
    /// are "is this text" and "what do I call it in a refusal", and an enum here
    /// would be a second vocabulary to keep in step with PDFium's.
    pub kind: String,
}

/// What removing a region from one page would take, and what it would miss.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Ordinals among the page's **text objects**, ascending.
    ///
    /// The same ordinals [`remove_shows`] addresses show operators by. Ascending
    /// because that is how a reader checks them; removal walks them backwards,
    /// which is that function's business rather than this type's.
    pub shows: Vec<usize>,
    /// Objects overlapping the region that this cannot remove.
    ///
    /// Non-empty means the region is **not** redactable by this module, and a
    /// caller that acts anyway ships a page whose words are gone and whose
    /// picture of those words is not.
    pub unhandled: Vec<Unhandled>,
}

/// One object a region covers that this cannot remove.
///
/// **The kind and the position rather than a sentence**, which is what this
/// held until 2026-08-26. A refusal wants a sentence and a panel wants the word
/// *image*, and building the sentence first makes the second reader parse the
/// first reader's prose --- the shape that drifts the moment somebody rewords
/// it. One decision, made in [`covered`], rendered by whoever is showing it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Unhandled {
    /// Its position in PDFium's enumeration of the page's objects.
    ///
    /// Carried because a page with three pictures on it otherwise reports the
    /// same thing three times and a reader cannot tell whether that is three
    /// findings or one printed thrice.
    pub at: usize,
    /// What PDFium calls it: `image`, `path`, `shading`, `form`, `unsupported`.
    ///
    /// A string for [`PageObject::kind`]'s reason, and it is the same string:
    /// this is that field, carried through.
    pub kind: String,
}

impl Unhandled {
    /// The sentence a refusal says.
    ///
    /// Here rather than at the call site so that every refusal about an object
    /// this cannot remove reads the same way, and so that the panel's shorter
    /// phrasing is a second *rendering* rather than a second decision.
    #[must_use]
    pub fn sentence(&self) -> String {
        let Unhandled { at, kind } = self;
        format!("object {at} is of kind {kind} and overlaps the region; only text is removed here")
    }
}

/// What removing one region would take, as somebody outside this process reads it.
///
/// [`Plan`] with the ordinals replaced by what they *draw*. A caller across the
/// IPC boundary cannot do anything with an ordinal --- it addresses a show
/// operator in a content stream this process parsed --- and what a reader
/// reviewing a redaction needs is the words and the refusals.
///
/// **`taking` is not the words the region covers**, and the difference is the
/// whole reason this crosses the boundary at all. Route B removes a whole
/// text-showing operation when any of its glyphs is inside the region, so this
/// is at least what the reader selected and commonly the rest of the line. The
/// frontend computes the covered words itself, from geometry it already holds;
/// what it cannot compute is this.
// `PartialEq` without `Eq`, because `area` is floats. Nothing compares two of
// these for equality outside a test, and an `Eq` on a type holding an `f32` is
// the wrong promise rather than a missing convenience.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RegionPlan {
    /// Which of the page's text-showing operations the removal would delete.
    ///
    /// [`Plan::shows`], carried through. Meaningless to the frontend, which
    /// reads only how many there are --- and meaningful to the **coordinator**,
    /// which parses the same file with `lopdf` and is what hands them to
    /// [`remove_shows`]. That is the whole reason they cross the boundary
    /// rather than being counted here.
    pub shows: Vec<usize>,
    /// How many text objects PDFium found on the page this region is on.
    ///
    /// Not about the region at all, and carried anyway: [`remove_shows`] refuses
    /// when this disagrees with the operators `lopdf` finds, and the caller that
    /// applies a plan has no other way to learn it.
    pub text_objects: usize,
    /// The region itself, in the page's own absolute space.
    ///
    /// **Carried because the writer cannot work it out**, and a second attempt
    /// at it would be a second geometry to disagree with this one. The reader
    /// drew a rectangle on a displayed page; turning that into page coordinates
    /// needs the page's `/Rotate` and its crop box, which is `render`'s
    /// `crop_from_display` and is where the one mapping lives.
    ///
    /// Measured on `links-cropped.pdf` rather than assumed, because the two
    /// spellings look alike: `FPDFPageObj_GetBounds` answers in the page's
    /// **absolute** space, media-box origin --- the same space an annotation's
    /// `/Rect` is in --- while `FPDF_GetPageWidthF` answers the **crop** box's
    /// size. A page object drawn at y 722 under `/CropBox [50 50 545 742]` comes
    /// back at 716.98..739.47, not at 672. See `docs/TRAPS.md`.
    pub area: Rect,
    /// What those operations draw, in the page's own object order.
    ///
    /// One string rather than one per operation: a row shows a line and a
    /// caller wanting them apart would be reconstructing the ordinals this type
    /// exists to keep out of the reply.
    pub taking: String,
    /// What the region covers that this cannot remove, one sentence each.
    ///
    /// Non-empty means the region is **not** redactable, and the sentences are
    /// what lets a caller say so rather than reporting a number. Deny by
    /// default: `docs/PLAN.md` §6 calls an object the sanitiser does not
    /// understand a verification failure, not a shrug.
    pub unhandled: Vec<Unhandled>,
}

impl RegionPlan {
    /// Whether acting on this removes everything in the region.
    ///
    /// [`Plan::is_complete`] on the far side of the boundary, and deliberately
    /// the same question rather than a field: a caller that reads `unhandled`
    /// itself has to decide what empty means, and the two decisions would drift.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unhandled.is_empty()
    }
}

impl Plan {
    /// Whether acting on this plan removes everything in the region.
    ///
    /// The question to ask before applying, and the reason [`Plan::unhandled`]
    /// is a list of sentences rather than a count: a caller refusing has to be
    /// able to say *what* it could not remove.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unhandled.is_empty()
    }
}

/// What a redaction did, and whether it can be proved.
///
/// **Never a bare success**, which is `docs/PLAN.md` §6 step 4 stated as a type:
/// a redaction that cannot be shown clean is a confident lie, and the reasons
/// are what tell a reader whether the next step is OCR, a different tool, or
/// giving up on the file. So `verified` false always arrives with `why`
/// non-empty, and a caller cannot report the first without the second.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    /// How many regions were removed from.
    pub regions: usize,
    /// How many text-showing operations went.
    ///
    /// Not the same as `regions`, in either direction: one region can cover
    /// several operations, and two regions on one line cover the same one.
    pub shows: usize,
    /// The source changed on disk since the reader opened it.
    ///
    /// [`crate::save::Copied`]'s field, carried for its reason: a copy written
    /// from a document that is no longer the one on screen is a fact the reader
    /// has to be told rather than a failure.
    pub changed: bool,
    /// Whether the written file could be shown clean.
    pub verified: bool,
    /// Why not, one reason each. Empty exactly when `verified`.
    pub why: Vec<String>,
}

/// Which of a page's objects a region covers.
///
/// `objects` is every object PDFium enumerated on the page, in its order.
/// Overlap is the test, not containment: a word half inside the region is a word
/// the reader meant to remove, and route B removes the whole operation anyway.
///
/// **Touching is not overlapping.** Two rectangles sharing only an edge do not
/// intersect, so a region drawn flush against a line of text does not silently
/// eat it. That is the boundary this is most likely to be wrong at, and it has
/// its own check.
#[must_use]
pub fn covered(objects: &[PageObject], region: Rect) -> Plan {
    let mut plan = Plan::default();
    let mut text_ordinal = 0usize;
    for (at, object) in objects.iter().enumerate() {
        let is_text = object.kind == "text";
        let ordinal = text_ordinal;
        if is_text {
            text_ordinal += 1;
        }
        if !overlaps(object.bounds, region) {
            continue;
        }
        if is_text {
            plan.shows.push(ordinal);
        } else {
            plan.unhandled.push(Unhandled {
                at,
                kind: object.kind.clone(),
            });
        }
    }
    plan
}

/// A rectangle with its corners the way round the comparison below assumes.
///
/// PDFium reports `[left, bottom, right, top]`, and a caller's region may arrive
/// with either pair the other way --- a drag upwards and to the left produces
/// exactly that --- which would otherwise make an ordinary rectangle overlap
/// nothing at all and redact nothing while reporting success.
///
/// One function rather than the same `min`/`max` pair written out twice: two
/// near-copies is the shape that drifts, and it drifted here first. The mutation
/// aimed at one copy survived, because the test that was meant to catch it
/// reversed *both* axes and the other copy rescued it.
#[must_use]
fn normalised(rect: Rect) -> Rect {
    [
        rect[0].min(rect[2]),
        rect[1].min(rect[3]),
        rect[0].max(rect[2]),
        rect[1].max(rect[3]),
    ]
}

/// Whether two page-space rectangles share any area.
///
/// Strict comparisons throughout: two rectangles sharing only an edge do not
/// overlap, so a region drawn flush against a line of text does not eat it.
///
/// `pub` for [`crate::ocr::control_from_page`], which has to ask the same
/// question of the same page: which words a region covers decides what the
/// removal takes *and* which words are left to read a control back from. Two
/// answers to that would let the gate certify against a word the removal was
/// supposed to have taken.
#[must_use]
pub fn overlaps(a: Rect, b: Rect) -> bool {
    let a = normalised(a);
    let b = normalised(b);
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// Which of a page's annotations a set of regions covers.
///
/// **A second carrier, and a different kind of one.** [`covered`] answers about
/// the page's own drawing instructions; this answers about the objects hanging
/// off `/Annots`, which is `docs/PLAN.md` §6's *Annotations* row --- a comment's
/// `/Contents`, its rich text, its author and subject, its appearance stream,
/// its popup and its replies. A sticky note anchored on the words a reader is
/// redacting routinely quotes them, and it goes on being displayed by every
/// reader afterwards.
///
/// `areas` are the regions in the page's own absolute space, as
/// [`RegionPlan::area`] carries them. An annotation is taken when its `/Rect`
/// overlaps any of them under the same strict rule [`covered`] uses, so an
/// annotation flush against a region is not eaten.
///
/// **An annotation whose `/Rect` cannot be read is taken as well**, and that is
/// the destructive direction on purpose. It is the one that cannot leave the
/// words behind, it matches route B's posture everywhere else in this module,
/// and an annotation with no readable rectangle is not being displayed by
/// anything anyway. The alternative --- keep what you cannot place --- decides a
/// safety question by giving up on it.
///
/// **Dependents go with it**, transitively: an annotation's `/Popup`, and any
/// annotation whose `/IRT` points at one being taken. A reply carries its own
/// `/Contents` and is a copy of the conversation about the words; leaving the
/// reply and taking the note it answers is the worst of both.
///
/// This does not decide anything about tpdf's own marks. A mark the reader added
/// in this session is an annotation on the page by the time this runs, and if it
/// is over the region it goes with the rest --- the file being written is a copy,
/// so the open document keeps it.
#[must_use]
pub fn covered_annots(doc: &Document, page: ObjectId, areas: &[Rect]) -> Vec<ObjectId> {
    let entries = annots_of(doc, page);
    let mut taken: Vec<ObjectId> = Vec::new();
    for id in &entries {
        let Ok(annot) = doc.get_dictionary(*id) else {
            // An entry naming no dictionary is not an annotation this can place
            // or read, so it is taken for the reason above.
            taken.push(*id);
            continue;
        };
        match rect_of(doc, annot) {
            Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)) => {}
            _ => taken.push(*id),
        }
    }

    // The dependents, to a fixed point. A chain of replies is a chain of copies
    // of the same conversation, and cutting it half way leaves the half that
    // quotes what went.
    loop {
        let mut grew = false;
        for id in &entries {
            if taken.contains(id) {
                // Its own popup, which is a separate object with its own
                // `/Contents` in most producers' output.
                let Ok(annot) = doc.get_dictionary(*id) else {
                    continue;
                };
                if let Ok(popup) = annot.get(b"Popup").and_then(Object::as_reference) {
                    if !taken.contains(&popup) {
                        taken.push(popup);
                        grew = true;
                    }
                }
                continue;
            }
            let Ok(annot) = doc.get_dictionary(*id) else {
                continue;
            };
            let replies_to_taken = annot
                .get(b"IRT")
                .and_then(Object::as_reference)
                .is_ok_and(|parent| taken.contains(&parent));
            if replies_to_taken {
                taken.push(*id);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    taken
}

/// The object ids on a page's `/Annots`, in whichever spelling it uses.
///
/// `/Annots 12 0 R` and `/Annots [5 0 R]` are both common and `save.rs` already
/// keeps them apart for the incremental writer's sake. Here the two are the same
/// question --- which annotations does this page have --- and an entry that is
/// not a reference is skipped, because an inline annotation dictionary has no id
/// to name and nothing to remove.
fn annots_of(doc: &Document, page: ObjectId) -> Vec<ObjectId> {
    let Ok(dictionary) = doc.get_dictionary(page) else {
        return Vec::new();
    };
    let Ok(entries) = dictionary
        .get(b"Annots")
        .and_then(|object| doc.dereference(object).map(|(_, object)| object))
        .and_then(Object::as_array)
    else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| entry.as_reference().ok())
        .collect()
}

/// An annotation's `/Rect`, or `None` when it is not four finite numbers.
///
/// **Deliberately not normalised here**, and that was measured rather than
/// decided. PDF 32000-1 §12.5.2 permits the corners either way round and says
/// the consumer normalises them, so the first version of this did --- and the
/// mutation written to prove that call mattered SURVIVED, because [`overlaps`]
/// normalises *both* of its arguments. The call changed nothing, which makes it
/// the shape `docs/TRAPS.md` records as a mutation that is a no-op: the edit
/// lands, the build passes, and the verdict is correct.
///
/// So the normalisation happens in exactly one place, which is where the
/// comparison is. `an_annotation_whose_corners_are_the_other_way_round_is_still_found`
/// still pins the property --- it is a statement about `covered_annots`, and what
/// answers it is `overlaps`.
fn rect_of(doc: &Document, annot: &Dictionary) -> Option<Rect> {
    let values = annot
        .get(b"Rect")
        .and_then(|object| doc.dereference(object).map(|(_, object)| object))
        .and_then(Object::as_array)
        .ok()?;
    if values.len() != 4 {
        return None;
    }
    let mut out = [0f32; 4];
    for (at, value) in values.iter().enumerate() {
        let (_, value) = doc.dereference(value).ok()?;
        let number = value.as_float().ok()?;
        if !number.is_finite() {
            return None;
        }
        out[at] = number;
    }
    Some(out)
}

/// What a removal did, so a caller can report it rather than assume it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// How many show operators the page had before.
    pub shows_before: usize,
    /// How many were removed.
    pub removed: usize,
    /// How many shadow-text keys went with them.
    ///
    /// An accounting observable, and it has readers rather than none: the tests
    /// pin the count for each shape of span, and `redact-probe` refuses a run
    /// reporting zero on a fixture built to carry one --- which is the failure a
    /// clearing that silently found nothing would otherwise produce.
    ///
    /// On its own it says nothing about whether the words went. Counting a key
    /// and removing it are two things, and the mutation that separates them is
    /// in the table; the probe asserts the stream afterwards for the same
    /// reason. See [`clear_shadow_text`].
    pub carriers: usize,
    /// How many went from the structure elements those spans belong to.
    ///
    /// Counted apart from [`Removed::carriers`] because they are two homes of
    /// one row of `docs/PLAN.md` §6's carrier table, reached by two different
    /// routes --- nesting in the content stream, and `/MCID` through the parent
    /// tree. A run that cleared one and not the other is the interesting case,
    /// and a single total could not say which. See [`clear_struct_shadow_text`].
    pub struct_carriers: usize,
}

/// The keys a marked-content property list can hold a copy of the text in.
///
/// `docs/PLAN.md` §6's carrier table calls these *shadow text*, and the reason
/// they are a carrier rather than a curiosity is that each one exists precisely
/// so that some reader gets the words when the glyphs will not do: `/ActualText`
/// is what a screen reader speaks and what a well-behaved extractor prefers over
/// the glyphs, `/Alt` is the description of a figure, `/E` the expansion of an
/// abbreviation. PDF 32000-1 §14.9.3 to §14.9.5 --- one clause each, and the
/// citation covered two of the three until it was read again.
///
/// **All three, not just the famous one.** `/ActualText` is the carrier spike
/// 0.3 measured and the one the fixture holds; clearing it alone would leave two
/// keys of the same kind, in the same dictionary, doing the same job.
const SHADOW_TEXT: [&[u8]; 3] = [b"ActualText", b"Alt", b"E"];

/// Strips shadow text from every marked-content span a removal emptied into.
///
/// `removed` is the ascending set of operation indices [`remove_shows`] is about
/// to delete. A span is *touched* when any of them lies between its `BDC`/`BMC`
/// and its `EMC`, and a touched span loses [`SHADOW_TEXT`] --- because those keys
/// are an alternate rendering of the very glyphs being taken away, written into
/// the content stream beside them.
///
/// **Touched, not emptied, and that is route B's own rule one level up.** §6's
/// route B removes the *whole* text-showing operation containing any redacted
/// glyph, on the grounds that a partial re-emission is the thing that gets
/// silently wrong; the same reasoning applies to a span whose `/ActualText`
/// restates a line of which one word was removed. Keeping it would leave a
/// verbatim copy of the removed words in the file, which is the one outcome this
/// module exists to prevent. Losing the alternate text for the words that
/// survive is the visible, honest cost, exactly as eating the rest of the line
/// is.
///
/// **An enclosing span counts too.** Spans nest, and an outer `/ActualText`
/// restates everything inside it, so every open span at the moment of a removal
/// is touched rather than only the innermost.
///
/// # Errors
///
/// A touched span whose property list is a **name** into the page's
/// `/Resources /Properties`, where the named dictionary carries one of these
/// keys. That dictionary may be shared with any number of other pages, and §6's
/// clone-on-write rule forbids editing a shared resource in place --- doing so
/// would silently alter every page that shares it. Cloning it is a page-resource
/// edit this does not do, so the honest answer is the same one the correspondence
/// guard gives: the redaction did not happen, and nothing was written.
///
/// The refusal is deliberately narrow. It fires only where the key was *seen*,
/// so the ordinary named property list --- `/OC /MC0 BDC`, optional content,
/// which carries no text at all --- passes straight through. A name that
/// resolves to nothing, and a property list this cannot reach because the
/// resource chain is malformed, are not refused either: there is no key in the
/// span to leave behind, and any copy sitting elsewhere in the file is what
/// [`crate::verify::scan`] is for.
fn clear_shadow_text(
    doc: &Document,
    page: ObjectId,
    operations: &mut [Operation],
    removed: &[usize],
) -> Result<Cleared, String> {
    // (index of the BDC/BMC, whether a removal fell inside it).
    let mut open: Vec<(usize, bool)> = Vec::new();
    let mut touched: Vec<usize> = Vec::new();
    for (at, operation) in operations.iter().enumerate() {
        match operation.operator.as_str() {
            "BDC" | "BMC" => open.push((at, false)),
            // An `EMC` with nothing open is a malformed stream rather than an
            // error worth refusing over: it closes no span, so there is no
            // property list it could have carried.
            "EMC" => {
                if let Some((start, inside)) = open.pop() {
                    if inside {
                        touched.push(start);
                    }
                }
            }
            _ => {}
        }
        if removed.binary_search(&at).is_ok() {
            for frame in &mut open {
                frame.1 = true;
            }
        }
    }
    // A span the stream never closed still marks everything after it, so a
    // removal inside one is inside it. Dropping these would leave the carrier in
    // exactly the malformed file least likely to be looked at twice.
    for (start, inside) in open {
        if inside {
            touched.push(start);
        }
    }

    let mut cleared = Cleared::default();
    for at in touched {
        // The span's `/MCID` before anything is removed from its dictionary ---
        // it is not shadow text and stays, and it is the only thing tying this
        // span to the structure element that holds the *other* copy.
        if let Some(Object::Dictionary(properties)) = operations[at].operands.get(1) {
            if let Ok(mcid) = properties.get(b"MCID").and_then(Object::as_i64) {
                cleared.mcids.push(mcid);
            }
        }
        match operations[at].operands.get_mut(1) {
            // `BMC` takes a tag and nothing else, so there is no property list.
            None => {}
            Some(Object::Dictionary(properties)) => {
                for key in SHADOW_TEXT {
                    if properties.remove(key).is_some() {
                        cleared.keys += 1;
                    }
                }
            }
            Some(Object::Name(name)) => {
                let name = name.clone();
                if let Some(shared) = property_list(doc, page, &name) {
                    if let Some(key) = SHADOW_TEXT.into_iter().find(|key| shared.has(key)) {
                        return Err(format!(
                            "the region covers a marked-content span whose property list is the \
                             shared resource /{}, and it carries /{}. Editing it would change \
                             every other page that uses it, so nothing was removed.",
                            String::from_utf8_lossy(&name),
                            String::from_utf8_lossy(key)
                        ));
                    }
                }
            }
            // Neither a dictionary nor a name: not a property list this can read.
            // There is nothing here to clear, and a copy of the words anywhere
            // else in the file is what the verifier looks for.
            Some(_) => {}
        }
    }
    Ok(cleared)
}

/// What one page's content-stream pass found and took.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Cleared {
    /// Shadow-text keys removed from marked-content property lists.
    keys: usize,
    /// The `/MCID` of every span a removal fell inside, in the order met.
    ///
    /// Not a carrier and not removed --- an `/MCID` is a *name*, and the words
    /// it leads to live in the structure tree. [`clear_struct_shadow_text`] is
    /// what follows it.
    mcids: Vec<i64>,
}

/// Strips shadow text from the structure elements a removal's spans belong to.
///
/// **The other home of the same carrier.** `docs/PLAN.md` §6's shadow-text row
/// names `/ActualText`, `/Alt` and `/E`, and PDF 32000-1 §14.9.3 to §14.9.5 put
/// them in two places: on a marked-content property list in the content stream,
/// which [`clear_shadow_text`] takes, and on a **structure element**, which this
/// does. Nothing in the content stream leads to the second one --- the link runs
/// the other way, through the page's `/StructParents` into the structure tree's
/// `/ParentTree`, a number tree whose entry for that key is an array indexed by
/// `/MCID`.
///
/// So a document tagged by Word or InDesign keeps a verbatim copy of a redacted
/// line in an object the page does not mention, and the only route to it is the
/// number the span carried.
///
/// **Ancestors go too**, for the reason an enclosing span does: an element above
/// the one that owns the content restates everything beneath it, so its
/// `/ActualText` is a copy of what was removed. The cost is real and is not
/// hidden --- a `/Sect` covering ten paragraphs loses its alternate text because
/// one word inside it went. That is route B's posture one level up again, and
/// the fixture shows it rather than arguing it: `text-marked.pdf`'s ancestor
/// also covers a line nobody redacted.
///
/// Every walk here is bounded. The parent tree and the `/P` chain are both
/// attacker-shaped and both can be made to loop.
///
/// # Errors
///
/// Nothing: a document with no structure tree, no `/StructParents`, a parent
/// tree this cannot follow, or an `/MCID` naming no element all mean there is no
/// second copy to take, which is the ordinary case for an untagged file. A
/// failure to *find* one is not a failure to remove it, and anything the walk
/// could not reach is still a copy of the words that [`crate::verify::scan`]
/// will find and report.
fn clear_struct_shadow_text(doc: &mut Document, page: ObjectId, mcids: &[i64]) -> usize {
    if mcids.is_empty() {
        return 0;
    }
    let Some(entries) = parent_tree_entry(doc, page) else {
        return 0;
    };
    // The elements first, then the edits: resolving needs `doc` immutably and
    // stripping needs it mutably, and an element reached twice --- two spans
    // under one element, or two elements under one ancestor --- must not be
    // counted twice.
    let mut doomed: Vec<ObjectId> = Vec::new();
    for mcid in mcids {
        let Ok(index) = usize::try_from(*mcid) else {
            continue;
        };
        let Some(element) = entries
            .get(index)
            .and_then(|entry| entry.as_reference().ok())
        else {
            continue;
        };
        let mut at = Some(element);
        // The `/P` chain, bounded for `pagetree::MAX_PARENTS`' reason: this runs
        // on a tree we did not write and a cycle in it is one dictionary away.
        //
        // **The count is the only thing stopping it, and that is deliberate.** A
        // visited-set filter on the step was written first and its mutation
        // SURVIVED --- correctly: an element already in `doomed` is not pushed
        // again, so a cycle changes nothing but how many idle turns the loop
        // takes before the bound ends it. Two mechanisms with the same limit
        // make one of them untestable, which `docs/TRAPS.md` records, and the
        // one to keep is the one that holds for *every* shape: a simple `A -> B
        // -> A` is caught by either, a chain a thousand deep only by the count.
        for _ in 0..MAX_ANCESTORS {
            let Some(id) = at else { break };
            if !doomed.contains(&id) {
                doomed.push(id);
            }
            at = doc
                .get_dictionary(id)
                .ok()
                .and_then(|element| element.get(b"P").ok())
                .and_then(|parent| parent.as_reference().ok());
        }
    }

    let mut cleared = 0usize;
    for id in doomed {
        let Ok(Object::Dictionary(element)) = doc.get_object_mut(id) else {
            continue;
        };
        for key in SHADOW_TEXT {
            if element.remove(key).is_some() {
                cleared += 1;
            }
        }
    }
    cleared
}

/// How far up a `/P` chain to walk before giving up.
///
/// A structure tree is a handful of levels deep in any real document. The bound
/// is here because the chain is the document's to shape and a cycle in it would
/// otherwise be an infinite loop inside a save --- the same reason `pagetree`
/// bounds its `/Parent` walk. The visited set below makes a *simple* cycle
/// terminate on its own; this catches the rest.
const MAX_ANCESTORS: usize = 64;

/// How deep a number tree may nest before this stops following it.
const MAX_TREE_DEPTH: usize = 32;

/// The parent tree's entry for a page: its structure elements, indexed by `/MCID`.
///
/// `/StructParents` on the page is the key; `/StructTreeRoot /ParentTree` is a
/// number tree; the value for that key is an array whose *n*th entry is the
/// element owning marked content `n`. PDF 32000-1 §14.7.4.4.
///
/// `None` for anything missing or the wrong shape, which is every untagged
/// document.
fn parent_tree_entry(doc: &Document, page: ObjectId) -> Option<Vec<Object>> {
    let key = doc
        .get_dictionary(page)
        .ok()?
        .get(b"StructParents")
        .and_then(Object::as_i64)
        .ok()?;
    let tree = doc
        .catalog()
        .ok()?
        .get(b"StructTreeRoot")
        .and_then(|object| doc.dereference(object).map(|(_, object)| object))
        .and_then(Object::as_dict)
        .ok()?
        .get(b"ParentTree")
        .and_then(|object| doc.dereference(object).map(|(_, object)| object))
        .and_then(Object::as_dict)
        .ok()?
        .clone();
    let found = number_tree_lookup(doc, &tree, key, 0)?;
    doc.dereference(&found)
        .ok()
        .and_then(|(_, object)| object.as_array().ok())
        .cloned()
}

/// One key's value in a number tree, following `/Kids` when the node has them.
///
/// Walked rather than assumed flat: a producer with many pages writes a tree of
/// `/Kids` with `/Limits`, and reading only `/Nums` would find nothing on every
/// document large enough to matter --- silently, since a miss and an untagged
/// page are the same answer.
///
/// `/Limits` is used to *skip* a subtree and never to conclude one holds the
/// key: a tree whose limits are wrong is malformed, and a search that trusts
/// them would answer nothing where a search that ignores them answers correctly.
fn number_tree_lookup(doc: &Document, node: &Dictionary, key: i64, depth: usize) -> Option<Object> {
    if depth > MAX_TREE_DEPTH {
        return None;
    }
    if let Ok(nums) = node
        .get(b"Nums")
        .and_then(|object| doc.dereference(object).map(|(_, object)| object))
        .and_then(Object::as_array)
    {
        for pair in nums.chunks_exact(2) {
            if doc
                .dereference(&pair[0])
                .ok()
                .and_then(|(_, object)| object.as_i64().ok())
                == Some(key)
            {
                return Some(pair[1].clone());
            }
        }
    }
    let kids = node
        .get(b"Kids")
        .and_then(|object| doc.dereference(object).map(|(_, object)| object))
        .and_then(Object::as_array)
        .ok()?
        .clone();
    for kid in kids {
        let Ok((_, Object::Dictionary(child))) = doc.dereference(&kid) else {
            continue;
        };
        if let Ok(limits) = child
            .get(b"Limits")
            .and_then(|object| doc.dereference(object).map(|(_, object)| object))
            .and_then(Object::as_array)
        {
            if let (Some(Ok(low)), Some(Ok(high))) = (
                limits.first().map(Object::as_i64),
                limits.get(1).map(Object::as_i64),
            ) {
                if key < low || key > high {
                    continue;
                }
            }
        }
        let child = child.clone();
        if let Some(found) = number_tree_lookup(doc, &child, key, depth + 1) {
            return Some(found);
        }
    }
    None
}

/// The dictionary a marked-content operand's name refers to.
///
/// `/Resources /Properties /<name>`, following the page's own resources and then
/// the ones it inherits, in that order --- which is the order a reader resolves
/// them in, so a page that shadows an inherited name gets its own.
///
/// `None` when anything on that path is missing or is not a dictionary. See
/// [`clear_shadow_text`] for why that is not refused.
fn property_list<'a>(doc: &'a Document, page: ObjectId, name: &[u8]) -> Option<&'a Dictionary> {
    let (inline, inherited) = doc.get_page_resources(page).ok()?;
    let mut sources: Vec<&Dictionary> = Vec::new();
    if let Some(dictionary) = inline {
        sources.push(dictionary);
    }
    for id in inherited {
        if let Ok(dictionary) = doc.get_dictionary(id) {
            sources.push(dictionary);
        }
    }
    for resources in sources {
        let Ok(properties) = resources
            .get(b"Properties")
            .and_then(|object| doc.dereference(object).map(|(_, object)| object))
            .and_then(Object::as_dict)
        else {
            continue;
        };
        if let Ok(entry) = properties
            .get(name)
            .and_then(|object| doc.dereference(object).map(|(_, object)| object))
            .and_then(Object::as_dict)
        {
            return Some(entry);
        }
    }
    None
}

/// Deletes the numbered show operators from a page's content stream.
///
/// `ordinals` are positions among the page's show operators, as [`covered`]
/// produces them. `text_objects` is how many text objects PDFium reported on
/// this page, and it is not decoration --- see the module docs.
///
/// The page's content is replaced with the re-encoded stream. Nothing else in
/// the document is touched, which is the point: a redaction that rewrote the
/// whole page would destroy tagged structure and optional-content membership as
/// a side effect, which spike 0.3 measured PDFium's own regeneration doing.
///
/// # Errors
///
/// The page has no content or it will not decode within [`MAX_CONTENT_BYTES`];
/// the show-operator count disagrees with `text_objects`; an ordinal names no
/// operator; or the rewritten stream will not encode.
pub fn remove_shows(
    doc: &mut Document,
    page: ObjectId,
    ordinals: &[usize],
    text_objects: usize,
) -> Result<Removed, String> {
    let data = doc
        .get_page_content_with_limit(page, MAX_CONTENT_BYTES)
        .map_err(|why| format!("the page's content stream could not be read: {why}"))?;
    let mut content = Content::decode(&data)
        .map_err(|why| format!("the content stream will not decode: {why}"))?;

    let shows: Vec<usize> = content
        .operations
        .iter()
        .enumerate()
        .filter(|(_, operation)| is_show(&operation.operator))
        .map(|(at, _)| at)
        .collect();

    // The correspondence guard. Refusing here is a redaction that did not
    // happen; proceeding on a mismatch is one that removes the wrong words and
    // reports success.
    if shows.len() != text_objects {
        return Err(format!(
            "the page has {} text-showing operator(s) and PDFium reported {text_objects} text \
             object(s). Removing by position needs those to agree, so nothing was removed.",
            shows.len()
        ));
    }

    // Descending, so an earlier removal does not move a later index. Collected
    // first because `ordinals` is the caller's order and this must not depend on
    // it -- an ascending list would silently delete the wrong operators.
    let mut positions: Vec<usize> = Vec::with_capacity(ordinals.len());
    for &ordinal in ordinals {
        let at = *shows.get(ordinal).ok_or_else(|| {
            format!(
                "there is no show operator {ordinal} on this page, which has {}",
                shows.len()
            )
        })?;
        positions.push(at);
    }
    positions.sort_unstable();
    positions.dedup();
    let removed = positions.len();

    // **Before the removal, and that is not a preference.** A span is addressed
    // by where its `BDC` sits among the operations, and deleting an operation
    // renumbers every one after it -- so a carrier cleared afterwards would be
    // found by walking indices that no longer mean what they meant. The same
    // renumbering `positions` is walked backwards to avoid, one level up.
    let carriers = clear_shadow_text(doc, page, &mut content.operations, &positions)?;

    for at in positions.into_iter().rev() {
        content.operations.remove(at);
    }

    // **After the content stream, and it could not be before it.** The link from
    // a span to its structure element is the `/MCID` the span carries, so the
    // numbers have to be read out of the operations first. Nothing about the
    // order matters to the tree itself --- it is addressed by number rather than
    // by position, which is exactly what the content-stream pass is not.
    let struct_carriers = clear_struct_shadow_text(doc, page, &carriers.mcids);

    let encoded = content
        .encode()
        .map_err(|why| format!("the rewritten content stream will not encode: {why}"))?;
    doc.change_page_content(page, encoded)
        .map_err(|why| format!("the page's content could not be replaced: {why}"))?;

    Ok(Removed {
        shows_before: shows.len(),
        removed,
        carriers: carriers.keys,
        struct_carriers,
    })
}

/// The four text-showing operators.
///
/// `Tj` and `TJ` show a string and an array; `'` and `"` show a string after
/// moving to the next line, and both draw glyphs exactly as the other two do.
/// Leaving either quote form out would make a redaction pass over a line that
/// used it.
#[must_use]
fn is_show(operator: &str) -> bool {
    matches!(operator, "Tj" | "TJ" | "'" | "\"")
}

/// How many outline entries to walk before giving up.
///
/// An outline is the document's to shape and a `/Next` or `/First` pointing
/// backwards is one dictionary away, so the walk needs a bound for the reason
/// [`MAX_ANCESTORS`] does. This one is a *visit* count rather than a depth,
/// because an outline's cycles are along the sibling chain as often as down it,
/// and a depth bound does not see a `/Next` that loops.
///
/// Generous on purpose: 131 entries is a real figure from one document measured
/// on 2026-08-27, and a table of contents for a standard runs to thousands. What
/// this bounds is a malformed file, not a large one --- so a document that hits
/// it has its outline walk cut short, which is reported as a carrier that could
/// not be cleared rather than passed over.
const MAX_OUTLINE_ITEMS: usize = 20_000;

/// The shortest title a removal will act on.
///
/// A title of one or two characters --- a chapter number, an initial --- is a
/// substring of almost any line, so matching on it would take the whole outline
/// off a document for the sake of a bookmark called `1`. The same floor the
/// verification scan uses, and for the same reason.
const MIN_OUTLINE_TITLE: usize = 4;

/// Outline entries whose title is text a removal took, and everything under
/// them.
///
/// **`docs/PLAN.md` §6's *Document level* row, and the one carrier a reader can
/// see in tpdf itself.** A bookmark's title is the heading it points at, so
/// redacting the heading leaves a verbatim copy in the outline --- and the
/// sidebar goes on showing it, so the words come back on screen in the file that
/// was supposed to have lost them.
///
/// **A string rule, where the same rule was refused for metadata**, and the
/// difference is measured rather than argued. Of 41 real PDFs, 8 carry outline
/// entries and **163 of their 165 titles are verbatim page text** --- against 4%
/// when each document's titles are matched against the *next* document's pages,
/// which is the control that makes the 99% mean anything. A title is the page's
/// own words; `/Info /Title` is a description of the document, and a description
/// that paraphrases a redacted line has nothing to match against at all.
///
/// **The direction is `taken.contains(title)`.** Route B removes the whole
/// text-showing operation, so what came out is a line and the bookmark names
/// part of it. The other direction would only fire on a bookmark that quotes
/// more than the line it points at.
///
/// **The entry and its subtree go; its ancestors do not**, which is the opposite
/// of [`clear_struct_shadow_text`] and is deliberate. A structure element's
/// `/Alt` on an ancestor *restates* what is beneath it, including what was
/// removed. An outline ancestor is a different heading, which nobody redacted,
/// and taking it would take every other section with it.
///
/// `taken` is the text the plan reported it would remove, which came from
/// PDFium through the font's own encoding. Not the operands
/// [`remove_shows`] deleted: those are font-encoded bytes, and on a Type0
/// document they are CIDs rather than characters.
#[must_use]
pub fn covered_outline(doc: &Document, taken: &[String]) -> Vec<ObjectId> {
    let Some(root) = outline_root(doc) else {
        return Vec::new();
    };
    let folded: Vec<String> = taken.iter().map(|line| fold(line)).collect();
    if folded.iter().all(|line| line.is_empty()) {
        return Vec::new();
    }

    let mut doomed: Vec<ObjectId> = Vec::new();
    // Depth-first, and the stack is what bounds it rather than recursion: this
    // walks a tree the document shaped, in a save, and a `/First` that points at
    // an ancestor is an infinite loop with no stack frame to run out of.
    let mut stack: Vec<ObjectId> = first_child(doc, root).into_iter().collect();
    let mut seen: Vec<ObjectId> = Vec::new();
    let mut visits = 0usize;
    while let Some(id) = stack.pop() {
        visits += 1;
        if visits > MAX_OUTLINE_ITEMS {
            break;
        }
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let Ok(item) = doc.get_dictionary(id) else {
            continue;
        };
        // Siblings and children both, so a match anywhere in the tree is found.
        // A doomed item's children are collected below rather than here: they go
        // because their parent does, whatever their own titles say.
        if let Ok(next) = item.get(b"Next").and_then(Object::as_reference) {
            stack.push(next);
        }
        if let Ok(child) = item.get(b"First").and_then(Object::as_reference) {
            stack.push(child);
        }
        let title = item
            .get(b"Title")
            .and_then(Object::as_str)
            .map(crate::annots::decode_text_string)
            .unwrap_or_default();
        let title = fold(&title);
        if title.len() < MIN_OUTLINE_TITLE {
            continue;
        }
        if folded.iter().any(|line| line.contains(&title)) {
            doomed.push(id);
        }
    }

    // The subtrees, after the search rather than during it, so an entry that
    // matches is taken whole exactly once however it was reached.
    let mut all: Vec<ObjectId> = Vec::new();
    for id in doomed {
        collect_subtree(doc, id, &mut all);
    }
    all
}

/// Folds a title or a line for comparison.
///
/// Whitespace collapses because a title is typed by a producer and the line is
/// laid out by a typesetter: the same words routinely differ by a line break, a
/// double space or a non-breaking space, and none of those is a difference in
/// what the bookmark says. Case folds for the reason `search.rs` folds --- see
/// its note on why `char::to_lowercase` is the wrong operation for matching.
fn fold(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut space = false;
    for ch in caseless::default_case_fold_str(value).chars() {
        if ch.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(ch);
    }
    out
}

/// The catalog's `/Outlines`, if the document has one.
fn outline_root(doc: &Document) -> Option<ObjectId> {
    doc.catalog()
        .ok()?
        .get(b"Outlines")
        .and_then(Object::as_reference)
        .ok()
}

/// The `/First` of a node, if it names one.
fn first_child(doc: &Document, node: ObjectId) -> Option<ObjectId> {
    doc.get_dictionary(node)
        .ok()?
        .get(b"First")
        .and_then(Object::as_reference)
        .ok()
}

/// Adds `id` and everything under it to `into`, once each.
fn collect_subtree(doc: &Document, id: ObjectId, into: &mut Vec<ObjectId>) {
    let mut stack = vec![id];
    let mut visits = 0usize;
    while let Some(at) = stack.pop() {
        visits += 1;
        if visits > MAX_OUTLINE_ITEMS {
            return;
        }
        if into.contains(&at) {
            continue;
        }
        into.push(at);
        let Ok(item) = doc.get_dictionary(at) else {
            continue;
        };
        if let Ok(child) = item.get(b"First").and_then(Object::as_reference) {
            stack.push(child);
        }
        // The children's siblings, and **not** `at`'s own `/Next`: a subtree is
        // one entry and what hangs under it. Following the sibling chain from
        // the root of the subtree would take everything after it in the
        // document, which is the outline equivalent of dropping the whole tree.
        if at != id {
            if let Ok(next) = item.get(b"Next").and_then(Object::as_reference) {
                stack.push(next);
            }
        }
    }
}

/// Removes outline entries, repairing the chains they were in first.
///
/// **`pagetree::forget` alone is wrong here, and it looks right.** It removes a
/// reference from an *array* by dropping the element, which is correct for
/// `/Annots`, and from a *dictionary* by dropping the key, which is correct for
/// `/Info`. An outline is neither: it is a doubly-linked sibling chain. Forget
/// an item in the middle of one and its predecessor loses the `/Next` key that
/// named it --- so a reader walks `/First`, `/Next`, and stops one entry early.
/// Every entry after the removed one is unreachable, the file is valid, no
/// parser complains, and nothing says so.
///
/// So the chain is spliced before the objects go: each removed subtree's root
/// has its neighbours joined to each other and its parent's `/First` and `/Last`
/// moved off it. Only the *roots* need it --- an item inside a doomed subtree
/// has a parent that is going too, and splicing a chain that is about to be
/// deleted whole is work with no reader.
///
/// `/Count` is **recomputed** for the whole tree afterwards rather than
/// decremented. A delta needs the number of visible items in each removed
/// subtree, which is the sign of every `/Count` beneath it; recomputing needs
/// only the tree that is left, and cannot drift. The sign is preserved, because
/// it is not a count at all: a negative `/Count` means the reader had that
/// section collapsed, which the removal has no business changing.
///
/// # Errors
///
/// Only what [`crate::pagetree::forget`] refuses: an object nested deeper than
/// the sweep's bound.
pub fn drop_outline_items(doc: &mut Document, doomed: &[ObjectId]) -> Result<usize, String> {
    if doomed.is_empty() {
        return Ok(0);
    }
    let doomed_set: std::collections::HashSet<ObjectId> = doomed.iter().copied().collect();

    for &id in doomed {
        let Ok(item) = doc.get_dictionary(id) else {
            continue;
        };
        let parent = item.get(b"Parent").and_then(Object::as_reference).ok();
        // A root of a removed subtree is one whose parent is staying. Anything
        // else is being deleted along with the dictionary that names it.
        if parent.is_some_and(|at| doomed_set.contains(&at)) {
            continue;
        }
        let prev = item.get(b"Prev").and_then(Object::as_reference).ok();
        let next = item.get(b"Next").and_then(Object::as_reference).ok();

        if let Some(at) = prev {
            set_or_clear(doc, at, b"Next", next);
        }
        if let Some(at) = next {
            set_or_clear(doc, at, b"Prev", prev);
        }
        if let Some(at) = parent {
            if names(doc, at, b"First", id) {
                set_or_clear(doc, at, b"First", next);
            }
            if names(doc, at, b"Last", id) {
                set_or_clear(doc, at, b"Last", prev);
            }
        }
    }

    crate::pagetree::forget(doc, &doomed_set)?;
    if let Some(root) = outline_root(doc) {
        // An outline whose every entry went is a `/Outlines` naming nothing.
        // Dropped rather than left: a root with no `/First` is legal and a
        // reader draws an empty panel for it, which reads as a document that
        // never had an outline. This one did, and saying so is not this
        // function's job --- the count it returns is.
        if first_child(doc, root).is_none() {
            crate::pagetree::drop_outline(doc)?;
        } else {
            recount(doc, root, 0);
        }
    }
    Ok(doomed.len())
}

/// Sets `key` on `at` to `value`, or removes it when there is no value.
///
/// The removal is the half that matters: a spliced-out first sibling leaves its
/// successor with no `/Prev`, and writing a null there rather than dropping the
/// key gives readers a reference to nothing.
fn set_or_clear(doc: &mut Document, at: ObjectId, key: &[u8], value: Option<ObjectId>) {
    let Ok(Object::Dictionary(dict)) = doc.get_object_mut(at) else {
        return;
    };
    match value {
        Some(id) => dict.set(key.to_vec(), Object::Reference(id)),
        None => {
            dict.remove(key);
        }
    }
}

/// Whether `at`'s `key` names `id`.
fn names(doc: &Document, at: ObjectId, key: &[u8], id: ObjectId) -> bool {
    doc.get_dictionary(at)
        .ok()
        .and_then(|dict| dict.get(key).ok())
        .and_then(|value| value.as_reference().ok())
        == Some(id)
}

/// Rewrites `/Count` on `node` and everything under it, returning how many
/// entries `node` shows.
///
/// A node's `/Count` is its visible descendants: its children, plus the
/// descendants of any child that is *open*. Sign carries whether it is open, and
/// is read off the value that is there --- the removal must not expand a section
/// the reader had collapsed.
fn recount(doc: &mut Document, node: ObjectId, depth: usize) -> i64 {
    if depth > MAX_TREE_DEPTH {
        return 0;
    }
    let mut children: Vec<ObjectId> = Vec::new();
    let mut at = first_child(doc, node);
    while let Some(id) = at {
        if children.len() > MAX_OUTLINE_ITEMS || children.contains(&id) {
            break;
        }
        children.push(id);
        at = doc
            .get_dictionary(id)
            .ok()
            .and_then(|item| item.get(b"Next").ok())
            .and_then(|value| value.as_reference().ok());
    }

    let mut visible = i64::try_from(children.len()).unwrap_or(i64::MAX);
    for child in children {
        let open = doc
            .get_dictionary(child)
            .ok()
            .and_then(|item| item.get(b"Count").ok())
            .and_then(|value| value.as_i64().ok())
            .is_none_or(|count| count > 0);
        let under = recount(doc, child, depth + 1);
        if under == 0 {
            // No children left. `/Count` is only defined for a node that has
            // some, so the key goes rather than being written as zero.
            if let Ok(Object::Dictionary(item)) = doc.get_object_mut(child) {
                item.remove(b"Count");
            }
            continue;
        }
        if open {
            visible += under;
        }
        if let Ok(Object::Dictionary(item)) = doc.get_object_mut(child) {
            item.set(
                b"Count".to_vec(),
                Object::Integer(if open { under } else { -under }),
            );
        }
    }
    if depth == 0 {
        if let Ok(Object::Dictionary(item)) = doc.get_object_mut(node) {
            item.set(b"Count".to_vec(), Object::Integer(visible));
        }
    }
    visible
}

/// The shortest field value a removal will act on.
///
/// [`MIN_OUTLINE_TITLE`]'s reason, and it bites harder here: a form is full of
/// short answers. A field holding `Yes`, `N` or a two-digit day is a substring
/// of almost any line, and matching on one would take a whole form apart for the
/// sake of a checkbox.
const MIN_FIELD_VALUE: usize = 4;

/// Whether the document carries an XFA form.
///
/// **`docs/PLAN.md` §6 refuses a redaction of one, and this is what that refusal
/// reads.** XFA is a dead Adobe extension that keeps a *complete second copy* of
/// the form's data as XML in `/AcroForm /XFA`, entirely separate from the
/// `/AcroForm` field values a redaction can reach. Removing a field's `/V` while
/// leaving the packet holding the same answer is exactly the confident lie §6
/// opens by forbidding --- and it is worse than an ordinary miss, because nothing
/// a reader can see in any viewer would show the copy is there.
///
/// Sanitising the packet properly is a project of its own: it is an XML dialect
/// with its own datasets, its own templates and its own scripting, so a rule
/// that reached into it would be a second document editor. Refusing is the
/// honest answer and §6 chose it before any of this was written.
///
/// **The key alone, not its contents.** `/XFA` may be a stream or an array of
/// alternating names and streams, and either way its presence is the whole
/// question --- so this reads no packet, which also means an XFA document costs
/// nothing to refuse.
#[must_use]
pub fn has_xfa(doc: &Document) -> bool {
    doc.catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"AcroForm").ok())
        .and_then(|object| doc.dereference(object).map(|(_, object)| object).ok())
        .and_then(|object| object.as_dict().ok().cloned())
        .is_some_and(|form| form.has(b"XFA"))
}

/// Form fields a redaction has to take with it, and the widgets under them.
///
/// **`docs/PLAN.md` §6's *Forms* row.** A filled field's value *is* the content ---
/// a name, an account number, a date somebody typed --- so where a form carries
/// one it is the document rather than a description of it.
///
/// Two rules, and only the second is a string match:
///
/// **A field whose widgets have all gone.** `covered_annots` already removes a
/// widget over a region, because a widget *is* an annotation. What it leaves is
/// the field dictionary above it, which is a separate object when the field has
/// `/Kids` --- so the value stays in the file with nothing drawing it. Nothing
/// displays it and every search finds it, which is the worst combination: a
/// reader looking at the redacted page sees the field gone. Measured before this
/// was written, on a fixture with a parent holding the value and one kid over
/// the region: the kid went, the parent survived with its `/V`.
///
/// **A field whose value is text the removal took.** §6 names *widgets outside
/// the redacted rectangle* explicitly, and this is what reaches them: the same
/// answer typed into a second copy of the field, or a field whose widget sits on
/// another page. `taken.contains(value)` for [`covered_outline`]'s reason ---
/// route B removes a whole line and the field holds part of it.
///
/// `/DV` is read as well as `/V`. A default value is the same string in the same
/// dictionary, put there by whoever built the form, and a redaction that took the
/// answer and left the default it was pre-filled from has removed nothing.
///
/// **A name is not a value.** A checkbox's `/V` is `/Off` or `/Yes` --- a *name*
/// object, not a string --- and comparing one against page text is comparing two
/// different things. Only strings are read, which also means a checkbox is never
/// taken by the value rule; if its widget is over the region it goes as an
/// annotation like anything else.
///
/// `gone` is what the annotation pass removed, which is why this runs after it
/// and not beside it: the first rule is *has everything under this field been
/// taken*, and that is not answerable until it has.
#[must_use]
pub fn covered_fields(doc: &Document, taken: &[String], gone: &HashSet<ObjectId>) -> Vec<ObjectId> {
    let Some(form) = doc
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"AcroForm").ok())
        .and_then(|object| doc.dereference(object).map(|(_, object)| object).ok())
        .and_then(|object| object.as_dict().ok().cloned())
    else {
        return Vec::new();
    };
    let Ok(fields) = form
        .get(b"Fields")
        .and_then(|object| doc.dereference(object).map(|(_, object)| object))
        .and_then(Object::as_array)
    else {
        return Vec::new();
    };
    let folded: Vec<String> = taken.iter().map(|line| fold(line)).collect();

    let mut doomed: Vec<ObjectId> = Vec::new();
    let mut seen: Vec<ObjectId> = Vec::new();
    let mut queue: Vec<ObjectId> = fields
        .iter()
        .filter_map(|entry| entry.as_reference().ok())
        .collect();
    let mut budget = MAX_FIELD_NODES;
    while let Some(id) = queue.pop() {
        // Charged before anything is read, so a refusal costs one pop rather
        // than one parse --- `docinfo::read_signatures` bounds the same walk the
        // same way and for the same reason.
        let Some(left) = budget.checked_sub(1) else {
            break;
        };
        budget = left;
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let Ok(field) = doc.get_dictionary(id) else {
            continue;
        };

        let kids: Vec<ObjectId> = field
            .get(b"Kids")
            .and_then(|object| doc.dereference(object).map(|(_, object)| object))
            .and_then(Object::as_array)
            .map(|array| {
                array
                    .iter()
                    .filter_map(|entry| entry.as_reference().ok())
                    .collect()
            })
            .unwrap_or_default();
        queue.extend(kids.iter().copied());

        // Rule one. `has(b"Kids")` rather than `!kids.is_empty()`: a field whose
        // array `forget` has emptied still has the key, and that emptiness is
        // precisely the signal. A merged field has no `/Kids` at all and was
        // removed as an annotation, so it must not fall in here.
        let orphaned =
            field.has(b"Kids") && (kids.is_empty() || kids.iter().all(|kid| gone.contains(kid)));

        // Rule two.
        let carries = [b"V".as_slice(), b"DV".as_slice()].into_iter().any(|key| {
            let Ok(value) = field
                .get(key)
                .and_then(|object| doc.dereference(object).map(|(_, object)| object))
            else {
                return false;
            };
            // A name is not a string, and `as_str` on `/Off` answers nothing.
            let Ok(bytes) = value.as_str() else {
                return false;
            };
            let text = fold(&crate::annots::decode_text_string(bytes));
            text.len() >= MIN_FIELD_VALUE && folded.iter().any(|line| line.contains(&text))
        });

        if orphaned || carries {
            doomed.push(id);
        }
    }

    // The subtrees, so a field taken by its value takes its widgets with it.
    let mut all: Vec<ObjectId> = Vec::new();
    for id in doomed {
        collect_field_subtree(doc, id, &mut all);
    }
    all
}

/// How many field-tree nodes to walk before giving up.
///
/// `docinfo.rs` bounds its own walk of the same tree at the same number and for
/// the same reason: the tree is the document's to shape, and a `/Kids` naming an
/// ancestor is one dictionary away.
const MAX_FIELD_NODES: usize = 20_000;

/// Adds `id` and every `/Kids` descendant to `into`, once each.
fn collect_field_subtree(doc: &Document, id: ObjectId, into: &mut Vec<ObjectId>) {
    let mut stack = vec![id];
    let mut visits = 0usize;
    while let Some(at) = stack.pop() {
        visits += 1;
        if visits > MAX_FIELD_NODES {
            return;
        }
        if into.contains(&at) {
            continue;
        }
        into.push(at);
        let Ok(field) = doc.get_dictionary(at) else {
            continue;
        };
        if let Ok(kids) = field
            .get(b"Kids")
            .and_then(|object| doc.dereference(object).map(|(_, object)| object))
            .and_then(Object::as_array)
        {
            stack.extend(kids.iter().filter_map(|kid| kid.as_reference().ok()));
        }
    }
}

/// Removes form fields, and the `/AcroForm` when nothing is left in it.
///
/// **`pagetree::forget` is the right instrument here, which is worth saying
/// after the outline.** Every structure naming a field is an *array* ---
/// `/AcroForm /Fields`, a field's `/Kids`, the page's `/Annots`, and `/CO`, the
/// calculation order --- and `forget` drops an array element by removing it,
/// leaving the array shorter and correct. The outline's chain was the exception,
/// not this. A field's `/Parent` back-pointer is the one dictionary key
/// involved, and it points *up*, so a subtree removed whole carries it away.
///
/// # Errors
///
/// Only what [`crate::pagetree::forget`] refuses: an object nested deeper than
/// the sweep's bound.
pub fn drop_fields(doc: &mut Document, doomed: &[ObjectId]) -> Result<usize, String> {
    if doomed.is_empty() {
        return Ok(0);
    }
    crate::pagetree::forget(doc, &doomed.iter().copied().collect())?;

    // An `/AcroForm` with no fields left. Dropped whole rather than kept empty,
    // for the reason an emptied outline is: a form with no fields reads as a
    // document that never had one, and what it still carries --- `/DA`, `/DR`,
    // `/NeedAppearances` --- describes fields that are gone.
    let empty = doc
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"AcroForm").ok())
        .and_then(|object| doc.dereference(object).map(|(_, object)| object).ok())
        .and_then(|object| object.as_dict().ok().cloned())
        .is_some_and(|form| {
            form.get(b"Fields")
                .and_then(Object::as_array)
                .is_ok_and(|fields| fields.is_empty())
        });
    if empty {
        if let Ok(catalog) = doc.catalog_mut() {
            catalog.remove(b"AcroForm");
        }
    }
    Ok(doomed.len())
}

#[cfg(test)]
mod tests {
    use super::{
        covered, covered_annots, is_show, overlaps, remove_shows, PageObject, Plan, Unhandled,
    };
    use lopdf::content::Content;
    use lopdf::{dictionary, Document, Object, Stream};

    fn text(bounds: [f32; 4]) -> PageObject {
        PageObject {
            bounds,
            kind: "text".to_string(),
        }
    }

    fn image(bounds: [f32; 4]) -> PageObject {
        PageObject {
            bounds,
            kind: "image".to_string(),
        }
    }

    /// A one-page document whose content stream is exactly `stream`.
    ///
    /// Built rather than loaded: these checks are about which *operator* is
    /// removed, and a fixture would make every assertion depend on what some
    /// generator happened to emit. `docs/TRAPS.md` records a hand-built fixture
    /// agreeing with its author, so nothing here asserts anything a real
    /// document decides --- the corpus control for that lives in the probe.
    fn one_page(stream: &str) -> (Document, lopdf::ObjectId) {
        let mut doc = Document::with_version("1.7");
        let content = doc.add_object(Stream::new(dictionary! {}, stream.as_bytes().to_vec()));
        let pages_id = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog);
        (doc, page)
    }

    /// The operators a page's content stream holds, in order.
    fn operators(doc: &Document, page: lopdf::ObjectId) -> Vec<String> {
        let data = doc
            .get_page_content_with_limit(page, super::MAX_CONTENT_BYTES)
            .expect("content");
        Content::decode(&data)
            .expect("decode")
            .operations
            .into_iter()
            .map(|operation| operation.operator)
            .collect()
    }

    /// The page's content stream as it is stored, which is where a carrier lives.
    ///
    /// [`operators`] and [`shown`] both read the decoded operation list, and a
    /// property list is an **operand** --- so neither of them can see a
    /// shadow-text key at all, and a check built on either would pass whether the
    /// key went or stayed. This is what makes the carrier assertions able to
    /// fail.
    fn stream(doc: &Document, page: lopdf::ObjectId) -> String {
        let data = doc
            .get_page_content_with_limit(page, super::MAX_CONTENT_BYTES)
            .expect("content");
        String::from_utf8_lossy(&data).into_owned()
    }

    /// A one-page document whose `/Resources /Properties` holds one named entry.
    ///
    /// The shared form of a marked-content property list: `BDC` names it instead
    /// of carrying it, and the dictionary it names may be used by any number of
    /// other pages --- which is the whole reason `clear_shadow_text` will not
    /// edit one.
    fn one_page_named(
        stream: &str,
        name: &str,
        entry: lopdf::Dictionary,
    ) -> (Document, lopdf::ObjectId) {
        let (mut doc, page) = one_page(stream);
        let mut properties = lopdf::Dictionary::new();
        properties.set(name.as_bytes().to_vec(), Object::Dictionary(entry));
        let resources = doc.add_object(dictionary! { "Properties" => properties });
        let Ok(Object::Dictionary(dictionary)) = doc.get_object_mut(page) else {
            panic!("the page is a dictionary")
        };
        dictionary.set("Resources", resources);
        (doc, page)
    }

    /// A one-page document whose `/Annots` is an inline array of these.
    ///
    /// Returns the ids in the order given, so a check can name the one it means.
    fn one_page_annots(
        stream: &str,
        annots: Vec<lopdf::Dictionary>,
    ) -> (Document, lopdf::ObjectId, Vec<lopdf::ObjectId>) {
        let (mut doc, page) = one_page(stream);
        let ids: Vec<lopdf::ObjectId> = annots
            .into_iter()
            .map(|annot| doc.add_object(annot))
            .collect();
        let entries: Vec<Object> = ids.iter().map(|id| Object::Reference(*id)).collect();
        let Ok(Object::Dictionary(dictionary)) = doc.get_object_mut(page) else {
            panic!("the page is a dictionary")
        };
        dictionary.set("Annots", entries);
        (doc, page, ids)
    }

    /// A `/Text` annotation at these corners, with a body nobody reads here.
    fn note(rect: [f32; 4]) -> lopdf::Dictionary {
        dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![
                Object::Real(rect[0]), Object::Real(rect[1]),
                Object::Real(rect[2]), Object::Real(rect[3]),
            ],
            "Contents" => Object::string_literal("a note"),
        }
    }

    /// A one-page tagged document: `leaves` structure elements, one per `/MCID`.
    ///
    /// The parent tree's entry for the page lists them in order, which is the
    /// shape §14.7.4.4 describes and the one every producer emits. Each element
    /// carries an `/ActualText` naming itself, so a check can say **which** one
    /// went. With `ancestor`, all of them hang under one `/Part` carrying its
    /// own.
    ///
    /// Returns the page, the leaves in `/MCID` order, and the ancestor.
    fn one_page_tagged(
        stream: &str,
        leaves: usize,
        ancestor: bool,
    ) -> (
        Document,
        lopdf::ObjectId,
        Vec<lopdf::ObjectId>,
        Option<lopdf::ObjectId>,
    ) {
        let (mut doc, page) = one_page(stream);
        let root = doc.new_object_id();
        let parent = ancestor.then(|| doc.new_object_id());
        let ids: Vec<lopdf::ObjectId> = (0..leaves)
            .map(|at| {
                doc.add_object(dictionary! {
                    "Type" => "StructElem",
                    "S" => "P",
                    "P" => parent.unwrap_or(root),
                    "Pg" => page,
                    "K" => at as i64,
                    "ActualText" => Object::string_literal(format!("leaf {at}")),
                })
            })
            .collect();
        if let Some(parent) = parent {
            let kids: Vec<Object> = ids.iter().map(|id| Object::Reference(*id)).collect();
            doc.objects.insert(
                parent,
                Object::Dictionary(dictionary! {
                    "Type" => "StructElem",
                    "S" => "Part",
                    "P" => root,
                    "K" => kids,
                    "ActualText" => Object::string_literal("ancestor"),
                }),
            );
        }
        let entry: Vec<Object> = ids.iter().map(|id| Object::Reference(*id)).collect();
        let tree = doc.add_object(dictionary! {
            "Nums" => vec![Object::Integer(0), Object::Array(entry)],
        });
        doc.objects.insert(
            root,
            Object::Dictionary(dictionary! {
                "Type" => "StructTreeRoot",
                "ParentTree" => tree,
            }),
        );
        let catalog = doc
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .expect("catalog");
        let Ok(Object::Dictionary(dictionary)) = doc.get_object_mut(catalog) else {
            panic!("the catalog is a dictionary")
        };
        dictionary.set("StructTreeRoot", root);
        let Ok(Object::Dictionary(dictionary)) = doc.get_object_mut(page) else {
            panic!("the page is a dictionary")
        };
        dictionary.set("StructParents", 0);
        (doc, page, ids, parent)
    }

    /// Whether an object still carries `/ActualText`.
    fn has_shadow_text(doc: &Document, id: lopdf::ObjectId) -> bool {
        doc.get_dictionary(id)
            .is_ok_and(|dictionary| dictionary.has(b"ActualText"))
    }

    /// The string each surviving show operator draws, in order.
    fn shown(doc: &Document, page: lopdf::ObjectId) -> Vec<String> {
        let data = doc
            .get_page_content_with_limit(page, super::MAX_CONTENT_BYTES)
            .expect("content");
        Content::decode(&data)
            .expect("decode")
            .operations
            .into_iter()
            .filter(|operation| is_show(&operation.operator))
            .filter_map(|operation| match operation.operands.first() {
                Some(Object::String(bytes, _)) => Some(String::from_utf8_lossy(bytes).into_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_region_over_one_text_object_names_it() {
        let objects = [
            text([0.0, 0.0, 100.0, 20.0]),
            text([0.0, 40.0, 100.0, 60.0]),
        ];
        let plan = covered(&objects, [10.0, 45.0, 20.0, 55.0]);
        assert_eq!(plan.shows, vec![1]);
        assert!(plan.is_complete());
    }

    /// The control: a region over nothing removes nothing, and says so.
    ///
    /// Without it every check here is satisfied by a function that names every
    /// object on the page.
    #[test]
    fn a_region_over_nothing_names_nothing() {
        let objects = [text([0.0, 0.0, 100.0, 20.0])];
        assert_eq!(
            covered(&objects, [500.0, 500.0, 600.0, 600.0]),
            Plan::default()
        );
    }

    /// **Ordinals count text objects, not objects.**
    ///
    /// The one that decides whether the right words are deleted. PDFium
    /// enumerates every object on the page and the content stream's show
    /// operators are only the text ones, so an image sitting between two lines
    /// shifts the numbering by one --- and a redaction addressed by a shifted
    /// ordinal removes a line the reader did not mark, while reporting success.
    #[test]
    fn an_image_between_two_lines_does_not_shift_the_text_ordinals() {
        let objects = [
            text([0.0, 0.0, 100.0, 20.0]),
            image([0.0, 25.0, 100.0, 35.0]),
            text([0.0, 40.0, 100.0, 60.0]),
        ];
        let plan = covered(&objects, [10.0, 45.0, 20.0, 55.0]);
        assert_eq!(
            plan.shows,
            vec![1],
            "the second TEXT object is ordinal 1, whatever else is on the page"
        );
    }

    /// §6's deny-by-default rule: an object this cannot remove is reported.
    #[test]
    fn an_image_in_the_region_makes_the_plan_incomplete() {
        let objects = [
            text([0.0, 0.0, 100.0, 20.0]),
            image([0.0, 0.0, 100.0, 20.0]),
        ];
        let plan = covered(&objects, [10.0, 5.0, 20.0, 15.0]);
        assert_eq!(plan.shows, vec![0], "the text is still removable");
        assert!(
            !plan.is_complete(),
            "and the picture over it is not, so the region is not redactable here"
        );
        assert_eq!(
            plan.unhandled,
            vec![Unhandled {
                at: 1,
                kind: "image".to_string()
            }],
            "the finding names which object and what it is"
        );
        assert!(
            plan.unhandled[0]
                .sentence()
                .contains("object 1 is of kind image"),
            "and the sentence it renders as names both: {:?}",
            plan.unhandled[0].sentence()
        );
    }

    /// Touching is not overlapping, in both directions.
    #[test]
    fn a_region_flush_against_a_line_does_not_eat_it() {
        let line = [0.0, 0.0, 100.0, 20.0];
        assert!(
            !overlaps(line, [100.0, 0.0, 200.0, 20.0]),
            "sharing the right edge"
        );
        assert!(!overlaps(line, [0.0, 20.0, 100.0, 40.0]), "sharing the top");
        // The control, one unit in: without it the two above are satisfied by an
        // `overlaps` that always answers no.
        assert!(
            overlaps(line, [99.0, 0.0, 200.0, 20.0]),
            "one unit of overlap"
        );
    }

    /// A region handed over upside down still covers what it looks like it covers.
    ///
    /// **Each axis reversed on its own**, which the first version of this did
    /// not do: reversing both at once left either normalisation able to rescue
    /// the other, and a mutation dropping one of them survived. The rectangles
    /// are narrower than the region on the reversed axis, because that is the
    /// only shape where an un-normalised comparison answers *no* --- a wide
    /// object overlaps either way and cannot tell the two apart.
    #[test]
    fn a_region_with_its_corners_the_other_way_round_still_overlaps() {
        // x reversed: region spans 20..80, written 80 then 20.
        let narrow_x = [text([50.0, 0.0, 60.0, 100.0])];
        assert_eq!(covered(&narrow_x, [80.0, 0.0, 20.0, 100.0]).shows, vec![0]);

        // y reversed: region spans 20..80, written 80 then 20.
        let narrow_y = [text([0.0, 50.0, 100.0, 60.0])];
        assert_eq!(covered(&narrow_y, [0.0, 80.0, 100.0, 20.0]).shows, vec![0]);

        // Both, which is the shape a reader's upward-left drag produces.
        assert_eq!(
            covered(&[text([50.0, 50.0, 60.0, 60.0])], [80.0, 80.0, 20.0, 20.0]).shows,
            vec![0]
        );
    }

    /// The same for an *object* whose bounds arrive reversed.
    ///
    /// `PageObject::bounds` is a public field, so this is the caller's to get
    /// wrong as much as the region is, and both are normalised by one function.
    #[test]
    fn an_object_with_its_corners_the_other_way_round_is_still_found() {
        let reversed = [PageObject {
            bounds: [60.0, 100.0, 50.0, 0.0],
            kind: "text".to_string(),
        }];
        assert_eq!(covered(&reversed, [20.0, 0.0, 80.0, 100.0]).shows, vec![0]);
    }

    #[test]
    fn the_two_quote_operators_are_show_operators() {
        for operator in ["Tj", "TJ", "'", "\""] {
            assert!(is_show(operator), "{operator}");
        }
        assert!(!is_show("Td"), "moving the cursor draws nothing");
    }

    #[test]
    fn removing_a_show_operator_leaves_every_other_operator_alone() {
        let (mut doc, page) = one_page("BT /F1 12 Tf (one) Tj 0 -14 Td (two) Tj ET");
        let before = operators(&doc, page);
        let removed = remove_shows(&mut doc, page, &[0], 2).expect("remove");
        assert_eq!(removed.shows_before, 2);
        assert_eq!(removed.removed, 1);
        assert_eq!(shown(&doc, page), vec!["two".to_string()]);
        assert_eq!(
            operators(&doc, page).len(),
            before.len() - 1,
            "exactly one operator went"
        );
        assert!(
            operators(&doc, page).contains(&"Tf".to_string()),
            "the font is still selected, so the surviving line still draws"
        );
    }

    /// **Removal walks backwards, and three lines is what proves it.**
    ///
    /// Removing two operators front-first shifts everything after the first
    /// removal down by one, so the second deletion lands on the wrong operator.
    /// With two lines the wrong answer and the right one are the same; with
    /// three, deleting 0 and 1 ascending would leave `two` and remove `three`.
    #[test]
    fn removing_two_operators_removes_the_two_that_were_named() {
        let (mut doc, page) = one_page("BT (one) Tj 0 -14 Td (two) Tj 0 -14 Td (three) Tj ET");
        remove_shows(&mut doc, page, &[0, 1], 3).expect("remove");
        assert_eq!(shown(&doc, page), vec!["three".to_string()]);
    }

    /// The correspondence guard, and nothing may change when it fires.
    #[test]
    fn a_count_that_disagrees_with_pdfium_refuses_and_removes_nothing() {
        let (mut doc, page) = one_page("BT (one) Tj 0 -14 Td (two) Tj ET");
        let before = shown(&doc, page);
        let why = remove_shows(&mut doc, page, &[0], 5).expect_err("must refuse");
        assert!(
            why.contains("2 text-showing operator(s)") && why.contains("reported 5"),
            "the message says both numbers: {why}"
        );
        assert!(why.contains("nothing was removed"), "{why}");
        assert_eq!(shown(&doc, page), before, "and nothing was");
    }

    #[test]
    fn an_ordinal_past_the_end_refuses_and_removes_nothing() {
        let (mut doc, page) = one_page("BT (one) Tj 0 -14 Td (two) Tj ET");
        let before = shown(&doc, page);
        let why = remove_shows(&mut doc, page, &[7], 2).expect_err("must refuse");
        assert!(why.contains("no show operator 7"), "{why}");
        assert_eq!(shown(&doc, page), before);
    }

    /// A repeated ordinal removes one operator, not two.
    ///
    /// `covered` cannot produce one, so this is about the function's own
    /// contract rather than about that path --- and without the dedup the second
    /// pass deletes whatever moved into the slot.
    #[test]
    fn the_same_ordinal_twice_removes_one_operator() {
        let (mut doc, page) = one_page("BT (one) Tj 0 -14 Td (two) Tj 0 -14 Td (three) Tj ET");
        let removed = remove_shows(&mut doc, page, &[1, 1], 3).expect("remove");
        assert_eq!(removed.removed, 1);
        assert_eq!(
            shown(&doc, page),
            vec!["one".to_string(), "three".to_string()]
        );
    }
    /// The carrier the whole increment is about, with its own control.
    ///
    /// `/ActualText` is a second copy of the line, written beside the glyphs by
    /// every tagged producer. Removing the show operator takes the drawing; the
    /// words stay unless the property list goes too. The first assertion is the
    /// control --- without it the check passes on a fixture that never had one.
    #[test]
    fn a_span_the_removal_touched_loses_its_copy_of_the_words() {
        let (mut doc, page) = one_page(
            "/Span << /ActualText (account 4711-0815) >> BDC\nBT (account 4711-0815) Tj ET\nEMC",
        );
        assert!(
            stream(&doc, page).contains("4711-0815"),
            "the fixture must carry the words twice, or this cannot fail"
        );
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.carriers, 1);
        assert!(
            !stream(&doc, page).contains("4711-0815"),
            "the words are gone from the property list as well: {}",
            stream(&doc, page)
        );
        assert!(
            !stream(&doc, page).contains("ActualText"),
            "and so is the key that held them"
        );
    }

    /// The over-removal control, and the reason the span is *touched* not *any*.
    ///
    /// A page with two tagged lines, one of them redacted. Clearing every span
    /// on the page would pass the test above perfectly and destroy the
    /// accessibility text of every line the reader did not mark.
    #[test]
    fn a_span_no_removal_touched_keeps_its_shadow_text() {
        let (mut doc, page) = one_page(
            "/Span << /ActualText (secret) >> BDC\nBT (secret) Tj ET\nEMC\n             /Span << /ActualText (public) >> BDC\nBT (public) Tj ET\nEMC",
        );
        let removed = remove_shows(&mut doc, page, &[0], 2).expect("remove");
        assert_eq!(removed.carriers, 1, "one span was touched, not both");
        let after = stream(&doc, page);
        assert!(!after.contains("(secret)"), "{after}");
        assert!(
            after.contains("ActualText") && after.contains("(public)"),
            "the untouched span keeps its alternate text: {after}"
        );
    }

    /// Both spans, which is what makes the ordering in [`remove_shows`] bite.
    ///
    /// `clear_shadow_text` addresses a span by where its `BDC` sits among the
    /// operations, so it has to run **before** the deletion renumbers them. With
    /// one span the two orders agree and nothing can tell them apart; with two,
    /// the second span's `EMC` has moved down by the time the walk reaches it,
    /// the frame is popped before the removal inside it is seen, and its copy of
    /// the words survives.
    #[test]
    fn two_spans_each_holding_a_removed_line_both_lose_their_shadow_text() {
        let (mut doc, page) = one_page(
            "/Span << /ActualText (first) >> BDC\nBT (first) Tj ET\nEMC\n\
             /Span << /ActualText (second) >> BDC\nBT (second) Tj ET\nEMC",
        );
        let removed = remove_shows(&mut doc, page, &[0, 1], 2).expect("remove");
        assert_eq!(removed.carriers, 2);
        let after = stream(&doc, page);
        assert!(
            !after.contains("first") && !after.contains("second"),
            "neither span kept its copy: {after}"
        );
    }

    /// An outer span restates everything inside it, so it is touched too.
    #[test]
    fn an_enclosing_span_loses_its_shadow_text_as_well() {
        let (mut doc, page) = one_page(
            "/Part << /ActualText (outer secret) >> BDC\n             /Span << /ActualText (inner secret) >> BDC\nBT (inner secret) Tj ET\nEMC\nEMC",
        );
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.carriers, 2, "both spans held a copy");
        assert!(
            !stream(&doc, page).contains("secret"),
            "{}",
            stream(&doc, page)
        );
    }

    /// All three keys, because they are three spellings of the same carrier.
    #[test]
    fn every_shadow_text_key_goes_not_just_the_famous_one() {
        let (mut doc, page) =
            one_page("/Span << /ActualText (a) /Alt (b) /E (c) /MCID 0 >> BDC\nBT (a) Tj ET\nEMC");
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.carriers, 3);
        let after = stream(&doc, page);
        // **By value, not by key name.** `lopdf` writes a dictionary entry with
        // no space --- `/E(c)` --- so an assertion looking for `"/E "` is
        // satisfied whether the key went or stayed, which is an assertion that
        // cannot fail. The values are distinct on purpose.
        for gone in ["(a)", "(b)", "(c)"] {
            assert!(!after.contains(gone), "{gone} is still there: {after}");
        }
        assert!(
            after.contains("MCID"),
            "and the rest of the property list is left alone: {after}"
        );
    }

    /// A span the stream never closed still marks the removal inside it.
    ///
    /// Malformed, and the file least likely to be looked at twice. Dropping the
    /// unclosed frame would leave the carrier exactly there.
    #[test]
    fn a_span_that_was_never_closed_still_loses_its_shadow_text() {
        let (mut doc, page) = one_page("/Span << /ActualText (secret) >> BDC\nBT (secret) Tj ET");
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.carriers, 1);
        assert!(
            !stream(&doc, page).contains("secret"),
            "{}",
            stream(&doc, page)
        );
    }

    /// `BMC` takes a tag and no property list, so there is nothing to clear.
    #[test]
    fn a_bmc_span_has_no_property_list_and_is_not_an_error() {
        let (mut doc, page) = one_page("/Span BMC\nBT (secret) Tj ET\nEMC");
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.carriers, 0);
        assert!(shown(&doc, page).is_empty());
    }

    /// The refusal, and it must leave the document exactly as it found it.
    ///
    /// A named property list may be shared with any number of other pages, so
    /// stripping it in place would silently alter them --- `docs/PLAN.md` §6's
    /// clone-on-write rule. Writing the file with the words still in it is the
    /// other option and is the confident lie the module exists to prevent.
    #[test]
    fn a_shared_property_list_carrying_the_words_refuses_and_removes_nothing() {
        let (mut doc, page) = one_page_named(
            "/Span /MC0 BDC\nBT (secret) Tj ET\nEMC",
            "MC0",
            dictionary! { "ActualText" => Object::string_literal("secret") },
        );
        let before = shown(&doc, page);
        let why = remove_shows(&mut doc, page, &[0], 1).expect_err("must refuse");
        assert!(why.contains("/MC0") && why.contains("/ActualText"), "{why}");
        assert!(why.contains("nothing was removed"), "{why}");
        assert_eq!(shown(&doc, page), before, "and nothing was");
    }

    /// The over-refusal control, and it is the common case rather than a corner.
    ///
    /// `/OC /MC0 BDC` is optional content --- a named property list on nearly
    /// every layered drawing --- and it carries no text at all. A refusal keyed
    /// on the *name* rather than on the key would make those pages unredactable.
    #[test]
    fn a_shared_property_list_with_no_shadow_text_does_not_refuse() {
        let (mut doc, page) = one_page_named(
            "/OC /MC0 BDC\nBT (secret) Tj ET\nEMC",
            "MC0",
            dictionary! { "Type" => "OCG", "Name" => Object::string_literal("Layer 1") },
        );
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.carriers, 0);
        assert!(shown(&doc, page).is_empty(), "and the words went");
    }

    /// A name that resolves to nothing is not a carrier, so it is not refused.
    #[test]
    fn a_property_list_name_that_resolves_to_nothing_does_not_refuse() {
        let (mut doc, page) = one_page("/Span /MC0 BDC\nBT (secret) Tj ET\nEMC");
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.carriers, 0);
        assert!(shown(&doc, page).is_empty());
    }
    /// The carrier: an annotation over the region goes with the words.
    #[test]
    fn an_annotation_over_the_region_is_taken() {
        let (doc, page, ids) =
            one_page_annots("BT (x) Tj ET", vec![note([100.0, 100.0, 200.0, 120.0])]);
        assert_eq!(
            covered_annots(&doc, page, &[[90.0, 90.0, 210.0, 130.0]]),
            vec![ids[0]]
        );
    }

    /// The over-removal control, and it is the half that keeps the rest honest.
    ///
    /// A page's other comments are not the reader's to lose. A rule that took
    /// every annotation on a page would pass the check above perfectly.
    #[test]
    fn an_annotation_away_from_the_region_is_left() {
        let (doc, page, _) =
            one_page_annots("BT (x) Tj ET", vec![note([400.0, 400.0, 500.0, 420.0])]);
        assert!(covered_annots(&doc, page, &[[90.0, 90.0, 210.0, 130.0]]).is_empty());
    }

    /// Touching is not overlapping, the same rule `covered` uses for objects.
    #[test]
    fn an_annotation_flush_against_the_region_is_left() {
        let (doc, page, _) =
            one_page_annots("BT (x) Tj ET", vec![note([200.0, 100.0, 300.0, 120.0])]);
        assert!(covered_annots(&doc, page, &[[100.0, 100.0, 200.0, 120.0]]).is_empty());
    }

    /// `/Rect` corners either way round, which §12.5.2 permits.
    ///
    /// Without the normalisation an upside-down rectangle overlaps nothing, so
    /// an annotation sitting squarely on the region reads as one nowhere near
    /// it --- the quiet direction, which leaves the words.
    #[test]
    fn an_annotation_whose_corners_are_the_other_way_round_is_still_found() {
        let (doc, page, ids) =
            one_page_annots("BT (x) Tj ET", vec![note([200.0, 120.0, 100.0, 100.0])]);
        assert_eq!(
            covered_annots(&doc, page, &[[90.0, 90.0, 210.0, 130.0]]),
            vec![ids[0]]
        );
    }

    /// An annotation this cannot place is taken, not kept.
    ///
    /// Deny by default in the only direction that cannot leave the words. The
    /// three shapes are one rule, and each is a real malformation.
    #[test]
    fn an_annotation_with_no_readable_rectangle_is_taken() {
        for broken in [
            dictionary! { "Type" => "Annot", "Subtype" => "Text" },
            dictionary! { "Type" => "Annot", "Rect" => vec![Object::Real(1.0), Object::Real(2.0)] },
            dictionary! { "Type" => "Annot", "Rect" => vec![
                Object::string_literal("no"), Object::Real(2.0),
                Object::Real(3.0), Object::Real(4.0),
            ] },
        ] {
            let (doc, page, ids) = one_page_annots("BT (x) Tj ET", vec![broken]);
            assert_eq!(
                covered_annots(&doc, page, &[[0.0, 0.0, 1.0, 1.0]]),
                vec![ids[0]],
                "an annotation with no readable rectangle is taken"
            );
        }
    }

    /// A popup carries its own `/Contents`, so it goes with its note.
    #[test]
    fn a_popup_goes_with_the_annotation_that_owns_it() {
        let (mut doc, page, ids) = one_page_annots(
            "BT (x) Tj ET",
            vec![
                note([100.0, 100.0, 200.0, 120.0]),
                note([400.0, 400.0, 500.0, 420.0]),
            ],
        );
        let popup = ids[1];
        let Ok(Object::Dictionary(parent)) = doc.get_object_mut(ids[0]) else {
            panic!("annotation")
        };
        parent.set("Popup", Object::Reference(popup));
        let taken = covered_annots(&doc, page, &[[90.0, 90.0, 210.0, 130.0]]);
        assert!(
            taken.contains(&ids[0]) && taken.contains(&popup),
            "{taken:?}"
        );
    }

    /// A reply is a copy of the conversation, and a chain of them goes whole.
    ///
    /// **The replies come first on `/Annots`, and that ordering is the test.**
    /// Two links written the other way round --- note, reply, reply --- are
    /// picked up by a single pass, because each reply's parent was added to the
    /// set earlier in the same walk. So the fixture that reads most naturally is
    /// exactly the one a single pass handles, and the mutation collapsing the
    /// fixed point to one pass SURVIVED against it. Written parent-last, one
    /// pass sees a reply whose parent is not in the set yet and leaves it.
    ///
    /// `/Annots` has no required order and a producer that appends replies as
    /// they are written gives this one, so it is the ordinary case rather than a
    /// contrived one.
    #[test]
    fn a_chain_of_replies_goes_with_the_note_it_answers() {
        let (mut doc, page, ids) = one_page_annots(
            "BT (x) Tj ET",
            vec![
                note([420.0, 420.0, 520.0, 440.0]),
                note([400.0, 400.0, 500.0, 420.0]),
                note([100.0, 100.0, 200.0, 120.0]),
            ],
        );
        let (note_id, reply, further) = (ids[2], ids[1], ids[0]);
        let Ok(Object::Dictionary(dictionary)) = doc.get_object_mut(reply) else {
            panic!("annotation")
        };
        dictionary.set("IRT", Object::Reference(note_id));
        let Ok(Object::Dictionary(dictionary)) = doc.get_object_mut(further) else {
            panic!("annotation")
        };
        dictionary.set("IRT", Object::Reference(reply));
        let taken = covered_annots(&doc, page, &[[90.0, 90.0, 210.0, 130.0]]);
        assert_eq!(taken.len(), 3, "the note and both replies: {taken:?}");
    }

    /// A reply to an annotation that stays, stays. The control for the loop.
    #[test]
    fn a_reply_to_an_annotation_nobody_touched_is_left() {
        let (mut doc, page, ids) = one_page_annots(
            "BT (x) Tj ET",
            vec![
                note([400.0, 400.0, 500.0, 420.0]),
                note([420.0, 420.0, 520.0, 440.0]),
            ],
        );
        let first = ids[0];
        let Ok(Object::Dictionary(reply)) = doc.get_object_mut(ids[1]) else {
            panic!("annotation")
        };
        reply.set("IRT", Object::Reference(first));
        assert!(covered_annots(&doc, page, &[[90.0, 90.0, 210.0, 130.0]]).is_empty());
    }

    /// `/Annots 12 0 R` is as common as the inline array and is not the same shape.
    #[test]
    fn an_annots_array_that_is_its_own_object_is_read() {
        let (mut doc, page, ids) =
            one_page_annots("BT (x) Tj ET", vec![note([100.0, 100.0, 200.0, 120.0])]);
        let array = doc.add_object(Object::Array(vec![Object::Reference(ids[0])]));
        let Ok(Object::Dictionary(dictionary)) = doc.get_object_mut(page) else {
            panic!("page")
        };
        dictionary.set("Annots", Object::Reference(array));
        assert_eq!(
            covered_annots(&doc, page, &[[90.0, 90.0, 210.0, 130.0]]),
            vec![ids[0]]
        );
    }

    /// A page with no annotations at all, which is most pages.
    #[test]
    fn a_page_with_no_annots_gives_up_nothing() {
        let (doc, page) = one_page("BT (x) Tj ET");
        assert!(covered_annots(&doc, page, &[[0.0, 0.0, 1000.0, 1000.0]]).is_empty());
    }

    /// No regions is no removal, the emptiness control for the whole walk.
    #[test]
    fn an_annotation_is_left_when_there_are_no_regions() {
        let (doc, page, _) =
            one_page_annots("BT (x) Tj ET", vec![note([100.0, 100.0, 200.0, 120.0])]);
        assert!(covered_annots(&doc, page, &[]).is_empty());
    }
    /// The second home of the shadow-text row, and the reason it needs its own
    /// pass: nothing in the content stream leads to it.
    #[test]
    fn the_structure_element_a_removed_span_belongs_to_loses_its_shadow_text() {
        let (mut doc, page, ids, _) =
            one_page_tagged("/Span << /MCID 0 >> BDC\nBT (secret) Tj ET\nEMC", 1, false);
        assert!(
            has_shadow_text(&doc, ids[0]),
            "the control: it starts there"
        );
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.struct_carriers, 1);
        assert!(!has_shadow_text(&doc, ids[0]));
    }

    /// An ancestor restates everything beneath it, so it goes too.
    #[test]
    fn an_ancestor_of_that_element_loses_its_shadow_text_as_well() {
        let (mut doc, page, ids, ancestor) =
            one_page_tagged("/Span << /MCID 0 >> BDC\nBT (secret) Tj ET\nEMC", 1, true);
        let ancestor = ancestor.expect("built with one");
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.struct_carriers, 2, "the leaf and the one above it");
        assert!(!has_shadow_text(&doc, ids[0]));
        assert!(!has_shadow_text(&doc, ancestor));
    }

    /// The over-removal control, and it is the half that keeps the rest honest.
    ///
    /// Two tagged lines, one redacted. A rule that stripped the whole tree would
    /// pass both checks above perfectly and take the alternate text of every
    /// line the reader did not mark.
    #[test]
    fn the_element_for_a_line_nobody_redacted_keeps_its_shadow_text() {
        let (mut doc, page, ids, _) = one_page_tagged(
            "/Span << /MCID 0 >> BDC\nBT (secret) Tj ET\nEMC\n\
             /Span << /MCID 1 >> BDC\nBT (public) Tj ET\nEMC",
            2,
            false,
        );
        let removed = remove_shows(&mut doc, page, &[0], 2).expect("remove");
        assert_eq!(removed.struct_carriers, 1, "one element, not both");
        assert!(!has_shadow_text(&doc, ids[0]));
        assert!(
            has_shadow_text(&doc, ids[1]),
            "the untouched line keeps its own"
        );
    }

    /// A span with no `/MCID` names no element, so nothing in the tree moves.
    ///
    /// The ordinary shape of `/ActualText` used for a ligature or an
    /// abbreviation, which a producer writes without tagging anything.
    #[test]
    fn a_span_with_no_mcid_reaches_no_structure_element() {
        let (mut doc, page, ids, _) = one_page_tagged(
            "/Span << /ActualText (secret) >> BDC\nBT (secret) Tj ET\nEMC",
            1,
            false,
        );
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.carriers, 1, "the span's own copy still goes");
        assert_eq!(removed.struct_carriers, 0);
        assert!(has_shadow_text(&doc, ids[0]));
    }

    /// An untagged page, which is most pages, and no error either.
    #[test]
    fn a_page_with_no_struct_parents_is_left_alone() {
        let (mut doc, page) = one_page("/Span << /MCID 0 >> BDC\nBT (secret) Tj ET\nEMC");
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.struct_carriers, 0);
    }

    /// A parent tree written as `/Kids` rather than one flat `/Nums`.
    ///
    /// **The shape every large document uses**, and the one no fixture here has:
    /// a producer with many pages writes a balanced tree with `/Limits`. Reading
    /// only `/Nums` would find nothing on all of them, silently, because a miss
    /// and an untagged page are the same answer.
    #[test]
    fn a_parent_tree_written_as_kids_is_followed() {
        let (mut doc, page, ids, _) =
            one_page_tagged("/Span << /MCID 0 >> BDC\nBT (secret) Tj ET\nEMC", 1, false);
        // Re-shape the flat tree into a root with two kids, the key in the
        // second -- so a walk that stops at the first kid fails too.
        let entry: Vec<Object> = ids.iter().map(|id| Object::Reference(*id)).collect();
        let far = doc.add_object(dictionary! {
            "Limits" => vec![Object::Integer(9), Object::Integer(9)],
            "Nums" => vec![Object::Integer(9), Object::Array(Vec::new())],
        });
        let near = doc.add_object(dictionary! {
            "Limits" => vec![Object::Integer(0), Object::Integer(0)],
            "Nums" => vec![Object::Integer(0), Object::Array(entry)],
        });
        let root = doc.add_object(dictionary! {
            "Kids" => vec![Object::Reference(far), Object::Reference(near)],
        });
        let tree_root = doc
            .catalog()
            .expect("catalog")
            .get(b"StructTreeRoot")
            .and_then(Object::as_reference)
            .expect("tree");
        let Ok(Object::Dictionary(dictionary)) = doc.get_object_mut(tree_root) else {
            panic!("the structure root is a dictionary")
        };
        dictionary.set("ParentTree", root);

        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.struct_carriers, 1);
        assert!(!has_shadow_text(&doc, ids[0]));
    }

    /// An `/MCID` past the end of the page's entry names no element.
    #[test]
    fn an_mcid_past_the_end_of_the_entry_names_no_element() {
        let (mut doc, page, ids, _) =
            one_page_tagged("/Span << /MCID 7 >> BDC\nBT (secret) Tj ET\nEMC", 1, false);
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.struct_carriers, 0);
        assert!(has_shadow_text(&doc, ids[0]));
    }

    /// A `/P` chain that loops terminates, and takes what it reached.
    ///
    /// The tree is the document's to shape and a cycle in it is one dictionary
    /// away. What stops the walk is `MAX_ANCESTORS`, and this is the test that
    /// says so on the input that needs it.
    ///
    /// **Its failure mode is a hang rather than a red line**, which
    /// `docs/TRAPS.md` records as the weakest shape a check can have: remove the
    /// bound and this does not fail, it stops answering, and a run that never
    /// finishes reads as a broken harness. It is kept because the property is
    /// worth stating and because the bound's *other* consequence --- clearing
    /// nothing past it --- is what the `0..1` mutation reddens outright.
    #[test]
    fn a_parent_chain_that_loops_terminates() {
        let (mut doc, page, ids, ancestor) =
            one_page_tagged("/Span << /MCID 0 >> BDC\nBT (secret) Tj ET\nEMC", 1, true);
        let ancestor = ancestor.expect("built with one");
        let Ok(Object::Dictionary(dictionary)) = doc.get_object_mut(ancestor) else {
            panic!("the ancestor is a dictionary")
        };
        dictionary.set("P", Object::Reference(ids[0]));
        let removed = remove_shows(&mut doc, page, &[0], 1).expect("remove");
        assert_eq!(removed.struct_carriers, 2);
    }
}
