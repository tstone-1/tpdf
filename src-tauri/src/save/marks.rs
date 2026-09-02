//! Turning a reader's marks into annotations, appearance streams and page ink.
//!
//! **Why this is not in `save.rs`.** That module is the *save*: it decides what
//! kind of write a plan needs, stages a file, hands the parse to a worker and
//! checks what landed. This is one step inside the write --- given a document and
//! a `PlannedMark`, produce the annotation dictionary and the content stream a
//! reader will see --- and it is the largest single thing in the file that a
//! reader of the save never has to open. It carries its own vocabulary
//! (`Paint`, `Upright`, the squiggle period, the stamp inset) which is meaningful
//! only here.
//!
//! Moved out on 2026-09-01, contiguously and unchanged: the bodies are the same
//! code in the same order, and the only edits are the `pub(super)` on the items
//! `save.rs` and its tests reach. `docs/RATIONALE.md` has the account.
//!
//! **`turn_pages` and `crop_pages` are here on purpose**, though a rotation is
//! not a mark. Both take a [`MarksWritten`] token and exist to state the one
//! ordering this module owns --- a crop moves the origin a mark's quads were
//! measured from, so the marks go on first. The general page-tree operations
//! they wrap are `crate::pagetree`'s, and `print.rs` calls those directly
//! because no mark has been made there.
//!
//! **What it depends on** is the import block below and nothing else. Two names
//! come from the parent: [`Refusal`], which every failure here becomes, and
//! [`MarksWritten`], whose whole purpose is to be produced here and demanded by
//! the two page operations at the end.

use lopdf::{dictionary, Dictionary, Document, IncrementalDocument, Object, ObjectId};

use crate::docmodel::MarkKind;
use crate::edits::PlannedMark;
use crate::pagetree::{apply_crops, apply_turns, displayed_page, DisplayedPage};
use crate::textbox;

use super::{MarksWritten, Refusal};

/// The opacity of a highlight's wash, as `/CA`.
///
/// Below 1 because a wash is meant to be read through. The blend mode does most
/// of that work --- see [`appearance_stream`] --- and this is what keeps the mark
/// legible in a reader that ignores the blend and paints the fill flat.
pub(super) const WASH_ALPHA: f32 = 0.4;

/// Where one annotation goes, read out of the document it is measured against.
///
/// **Everything a mark needs from the *reader*, gathered before anything is
/// written.** The two save paths write into different documents: a rewrite adds
/// objects to the same `Document` it measured, and an append adds them to an
/// update section while the geometry, the page boxes and the existing `/Annots`
/// all live in the previous revision. Splitting the read from the write is what
/// lets one implementation serve both --- and this repository has just paid for
/// the alternative, where `print.rs` grew a second page walk and it silently
/// stopped writing marks at all.
///
/// It is also the reason [`attach`] takes a site rather than re-reading the
/// page: on the append path the page dictionary is frequently *not* in the
/// document being written to, because a page whose `/Annots` is its own object
/// never has to be rewritten.
pub(super) struct MarkSite {
    /// The page object the annotation hangs off, in the previous revision.
    pub(super) page: ObjectId,
    /// The page as it is displayed, for mapping the mark's quads into it.
    pub(super) shown: DisplayedPage,
    /// Where this page keeps its annotation list.
    pub(super) annots: AnnotsSite,
}

/// The three shapes a page's `/Annots` comes in, and they are not equivalent.
///
/// Which one a page has decides **how large the edit is**, which matters only on
/// the append path and matters a great deal there: extending an array that is
/// its own object leaves the page dictionary untouched, and an inline array
/// cannot be extended without rewriting the page. `docs/PLAN.md` §5 records that
/// as the one document-shape dependency to carry into Phase 2, and it is
/// measured rather than assumed --- the spike narrowed a signed document's
/// complaint to two objects by preferring the array.
pub(super) enum AnnotsSite {
    /// `/Annots 12 0 R` --- its own object, and the cheap case.
    ArrayObject(ObjectId),
    /// Written out inside the page dictionary, which therefore has to be
    /// rewritten. The entries come along, because the writer does not have the
    /// page to read them back from.
    Inline(Vec<Object>),
    /// No `/Annots` at all. The page is rewritten, as for an inline array.
    Absent,
}

/// Reads what every mark needs, before anything is written.
///
/// `kept` is the one-based page numbers being written, used only for the
/// shared-object refusal.
///
/// # Errors
///
/// A mark naming a page the file does not have; a mark on a page object that
/// more than one kept page number names; or a mark whose quads map to nothing.
pub(super) fn mark_sites(
    read: &Document,
    sheet: &[ObjectId],
    marks: &[PlannedMark],
) -> Result<Vec<MarkSite>, String> {
    let mut sites = Vec::with_capacity(marks.len());
    for mark in marks {
        let page = *sheet.get(mark.at as usize).ok_or_else(|| {
            format!(
                "a mark names page {}, which this document does not have",
                mark.at + 1
            )
        })?;

        // The same refusal as `unshared` and for the same reason, one level on:
        // an annotation is attached to a page *object*, so a mark made on page 3
        // would appear on page 7 as well when `/Kids` names one object twice.
        // `docs/TRAPS.md` has this shape twice already, once live in `print.rs`
        // for months.
        //
        // Counted over `sheet` rather than over the kept baseline numbers, which
        // is what this did while a mark was addressed by one. The question is
        // the same and is now asked of the thing it is really about: how many
        // positions in the document being written hold this page object. A page
        // tpdf made is its own object and can never collide, so it answers one
        // and passes without a case of its own.
        if sheet.iter().filter(|id| **id == page).count() > 1 {
            return Err(format!(
                "page {} is the same page object as another page in this file, so a mark on it \
                 would appear on both. tpdf will not write it.",
                mark.at + 1
            ));
        }

        let annots = match read
            .get_object(page)
            .and_then(Object::as_dict)
            .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?
            .get(b"Annots")
        {
            Ok(Object::Reference(array)) => AnnotsSite::ArrayObject(*array),
            Ok(Object::Array(entries)) => AnnotsSite::Inline(entries.clone()),
            // Anything else is a page whose `/Annots` is not a list --- a
            // malformed document, and the same answer as having none: a list of
            // our own replaces it. That is what the previous implementation did
            // through its `_` arm, stated rather than inherited.
            _ => AnnotsSite::Absent,
        };

        sites.push(MarkSite {
            page,
            shown: displayed_page(read, page),
            annots,
        });
    }
    Ok(sites)
}

/// Proof that every reply in a plan names an annotation in the document the plan
/// was made against.
///
/// **A token rather than a comment, and that is the whole reason it exists.**
/// The check cannot live inside [`write_marks`], because the two save paths hand
/// that function different documents: the rewrite gives it the document being
/// written, and the append gives it a *new, empty* one whose previous revision
/// holds the annotation being answered. So the lookup has to happen in each
/// path, against the document that path was planned against --- and a refusal
/// with two call sites is exactly the shape `docs/TRAPS.md` records drifting,
/// where one caller reaches the writer directly and never meets the guard its
/// sibling has. Requiring the token as an argument makes that unspellable: a
/// third save path cannot write a mark until it has produced one.
#[must_use]
pub(super) struct RepliesChecked;

/// Refuses any reply whose parent is not an annotation in `doc`.
///
/// `doc` is the document the plan was made against --- the previous revision for
/// an append, the loaded document for a rewrite --- because that is where the
/// object a reply names lives. Nothing is written here; the answer is only
/// whether writing would be honest.
///
/// The model has already refused a reply on a kind that cannot carry one. This
/// is the half the model *cannot* know, for the reason [`set_note`] gives about
/// the comment it edits: the model has never read the file, so a plan naming an
/// arbitrary object would otherwise thread a reply onto a font, a page or the
/// catalog, and `/IRT` on a page means nothing at all.
///
/// # Errors
///
/// The object not being in the document, not being a dictionary, or not being an
/// annotation.
pub(super) fn check_replies(
    doc: &Document,
    marks: &[PlannedMark],
) -> Result<RepliesChecked, Refusal> {
    for mark in marks {
        let Some(parent) = mark.reply_to else {
            continue;
        };
        let object = doc.get_object((parent.0, parent.1)).map_err(|e| {
            Refusal::changed(format!(
                "the comment being answered is not in this document any more: {e}"
            ))
        })?;
        let dictionary = object
            .as_dict()
            .map_err(|_| Refusal::from("that comment is not an annotation"))?;
        // Not "has the subtype we expect": any annotation has one, and the check
        // that matters is that this is an annotation at all. `set_note` asks the
        // same question of the same kind of object, in the same words.
        if dictionary
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_err()
        {
            return Err("that comment is not an annotation".into());
        }
    }
    Ok(RepliesChecked)
}

/// Writes each mark as an annotation, into whichever document is being built.
///
/// `sites` is [`mark_sites`]'s answer for the same `marks`, in the same order.
/// The pairing is positional, which is why neither is public and both are
/// produced side by side at each of the two call sites.
///
/// **One object per mark, appended to its page's `/Annots`.** That array may be
/// absent, written inline, or an indirect reference to an array; `AGENTS.md`
/// records that the distinction decides how large an annotation edit is, and
/// [`attach`] handles all three.
///
/// **The coordinates are the whole of the difficulty.** A mark is held in
/// display space --- what the reader dragged across, y downwards from the
/// displayed page's top-left corner --- and `/QuadPoints` is the page's own
/// space, y upwards, before `/Rotate` and measured from the media box. The
/// mapping is [`crate::text::from_device`] followed by the crop box's origin,
/// which is exactly the two steps `annots.rs` performs in reverse when it reads
/// one back. Those are separate implementations, which is what makes
/// `annot-probe`'s round trip a differential rather than a tautology.
///
/// # Errors
///
/// A mark whose quads map to nothing.
pub(super) fn write_marks(
    doc: &mut Document,
    marks: &[PlannedMark],
    sites: &[MarkSite],
    _replies_were_checked: RepliesChecked,
) -> Result<MarksWritten, String> {
    for (mark, site) in marks.iter().zip(sites) {
        let MarkSite {
            page,
            shown,
            annots,
        } = site;
        let (page, shown) = (*page, *shown);
        let quads = user_quads(mark, shown);
        if quads.is_empty() {
            return Err(format!(
                "a mark on page {} covers no area in that page's own space",
                mark.at + 1
            ));
        }

        let rect = bounds(&quads);
        // **A comment gets no appearance stream from us, and that is not a gap.**
        // Every reader synthesises the icon for a `/Text` annotation --- the
        // specification describes `/Name` as choosing one and readers are
        // expected to draw it --- and they draw their own house style at their
        // own size whatever we write. Supplying one would mean shipping a
        // hand-drawn speech bubble that looks foreign in Acrobat and in Preview,
        // and `--mode noap` already measures that PDFium renders these without
        // one: the note icon it generates fills 637 of the 756 pixels in its
        // rectangle.
        //
        // The three markup kinds are the opposite case and keep theirs: a
        // reader that declines to synthesise a highlight shows nothing at all,
        // which is why `appearance_stream` exists. **A box is on that side of
        // the line, not the comment's**, and it is the reason this asks `paint`
        // rather than `is_note`: nothing synthesises a rectangle, so a
        // `/Square` with no `/AP` is an annotation Acrobat draws as nothing.
        let strokes = user_strokes(mark, shown);
        let appearance = if paint(mark.kind) == Paint::None {
            None
        } else {
            Some(appearance_stream(
                doc,
                mark,
                &quads,
                &strokes,
                rect,
                shown.turns,
            ))
        };
        let dictionary = mark_dictionary(mark, page, &quads, &strokes, rect, appearance);
        let annotation = doc.add_object(dictionary);
        attach(doc, page, annots, annotation)?;
    }
    Ok(MarksWritten)
}

/// [`pagetree::apply_turns`], once the marks are written.
///
/// A wrapper for exactly one reason, and it is not indirection for its own sake:
/// `apply_turns` is general --- `print.rs` calls it too, and there no mark has
/// been made --- so the ordering token belongs *here*, where the ordering is,
/// rather than on a page-tree function whose other callers have other orders.
///
/// See [`MarksWritten`] for what the order is and what going the other way costs.
pub(super) fn turn_pages(
    doc: &mut Document,
    turns: &[(lopdf::ObjectId, u8)],
    _written: &MarksWritten,
) -> Result<(), String> {
    apply_turns(doc, turns)
}

/// [`pagetree::apply_crops`], once the marks are written.
///
/// Here for the reason [`turn_pages`] is, and for the same constraint: a crop
/// moves the origin a mark's quads were measured from.
pub(super) fn crop_pages(
    doc: &mut Document,
    crops: &[(lopdf::ObjectId, [f64; 4])],
    _written: &MarksWritten,
) -> Result<(), String> {
    apply_crops(doc, crops)
}

/// A mark's quads in the page's own space, `[llx, lly, urx, ury]` each.
///
/// Degenerate quads are dropped rather than written. The model already refuses a
/// mark where *every* quad is empty ([`crate::docmodel::Refusal::EmptyMark`]);
/// this is the per-quad half, and it exists because a selection that runs to the
/// end of a line legitimately produces one empty rectangle after a real one.
fn user_quads(mark: &PlannedMark, shown: DisplayedPage) -> Vec<[f64; 4]> {
    let (ox, oy) = (f64::from(shown.origin.0), f64::from(shown.origin.1));
    mark.quads
        .iter()
        .filter(|quad| quad.covers_area())
        .map(|quad| {
            let page = crate::text::from_device(
                shown.turns,
                shown.width,
                shown.height,
                [quad.left, quad.top, quad.right, quad.bottom],
            );
            // The origin comes back on *after* the turn, because it came off
            // before one: `annots.rs` shifts into crop space and then maps.
            [page[0] + ox, page[1] + oy, page[2] + ox, page[3] + oy]
        })
        .collect()
}

/// A mark's strokes in the page's own space, one `(x, y)` list each.
///
/// **Built on [`crate::text::from_device`] rather than beside it**, by handing
/// it the point as a rectangle of no size and taking the corner back. That looks
/// roundabout and is the point: the flip and the turn that map display space
/// onto the page are one rule, and a second copy written for points would be a
/// second thing to get right at every `/Rotate`. The trap index has that under
/// *"two copies of a distinction drift, and a mutation of one survives"*, and
/// the mapping is exactly where it would bite --- a wrong turn puts ink on the
/// page, in a plausible place, sideways.
///
/// A degenerate rectangle is safe here for the one reason it is unsafe in
/// [`user_quads`]: nothing downstream asks whether it covers area. `from_device`
/// is pure arithmetic on the corners.
fn user_strokes(mark: &PlannedMark, shown: DisplayedPage) -> Vec<Vec<(f64, f64)>> {
    let (ox, oy) = (f64::from(shown.origin.0), f64::from(shown.origin.1));
    mark.strokes
        .iter()
        .map(|stroke| {
            stroke
                .points
                .iter()
                .map(|point| {
                    let mapped = crate::text::from_device(
                        shown.turns,
                        shown.width,
                        shown.height,
                        [point.x, point.y, point.x, point.y],
                    );
                    (mapped[0] + ox, mapped[1] + oy)
                })
                .collect()
        })
        .collect()
}

/// The rectangle enclosing every quad.
pub(super) fn bounds(quads: &[[f64; 4]]) -> [f64; 4] {
    quads
        .iter()
        .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |acc, q| {
            [
                acc[0].min(q[0]),
                acc[1].min(q[1]),
                acc[2].max(q[2]),
                acc[3].max(q[3]),
            ]
        })
}

/// The annotation dictionary for one mark.
///
/// `/F 4` sets the Print flag, which is what makes a highlight appear on paper
/// and in a print-to-PDF --- an annotation without it is a screen-only mark, and
/// a reader who highlights a document in order to print it would get a blank
/// page back.
///
/// `/NM` is the mark's own id. It has to be unique within the page and ours are
/// unique within the document, which is the stronger property; it is written
/// because a reader that reopens the file and edits it needs a name for the
/// annotation that is not its position in an array.
fn mark_dictionary(
    mark: &PlannedMark,
    page: ObjectId,
    quads: &[[f64; 4]],
    strokes: &[Vec<(f64, f64)>],
    rect: [f64; 4],
    appearance: Option<ObjectId>,
) -> Dictionary {
    let note = is_note(mark.kind);
    let mut dictionary = Dictionary::new();
    dictionary.set("Type", Object::Name(b"Annot".to_vec()));
    dictionary.set("Subtype", Object::Name(subtype(mark.kind).to_vec()));
    dictionary.set("Rect", numbers(rect));
    // **`/QuadPoints` is a text-markup key**, and neither of the two kinds a
    // reader places themselves may carry one --- see [`is_text_markup`], which
    // is the question this used to ask as "is it a comment" because the comment
    // was then the only kind it was true of. Writing quads on a `/Square` is the
    // kind of thing most readers ignore and one day something does not, and it
    // would also be a lie: the quad there is the mark's own box, not a run of
    // words it covers.
    if is_text_markup(mark.kind) {
        dictionary.set("QuadPoints", quad_points(quads));
    }
    // **`/InkList` is required on an `/Ink` and meaningless on anything else**,
    // so it is asked of the paint rather than of the kind --- the same reasoning
    // `is_text_markup` above records, one kind later. It is written as well as
    // the appearance stream, not instead of it: the `/AP` is what every reader
    // actually draws, and the list is what a reader that regenerates
    // appearances, or an editor that wants to reshape the line, reads to find
    // out what was drawn. A file with only the `/AP` is a picture of ink rather
    // than ink.
    if paint(mark.kind) == Paint::Path {
        dictionary.set(
            "InkList",
            Object::Array(
                strokes
                    .iter()
                    .map(|stroke| {
                        Object::Array(
                            stroke
                                .iter()
                                .flat_map(|(x, y)| {
                                    [Object::Real(*x as f32), Object::Real(*y as f32)]
                                })
                                .collect(),
                        )
                    })
                    .collect(),
            ),
        );
    }
    dictionary.set(
        "C",
        Object::Array(mark.color.iter().map(|c| Object::Real(*c)).collect()),
    );
    dictionary.set(
        "CA",
        Object::Real(if is_wash(mark.kind) { WASH_ALPHA } else { 1.0 }),
    );
    dictionary.set("F", Object::Integer(4));
    dictionary.set("P", Object::Reference(page));
    // Which standard stamp this is, in the specification's spelling.
    //
    // **Written even though it is not what draws the stamp here.** `/AP` wins in
    // every reader that has one, and this file always writes one --- so this is
    // for a reader that would synthesise an appearance instead, and that reader
    // draws from PDF 32000-1's list and nothing else. Writing a name outside it
    // would be worse than writing none.
    if let Some(name) = mark.stamp {
        dictionary.set("Name", Object::Name(name.pdf_name().to_vec()));
    }
    // **`/IRT` and `/RT` together, and neither alone.** `/IRT` names the
    // annotation this one is in reply to; `/RT /R` says the relationship is a
    // reply rather than `/Group`, which is the other value and means "these are
    // one annotation shown as several". A reader that sees `/IRT` without `/RT`
    // defaults to `/R` --- so writing it is belt and braces --- but a reader that
    // sees `/Group` threads nothing, and being explicit is what makes the
    // intention survive a round trip through an editor that rewrites the
    // dictionary.
    //
    // Nothing is asked of the kind here: the model refuses `reply_to` on
    // anything but a comment, so a mark carrying one is a comment by
    // construction. What *is* checked, one layer up, is that the object exists
    // and is an annotation --- see [`check_replies`], which cannot be done here
    // because this function is handed the document being written rather than the
    // one the plan was made against.
    if let Some((number, generation)) = mark.reply_to {
        dictionary.set("IRT", Object::Reference((number, generation)));
        dictionary.set("RT", Object::Name(b"R".to_vec()));
    }
    if note {
        // The icon a reader sees. `/Comment` is the speech bubble in every
        // reader that draws these; `/Note` is the folded page, which is the
        // name a reader would guess from our own serde spelling and the wrong
        // picture for the thing this command makes.
        dictionary.set("Name", Object::Name(b"Comment".to_vec()));
        // Closed. A file whose comments all spring open on load buries the page
        // under popups, and every reader offers its own way to open one.
        dictionary.set("Open", Object::Boolean(false));
    }
    if mark.kind == MarkKind::TextBox {
        // **`/DA` is required on a `/FreeText` and on nothing else.** It is the
        // appearance a reader falls back to when it regenerates the annotation
        // itself -- which Acrobat does whenever the text is edited in *its* UI,
        // and which it cannot do at all without this. An `/AP` alone is enough
        // to *display* the mark and leaves it uneditable everywhere but here.
        //
        // The font name and size have to match the appearance stream's, or a
        // reader that regenerates redraws the same words at a different size.
        // Both come from the same two constants, so they cannot drift.
        //
        // The colour is the text's, written as `rg` because `/DA` describes a
        // fill. `/C` above is the annotation's *background* for this subtype
        // rather than its ink, which is why the two are not the same operator
        // and why a text box is the one kind whose `/C` a reader does not see as
        // the mark's colour.
        let [r, g, b] = mark.color;
        dictionary.set(
            "DA",
            Object::string_literal(format!(
                "/{TEXT_FONT} {size} Tf {r} {g} {b} rg",
                size = textbox::SIZE
            )),
        );
    }
    if let Some(appearance) = appearance {
        dictionary.set("AP", {
            let mut ap = Dictionary::new();
            ap.set("N", Object::Reference(appearance));
            Object::Dictionary(ap)
        });
    }
    // Written as PDFDocEncoded literals. Both are the reader's own text rather
    // than a document's, so the encoding question `annots.rs` answers on the way
    // in does not arise on the way out --- but a non-ASCII author would be
    // mangled by a literal, so anything outside ASCII goes out as UTF-16BE with
    // the byte-order mark the specification asks for.
    dictionary.set("T", text_string(&mark.author));
    dictionary.set("Contents", text_string(&mark.note));
    dictionary.set("M", text_string(&mark.made));
    dictionary
}

/// The PDF name for a mark's kind.
///
/// A `match` rather than a table, so that adding a [`MarkKind`] is a compile
/// error here rather than a mark that silently writes as a highlight.
pub(super) fn subtype(kind: MarkKind) -> &'static [u8] {
    match kind {
        MarkKind::Highlight => b"Highlight",
        MarkKind::Underline => b"Underline",
        MarkKind::Squiggly => b"Squiggly",
        // `/StrikeOut`, with that capitalisation. The variant is `StrikeOut`
        // and the serde name is `strikeout`; this is the only place all three
        // spellings meet, which is why it is a `match` and not a `to_lowercase`.
        MarkKind::StrikeOut => b"StrikeOut",
        // `/Text`, which is a comment bubble and not text on the page. The
        // reader's word for it is "comment", the serde name is `note`, and this
        // is the third spelling --- the same arrangement as `StrikeOut` above,
        // and the reason both are a `match` rather than a `to_lowercase`.
        MarkKind::Note => b"Text",
        // `/Square`, which is a rectangle and is not necessarily square: the
        // specification uses that name for the family holding `/Circle` too.
        // The word a reader sees is "box"; the third spelling once again.
        MarkKind::Square => b"Square",
        // `/Ink`, and the one place in this `match` where the PDF name and the
        // variant agree while the reader's word does not: a reader sees "Draw".
        // Same three-spelling arrangement as the four above it.
        // `/Circle`, which is the specification's name for an ellipse and not
        // a claim that it is round --- exactly as `/Square` above is not a claim
        // that the box is square. Both are the names of one family.
        MarkKind::Ellipse => b"Circle",
        // `/FreeText`, where "free" means unattached to a text selection rather
        // than anything about the words. A reader sees "Text box".
        MarkKind::TextBox => b"FreeText",
        MarkKind::Ink => b"Ink",
        // `/Stamp`, and the one kind whose three spellings all agree.
        MarkKind::Stamp => b"Stamp",
    }
}

/// How a kind's ink is laid down.
///
/// **Called `Paint` rather than `Ink`, and the rename came with `MarkKind::Ink`.**
/// This answers *how* a mark is drawn; that names *which* mark it is, and
/// `ink(kind) -> Ink` beside `MarkKind::Ink` is legal Rust that reads as one
/// thing referring to itself. `Paint::Path` is the variant ink uses.
///
/// **One question with one exhaustive `match`.** This started as two booleans
/// and the box would have made it three, which is where copies of a distinction
/// begin to drift --- the trap index has that under its own title, and it is the
/// same argument `markband.ts` makes for being one function. What the writer
/// needs is a single value that decides the geometry, the blend mode and both
/// opacities together, because those four have never been independent.
///
/// `markband.ts` mirrors this across the language boundary for the overlay.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Paint {
    /// The whole quad, multiplied, so the words underneath stay readable.
    Wash,
    /// A band inside the quad, opaque and on top --- see [`line_rect`]. A
    /// translucent line reads as a smudge, and multiplied red over black text
    /// is black.
    Line,
    /// The quad's edge, opaque, leaving whatever is inside it visible. Which is
    /// the entire point of a box: it says "this", it does not cover it.
    Outline,
    /// The reader's own words, set in Helvetica inside the quad.
    ///
    /// **The only style whose content is not geometry.** Every other variant
    /// draws the mark's rectangle, a band inside it, its edge or a path; this
    /// one draws a string, which means the appearance stream needs a font in its
    /// resources and the writer needs to know how wide each glyph is. See
    /// `textbox.rs` for both.
    ///
    /// It is also the only one that reads [`PlannedMark::note`]. That field has
    /// always been carried to the writer --- it becomes `/Contents` for every
    /// kind --- and until now nothing drew from it.
    Text,
    /// A wave along the bottom of the quad, stroked.
    ///
    /// **Separate from [`Paint::Line`] for the reason [`Paint::Ellipse`] is
    /// separate from [`Paint::Outline`]**: the geometry differs, and geometry is
    /// what this enum decides. A rule is one filled rectangle; a wave is a
    /// stroked zigzag, and it is the only style here whose operator count
    /// depends on how wide the quad is.
    ///
    /// It is also the only one that sets its own line width. The header writes a
    /// single `w` for the whole stream, and a wave's thickness is
    /// [`LINE_FRACTION`] of *its own quad's* height --- which differs per quad on
    /// a run that crosses a heading. So the arm emits a `w` before each path.
    Wave,
    /// The quad's inscribed ellipse, stroked, leaving its inside visible.
    ///
    /// **Separate from [`Paint::Outline`] rather than a flag on it**, because
    /// the two differ in the one thing this enum exists to decide: the geometry.
    /// A box is one `re` operator; an ellipse is four Bézier arcs, since a PDF
    /// content stream has no ellipse primitive to call. Folding them together
    /// would mean an `Outline` arm that asks the kind again --- a second copy of
    /// the distinction `paint` already makes, which is the drift this enum's own
    /// doc comment above is about.
    ///
    /// Everything else the variant decides is the box's: opaque, on top, and
    /// inset by half the stroke width for [`outline_path`]'s reason.
    Ellipse,
    /// The strokes a reader drew, opaque, with round joins and caps.
    ///
    /// **The first that does not derive its geometry from the quad at all.** The
    /// other four are the quad, a band inside it, or its edge; this one is the
    /// path, and the quad is merely the rectangle that path happens to occupy.
    /// Round rather than mitred because the line is freehand: a mitre on a
    /// hand-drawn corner spikes, which reads as a rendering fault rather than as
    /// a style.
    Path,
    /// A word inside a border, both in the mark's colour.
    ///
    /// **The second style whose content is not geometry**, after [`Paint::Text`]
    /// --- and unlike that one the string is not the reader's. It is
    /// [`StampName::word`], one of a closed list, which is what lets the size be
    /// chosen to fit rather than wrapped: a stamp is one word and it should fill
    /// the rectangle the reader dragged.
    ///
    /// It is a border *and* a word rather than either alone. A word with no
    /// border reads as a text box, and a border with no word is a
    /// [`Paint::Outline`]; what makes a stamp recognisable is both together.
    Stamp,
    /// None of ours. The reader draws its own, which for `/Text` is the only
    /// way the icon can look like that reader's other comments.
    None,
}

/// Which of the seven a kind uses.
///
/// A `match` for [`subtype`]'s reason: adding a [`MarkKind`] has to be a compile
/// error here rather than a mark that silently draws as a highlight.
fn paint(kind: MarkKind) -> Paint {
    match kind {
        MarkKind::Highlight => Paint::Wash,
        MarkKind::Underline | MarkKind::StrikeOut => Paint::Line,
        MarkKind::Squiggly => Paint::Wave,
        MarkKind::Square => Paint::Outline,
        MarkKind::Ellipse => Paint::Ellipse,
        MarkKind::TextBox => Paint::Text,
        MarkKind::Ink => Paint::Path,
        MarkKind::Note => Paint::None,
        // **Not `Paint::None`, and the difference from the comment above was
        // measured.** A `/Stamp` with `/Name /Approved` and no `/AP` renders 0
        // non-white pixels through PDFium, against 336 for a `/Text` with no
        // `/AP` on the same page --- so a stamp with no appearance of ours is an
        // annotation that draws nothing at all, which is `MarkKind::Square`'s
        // situation rather than the comment's.
        MarkKind::Stamp => Paint::Stamp,
    }
}

/// Whether a kind is a comment bubble.
///
/// Narrower than it looks, and deliberately so. It used to answer three
/// questions at once --- whether to write `/Name` and `/Open`, whether to skip
/// `/QuadPoints`, and whether to write an appearance stream --- because the
/// comment was the only kind for which all three answers happened to coincide.
/// The box separated them: it also skips `/QuadPoints` and it very much needs an
/// appearance stream. So this now answers only the first, [`is_text_markup`]
/// answers the second and [`Paint::None`] the third.
fn is_note(kind: MarkKind) -> bool {
    matches!(kind, MarkKind::Note)
}

/// Whether a kind is a text-markup annotation, and therefore carries quads.
///
/// PDF 32000-1 lists `/QuadPoints` on `/Highlight`, `/Underline`, `/Squiggly`
/// and `/StrikeOut` and on no other subtype. The two kinds a reader places
/// themselves are positioned by `/Rect` alone, and writing quads on one would be
/// a lie as well as an unlisted key: the quad there is the mark's own box rather
/// than a run of words it covers.
fn is_text_markup(kind: MarkKind) -> bool {
    matches!(
        kind,
        MarkKind::Highlight | MarkKind::Underline | MarkKind::Squiggly | MarkKind::StrikeOut
    )
}

/// Whether a kind covers its quads rather than drawing inside or around them.
///
/// Derived from [`paint`] rather than matching again, so that a kind can never be
/// a wash here and something else there. It decides the blend mode and `/CA`.
///
/// **Public for `examples/turned_probe.rs`**, which has to know the same thing
/// for a reason that follows from the blend mode: a multiplied mark leaves a
/// pixel alone wherever the paper under it is already dark, so how much of its
/// box it inks is a reading about the page's content as well as about the mark.
/// A copy of this predicate there would be the second copy this doc comment
/// exists to refuse.
pub fn is_wash(kind: MarkKind) -> bool {
    paint(kind) == Paint::Wash
}

/// A line's thickness as a fraction of the marked text's height.
///
/// Proportional rather than PDFium's fixed 1 pt. Both are defensible for body
/// text and only one survives a heading: a 1 pt strikeout across 36 pt type is
/// a hairline, and a reader who cannot see the line they just drew tries again.
/// No floor is needed --- a quad with no area is dropped by [`user_quads`]
/// before this is reached.
const LINE_FRACTION: f64 = 0.07;

/// How tall a squiggle's band is, as a fraction of the marked text's height.
///
/// Peak to trough, measured from the bottom of the quad up. Proportional for
/// [`LINE_FRACTION`]'s reason --- the text decides how big the mark is --- and
/// **larger than it on purpose**: this is the number that makes a squiggle
/// distinguishable from an underline rather than a wobbly one.
///
/// At 0.18 against the rule's 0.07 there is a clear strip of quad, from 7% to
/// 18% of the height, where an underline has no ink and a squiggle does. Every
/// check that tells the two kinds apart reads somewhere in that strip, in the
/// file and on the overlay both.
///
/// **No check derives its band from this constant**, which would make the test
/// move with the thing it polices and stop being able to fail --- see the trap
/// about a check that measures along the axis it is policing. They use fixed
/// fractions chosen to sit inside the gap.
const SQUIGGLE_HEIGHT: f64 = 0.18;

/// One full cycle of a squiggle, as a multiple of [`SQUIGGLE_HEIGHT`]'s band.
///
/// Two, so a cycle is as wide as the band is tall twice over, and the zigzag
/// climbs at 45 degrees. Tied to the band rather than to the quad's width
/// because a wave whose period was a fraction of the *width* would have fewer,
/// longer cycles on a long run and more on a short one --- the same mark drawn
/// at two frequencies depending on how many words the reader picked.
///
/// `markband.ts` holds both of these, unavoidably: the overlay draws the same
/// wave in another language. They are compared by rendering rather than by
/// sharing a literal --- `annot-probe --mode wave` reads the file's and
/// `viewer_check.py`'s overlay phase reads the screen's, and **neither reads
/// these constants**, which is what lets either of them fail.
///
/// Private, like [`LINE_FRACTION`], because nothing outside this module has a
/// reason to know them: [`OUTLINE_WIDTH`] is `pub` only because the probe
/// measures a stroke it has to predict the width of, and no check here predicts
/// a wave's geometry --- they read a strip chosen to sit between the two
/// constants rather than on either.
const SQUIGGLE_PERIOD: f64 = 2.0;

/// The most half-period segments one squiggle may emit, whatever it is given.
///
/// **A bound on the loop rather than on its inputs, and it is here because
/// neither input is ours.** `draw_wave` steps across the quad one half-period
/// at a time, so its trip count is `width / half` --- and `half` comes from the
/// quad's *height* while the width comes from the quad, both of them `f32`
/// fields of a plan that reaches the writer from outside it. A tall thin quad
/// on an ordinary page needs about 230 segments; the plan
/// `fuzz/fuzz_targets/save_rewrite_update.rs` found needed roughly two hundred
/// million, and each one appends about thirty bytes to a `String`. That is
/// 6.2 GB of allocation from a 2,937-byte input, measured, in one pass.
///
/// **The guard above this one does not help**, and that is worth stating rather
/// than discovering: `half <= 0.0 || high <= low` is about the arithmetic
/// producing a stroke at all, and every one of those two hundred million
/// segments was arithmetically fine.
///
/// 14,400 because that is the widest page PDF 1.7 permits in points, so this is
/// one half-period per point across the widest page there can be --- a density
/// no reader can resolve and no real annotation approaches. Exceeding it widens
/// the period instead of refusing: a squiggle is decoration, it is still drawn
/// in the right place, and refusing a mark that a reader can see on screen
/// because of its aspect ratio would lose an edit to protect a memory bound.
///
/// **Clamping is not sufficient on its own.** `save::rewrite_update` also
/// refuses a made page whose size no reader could have asked for, which is what
/// stops a quad being mapped into that shape in the first place; this is the
/// half that holds when the quad is extreme on a page that is not.
const MAX_WAVE_SEGMENTS: f64 = 14_400.0;

/// One line of text as a hex string of `/WinAnsiEncoding` bytes.
///
/// **A hex string rather than a literal `(...)`, and the reason is an encoding
/// bug that would have been invisible in ASCII.** The content stream is built as
/// a Rust `String`, which is UTF-8, so pushing `ü` into it writes the two bytes
/// `C3 BC` where WinAnsi wants the one byte `FC`. Every English text box would
/// have looked perfect and every German one would have drawn `Ã¼`.
///
/// Hex also removes the other half of the problem: no escaping. A literal string
/// has to escape `(`, `)` and `\`, and a reader typing a smiley `:-)` into a
/// text box is not an unusual thing to do.
///
/// Latin-1 and WinAnsi agree byte for byte over `A0..=FF`, and
/// `textbox::encodable` admits nothing else above ASCII, so the code point *is*
/// the byte.
fn winansi_hex(line: &str) -> String {
    let mut out = String::with_capacity(line.len() * 2);
    for ch in line.chars() {
        let code = ch as u32;
        // Unencodable characters are refused long before a plan is built; this
        // is the floor under that, and it writes a space rather than a byte that
        // would decode to something else entirely.
        let byte = if code <= 0xff { code as u8 } else { b' ' };
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

/// The name the appearance stream's resources give Helvetica.
///
/// Written into `/DA` as well as into the stream, and they have to agree: a
/// `/DA` naming a font the resources do not have is what makes a reader
/// substitute one, which is the whole failure `textbox.rs` avoids by measuring a
/// font every reader is required to have.
pub(super) const TEXT_FONT: &str = "Helv";

/// A line's own rectangle inside a quad: `(bottom, height)` in the page's space.
///
/// **It stays inside the quad**, which is not a nicety. The appearance stream's
/// `/BBox` is the bounds of every quad, so anything drawn outside is clipped ---
/// an underline centred on the quad's bottom edge would lose its lower half in
/// every reader, and look like a thinner line rather than like a bug.
///
/// So an underline sits *on* the bottom edge and a strikeout is centred on the
/// middle. Both are expressed here rather than as an offset the caller applies,
/// because the two need different arithmetic and an offset that had to be
/// `LINE_FRACTION / 2.0` for one of them is a coincidence waiting to be tidied
/// into a defect.
pub(super) fn line_rect(kind: MarkKind, bottom: f64, top: f64) -> (f64, f64) {
    let full = top - bottom;
    let thickness = full * LINE_FRACTION;
    match kind {
        // Not reached: a wash fills its quad and `appearance_stream` branches
        // before asking. Answered rather than `unreachable!()`, so that a fourth
        // kind added without reading this is a wrong-looking mark rather than a
        // panic in front of a reader.
        MarkKind::Highlight => (bottom, full),
        MarkKind::Underline => (bottom, thickness),
        // **Reached, unlike five of the six arms around it.** A wave is drawn by
        // `Paint::Wave` rather than filled, and it asks here for the same reason
        // the two rules do: where a kind's ink sits inside its quad is one
        // question, and answering it in two places is how a mark comes out at
        // one height in the file and another on screen.
        //
        // The band is taller than a rule and starts at the same edge, which is
        // the whole of what tells the two apart once they are drawn.
        MarkKind::Squiggly => (bottom, full * SQUIGGLE_HEIGHT),
        // Not reached: a text box's ink is lines of type placed from its top
        // edge downwards, which is not a band inside a quad at all. The whole
        // quad, for the box's reason.
        MarkKind::TextBox => (bottom, full),
        MarkKind::StrikeOut => (bottom + full / 2.0 - thickness / 2.0, thickness),
        // Not reached either, and one step further out than the highlight
        // above: a comment has no appearance stream of ours at all, so nothing
        // asks where its line goes. Answered with the whole quad for the same
        // reason.
        MarkKind::Note => (bottom, full),
        // Not reached, and for a third reason: a box has an appearance stream,
        // it is drawn by `outline_path` rather than by a filled rectangle, and
        // it has no band inside its quad to describe. The whole quad again.
        MarkKind::Square => (bottom, full),
        // Not reached, for the box's reason exactly: ink is drawn from its
        // strokes and has no band either. The whole quad, a fourth time, and
        // the fourth unreached arm is the argument for `line_rect` eventually
        // taking only the kinds that have a band --- not today, because
        // narrowing it would need a second enum whose only job is to say which
        // three those are.
        MarkKind::Ink => (bottom, full),
        // Not reached, a sixth time: a stamp is a border and a word, both placed
        // from its own rectangle by `Paint::Stamp`. The whole quad.
        MarkKind::Stamp => (bottom, full),
        // Not reached, for the box's reason exactly: an ellipse is drawn from
        // its quad by `Paint::Ellipse` and has no band inside it either. The
        // whole quad a fifth time, which is the argument above getting stronger
        // rather than weaker -- five of six arms are now unreachable.
        MarkKind::Ellipse => (bottom, full),
    }
}

/// How thick a box's outline is, in points.
///
/// **Fixed, where [`LINE_FRACTION`] is proportional**, and the reason the two
/// differ is worth stating because the obvious move is to make them agree. A
/// line through text scales with the text because the *text* decides how big
/// that mark is; nothing decides how big a box is except the reader, so a
/// border that grew with the rectangle would draw a box round a figure four
/// times heavier than one round a word. `markband.ts` holds the same number.
///
/// Public because `annot-probe --mode outline` measures the stroke it draws and
/// has to know how thick to expect it. A second copy of the number in the probe
/// would agree with a wrong value here as readily as with a right one.
pub const OUTLINE_WIDTH: f64 = 1.5;

/// The Bézier circle constant: `4/3 * (sqrt(2) - 1)`.
///
/// How far a quarter-arc's control points sit from its endpoints, as a fraction
/// of the radius, for the cubic that best approximates it. **Not an arbitrary
/// tuning value** --- it is what makes the curve pass through the arc's midpoint
/// exactly, and the worst radial error anywhere else is about 0.027% of the
/// radius. On a 200 pt radius that is 0.05 pt, a thirtieth of the stroke's own
/// width.
///
/// Written out rather than computed, because `f64::sqrt` is not a `const fn`,
/// and named rather than inlined four times, because a reader meeting
/// `0.5522847498307936` in a content stream has no way to tell a constant from a
/// typo.
///
/// **`markband.ts` does *not* hold a copy of this**, which is the one place the
/// overlay and the writer deliberately do different arithmetic. A canvas has
/// `ctx.ellipse` and draws a true ellipse; a content stream has no ellipse
/// operator and has to approximate. So the constant stays in the one place that
/// cannot avoid it, and the two are compared by rendering rather than by sharing
/// a literal --- `annot-probe --mode outline --kind ellipse` is that comparison.
/// (`OUTLINE_WIDTH` above *is* duplicated there, and saying so here is the
/// point: the neighbouring constant's rule is not this one's.)
const KAPPA: f64 = 0.5522847498307936;

/// How far a stamp's word sits inside its border, in points.
///
/// Larger than [`textbox::INSET`] and deliberately: a text box's inset stops
/// type touching an edge it has no border on, and a stamp's has to leave the
/// border visible as a border rather than as an underline to the word.
pub const STAMP_INSET: f64 = 4.0;

/// A capital's height as a fraction of the font size, for Helvetica.
///
/// **Used to place a stamp's baseline and to bound its size, and it is a
/// property of the face rather than a constant to tune.** Helvetica's capital
/// height is 718 units of 1000, and every word a stamp draws is upper case ---
/// so the ink's height is this and not the font size, which includes descender
/// space no stamp uses. Centring on the size instead leaves a stamp visibly high
/// in its box.
pub const STAMP_CAP: f64 = 0.718;

/// A box's path inside its quad: `[x, y, width, height]` in the page's space.
///
/// **Inset by half the stroke width**, which is the same trap [`line_rect`] is
/// written around and it bites harder here. A stroke straddles its path, so a
/// rectangle stroked exactly on the quad's edge puts half of every side outside
/// the appearance stream's `/BBox`, and a `/BBox` clips. The result is a box
/// with hairline edges rather than a missing one --- it looks like a thin
/// border, not like a bug, which is why it is arithmetic here rather than a
/// comment somewhere.
///
/// A quad thinner than the stroke is not special-cased: the inset then crosses
/// over and the rectangle is drawn inside out, which PDF renders as nothing.
/// [`crate::docmodel`] refuses an empty mark and the frontend refuses a box
/// under four points, so reaching this needs a caller that has bypassed both.
fn outline_path(quad: [f64; 4]) -> [f64; 4] {
    let inset = OUTLINE_WIDTH / 2.0;
    [
        quad[0] + inset,
        quad[1] + inset,
        (quad[2] - quad[0]) - OUTLINE_WIDTH,
        (quad[3] - quad[1]) - OUTLINE_WIDTH,
    ]
}

/// `/QuadPoints`: four corners per quad, upper-left, upper-right, lower-left,
/// lower-right.
///
/// **That order is not the one PDF 32000-1 §12.5.6.10 appears to describe**, and
/// it is the one every producer writes and every consumer expects --- the
/// specification's wording is a known erratum. Writing the specification's
/// literal reading produces a highlight that draws as an hourglass or not at
/// all, which is why this is stated here rather than left to look arbitrary.
fn quad_points(quads: &[[f64; 4]]) -> Object {
    Object::Array(
        quads
            .iter()
            .flat_map(|&[llx, lly, urx, ury]| {
                [
                    Object::Real(llx as f32),
                    Object::Real(ury as f32),
                    Object::Real(urx as f32),
                    Object::Real(ury as f32),
                    Object::Real(llx as f32),
                    Object::Real(lly as f32),
                    Object::Real(urx as f32),
                    Object::Real(lly as f32),
                ]
            })
            .collect(),
    )
}

pub(super) fn numbers(values: [f64; 4]) -> Object {
    Object::Array(values.iter().map(|v| Object::Real(*v as f32)).collect())
}

/// Writes each planned body over the annotation it names.
///
/// **Every refusal here is about the plan disagreeing with the file**, which is
/// the case a reader can act on: the document changed under them, or the comment
/// they edited is gone. None of them is about the text, which is theirs.
///
/// The object is named rather than searched for, and that is the point of
/// `annots::Comment::object` --- a scan-order id could not survive the round
/// trip through the frontend and back, because inserting a comment anywhere
/// earlier renumbers every later one.
///
/// # Errors
///
/// The object not being in the document, not being a dictionary, or not being an
/// annotation. The last is checked rather than assumed: a plan naming an
/// arbitrary object would otherwise let a caller write `/Contents` onto a page,
/// a font or the catalog, and `/Contents` means something else entirely on a
/// page. Nothing in the application builds such a plan --- this runs in the
/// worker, on a plan that crossed a process boundary, and a refusal is cheaper
/// than reasoning about who could have sent it.
pub(super) fn write_note_edits(
    incremental: &mut IncrementalDocument,
    notes: &[crate::edits::PlannedNoteEdit],
) -> Result<(), Refusal> {
    for note in notes {
        // The one thing an append does that a rewrite does not: an incremental
        // update writes only the objects it holds, so the annotation has to be
        // brought across before it can be changed. `opt_clone_*` is
        // clone-if-absent, which is what each page's `/Annots` already uses.
        incremental
            .opt_clone_object_to_new_document((note.object.0, note.object.1))
            .map_err(|e| {
                Refusal::changed(format!(
                    "the comment being edited is not in this document any more: {e}"
                ))
            })?;
        set_note(&mut incremental.new_document, note)?;
    }
    Ok(())
}

/// [`write_note_edits`] for the rewrite path.
///
/// **The same bodies, written into the document itself**, because a rewrite
/// serialises every object it holds and there is nothing to clone across. That
/// is the whole difference, and sharing [`set_note`] is what keeps it the whole
/// difference: the `/Subtype` guard exists so that a plan naming an arbitrary
/// object cannot write `/Contents` onto a page, and a second copy of this loop
/// would be free to be written without it. The trap index has that shape under
/// removing a refusal for every caller; this is the same lesson arriving as a
/// caller being *added*.
///
/// # Errors
///
/// [`set_note`]'s, one comment at a time.
pub(super) fn rewrite_note_edits(
    doc: &mut Document,
    notes: &[crate::edits::PlannedNoteEdit],
) -> Result<(), Refusal> {
    for note in notes {
        set_note(doc, note)?;
    }
    Ok(())
}

/// Takes every planned deletion off the document, and says how many.
///
/// **`pagetree::forget` rather than pruning `/Annots`**, which is the redaction
/// path's argument word for word: pruning the one list a caller has in mind is
/// what leaves the object alive, because a structure element's `/OBJR` or an
/// AcroForm's `/Fields` names it too, and an annotation still reachable is an
/// annotation still written.
///
/// The count is what the sweep is conditioned on. It is returned rather than
/// inferred from `plan.discards.len()`, and the difference is real: a plan may
/// name an object this document does not have, which is refused below, so the
/// two numbers are equal exactly when nothing went wrong --- and conditioning a
/// sweep on the number of things *asked for* is how a sweep comes to run when
/// nothing happened, or not run when something did.
///
/// # Errors
///
/// The object not being in the document, or not being an annotation. Both are
/// `set_note`'s refusals and are checked here for its reason: a plan naming an
/// arbitrary object would otherwise let a caller delete a font, a page or the
/// catalog out of the file.
pub(super) fn discard_notes(
    doc: &mut Document,
    discards: &[crate::edits::PlannedDiscard],
) -> Result<usize, Refusal> {
    let mut doomed = std::collections::HashSet::new();
    for discard in discards {
        let id = (discard.object.0, discard.object.1);
        let dictionary = doc
            .get_object(id)
            .map_err(|e| {
                Refusal::changed(format!(
                    "the comment being deleted is not in this document any more: {e}"
                ))
            })?
            .as_dict()
            .map_err(|_| Refusal::from("that comment is not an annotation"))?;
        if dictionary
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_err()
        {
            // `set_note`'s check and its reason: not "has the subtype we
            // expect", because any annotation has one and what matters is that
            // this is an annotation at all.
            return Err("that comment is not an annotation".into());
        }
        doomed.insert(id);
    }
    if doomed.is_empty() {
        return Ok(0);
    }
    let taken = doomed.len();
    crate::pagetree::forget(doc, &doomed).map_err(Refusal::from)?;
    Ok(taken)
}

/// Writes one planned body over the annotation it names.
///
/// # Errors
///
/// The object not being in the document, not being a dictionary, or not being an
/// annotation. The last is checked rather than assumed: a plan naming an
/// arbitrary object would otherwise let a caller write `/Contents` onto a page,
/// a font or the catalog, and `/Contents` means something else entirely on a
/// page.
fn set_note(doc: &mut Document, note: &crate::edits::PlannedNoteEdit) -> Result<(), Refusal> {
    let object = doc
        .get_object_mut((note.object.0, note.object.1))
        .map_err(|e| {
            Refusal::changed(format!(
                "the comment being edited is not in this document any more: {e}"
            ))
        })?;
    let dictionary = object
        .as_dict_mut()
        .map_err(|_| Refusal::from("that comment is not an annotation"))?;
    if dictionary
        .get(b"Subtype")
        .and_then(Object::as_name)
        .is_err()
    {
        // Not "has the subtype we expect": any annotation has one, and the
        // check that matters is that this is an annotation at all.
        return Err("that comment is not an annotation".into());
    }
    dictionary.set("Contents", text_string(&note.body));
    dictionary.set("M", text_string(&note.made));
    Ok(())
}

/// A PDF text string: an ASCII literal, or UTF-16BE with a byte-order mark.
///
/// The two encodings `annots.rs` reads are PDFDocEncoding and UTF-16BE, and this
/// writes the subset of the first that needs no table --- ASCII --- falling back
/// to the second for anything else. Choosing by content rather than always
/// writing UTF-16 keeps an ordinary author's name readable in a hex dump, which
/// is worth something when the next person to debug this is reading bytes.
fn text_string(value: &str) -> Object {
    if value.is_ascii() {
        return Object::string_literal(value);
    }
    let mut bytes = vec![0xFE, 0xFF];
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Object::String(bytes, lopdf::StringFormat::Hexadecimal)
}

/// A mark's box as the reader saw it, and the map back into the page.
///
/// Every quad reaching [`appearance_stream`] has been through [`user_quads`],
/// which maps the reader's rectangle into the page's own space. That is right
/// for the rectangle, which is a set of points, and wrong for everything drawn
/// *inside* it that has a direction: a rule belongs under the words as they are
/// read, and a line of type runs the way they are read. On a page carrying
/// `/Rotate 90` those two directions are a quarter turn from the page's own.
///
/// **Measured before this was written**, one mark of each kind on a 300 x 40
/// box, `testdata/inherited.pdf` (`/Rotate 90`) against
/// `testdata/text-base14.pdf`, reading where the saved file's ink landed inside
/// the box *as displayed*:
///
/// | kind | upright | turned |
/// |------|---------|--------|
/// | underline | a band at y 0.93..0.99 | a rule down the left edge, x 0.00..0.07 |
/// | strikeout | y 0.46..0.53 | a vertical line, x 0.46..0.53 |
/// | squiggly | y 0.81..0.99 | x 0.00..0.15 |
/// | text box | x 0.01..0.34 | a column at x 0.82..0.98, wrapped to the box's *height* |
/// | stamp | 25,011 px | 11,024 px, sideways |
///
/// A highlight, a box and an ellipse came out right at both, and they are the
/// three whose shape is symmetric under a quarter turn --- which is why nothing
/// caught this: the window sweep's agreement check compares *coverage
/// fractions*, and a band turned through a right angle covers the same
/// fraction of the same rectangle. The text box was the one kind it did report,
/// and the diagnosis recorded at the time was the box being too short.
///
/// The text box's own arithmetic says the rest: `textbox::wrap` was being given
/// 40 points where the reader had dragged 300, so the model broke those words
/// into one line and the file into eighteen.
pub(super) struct Upright {
    /// The box's width as the reader saw it, in points.
    pub(super) width: f64,
    /// Its height as the reader saw it, in points.
    pub(super) height: f64,
    /// The page-space point the box's displayed top-left corner sits at.
    origin: (f64, f64),
    /// One point to the reader's right, in page space.
    right: (f64, f64),
    /// One point down the reader's page, in page space.
    down: (f64, f64),
}

impl Upright {
    /// The reader's view of a page-space quad, on a page turned `turns` quarters.
    ///
    /// The inverse of what [`crate::text::from_device`] applies, for corners and
    /// directions rather than for rectangles. **Two copies of one turn is the
    /// drift the trap index warns about**, so this is not left to agree with that
    /// function by inspection: `an_upright_box_is_the_rectangle_the_reader_dragged`
    /// composes the two at every quarter and asserts the round trip.
    pub(super) fn of(turns: u8, quad: [f64; 4]) -> Self {
        let (w, h) = (quad[2] - quad[0], quad[3] - quad[1]);
        match turns % 4 {
            0 => Self {
                width: w,
                height: h,
                origin: (quad[0], quad[3]),
                right: (1.0, 0.0),
                down: (0.0, -1.0),
            },
            1 => Self {
                width: h,
                height: w,
                origin: (quad[0], quad[1]),
                right: (0.0, 1.0),
                down: (1.0, 0.0),
            },
            2 => Self {
                width: w,
                height: h,
                origin: (quad[2], quad[1]),
                right: (-1.0, 0.0),
                down: (0.0, 1.0),
            },
            _ => Self {
                width: h,
                height: w,
                origin: (quad[2], quad[3]),
                right: (0.0, -1.0),
                down: (-1.0, 0.0),
            },
        }
    }

    /// The page-space point `u` to the right of the box's displayed top-left
    /// corner and `v` below it.
    pub(super) fn at(&self, u: f64, v: f64) -> (f64, f64) {
        (
            self.origin.0 + u * self.right.0 + v * self.down.0,
            self.origin.1 + u * self.right.1 + v * self.down.1,
        )
    }

    /// A `Tm` operator setting type running the reader's way, its baseline at
    /// [`Upright::at`].
    ///
    /// **`Tm` rather than the `Td` this replaced**, and the reason is the turn:
    /// `Td` can only move an origin, so it cannot say which way the glyphs face,
    /// and every line of a turned text box would still come out along the page's
    /// own axis. Absolute rather than relative also removes the trap the old
    /// comment here warned about --- a `Td` chain stacks every line on the first
    /// if one offset is written as an absolute.
    ///
    /// The third and fourth coefficients are the *negated* downward direction,
    /// because text space measures up and a reader's box measures down.
    fn text_matrix(&self, u: f64, v: f64) -> String {
        let (x, y) = self.at(u, v);
        // Negating a zero gives `-0.0`, which formats as `-0`: a legal number
        // that every reader accepts and no human recognises as the identity.
        // `v == 0.0` is true of both zeros, so this returns the positive one.
        let flat = |value: f64| if value == 0.0 { 0.0 } else { value };
        format!(
            "{} {} {} {} {x} {y} Tm",
            flat(self.right.0),
            flat(self.right.1),
            flat(-self.down.0),
            flat(-self.down.1)
        )
    }

    /// `[x, y, width, height]` for a `re`, covering the reader's `u0..u1` by
    /// `v0..v1`.
    ///
    /// A quarter turn keeps a rectangle axis-aligned and swaps which corner is
    /// which, so the two mapped corners are sorted rather than assumed.
    fn rect(&self, u0: f64, v0: f64, u1: f64, v1: f64) -> [f64; 4] {
        let (ax, ay) = self.at(u0, v0);
        let (bx, by) = self.at(u1, v1);
        [ax.min(bx), ay.min(by), (bx - ax).abs(), (by - ay).abs()]
    }
}

/// The appearance stream a reader draws when it does not generate its own.
///
/// **Not optional, even though both PDFKit and PDFium generate one.** Measured
/// before this was written: a `/Highlight` with no `/AP` renders in Preview, so
/// the file would look right in the two readers this repository can drive --- and
/// what a reader generates is *its* wash, not ours, so the same file would
/// differ between them and could differ again after an update. An `/AP` is what
/// makes the appearance the document's own.
///
/// `/BBox` is the annotation's rectangle in page coordinates and the matrix is
/// the identity, so §12.5.5's mapping of the transformed box onto `/Rect` is a
/// no-op and the content below can be written in page coordinates rather than in
/// a translated space of its own.
///
/// `/Multiply` is what keeps the glyphs readable: a flat fill over the text
/// hides it, and the text is already in the pixels underneath. It is the same
/// choice, for the same reason, that the viewer's overlay makes with
/// `globalCompositeOperation`.
fn appearance_stream(
    doc: &mut Document,
    mark: &PlannedMark,
    quads: &[[f64; 4]],
    strokes: &[Vec<(f64, f64)>],
    rect: [f64; 4],
    turns: u8,
) -> ObjectId {
    let style = paint(mark.kind);
    let mut state = Dictionary::new();
    state.set("Type", Object::Name(b"ExtGState".to_vec()));
    // Multiply for a wash so the words show through it, Normal for anything
    // opaque so it is the colour it says it is. A multiplied red line over black
    // text is black, which is a strikeout nobody can see.
    state.set(
        "BM",
        Object::Name(if style == Paint::Wash {
            b"Multiply".to_vec()
        } else {
            b"Normal".to_vec()
        }),
    );
    state.set("CA", Object::Real(1.0));
    state.set("ca", Object::Real(1.0));
    let state = doc.add_object(state);

    let mut states = Dictionary::new();
    states.set("GS0", Object::Reference(state));
    let mut resources = Dictionary::new();
    resources.set("ExtGState", Object::Dictionary(states));
    // A font, for the one style that draws words. **Only for that style**: a
    // `/Font` entry on a highlight's resources is dead weight in every saved
    // file, and one of the standard fourteen costs nothing to name but is still
    // a dictionary and a reference per mark.
    //
    // Helvetica with `/WinAnsiEncoding` and no `/FontDescriptor`, `/Widths` or
    // `/FirstChar`: it is one of the fourteen every reader is required to have,
    // so there is no file to embed and nothing to subset -- which is what keeps
    // this clear of the two font traps this repository already records.
    if style == Paint::Text {
        let font = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let mut fonts = Dictionary::new();
        fonts.set(TEXT_FONT, Object::Reference(font));
        resources.set("Font", Object::Dictionary(fonts));
    }

    // `rg` sets the *fill* colour and `RG` the stroke's, and one operator does
    // not imply the other: a path stroked after only `rg` comes out black,
    // which on a red box looks like a colour that was ignored rather than one
    // that was never set. Both are written, in one colour, so the two can never
    // disagree.
    // **The stroke width and the joins belong to the style, not to the file.**
    // A box is four right angles, which mitre cleanly at the default `0 j`; a
    // freehand line turns at whatever angle the reader's hand made, and a mitre
    // on a sharp one spikes out to a point --- which reads as a rendering fault
    // rather than as a style. `1 J 1 j` is round caps and round joins, and it
    // also gives a stroke its ends, without which a line stops square.
    //
    // **The mark's own width for a path, and a constant for everything else.**
    // A drawing is the one kind whose weight the reader chooses --- see
    // `Mark::width` --- and a box's border is not: `OUTLINE_WIDTH` argues there
    // that a frame which competes with its contents is a worse frame, which is
    // a statement about what a box is for rather than a default somebody has
    // not got round to making adjustable.
    let (width, joins) = match style {
        Paint::Path => (mark.width, "1 J 1 j "),
        _ => (OUTLINE_WIDTH, ""),
    };
    let mut content = format!(
        "/GS0 gs {r} {g} {b} rg {r} {g} {b} RG {width} w {joins}\n",
        r = mark.color[0],
        g = mark.color[1],
        b = mark.color[2],
    );
    // **Each style draws in a function of its own, and the loop is inside it.**
    // Two properties, and the signatures hold both rather than care holding
    // them: an arm cannot be reached with the wrong collection, because
    // `draw_path` takes strokes where the seven per-quad styles take quads and
    // the types differ; and a tenth style is a compile error here, which is the
    // whole reason this is a `match` on an enum rather than a pair of booleans.
    //
    // **The extraction is what made the first property structural.** Until
    // 2026-08-26 the nine bodies were written out inline --- 268 lines between
    // this comment and the close --- and nothing stopped an arm looping over the
    // wrong collection except that none of them did, with both in scope
    // throughout. The comment defending that arrangement carried three counts and
    // every one had gone stale: it said *five* styles, the loop *written out
    // three times*, and a *sixth* style being the compile error, against nine
    // arms, seven per-quad loops and a tenth. `docs/TRAPS.md` has that entry, and
    // this is the extraction it measured.
    //
    // **No context type, which that measurement expected to be the cost.** Each
    // arm reads at most a collection, one field of the mark and the turn, so a
    // parameter list per style says exactly what that style draws from --- which
    // is the property, and is precisely what one shared struct would hand back.
    match style {
        Paint::Wash => draw_wash(&mut content, quads),
        Paint::Line => draw_line(&mut content, quads, mark.kind, turns),
        Paint::Outline => draw_outline(&mut content, quads),
        Paint::Text => draw_text(&mut content, quads, &mark.note, turns),
        Paint::Stamp => draw_stamp(&mut content, quads, mark.stamp, turns),
        Paint::Wave => draw_wave(&mut content, quads, mark.kind, turns),
        Paint::Ellipse => draw_ellipse(&mut content, quads),
        Paint::Path => draw_path(&mut content, strokes),
        // Nothing. Unreachable, because the caller does not build an
        // appearance stream for a kind that has none; written out rather
        // than caught by a wildcard so that a kind added later is a compile
        // error here as well as everywhere else.
        Paint::None => {}
    }

    let mut dictionary = Dictionary::new();
    dictionary.set("Type", Object::Name(b"XObject".to_vec()));
    dictionary.set("Subtype", Object::Name(b"Form".to_vec()));
    dictionary.set("FormType", Object::Integer(1));
    dictionary.set("BBox", numbers(rect));
    dictionary.set("Resources", Object::Dictionary(resources));
    doc.add_object(lopdf::Stream::new(dictionary, content.into_bytes()))
}

/// The whole quad, filled.
fn draw_wash(out: &mut String, quads: &[[f64; 4]]) {
    for quad in quads {
        let (x, y) = (quad[0], quad[1]);
        let (width, height) = (quad[2] - quad[0], quad[3] - quad[1]);
        out.push_str(&format!("{x} {y} {width} {height} re f\n"));
    }
}

/// A band inside it, filled. Same operator, different rectangle.
///
/// **Measured in the reader's frame, not the page's**, because "the
/// bottom of the quad" is where a rule goes and a turned page has two
/// bottoms. `line_rect` answers in a y-up frame, so it is handed the
/// reader's box and its answer read back as a distance from the reader's
/// own bottom edge.
fn draw_line(out: &mut String, quads: &[[f64; 4]], kind: MarkKind, turns: u8) {
    for quad in quads {
        let seen = Upright::of(turns, *quad);
        let (base, band) = line_rect(kind, 0.0, seen.height);
        let [x, y, width, height] = seen.rect(
            0.0,
            seen.height - base - band,
            seen.width,
            seen.height - base,
        );
        out.push_str(&format!("{x} {y} {width} {height} re f\n"));
    }
}

/// Its edge, stroked. `re S` rather than `re f`, and the path is
/// inset so the stroke lands inside the /BBox -- see `outline_path`.
fn draw_outline(out: &mut String, quads: &[[f64; 4]]) {
    for quad in quads {
        let [x, y, width, height] = outline_path(*quad);
        out.push_str(&format!("{x} {y} {width} {height} re S\n"));
    }
}

/// The reader's words, one `Tj` per line, from the top of the box down.
///
/// **The whole layout is in the reader's frame**, which is what the box's
/// width has to be: `wrap` decides where the lines break, and handing it
/// the page's width breaks them against the box's *height* on a turned
/// page --- eighteen lines two glyphs wide where the model, which works in
/// the reader's frame throughout, had made one. `Upright` records the
/// measurement.
///
/// Lines that would fall below the box are dropped rather than drawn: the
/// /BBox clips them anyway, and emitting ink nobody can see makes the
/// stream disagree with what the overlay shows. The rule is
/// `viewer.ts`'s exactly --- a line is drawn while its baseline is still
/// inside the box --- so the two renderers stop at the same line.
fn draw_text(out: &mut String, quads: &[[f64; 4]], note: &str, turns: u8) {
    for quad in quads {
        let seen = Upright::of(turns, *quad);
        let width = seen.width - textbox::INSET * 2.0;
        let lines = textbox::wrap(note, textbox::SIZE, width.max(1.0));
        if lines.is_empty() {
            continue;
        }
        let leading = textbox::SIZE * textbox::LEADING;
        out.push_str(&format!(
            "BT /{TEXT_FONT} {size} Tf\n",
            size = textbox::SIZE
        ));
        for (index, line) in lines.iter().enumerate() {
            // The baseline sits one ascent below the top inset, not at
            // it: a line placed *at* the top edge hangs its whole body
            // above the box.
            let down = textbox::INSET + textbox::SIZE + leading * (index as f64);
            if down > seen.height {
                break;
            }
            out.push_str(&format!("{}\n", seen.text_matrix(textbox::INSET, down)));
            out.push_str(&format!("<{}> Tj\n", winansi_hex(line)));
        }
        out.push_str("ET\n");
    }
}

/// A border and one word, both in the mark's colour.
///
/// **The size is computed rather than fixed**, which is the difference
/// from `Paint::Text` above and the reason a stamp needs no wrapping. A
/// stamp is one word and a reader who drags a large rectangle means a
/// large stamp, so the size is whatever makes the word span the box
/// between its insets --- bounded above by what the height can hold, so a
/// wide flat rectangle gives a word that fits rather than one clipped by
/// the /BBox.
///
/// `advance` is the same Helvetica table `textbox.rs` wraps with, and
/// `helvetica-probe` measures it against what PDFium actually inks. A
/// stamp is the second consumer of it, which is worth noting because a
/// wrong entry here is visible as a word that is off-centre rather than
/// as a word in the wrong place.
fn draw_stamp(
    out: &mut String,
    quads: &[[f64; 4]],
    stamp: Option<crate::docmodel::StampName>,
    turns: u8,
) {
    for quad in quads {
        let Some(name) = stamp else {
            continue;
        };
        let word = name.word();
        // The reader's box, for `Paint::Text`'s reason and one of its
        // own: the size is a ratio of width to height, so on a turned
        // page the page's own box does not merely rotate the word, it
        // sets it at the size a rectangle of the other shape would take.
        // The border is unaffected and stays in page space --- a
        // rectangle is the same set of points at every quarter.
        let seen = Upright::of(turns, *quad);
        let inner_w = seen.width - STAMP_INSET * 2.0;
        let inner_h = seen.height - STAMP_INSET * 2.0;
        let [x, y, width, height] = outline_path(*quad);
        out.push_str(&format!("{x} {y} {width} {height} re S\n"));
        if inner_w <= 0.0 || inner_h <= 0.0 {
            continue;
        }
        // The advance at one point, so the ratio is a division rather
        // than a search. `max` guards a name that measured zero, which
        // no entry in the list does and which a table edit could make
        // true.
        let unit = textbox::advance(word, 1.0).max(f64::EPSILON);
        let size = (inner_w / unit).min(inner_h / STAMP_CAP).max(1.0);
        // Centred both ways. The baseline sits half a cap height below
        // the middle, because a word centred *on* the middle hangs half
        // its body below it.
        let drawn = textbox::advance(word, size);
        let across = (seen.width - drawn) / 2.0;
        let down = (seen.height + size * STAMP_CAP) / 2.0;
        out.push_str(&format!("BT /{TEXT_FONT} {size} Tf\n"));
        out.push_str(&format!("{}\n", seen.text_matrix(across, down)));
        out.push_str(&format!("<{}> Tj\n", winansi_hex(word)));
        out.push_str("ET\n");
    }
}

/// A zigzag along the bottom of the quad, stroked.
///
/// **Straight segments rather than curves, and that is a decision.** A
/// squiggle could be drawn as arcs, and Acrobat's is; at this size the
/// difference is invisible and the cost is not. A zigzag is exact -- `l`
/// says what it means -- where a curve would put a second approximation
/// constant beside `KAPPA` for a shape whose whole peak-to-trough height
/// is under two points on body text.
///
/// Its own `w`, because the header wrote one width for the stream and
/// this thickness is a fraction of *this quad's* height. A run crossing a
/// heading has quads of two sizes and would otherwise get one thickness.
///
/// The trough sits half a stroke above the quad's bottom edge and the
/// peak half a stroke below the band's top, for `outline_path`'s reason:
/// the /BBox clips, and a stroke centred on the edge loses half its width
/// in every reader -- which reads as a thinner wave rather than as a bug.
fn draw_wave(out: &mut String, quads: &[[f64; 4]], kind: MarkKind, turns: u8) {
    for quad in quads {
        // The reader's frame, for the rule's reason: a wave runs along
        // the words and climbs away from them, and both of those are
        // directions rather than page axes.
        let seen = Upright::of(turns, *quad);
        let thickness = seen.height * LINE_FRACTION;
        let (base, band) = line_rect(kind, 0.0, seen.height);
        let low = base + thickness / 2.0;
        let high = base + band - thickness / 2.0;
        let mut half = band * SQUIGGLE_PERIOD / 2.0;
        // A quad too short to hold one climb would emit `m` and no
        // segment, which strokes nothing; a degenerate band is dropped
        // by `user_quads` long before this, and this guard is for the
        // arithmetic rather than for the data.
        //
        // **`is_finite` on both, and not for tidiness.** A `NaN` fails
        // every comparison, so `half <= 0.0` is *false* for one and this
        // guard would pass it through; the loop below then happens not
        // to run, because `0.0 < NaN` is false too. Relying on that is
        // relying on the second of two accidents, and the width arrives
        // from a plan the writer did not build.
        if !half.is_finite() || !seen.width.is_finite() || half <= 0.0 || high <= low {
            continue;
        }
        // The bound. See [`MAX_WAVE_SEGMENTS`]: widen the period rather
        // than refuse, so the mark is still drawn where the reader put
        // it, and do it here rather than clamping the quad, because the
        // quad's *position* is not in question --- only how finely the
        // wave across it is stepped.
        if seen.width / half > MAX_WAVE_SEGMENTS {
            half = seen.width / MAX_WAVE_SEGMENTS;
        }
        // `across` runs the way the words do and `up` measures from the
        // reader's bottom edge, which is what `line_rect` answered in.
        let point = |across: f64, up: f64| seen.at(across, seen.height - up);
        out.push_str(&format!("{thickness} w\n"));
        let (mx, my) = point(0.0, low);
        out.push_str(&format!("{mx} {my} m\n"));
        let mut along = 0.0;
        let mut climbing = true;
        while along < seen.width {
            let next = (along + half).min(seen.width);
            // The last segment is clipped to the quad's right edge, so
            // it ends part-way up its climb rather than overshooting.
            // Interpolated rather than snapped to the peak: a wave that
            // jumped to full height in a tenth of a period ends on a
            // near-vertical stroke, which looks like a stray tick.
            let reached = (next - along) / half;
            let (from, to) = if climbing { (low, high) } else { (high, low) };
            let (lx, ly) = point(next, from + (to - from) * reached);
            out.push_str(&format!("{lx} {ly} l\n"));
            along = next;
            climbing = !climbing;
        }
        out.push_str("S\n");
    }
}

/// Its inscribed ellipse, stroked. Four Bézier arcs, because a content
/// stream has no ellipse operator -- `re` is the only built-in shape
/// there is, and it is a rectangle.
///
/// KAPPA is what makes four cubics look like an ellipse rather than
/// nearly like one; `outline_path` insets first, for the reason it
/// gives, so the stroke lands inside the /BBox exactly as the box's does.
fn draw_ellipse(out: &mut String, quads: &[[f64; 4]]) {
    for quad in quads {
        let [x, y, width, height] = outline_path(*quad);
        let (rx, ry) = (width / 2.0, height / 2.0);
        let (cx, cy) = (x + rx, y + ry);
        let (ox, oy) = (rx * KAPPA, ry * KAPPA);
        // From the right of the ellipse, anticlockwise. `h` closes it
        // rather than the fourth arc's endpoint being trusted to land
        // back on the first: they agree to the last bit here, and a
        // path left open joins with a cap instead of a join, which
        // shows as a nick at three o'clock on a thick stroke.
        out.push_str(&format!("{} {cy} m\n", cx + rx));
        out.push_str(&format!(
            "{} {} {} {} {cx} {} c\n",
            cx + rx,
            cy + oy,
            cx + ox,
            cy + ry,
            cy + ry
        ));
        out.push_str(&format!(
            "{} {} {} {} {} {cy} c\n",
            cx - ox,
            cy + ry,
            cx - rx,
            cy + oy,
            cx - rx
        ));
        out.push_str(&format!(
            "{} {} {} {} {cx} {} c\n",
            cx - rx,
            cy - oy,
            cx - ox,
            cy - ry,
            cy - ry
        ));
        out.push_str(&format!(
            "{} {} {} {} {} {cy} c\n",
            cx + ox,
            cy - ry,
            cx + rx,
            cy - oy,
            cx + rx
        ));
        out.push_str("h S\n");
    }
}

/// The path itself: `m` to the first point, `l` to each of the rest, and
/// one `S` per stroke. A single `S` after all of them would join the end
/// of each stroke to the start of the next with a line the reader never
/// drew --- which is precisely the join `/InkList` exists to keep apart,
/// and it would look like a drawing rather than like a bug.
fn draw_path(out: &mut String, strokes: &[Vec<(f64, f64)>]) {
    for stroke in strokes {
        let Some(((x0, y0), rest)) = stroke.split_first() else {
            continue;
        };
        out.push_str(&format!("{x0} {y0} m\n"));
        for (x, y) in rest {
            out.push_str(&format!("{x} {y} l\n"));
        }
        out.push_str("S\n");
    }
}

/// Appends an annotation to a page's `/Annots`, whatever shape that array is in.
///
/// Three cases, and the middle one is why this is a function rather than three
/// lines at the call site: the entry may be missing, an inline array, or a
/// reference to an array object that other pages may also name. The reference is
/// followed and the array it points at is extended, so the file's own structure
/// is preserved rather than replaced with an inline copy.
///
/// # Errors
///
/// The page is not a dictionary, or `/Annots` is a reference to something that
/// is not an array. Both are malformed documents, and a mark written into one
/// anyway would be a mark nothing displays.
fn attach(
    doc: &mut Document,
    page: ObjectId,
    site: &AnnotsSite,
    annotation: ObjectId,
) -> Result<(), String> {
    // **The site is read rather than re-read, and that is what makes this work
    // on both paths.** It used to look the page up in `doc` --- correct for a
    // rewrite, where `doc` is the whole document, and wrong for an append, where
    // a page whose `/Annots` is its own object is deliberately *not* in the
    // update section. Asking there would fail on exactly the documents the
    // append is cheapest on.
    match site {
        AnnotsSite::ArrayObject(array_id) => {
            let array = doc
                .get_object_mut(*array_id)
                .and_then(Object::as_array_mut)
                .map_err(|e| format!("this page's /Annots is not an array: {e}"))?;
            array.push(Object::Reference(annotation));
        }
        // Both rewrite the page dictionary, and the entries come from the site
        // because the document being written to may not hold the page to read
        // them back from. A second mark on the same page finds the first through
        // `doc` --- which is why the array is read from there when it is already
        // present, and from the site when it is not.
        AnnotsSite::Inline(_) | AnnotsSite::Absent => {
            let existing = doc
                .get_object(page)
                .and_then(Object::as_dict)
                .ok()
                .and_then(|dict| dict.get(b"Annots").ok())
                .and_then(|found| found.as_array().ok())
                .cloned();
            let mut array = match (existing, site) {
                (Some(already), _) => already,
                (None, AnnotsSite::Inline(entries)) => entries.clone(),
                (None, _) => Vec::new(),
            };
            array.push(Object::Reference(annotation));
            doc.get_object_mut(page)
                .and_then(Object::as_dict_mut)
                .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?
                .set("Annots", Object::Array(array));
        }
    }
    Ok(())
}
