//! The comments in a document --- what a PDF calls markup annotations and a
//! reader calls notes, highlights and replies.
//!
//! ## The marks are already on the page; the words are not
//!
//! `progressive.rs` renders with `FPDF_ANNOT`, so a sticky note's icon and a
//! highlight's wash are painted by PDFium whether or not this module exists ---
//! measured on a fixture with no appearance streams at all, where PDFium
//! generates them: the note icon fills 637 of the 756 pixels in its own
//! rectangle and the highlight 6,690 of 9,436. What no reader can reach today is
//! the *text*: who wrote it, when, what it says, and what somebody replied. That
//! is what this reads.
//!
//! ## Why `lopdf` and not PDFium, when PDFium can do it
//!
//! `FPDFPage_GetAnnot` and friends work --- verified before this was written ---
//! but they take an `FPDF_PAGE`, and the sidebar's question is about the whole
//! document. `FPDF_LoadPage` re-parses every time and costs 44 ms on a complex
//! page (`docs/PLAN.md` §4), so answering "list every comment" through PDFium is
//! a page load per page: on the 775-page fixture that is seconds, on the render
//! thread, for a panel somebody may never open.
//!
//! The object graph answers it without loading anything. `/Annots` is a page
//! dictionary entry and every field wanted here is a direct value in the
//! annotation dictionary, so this is the same shape as [`crate::encoding`] ---
//! one `lopdf` parse of the whole file, measured there at 0.1 ms on a small
//! document and 11.9 ms on a 337 MB scan, because `lopdf` reads the xref and
//! object headers rather than every stream.
//!
//! It also gets `/IRT` for free, and PDFium does not expose it through
//! `pdfium-render` at all --- so on the same fixture a reply arrives as an
//! unrelated second note by another author, which is worse than not showing it.
//!
//! ## Everything here is attacker-controlled, and two things follow
//!
//! **No field may carry a URL.** A comment is text a stranger wrote, and it
//! reaches the DOM. `docs/THREAT-MODEL.md` T8 holds because nothing
//! attacker-controlled arrives in a position where the frontend could turn it
//! into a navigation --- the same property `outline.rs` keeps with
//! [`crate::outline::Target`], and `no_comment_field_may_carry_a_url` below is
//! the test that says so. [`Kind`] is an enum of our own literals rather than the
//! document's `/Subtype` string, and the date is *built* here from parsed
//! numbers rather than passed through, precisely so that neither is a place a
//! string from the file can hide.
//!
//! **Every bound reports what it cut.** [`Limits`] is counted and shown, for the
//! reason `outline.rs` states: a truncated answer displayed as a complete one is
//! the same class of failure as a leak scanner reporting clean on a carrier it
//! could not decode.
//!
//! ## Three encodings for one string, and a fourth thing that is not one
//!
//! A PDF text string is UTF-16BE when it starts with a byte-order mark, UTF-8
//! when it starts with the one PDF 2.0 added, and PDFDocEncoding otherwise
//! (PDF 32000-1 §7.9.2.2). The last is *not* Latin-1: they agree on the accented
//! range and disagree over 0x18--0x1F and 0x80--0x9F, where PDFDocEncoding has
//! punctuation and Latin-1 has control codes. [`decode_text_string`] uses
//! `lopdf`'s own `PDF_DOC_ENCODING` table rather than a transcription of the
//! specification's, since a table typed out here would be a second authority
//! agreeing with itself.
//!
//! With one deliberate exception, and it matters for exactly this data: that
//! table is built from *glyph names*, so it maps nothing below 0x18 --- and a
//! comment body's newlines live there. Running a two-paragraph note through it
//! unchanged returns one paragraph. So bytes below 0x18 are handled here: tab
//! and the two newline characters are content, everything else is dropped.

use std::collections::{HashMap, HashSet};

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId};

use crate::encoding::resolve;

use crate::encoding::MAX_DECODE;

/// Deepest the `/MediaBox` and `/Rotate` inheritance walk will follow `/Parent`.
const MAX_INHERIT: usize = 32;

/// Most comments the scan will report for one page.
const MAX_PER_PAGE: usize = 1_000;

/// Most comments the scan will report for the whole document.
///
/// Every one of these is a real DOM row in the sidebar --- the list is bounded
/// rather than virtualized, the same trade `outline.rs` makes and for the same
/// reason.
const MAX_TOTAL: usize = 5_000;

/// Longest body kept, in characters.
///
/// A comment is a sentence or a paragraph. This is generous enough that no
/// genuine one is touched and small enough that a document cannot fill the
/// sidebar's memory from a field nobody bounded.
const MAX_BODY_CHARS: usize = 4_000;

/// Longest author or subject kept, in characters.
const MAX_LINE_CHARS: usize = 120;

/// Most quadrilaterals kept for one comment.
///
/// A highlight is a phrase, a line or a paragraph, so this is generous for
/// anything a person made by dragging. What it bounds is a producer that writes
/// one quad per glyph, which is legal and which a page of them would otherwise
/// turn into a megabyte of geometry in a panel that draws none of it.
const MAX_QUADS: usize = 512;

/// Longest raw string the scan will decode, in bytes.
///
/// Decoding is linear, so this bounds the work rather than a correctness
/// problem: past it the string is declined and counted, never truncated
/// mid-character.
const MAX_STRING_BYTES: usize = 1 << 20;

/// Deepest reply chain walked when breaking `/IRT` cycles.
const MAX_THREAD_DEPTH: usize = 64;

/// What kind of mark a comment is.
///
/// **Ours, not the document's.** The variants are the markup annotation subtypes
/// of PDF 32000-1 §12.5.6.2 that a reader would recognise; a `/Subtype` this does
/// not know is counted in [`Limits::unknown_kinds`] and left out, rather than
/// carried through as a string. That is what keeps a document from choosing a
/// value that reaches the frontend --- see the module note on T8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    FreeText,
    Highlight,
    Underline,
    Squiggly,
    StrikeOut,
    Square,
    Circle,
    Line,
    Polygon,
    PolyLine,
    Ink,
    Stamp,
    Caret,
    FileAttachment,
    Sound,
    Redact,
}

impl Kind {
    /// The markup subtype this name stands for, or `None` for anything else.
    ///
    /// `/Popup`, `/Link` and `/Widget` land here as `None` on purpose and are
    /// **not** counted as unknown: a popup is the note's own window rather than a
    /// second note, and a link or a form field is not a comment at all. Counting
    /// them would put a permanent "some comments were dropped" notice on every
    /// document that has a hyperlink in it.
    /// **Public so that `annot-probe --mode preview` can put PDFKit's reading of
    /// `/Subtype` through this reader's own parser**, and compare the kind
    /// rather than the string. The probe holding a table of its own would be a
    /// copy free to drift from this one; borrowing the *writer's* table instead
    /// was tried and is worse, because then a mutation of the writer moves the
    /// check with it and the assertion cannot fail --- measured, and recorded in
    /// `docs/TRAPS.md`.
    pub fn of(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"Text" => Self::Text,
            b"FreeText" => Self::FreeText,
            b"Highlight" => Self::Highlight,
            b"Underline" => Self::Underline,
            b"Squiggly" => Self::Squiggly,
            b"StrikeOut" => Self::StrikeOut,
            b"Square" => Self::Square,
            b"Circle" => Self::Circle,
            b"Line" => Self::Line,
            b"Polygon" => Self::Polygon,
            b"PolyLine" => Self::PolyLine,
            b"Ink" => Self::Ink,
            b"Stamp" => Self::Stamp,
            b"Caret" => Self::Caret,
            b"FileAttachment" => Self::FileAttachment,
            b"Sound" => Self::Sound,
            b"Redact" => Self::Redact,
            _ => return None,
        })
    }

    /// Whether this kind marks *words*, and so carries `/QuadPoints`.
    ///
    /// The four text markup annotations of PDF 32000-1 §12.5.6.10. `/Redact`
    /// carries the key too and is deliberately not here: a redaction names the
    /// words it is going to remove, which is not the same statement as a
    /// highlight naming the words somebody found interesting, and a panel that
    /// listed one by the other would be reading out text the document's author
    /// marked for deletion.
    pub fn covers_text(self) -> bool {
        matches!(
            self,
            Self::Highlight | Self::Underline | Self::Squiggly | Self::StrikeOut
        )
    }

    /// Whether a subtype is one deliberately not reported, as distinct from one
    /// this does not recognise.
    fn is_not_a_comment(name: &[u8]) -> bool {
        matches!(
            name,
            b"Popup" | b"Link" | b"Widget" | b"Screen" | b"PrinterMark" | b"TrapNet" | b"Watermark"
        )
    }
}

/// One comment.
///
/// No field here can hold a URL, and that is asserted rather than intended ---
/// see `no_comment_field_may_carry_a_url`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Comment {
    /// Ours, assigned in document order. Stable for one scan, and the only name
    /// the frontend ever uses for a comment.
    pub id: u32,
    /// Zero-based page index.
    pub page: u32,
    pub kind: Kind,
    /// `/T`, the annotation's author. Empty when the document names none.
    pub author: String,
    /// `/Contents`, flattened to something a panel can show. Empty for a mark
    /// somebody made without typing anything, which is reported rather than
    /// dropped --- the mark is on the page either way.
    pub body: String,
    /// `/Subj`, which Acrobat shows as a title above the body.
    pub subject: String,
    /// `/M`, parsed and re-emitted as `YYYY-MM-DD HH:MM`. `None` when the
    /// document's date is missing or is not a date.
    pub date: Option<String>,
    /// `/Rect` in **display** space: x and y from the displayed page's top-left
    /// corner, in points, after `/Rotate`. The same space `text.rs` reports
    /// character boxes in, which is what lets the viewer place both with one
    /// mapping.
    pub rect: [f32; 4],
    /// The words this comment covers, four values per rectangle, in the same
    /// **display** space as [`Comment::rect`].
    ///
    /// `/QuadPoints`, and empty for every kind that does not mark words --- see
    /// [`Kind::covers_text`] --- and for a text markup annotation that omits the
    /// key, which §12.5.6.10 permits and which means the `/Rect` is the region.
    ///
    /// Flattened, four to a rectangle, matching `edits::MarkView::quads`: the
    /// frontend already has that vocabulary for the reader's own marks and a
    /// second shape here would be a second thing to get right at every call
    /// site.
    ///
    /// This is geometry rather than text, and the panel that wants the *words*
    /// intersects it with the page's characters on the frontend --- which is
    /// where the one extraction everything else reads already is. Doing it here
    /// would put an `FPDF_LoadPage` per annotated page inside a scan whose whole
    /// design, stated at the top of this file, is to avoid exactly that.
    pub quads: Vec<f32>,
    /// The comment this one replies to, by [`Comment::id`].
    ///
    /// Acyclic by construction: a chain that loops has its last link cut, so a
    /// consumer can walk parents without a visited set of its own.
    pub reply_to: Option<u32>,
    /// `/F` bit 2. A hidden comment is listed and marked, not dropped --- it is
    /// still something somebody wrote, and it is on the page's `/Annots` either
    /// way.
    pub hidden: bool,
    /// The object this annotation **is**, as `[number, generation]`.
    ///
    /// [`Comment::id`] cannot survive a save: it is a position in one scan, so
    /// inserting a comment on an earlier page renumbers every later one. An
    /// object number is what the file itself calls this annotation, and it is
    /// what an incremental update overrides --- so it is the only name by which
    /// a reader's edit to somebody else's note can be written back.
    ///
    /// **`None` is a structural limit rather than a gap.** PDF permits an
    /// annotation to be a *direct dictionary* inside the page's `/Annots` array
    /// rather than a reference to an object of its own, and this scan accepts
    /// those --- they are on the page and somebody wrote them. Such a comment
    /// has no object number, so an incremental update has nothing to override:
    /// changing it means rewriting the page that contains it. A consumer must
    /// therefore treat `None` as *this one cannot be edited in place*, not as
    /// *this one was missed*. The same annotation can never be an `/IRT` target
    /// either, for the same reason, which is why `ids` below has always skipped
    /// it.
    pub object: Option<(u32, u16)>,
}

/// What the bounds cut off, so the UI can say the list is incomplete.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Limits {
    /// Pages whose comments were cut at [`MAX_PER_PAGE`].
    pub crowded_pages: usize,
    /// Whether [`MAX_TOTAL`] was reached, so later pages were not read at all.
    pub over_budget: bool,
    /// Bodies shortened at [`MAX_BODY_CHARS`], or declined at
    /// [`MAX_STRING_BYTES`].
    pub bodies_clipped: usize,
    /// Annotations with a `/Subtype` this does not know, left out.
    pub unknown_kinds: usize,
    /// `/Annots` entries that could not be read at all: a reference to nothing,
    /// an entry that is not a dictionary, a page that does not resolve.
    pub unreadable: usize,
    /// Reply links dropped because following them would have looped.
    pub cycles: usize,
    /// Pages PDFium has that `lopdf` could not account for.
    ///
    /// See [`crate::links::Limits::pages_missed`] for the case, and note the
    /// example this used to give was the wrong one: an *empty* user password is
    /// no obstacle to `lopdf`, which tries it unprompted. The document that
    /// produces this is one behind a **real** password, parsed without it ---
    /// `lopdf` then reads no objects at all, every loop here runs zero times,
    /// and a reader is told the document has no comments when what happened is
    /// that nothing could look.
    ///
    /// It is also the only observable that catches it: this fixture carries no
    /// comments, so counting them cannot tell "none" from "could not look".
    /// `examples/password_probe.rs` checks exactly this field, because a check
    /// on the count reddened nothing when the password was taken away.
    pub pages_missed: usize,
}

impl Limits {
    /// Whether anything was cut. The UI shows a notice on exactly this.
    pub fn any(&self) -> bool {
        self.crowded_pages > 0
            || self.over_budget
            || self.bodies_clipped > 0
            || self.unknown_kinds > 0
            || self.unreadable > 0
            || self.cycles > 0
            || self.pages_missed > 0
    }
}

/// Every comment in a document, in page order.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Comments {
    pub items: Vec<Comment>,
    pub limits: Limits,
    /// Time spent scanning, in milliseconds.
    pub scan_ms: f64,
}

/// Reads every comment in a document.
///
/// # Errors
///
/// The bytes not parsing as a PDF, or a stream exceeding [`MAX_DECODE`]. A
/// failure is reported rather than answered with an empty list: "this document
/// has no comments" and "this document could not be read" are different things
/// to tell a reader, and the second one is not reassuring.
///
/// **`password` is the reader's, when the document needed one, and it decides
/// whether this reads anything at all.** `lopdf` tries the empty password by
/// itself, so a permission-restricted document needs nothing here --- but one
/// behind a real password parses to a `Document` with **no objects in it**,
/// which loads cleanly and reports zero pages. Without the key that is
/// indistinguishable from a document that simply has none of what is being
/// looked for. See [`crate::progressive::RawDocument::password`].
pub fn scan(bytes: &[u8], page_count: usize, password: Option<&str>) -> Result<Comments, String> {
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
    let mut items: Vec<Comment> = Vec::new();
    // Where each annotation object ended up, so `/IRT` can name it.
    let mut ids: HashMap<ObjectId, u32> = HashMap::new();
    let mut parents: Vec<Option<ObjectId>> = Vec::new();

    let pages = document.get_pages();
    // What PDFium can see and this cannot, counted before the walk --- whose own
    // emptiness is exactly what it cannot distinguish. See `Limits`.
    limits.pages_missed = page_count.saturating_sub(pages.len());

    for (index, page) in pages.values().take(page_count).enumerate() {
        if items.len() >= MAX_TOTAL {
            limits.over_budget = true;
            break;
        }
        read_page(
            &document,
            *page,
            index as u32,
            &mut items,
            &mut ids,
            &mut parents,
            &mut limits,
        );
    }

    resolve_replies(&mut items, &ids, &parents, &mut limits);

    Ok(Comments {
        items,
        limits,
        scan_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

/// Reads one page's `/Annots`, appending what it finds.
fn read_page(
    document: &Document,
    page: ObjectId,
    number: u32,
    items: &mut Vec<Comment>,
    ids: &mut HashMap<ObjectId, u32>,
    parents: &mut Vec<Option<ObjectId>>,
    limits: &mut Limits,
) {
    let Ok(dict) = document.get_dictionary(page) else {
        limits.unreadable += 1;
        return;
    };
    let Ok(annots) = dict.get(b"Annots") else {
        return;
    };
    // An indirect `/Annots` is ordinary --- `AGENTS.md` records that whether it
    // is indirect decides how large an annotation *edit* is --- and a scan that
    // did not resolve it would report a page of comments as having none.
    let Object::Array(entries) = resolve(document, annots) else {
        limits.unreadable += 1;
        return;
    };

    let (width, height, turns, ox, oy) = page_geometry(document, page);
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

        let Ok(subtype) = annot.get(b"Subtype").and_then(Object::as_name) else {
            // No subtype at all. Unreadable rather than unknown: there is
            // nothing to have failed to recognise.
            limits.unreadable += 1;
            continue;
        };
        let Some(kind) = Kind::of(subtype) else {
            if !Kind::is_not_a_comment(subtype) {
                limits.unknown_kinds += 1;
            }
            continue;
        };

        // Matched once and used twice: `/IRT` needs it to name a parent, and
        // `Comment::object` needs it because a scan-order id cannot outlive the
        // scan. A direct dictionary has no object of its own and gets `None`
        // from the same expression, which is what keeps the two readings of
        // "this annotation has no name" from drifting apart.
        let own = match entry {
            Object::Reference(id) => Some(*id),
            _ => None,
        };
        let id = items.len() as u32;
        let mut built = comment(
            annot,
            document,
            id,
            number,
            kind,
            Frame {
                width,
                height,
                turns,
                origin: (ox, oy),
            },
            limits,
        );
        built.object = own;
        items.push(built);
        parents.push(match annot.get(b"IRT") {
            Ok(Object::Reference(target)) => Some(*target),
            _ => None,
        });
        if let Some(own) = own {
            ids.insert(own, id);
        }
        on_this_page += 1;
    }
}

/// Builds one comment from its annotation dictionary.
fn comment(
    annot: &Dictionary,
    document: &Document,
    id: u32,
    page: u32,
    kind: Kind,
    frame: Frame,
    limits: &mut Limits,
) -> Comment {
    let (body, clipped) = text_field(annot, document, b"Contents", MAX_BODY_CHARS, true);
    if clipped {
        limits.bodies_clipped += 1;
    }
    let (author, _) = text_field(annot, document, b"T", MAX_LINE_CHARS, false);
    let (subject, _) = text_field(annot, document, b"Subj", MAX_LINE_CHARS, false);

    Comment {
        id,
        page,
        kind,
        author,
        body,
        subject,
        date: annot
            .get(b"M")
            .ok()
            .map(|object| resolve(document, object))
            .and_then(|object| object.as_str().ok())
            .and_then(parse_date),
        rect: rect_of(annot, document, frame),
        // By kind rather than by whether the key is there, for the reason
        // `covers_text` gives: a `/Redact` carries `/QuadPoints` too and its
        // words are the ones somebody marked for deletion.
        quads: if kind.covers_text() {
            quads_of(annot, document, frame)
        } else {
            Vec::new()
        },
        // Filled in by `resolve_replies`, which is the only place that can know
        // whether a link would loop.
        reply_to: None,
        // Filled in by the caller, which is the only place that holds the
        // `/Annots` entry --- this function is given the dictionary, and a
        // dictionary does not know whether it reached here through a reference
        // or was written inline in the array.
        object: None,
        hidden: annot
            .get(b"F")
            .ok()
            .and_then(|object| resolve(document, object).as_i64().ok())
            .is_some_and(|flags| flags & 2 != 0),
    }
}

/// Reads one text-string field, decoded and flattened.
///
/// Returns the value and whether it was shortened.
fn text_field(
    annot: &Dictionary,
    document: &Document,
    key: &[u8],
    limit: usize,
    keep_paragraphs: bool,
) -> (String, bool) {
    let Ok(object) = annot.get(key) else {
        return (String::new(), false);
    };
    let Ok(raw) = resolve(document, object).as_str() else {
        return (String::new(), false);
    };
    if raw.len() > MAX_STRING_BYTES {
        // Declined whole rather than cut: a prefix of a megabyte string is not
        // worth the risk of splitting a multi-byte character, and the caller
        // counts this as a clip either way.
        return (String::new(), true);
    }
    let decoded = decode_text_string(raw);
    if keep_paragraphs {
        sanitize_body(&decoded, limit)
    } else {
        crate::outline::sanitize_title(&decoded, limit)
    }
}

/// A page's displayed size and quarter-turns, following inheritance.
///
/// Falls back to US Letter unrotated, which is what a page with no `/MediaBox`
/// anywhere in its ancestry is treated as by most readers. The rectangle is the
/// only thing that depends on it, so a wrong guess misplaces a marker rather
/// than losing a comment.
fn page_geometry(document: &Document, page: ObjectId) -> (f32, f32, u8, f32, f32) {
    // `/CropBox` where there is one, else `/MediaBox`, intersected per §14.11.2.
    // **PDFium lays a page out from its crop box**, so the viewer's coordinates
    // start at that box's corner --- and reading the media box alone put every
    // rectangle out by the difference, on a page that looks entirely normal. The
    // measurement is on `crate::links::page_geometry`, which does the same thing
    // and which `both_scans_agree_about_a_rotated_page` holds this one to.
    let box_of = |key: &[u8]| {
        inherited(document, page, key)
            .and_then(|object| numbers(document, &object, 4))
            // `is_finite` as well as ordered, for the reason `outline.rs`
            // states: a NaN read from the file passes `> 0.0` being false and
            // poisons every subtraction downstream.
            .filter(|values| values.iter().all(|value| value.is_finite()))
            .map(|v| {
                [
                    v[0].min(v[2]),
                    v[1].min(v[3]),
                    v[0].max(v[2]),
                    v[1].max(v[3]),
                ]
            })
            .filter(|b| b[2] > b[0] && b[3] > b[1])
    };

    let media = box_of(b"MediaBox").unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let crop = box_of(b"CropBox").map(|crop| {
        [
            crop[0].max(media[0]),
            crop[1].max(media[1]),
            crop[2].min(media[2]),
            crop[3].min(media[3]),
        ]
    });
    // An intersection can be empty if the boxes do not overlap, which is a
    // malformed document rather than a page of no size.
    let shown = match crop {
        Some(crop) if crop[2] > crop[0] && crop[3] > crop[1] => crop,
        _ => media,
    };

    let turns = inherited(document, page, b"Rotate")
        .and_then(|object| resolve(document, &object).as_i64().ok())
        .map(|degrees| (((degrees / 90) % 4 + 4) % 4) as u8)
        .unwrap_or(0);

    let (width, height) = (shown[2] - shown[0], shown[3] - shown[1]);
    // The displayed size, which is what `/Rotate` produces and what the viewer
    // lays out --- so a quarter turn swaps them.
    if turns % 2 == 1 {
        (height, width, turns, shown[0], shown[1])
    } else {
        (width, height, turns, shown[0], shown[1])
    }
}

/// An inheritable page attribute, walking `/Parent` under a bound.
///
/// `lopdf` has `get_inherited_page_property` and it is not used here for the
/// reason `encoding.rs` gives: it does not bound the walk, and a `/Parent` cycle
/// is a hostile document's cheapest trick.
fn inherited(document: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let mut node = page;
    let mut seen: HashSet<ObjectId> = HashSet::new();

    for _ in 0..MAX_INHERIT {
        if !seen.insert(node) {
            return None;
        }
        let dict = document.get_dictionary(node).ok()?;
        if let Ok(value) = dict.get(key) {
            return Some(value.clone());
        }
        match dict.get(b"Parent") {
            Ok(Object::Reference(parent)) => node = *parent,
            _ => return None,
        }
    }
    None
}

/// Reads an array of `count` numbers, following one reference at each level.
fn numbers(document: &Document, object: &Object, count: usize) -> Option<Vec<f32>> {
    let Object::Array(array) = resolve(document, object) else {
        return None;
    };
    if array.len() < count {
        return None;
    }
    let values: Vec<f32> = array
        .iter()
        .take(count)
        .filter_map(|item| resolve(document, item).as_float().ok())
        .collect();
    (values.len() == count).then_some(values)
}

/// An annotation's `/Rect`, normalised and mapped into display space.
///
/// Three things happen here and each is a way real documents differ from the
/// specification's description of them. The corners are sorted, because a
/// producer may write the rectangle in either order and §12.5.2 says a consumer
/// shall normalise it. Non-finite values become the page's top-left corner
/// rather than poisoning the layout. And the result is clamped to the page,
/// because a rectangle at 1e10 is a marker the viewer would place off any
/// surface it could scroll to.
fn rect_of(annot: &Dictionary, document: &Document, frame: Frame) -> [f32; 4] {
    let Some(values) = annot
        .get(b"Rect")
        .ok()
        .and_then(|object| numbers(document, object, 4))
    else {
        return [0.0, 0.0, 0.0, 0.0];
    };
    if values.iter().any(|value| !value.is_finite()) {
        return [0.0, 0.0, 0.0, 0.0];
    }

    let left = values[0].min(values[2]);
    let right = values[0].max(values[2]);
    let bottom = values[1].min(values[3]);
    let top = values[1].max(values[3]);

    place(frame, left, bottom, right, top)
}

/// The page an annotation is placed against.
///
/// The four values travel together through every function here and mean nothing
/// apart --- a width without the turn that applies to it places a rectangle
/// confidently in the wrong place. Grouping them is also what keeps [`place`]
/// inside clippy's argument bound without an `allow`, which would have been the
/// lazier half of the same observation.
#[derive(Clone, Copy)]
struct Frame {
    width: f32,
    height: f32,
    turns: u8,
    /// The displayed page box's lower-left corner --- see `page_geometry`.
    origin: (f32, f32),
}

/// One rectangle of the page's own space, in display space and on the page.
///
/// Shared by [`rect_of`] and [`quads_of`] rather than written twice, which is
/// the whole reason it is a function: the turn, the crop shift and the clamp are
/// one rule about where an annotation sits, and two copies of it would be free
/// to disagree about a rotated page --- with the `/Rect` landing correctly and
/// the words it covers landing somewhere else, on exactly the documents nobody
/// opens twice.
fn place(frame: Frame, left: f32, bottom: f32, right: f32, top: f32) -> [f32; 4] {
    let Frame {
        width,
        height,
        turns,
        origin,
    } = frame;
    // The same mapping `text.rs` uses for character boxes, and through the same
    // function on purpose: a page carrying `/Rotate` is described in its own
    // unrotated space while everything downstream works in the displayed one,
    // and a second implementation of that turn is a second place to get it
    // wrong.
    // Shifted into crop space before the turn: `to_device` works in the
    // displayed page's coordinates, and the displayed page starts at the crop
    // box's corner rather than at the media box's.
    let (ox, oy) = (origin.0 as f64, origin.1 as f64);
    let placed = crate::text::to_device(
        turns,
        width,
        height,
        [
            left as f64 - ox,
            bottom as f64 - oy,
            right as f64 - ox,
            top as f64 - oy,
        ],
    );

    [
        placed[0].clamp(0.0, width),
        placed[1].clamp(0.0, height),
        placed[2].clamp(0.0, width),
        placed[3].clamp(0.0, height),
    ]
}

/// An annotation's `/QuadPoints`, normalised and mapped into display space.
///
/// Four values per quadrilateral --- left, top, right, bottom --- flattened, the
/// same shape and the same space `edits::MarkView::quads` uses, so the frontend
/// has one vocabulary for "the rectangles a mark covers" rather than two.
///
/// **The four corners are read as a set, not as an order, and that is the whole
/// difficulty.** §12.5.6.10 gives them as upper-left, upper-right, lower-left,
/// lower-right; Acrobat has written them in a different order for many years and
/// every reader in the wild takes the extremes instead. Our own `save.rs` writes
/// the specification's order exactly, so a reader built by reading our writer
/// would round-trip perfectly against every fixture here and mangle somebody
/// else's file --- the writer-and-its-own-reader trap, arriving as a geometry
/// bug rather than as a parse failure.
///
/// Empty for a kind that has no such key, for an array whose length is not a
/// multiple of eight, for a non-finite value anywhere in it, and past
/// [`MAX_QUADS`]. Empty is not an error: a text markup annotation is permitted
/// to omit `/QuadPoints`, in which case §12.5.6.10 says the `/Rect` is the
/// region, and [`Comment::rect`] already carries that.
fn quads_of(annot: &Dictionary, document: &Document, frame: Frame) -> Vec<f32> {
    let Ok(object) = annot.get(b"QuadPoints") else {
        return Vec::new();
    };
    let Object::Array(array) = resolve(document, object) else {
        return Vec::new();
    };
    if array.is_empty() || array.len() % 8 != 0 {
        return Vec::new();
    }

    let values: Vec<f32> = array
        .iter()
        .filter_map(|item| resolve(document, item).as_float().ok())
        .collect();
    if values.len() != array.len() || values.iter().any(|value| !value.is_finite()) {
        return Vec::new();
    }

    let mut out = Vec::new();
    for corners in values.chunks_exact(8).take(MAX_QUADS) {
        let xs = [corners[0], corners[2], corners[4], corners[6]];
        let ys = [corners[1], corners[3], corners[5], corners[7]];
        let left = xs.iter().copied().fold(f32::INFINITY, f32::min);
        let right = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let bottom = ys.iter().copied().fold(f32::INFINITY, f32::min);
        let top = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        out.extend_from_slice(&place(frame, left, bottom, right, top));
    }
    out
}

/// Fills in `reply_to`, dropping any link that would close a loop.
///
/// The `/IRT` graph is whatever the file says, and a document can make it a
/// cycle in two objects --- `comments.pdf` page 2 does. Breaking it here rather
/// than in the consumer is what lets the sidebar walk a thread with no visited
/// set: the invariant is carried by the data instead of by every reader of it.
///
/// A link to something that is not a comment in this list --- an annotation on
/// another page that was cut, the page object itself, an object that does not
/// exist --- is simply absent, which makes the reply a root. That is the right
/// answer rather than a fallback: a reply whose parent is not shown has to be
/// shown *somewhere*, and a thread nobody can see is a comment lost.
fn resolve_replies(
    items: &mut [Comment],
    ids: &HashMap<ObjectId, u32>,
    parents: &[Option<ObjectId>],
    limits: &mut Limits,
) {
    // Proposed links first, then each is accepted only if walking up from it
    // terminates. Two passes rather than one because a cycle cannot be seen
    // from either of its ends alone.
    //
    // A comment replying to *itself* is deliberately not special-cased here.
    // It was, and the shortcut dropped the link without counting a cycle: the
    // answer was right and the report was silent, which is the one outcome this
    // module is arranged to prevent. The walk catches it on its first step and
    // counts it like any other loop.
    let proposed: Vec<Option<u32>> = parents
        .iter()
        .map(|parent| ids.get(&(*parent)?).copied())
        .collect();

    for index in 0..items.len() {
        let Some(first) = proposed[index] else {
            continue;
        };
        let mut at = first;
        let mut steps = 0;
        let looped = loop {
            if at == index as u32 {
                break true;
            }
            let Some(next) = proposed.get(at as usize).copied().flatten() else {
                break false;
            };
            steps += 1;
            if steps > MAX_THREAD_DEPTH {
                // Deep rather than looping, and cut for the same reason: a
                // thread this long is not one a panel can show, and the walk
                // has to terminate whatever the file says.
                break true;
            }
            at = next;
        };

        if looped {
            limits.cycles += 1;
        } else {
            items[index].reply_to = Some(first);
        }
    }
}

/// Decodes a PDF text string.
///
/// UTF-16BE and UTF-8 announce themselves with a byte-order mark; anything else
/// is PDFDocEncoding (PDF 32000-1 §7.9.2.2). See the module note for why the
/// table comes from `lopdf` and what is handled here instead of by it.
pub fn decode_text_string(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        // Lossy for the reason `outline::decode_title` is: `String::from_utf16`
        // fails the *whole* string on one bad code unit, so a single malformed
        // pair would blank an otherwise readable comment.
        return char::decode_utf16(units)
            .map(|result| result.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }

    // PDFDocEncoding, in runs, because the table maps nothing below 0x18 and a
    // comment's newlines live there --- see the module note.
    let mut out = String::with_capacity(bytes.len());
    let mut run: Vec<u8> = Vec::new();
    for &byte in bytes {
        if byte >= 0x18 {
            run.push(byte);
            continue;
        }
        if !run.is_empty() {
            out.push_str(&pdf_doc_encoded(&run));
            run.clear();
        }
        // Tab and the two newline characters are content in a comment body.
        // Everything else below the table is not.
        if byte == b'\t' || byte == b'\n' || byte == b'\r' {
            out.push(byte as char);
        }
    }
    if !run.is_empty() {
        out.push_str(&pdf_doc_encoded(&run));
    }
    out
}

/// Decodes one PDFDocEncoded run through `lopdf`'s own table.
///
/// The table is `lopdf`'s rather than a transcription for the reason the module
/// note gives, and it is reached through `lopdf::decode_text_string` because the
/// table itself is not exported.
///
/// **The sentinel is load-bearing.** That function sniffs a byte-order mark at
/// the start of whatever it is handed, and a run here begins wherever the
/// previous control character ended --- so a body reading `"a\nþÿ..."` would
/// hand it a run starting `FE FF` and get the rest read as UTF-16. An ASCII `A`
/// in front makes that impossible, and every encoding involved agrees about what
/// an `A` is, so dropping one character afterwards drops exactly it.
fn pdf_doc_encoded(run: &[u8]) -> String {
    let mut framed = Vec::with_capacity(run.len() + 1);
    framed.push(b'A');
    framed.extend_from_slice(run);
    let object = Object::String(framed, lopdf::StringFormat::Literal);
    match lopdf::decode_text_string(&object) {
        // Skipping one *character*, not one byte: the sentinel is one of each,
        // and the rest of the string is not.
        Ok(text) => text.chars().skip(1).collect(),
        // A run this cannot decode is dropped rather than guessed at. The
        // caller's `bodies_clipped` does not fire here, which is deliberate ---
        // nothing was cut for length, and the alternative is inventing
        // characters.
        Err(_) => String::new(),
    }
}

/// Flattens a body into something a panel can show, keeping its paragraphs.
///
/// Returns the body and whether it was shortened. Deliberately *not*
/// [`crate::outline::sanitize_title`], which collapses every run of whitespace
/// including newlines: a title is one line by definition and a comment is not,
/// and running a two-paragraph note through it returns one paragraph. Runs of
/// spaces and tabs still collapse, and a run of blank lines collapses to one, so
/// a body carrying forty newlines costs one gap rather than forty.
pub fn sanitize_body(body: &str, limit: usize) -> (String, bool) {
    let mut out = String::new();
    let mut pending_space = false;
    let mut pending_breaks = 0usize;
    let mut kept = 0usize;
    let mut clipped = false;

    let mut chars = body.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            // A CRLF is one break, not two.
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            pending_breaks = (pending_breaks + 1).min(2);
            pending_space = false;
            continue;
        }
        if ch == '\n' {
            pending_breaks = (pending_breaks + 1).min(2);
            pending_space = false;
            continue;
        }
        if ch.is_whitespace() || ch.is_control() {
            // A control character that is not a break becomes a space, so
            // "Chapter\u{7}One" stays two words --- the rule `outline.rs`
            // states for titles.
            if pending_breaks == 0 {
                pending_space = !out.is_empty();
            }
            continue;
        }
        if kept >= limit {
            clipped = true;
            break;
        }
        if pending_breaks > 0 {
            // Held rather than pushed, so neither a leading nor a trailing
            // blank line survives --- the `is_empty` is what makes it both,
            // and a body opening with two newlines is what a producer emits
            // when somebody starts typing on the second line.
            if !out.is_empty() {
                for _ in 0..pending_breaks {
                    out.push('\n');
                }
            }
            pending_breaks = 0;
            pending_space = false;
        } else if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
        kept += 1;
    }

    (out, clipped)
}

/// Parses a PDF date and re-emits it as `YYYY-MM-DD HH:MM`.
///
/// **The output is built here from parsed numbers, and that is the point.** A
/// date is a string in the file like everything else, and passing it through
/// would put an attacker-chosen value in a field the frontend renders. Nothing
/// but digits from the document survives this, and only in the ranges a
/// calendar has.
///
/// The format is `D:YYYYMMDDHHmmSSOHH'mm'` with everything after the year
/// optional (PDF 32000-1 §7.9.4). The offset is parsed only far enough to be
/// rejected: showing a local time in the document's own zone is what a reader
/// expects from a comment, and converting it to theirs would relabel a note
/// somebody wrote at nine in the morning.
fn parse_date(raw: &[u8]) -> Option<String> {
    let text = raw.strip_prefix(b"D:").unwrap_or(raw);
    let digits: Vec<u8> = text
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .copied()
        .collect();
    if digits.len() < 4 {
        return None;
    }

    let field = |from: usize, len: usize, fallback: u32| -> Option<u32> {
        if digits.len() < from + len {
            return Some(fallback);
        }
        std::str::from_utf8(&digits[from..from + len])
            .ok()?
            .parse::<u32>()
            .ok()
    };

    let year = field(0, 4, 0)?;
    let month = field(4, 2, 1)?;
    let day = field(6, 2, 1)?;
    let hour = field(8, 2, 0)?;
    let minute = field(10, 2, 0)?;

    // A calendar's own bounds, so a date of month 47 is reported as no date
    // rather than shown. Days are not checked against the month's length: the
    // difference between 31 February and no date at all is not worth a table,
    // and one of the two is still recognisably a date.
    if !(1000..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 {
        return None;
    }

    Some(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    /// Builds a one-page document whose page carries the annotations given.
    fn document_with(annots: Vec<Dictionary>, page_extra: Dictionary) -> Vec<u8> {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let refs: Vec<Object> = annots
            .into_iter()
            .map(|annot| Object::Reference(document.add_object(annot)))
            .collect();

        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Annots" => Object::Array(refs),
        };
        for (key, value) in page_extra.iter() {
            page.set(key.clone(), value.clone());
        }
        let page_id = document.add_object(page);

        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        };
        document.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog);

        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("the fixture must save");
        bytes
    }

    /// One `/Annots` entry, in the two shapes PDF allows.
    ///
    /// `document_with` above writes every annotation as a reference, which is
    /// what real producers do and what every other test here needs. It therefore
    /// cannot express the case [`Comment::object`] is `None` for, and a fixture
    /// that cannot produce both shapes cannot tell them apart.
    enum Entry {
        Referenced(Dictionary),
        Inline(Dictionary),
    }

    /// A one-page document whose `/Annots` holds these entries in this order,
    /// with every referenced object created in the **reverse** of it.
    ///
    /// The reversal is the discriminating property and not decoration. A scan
    /// assigns `Comment::id` by array position, so in an ordinary document the
    /// ids and the object numbers ascend together --- and a field that merely
    /// echoed the position, or that was off by a constant, would agree with the
    /// object numbers on every fixture in this module. Here they run in opposite
    /// directions, so only a value taken from the file itself can pass.
    fn scan_entries(entries: Vec<Entry>) -> Comments {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();

        // Reverse, so that array position and object number disagree.
        let mut assigned: Vec<Option<lopdf::ObjectId>> = vec![None; entries.len()];
        let mut dicts: Vec<Option<Dictionary>> = Vec::with_capacity(entries.len());
        for entry in &entries {
            dicts.push(match entry {
                Entry::Referenced(_) => None,
                Entry::Inline(d) => Some(d.clone()),
            });
        }
        for (i, entry) in entries.into_iter().enumerate().rev() {
            if let Entry::Referenced(annot) = entry {
                assigned[i] = Some(document.add_object(annot));
            }
        }

        let array: Vec<Object> = assigned
            .iter()
            .zip(dicts)
            .map(|(id, inline)| match (id, inline) {
                (Some(id), _) => Object::Reference(*id),
                (None, Some(d)) => Object::Dictionary(d),
                (None, None) => unreachable!("every entry is one shape or the other"),
            })
            .collect();

        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Annots" => Object::Array(array),
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
        document.save_to(&mut bytes).expect("the fixture must save");
        scan(&bytes, 1, None).expect("the fixture must parse")
    }

    /// A comment is named by the object it is, and an inline one has no name.
    ///
    /// `Comment::id` is a position in one scan, so it cannot survive a save ---
    /// which is the whole reason this field exists. The three entries are
    /// deliberately mixed: two referenced and one written straight into the
    /// `/Annots` array, because a fixture carrying only the first shape would
    /// pass with `object` hardcoded to `Some(...)` and one carrying only the
    /// second would pass with it hardcoded to `None`.
    #[test]
    fn a_comment_is_named_by_its_object_and_an_inline_one_is_not() {
        let comments = scan_entries(vec![
            Entry::Referenced(note("first")),
            Entry::Inline(note("second")),
            Entry::Referenced(note("third")),
        ]);
        assert_eq!(comments.items.len(), 3, "all three are on the page");

        let objects: Vec<Option<(u32, u16)>> = comments.items.iter().map(|c| c.object).collect();
        assert!(
            objects[0].is_some(),
            "a referenced annotation has an object"
        );
        assert_eq!(
            objects[1], None,
            "an annotation written into the array is not an object, so nothing \
             can override it in an incremental update"
        );
        assert!(objects[2].is_some(), "and so does the third");
        assert_ne!(
            objects[0], objects[2],
            "two annotations are two objects --- one value for both would satisfy \
             every other assertion here"
        );
    }

    /// An id ascends with position; an object number does not have to.
    ///
    /// The fixture writes its `/Annots` array in the reverse of the order the
    /// objects were created, so the two orderings run opposite ways. A field
    /// that echoed the scan position --- or the position plus a constant ---
    /// agrees with the object numbers on every ordinary document and fails only
    /// here.
    #[test]
    fn an_id_is_a_position_and_an_object_is_a_name() {
        let comments = scan_entries(vec![
            Entry::Referenced(note("first")),
            Entry::Referenced(note("second")),
            Entry::Referenced(note("third")),
        ]);
        let ids: Vec<u32> = comments.items.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![0, 1, 2], "ids are the array order");

        let numbers: Vec<u32> = comments
            .items
            .iter()
            .map(|c| c.object.expect("all three are referenced").0)
            .collect();
        assert!(
            numbers[0] > numbers[1] && numbers[1] > numbers[2],
            "the objects were created in reverse, so their numbers descend \
             while the ids ascend: {numbers:?}"
        );
    }

    /// Scans a synthetic document holding exactly these annotations.
    fn scan_annots(annots: Vec<Dictionary>) -> Comments {
        let bytes = document_with(annots, Dictionary::new());
        scan(&bytes, 1, None).expect("the fixture must parse")
    }

    /// A page PDFium has and `lopdf` cannot read is reported, not answered "none".
    ///
    /// The same distinction `crate::links` draws and `encoding.rs` drew first:
    /// an empty list means "this document has no comments", and a scan that
    /// could not see the pages must not say that. No fixture on disk produces
    /// it --- swept on 2026-08-16 --- so the shape is synthetic, which is what
    /// `encoding.rs` does for the same reason.
    #[test]
    fn a_page_lopdf_cannot_account_for_is_reported() {
        let bytes = document_with(vec![note("visible")], Dictionary::new());
        let comments = scan(&bytes, 4, None).expect("the fixture must parse");
        assert_eq!(comments.limits.pages_missed, 3, "4 claimed, 1 readable");
        assert!(comments.limits.any());
        // The comment it could read is still returned: a notice, not a refusal.
        assert_eq!(comments.items.len(), 1);
    }

    /// The control: agreement charges nothing, or every document carries a
    /// warning and a reader learns to ignore the one that matters.
    #[test]
    fn a_document_both_parsers_agree_about_reports_nothing_missing() {
        let comments = scan_annots(vec![note("visible")]);
        assert_eq!(comments.limits.pages_missed, 0);
        assert!(!comments.limits.any());
    }

    /// A page displayed from an inset `/CropBox` places a comment in *its*
    /// space, not the media box's.
    ///
    /// The mirror of `links::a_cropped_page_places_a_rectangle_in_the_crop_box_s_space`,
    /// and it has to exist separately: the two modules compute their geometry
    /// independently, so a mutation that blinds one is invisible to the other's
    /// tests. The control is the same page uncropped --- a rule that ignores the
    /// origin is right there and wrong here, and only the pair discriminates.
    #[test]
    fn a_cropped_page_places_a_comment_in_the_crop_box_s_space() {
        let mut note = note("in the margin");
        note.set("Rect", vec![100.into(), 690.into(), 300.into(), 720.into()]);

        let plain = document_with(vec![note.clone()], Dictionary::new());
        // The default page here is 595x842, so 842 - 720 = 122.
        assert_eq!(
            scan(&plain, 1, None).expect("parse").items[0].rect,
            [100.0, 122.0, 300.0, 152.0]
        );

        let cropped = document_with(
            vec![note],
            dictionary! {
                "CropBox" => vec![50.into(), 50.into(), 545.into(), 742.into()],
            },
        );
        // Inset by (50, 50) and 692 tall: 50 left of where it was, 742 - 720 = 22
        // from the top.
        assert_eq!(
            scan(&cropped, 1, None).expect("parse").items[0].rect,
            [50.0, 22.0, 250.0, 52.0]
        );
    }

    /// A `/CropBox` larger than the sheet is intersected with it, per §14.11.2.
    ///
    /// The mirror of `links::a_crop_box_larger_than_the_page_is_intersected_with_it`,
    /// and it was missing for a day while its twin had it. That asymmetry is the
    /// whole reason both exist: the two modules compute their geometry from
    /// separate code, so `links.rs` having three crop-box tests said nothing
    /// about `annots.rs`, which had one --- and the clamp here was reachable by
    /// **no test at all**, in the module where a comment's rectangle is placed.
    /// Established by reading the test list rather than by watching a mutation
    /// survive, which is the weaker of the two and worth saying so.
    ///
    /// The control is the assertion itself: an unclamped crop box would scale
    /// this rectangle against a 1000x1300 page the renderer never uses, so the
    /// answer being *identical* to the uncropped one is what proves the
    /// intersection happened.
    #[test]
    fn a_crop_box_larger_than_the_page_is_intersected_with_it() {
        let mut note = note("past the paper");
        note.set("Rect", vec![100.into(), 690.into(), 300.into(), 720.into()]);

        let document = document_with(
            vec![note],
            dictionary! {
                "CropBox" => vec![(-100).into(), (-100).into(), 900.into(), 1200.into()],
            },
        );
        assert_eq!(
            scan(&document, 1, None).expect("parse").items[0].rect,
            [100.0, 122.0, 300.0, 152.0]
        );
    }

    /// A sticky note with a body and an author.
    fn note(body: &str) -> Dictionary {
        dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![100.into(), 700.into(), 124.into(), 724.into()],
            "Contents" => Object::string_literal(body),
            "T" => Object::string_literal("Timo"),
        }
    }

    #[test]
    fn a_note_is_read_with_its_author_and_body() {
        let comments = scan_annots(vec![note("Check this figure.")]);
        assert_eq!(comments.items.len(), 1);
        let first = &comments.items[0];
        assert_eq!(first.kind, Kind::Text);
        assert_eq!(first.author, "Timo");
        assert_eq!(first.body, "Check this figure.");
        assert_eq!(first.page, 0);
        assert!(
            !comments.limits.any(),
            "nothing was cut, so nothing may be reported"
        );
    }

    #[test]
    fn a_link_and_a_widget_are_not_comments_and_are_not_counted_as_unknown() {
        // Both carry text that looks like a comment to a scan keyed on
        // `/Contents`, and neither is one. Counting them as unknown would put a
        // permanent "some were dropped" notice on every document with a
        // hyperlink.
        let comments = scan_annots(vec![
            dictionary! {
                "Type" => "Annot",
                "Subtype" => "Link",
                "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                "Contents" => Object::string_literal("Follow me"),
            },
            dictionary! {
                "Type" => "Annot",
                "Subtype" => "Widget",
                "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                "V" => Object::string_literal("typed into a form"),
            },
            note("A real comment."),
        ]);
        assert_eq!(comments.items.len(), 1);
        assert_eq!(comments.limits.unknown_kinds, 0);
        assert_eq!(comments.items[0].body, "A real comment.");
    }

    #[test]
    fn an_unrecognised_subtype_is_left_out_and_counted() {
        // The control for the test above: a subtype nobody knows *is* a cut,
        // and a cut that is not counted is one nobody can be told about.
        let comments = scan_annots(vec![dictionary! {
            "Type" => "Annot",
            "Subtype" => "SomethingNew",
            "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "Contents" => Object::string_literal("From the future."),
        }]);
        assert!(comments.items.is_empty());
        assert_eq!(comments.limits.unknown_kinds, 1);
        assert!(comments.limits.any());
    }

    #[test]
    fn a_mark_with_no_body_is_still_a_comment() {
        let comments = scan_annots(vec![dictionary! {
            "Type" => "Annot",
            "Subtype" => "Highlight",
            "Rect" => vec![10.into(), 10.into(), 20.into(), 20.into()],
            "T" => Object::string_literal("Timo"),
        }]);
        assert_eq!(comments.items.len(), 1);
        assert_eq!(comments.items[0].body, "");
        assert_eq!(comments.items[0].author, "Timo");
        assert_eq!(comments.items[0].kind, Kind::Highlight);
    }

    /// A document whose annotations may refer to each other by object id.
    ///
    /// Separate from [`document_with`] because `/IRT` names an object, and an
    /// annotation cannot name one that does not exist yet: `build` is handed the
    /// document and returns the annotations in `/Annots` order, having reserved
    /// whatever ids it needed on the way.
    fn document_with_links(build: impl FnOnce(&mut Document) -> Vec<ObjectId>) -> Vec<u8> {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let annots = build(&mut document);
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Annots" => annots.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
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
        document.save_to(&mut bytes).expect("the fixture must save");
        bytes
    }

    /// A sticky note replying to `parent`.
    fn reply(body: &str, parent: ObjectId) -> Dictionary {
        dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![10.into(), 10.into(), 20.into(), 20.into()],
            "Contents" => Object::string_literal(body),
            "IRT" => parent,
        }
    }

    #[test]
    fn a_reply_names_the_comment_it_answers() {
        let bytes = document_with_links(|document| {
            let first = document.add_object(note("The original."));
            let second = document.add_object(reply("The reply.", first));
            vec![first, second]
        });

        let comments = scan(&bytes, 1, None).expect("parse");
        assert_eq!(comments.items.len(), 2);
        assert_eq!(comments.items[0].reply_to, None);
        assert_eq!(comments.items[1].reply_to, Some(0));
        assert_eq!(comments.limits.cycles, 0);
    }

    #[test]
    fn a_reply_cycle_is_broken_and_counted() {
        // Two notes replying to each other, which `comments.pdf` page 2 also
        // carries. The frontend walks `reply_to` with no visited set of its
        // own --- `commentlist.ts` says so --- so this is where that promise is
        // kept, and a cycle that survived would hang the panel rather than
        // showing a wrong row.
        let bytes = document_with_links(|document| {
            let first = document.new_object_id();
            let second = document.new_object_id();
            document
                .objects
                .insert(first, Object::Dictionary(reply("A answers B.", second)));
            document
                .objects
                .insert(second, Object::Dictionary(reply("B answers A.", first)));
            vec![first, second]
        });

        let comments = scan(&bytes, 1, None).expect("parse");
        assert_eq!(comments.items.len(), 2, "both notes are still listed");
        assert!(
            comments.items.iter().all(|item| item.reply_to.is_none()),
            "a link that closes a loop must be cut: {:?}",
            comments
                .items
                .iter()
                .map(|item| item.reply_to)
                .collect::<Vec<_>>()
        );
        assert!(comments.limits.cycles > 0, "and the cut must be reported");
    }

    #[test]
    fn a_comment_replying_to_itself_is_a_root() {
        // The one-element cycle, which the chain walk would also catch --- it is
        // named separately because a producer writing `/IRT` to the annotation
        // itself is a real bug in the wild rather than a hostile construction.
        let bytes = document_with_links(|document| {
            let only = document.new_object_id();
            document
                .objects
                .insert(only, Object::Dictionary(reply("I answer myself.", only)));
            vec![only]
        });

        let comments = scan(&bytes, 1, None).expect("parse");
        assert_eq!(comments.items.len(), 1);
        assert_eq!(comments.items[0].reply_to, None);
        assert!(comments.limits.cycles > 0);
    }

    #[test]
    fn a_reply_to_something_that_is_not_a_comment_is_a_root() {
        // A `/IRT` pointing at the page object. Not a cycle, not an error, and
        // not a reason to hide the comment: it becomes a root, which is the
        // rule `resolve_replies` states.
        let bytes = document_with_links(|document| {
            let stray = document.add_object(dictionary! { "Type" => "Whatever" });
            let one = document.add_object(reply("I answer a page.", stray));
            vec![one]
        });

        let comments = scan(&bytes, 1, None).expect("parse");
        assert_eq!(comments.items.len(), 1);
        assert_eq!(comments.items[0].reply_to, None);
        assert_eq!(comments.limits.cycles, 0, "a link to nothing is not a loop");
    }

    #[test]
    fn a_date_is_rebuilt_from_its_digits() {
        assert_eq!(
            parse_date(b"D:20260812101500Z").as_deref(),
            Some("2026-08-12 10:15")
        );
        assert_eq!(
            parse_date(b"D:20260812101500+02'00'").as_deref(),
            Some("2026-08-12 10:15")
        );
        // The offset is not applied: a note written at ten in the morning says
        // ten, in the zone the writer was in.
        assert_eq!(
            parse_date(b"D:20260812101500-08'00'").as_deref(),
            Some("2026-08-12 10:15")
        );
    }

    #[test]
    fn a_partial_date_keeps_what_it_states() {
        assert_eq!(parse_date(b"D:2026").as_deref(), Some("2026-01-01 00:00"));
        assert_eq!(parse_date(b"D:202608").as_deref(), Some("2026-08-01 00:00"));
        assert_eq!(
            parse_date(b"D:2026081210").as_deref(),
            Some("2026-08-12 10:00")
        );
    }

    #[test]
    fn a_string_that_is_not_a_date_produces_no_date() {
        // The control for every assertion above: a parser that returned
        // something for anything would satisfy them all.
        for raw in [
            &b"yesterday"[..],
            b"D:",
            b"",
            b"D:20261312101500", // month 13
            b"D:20260832101500", // day 32
            b"D:20260812991500", // hour 99
            b"D:20260812109900", // minute 99
            b"D:0000",           // year 0
            b"<script>alert(1)", // and nothing document-chosen gets through
        ] {
            assert_eq!(parse_date(raw), None, "parsed {:?} as a date", raw);
        }
    }

    #[test]
    fn a_utf16_string_decodes_including_astral_characters() {
        let mut bytes = vec![0xFE, 0xFF];
        bytes.extend(
            "Ávila 第三章 \u{1d11e}"
                .encode_utf16()
                .flat_map(u16::to_be_bytes),
        );
        assert_eq!(decode_text_string(&bytes), "Ávila 第三章 \u{1d11e}");
    }

    #[test]
    fn an_unpaired_surrogate_becomes_one_replacement_character() {
        // Big-endian, unlike `outline::decode_title`, and the whole string has
        // to survive one bad unit.
        let bytes = [0xFE, 0xFF, 0xD8, 0x00, 0x00, 0x41];
        assert_eq!(decode_text_string(&bytes), "\u{fffd}A");
    }

    #[test]
    fn a_utf8_string_decodes_when_it_says_so() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("Zoë — PDF 2.0".as_bytes());
        assert_eq!(decode_text_string(&bytes), "Zoë — PDF 2.0");
    }

    #[test]
    fn pdfdocencoding_is_not_latin1() {
        // 0x90 is a right single quotation mark here and a C1 control in
        // Latin-1, which is the byte that tells the two apart. 0xE1 is á in
        // both, and is the control that says the rest of the string is not
        // simply being dropped.
        assert_eq!(decode_text_string(b"It\x90s fine"), "It\u{2019}s fine");
        assert_eq!(decode_text_string(b"Se\xE1n"), "Seán");
    }

    #[test]
    fn a_documents_body_keeps_its_paragraphs() {
        // Through the scan rather than through `sanitize_body` directly, which
        // is the difference between testing the rule and testing that the rule
        // is *used*. A mutation routing bodies through `sanitize_title` --- the
        // one-line flattener --- passed every other test in this module.
        let comments = scan_annots(vec![dictionary! {
            "Type" => "Annot",
            "Subtype" => "FreeText",
            "Rect" => vec![10.into(), 10.into(), 20.into(), 20.into()],
            "Contents" => Object::string_literal("First paragraph.\n\nSecond one."),
        }]);
        assert_eq!(comments.items[0].body, "First paragraph.\n\nSecond one.");
    }

    #[test]
    fn an_author_is_flattened_to_one_line() {
        // The other half of the routing, and the control for the test above: a
        // byline is one line, so an author carrying a newline must *not* keep
        // it. Without this, routing everything through `sanitize_body` would
        // look correct.
        let comments = scan_annots(vec![dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![10.into(), 10.into(), 20.into(), 20.into()],
            "Contents" => Object::string_literal("Body."),
            "T" => Object::string_literal("Timo\nStein"),
        }]);
        assert_eq!(comments.items[0].author, "Timo Stein");
    }

    #[test]
    fn a_body_keeps_its_newlines_where_a_title_would_not() {
        // The reason this module does not reuse `sanitize_title`: a comment is
        // not one line, and collapsing its breaks turns two paragraphs into one.
        let (body, clipped) = sanitize_body("First paragraph.\n\nSecond one.", 4_000);
        assert_eq!(body, "First paragraph.\n\nSecond one.");
        assert!(!clipped);
        assert_eq!(
            crate::outline::sanitize_title("First paragraph.\n\nSecond one.", 4_000).0,
            "First paragraph. Second one."
        );
    }

    #[test]
    fn a_body_collapses_runs_and_drops_other_controls() {
        assert_eq!(sanitize_body("a  \t  b", 4_000).0, "a b");
        assert_eq!(sanitize_body("Chapter\u{7}One", 4_000).0, "Chapter One");
        // More than one blank line is one blank line.
        assert_eq!(sanitize_body("a\n\n\n\n\nb", 4_000).0, "a\n\nb");
        // And the edges are trimmed, in both directions and both kinds.
        assert_eq!(sanitize_body("\n\n  a  \n\n", 4_000).0, "a");
    }

    #[test]
    fn a_long_body_is_clipped_and_says_so() {
        let (body, clipped) = sanitize_body(&"H".repeat(60_000), MAX_BODY_CHARS);
        assert_eq!(body.chars().count(), MAX_BODY_CHARS);
        assert!(clipped);
        // The control: a short body is not reported as clipped, or the flag
        // carries no information at all.
        assert!(!sanitize_body("short", MAX_BODY_CHARS).1);
    }

    #[test]
    fn the_body_limit_counts_characters_not_bytes() {
        let (body, _) = sanitize_body(&"第".repeat(5_000), 300);
        assert_eq!(body.chars().count(), 300);
    }

    #[test]
    fn a_rectangle_written_backwards_is_normalised() {
        let comments = scan_annots(vec![dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![500.into(), 700.into(), 300.into(), 600.into()],
            "Contents" => Object::string_literal("Backwards."),
        }]);
        let rect = comments.items[0].rect;
        assert!(
            rect[0] < rect[2],
            "left {} is not left of right {}",
            rect[0],
            rect[2]
        );
        assert!(
            rect[1] < rect[3],
            "top {} is not above bottom {}",
            rect[1],
            rect[3]
        );
        assert_eq!(rect[0], 300.0);
    }

    #[test]
    fn a_rectangle_off_the_page_is_clamped_to_it() {
        let comments = scan_annots(vec![dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![
                Object::Real(-1e10),
                Object::Real(-1e10),
                Object::Real(1e10),
                Object::Real(1e10),
            ],
            "Contents" => Object::string_literal("The whole plane."),
        }]);
        let rect = comments.items[0].rect;
        assert_eq!(rect, [0.0, 0.0, 595.0, 842.0]);
    }

    #[test]
    fn a_rotated_page_places_a_rectangle_in_display_space() {
        // The bottom-left corner of an unrotated A4 page. Under `/Rotate 90`
        // the displayed page is 842 wide and 595 tall and that corner is at the
        // *top* left, so a scan that ignores the rotation puts it at the bottom.
        let bytes = document_with(
            vec![dictionary! {
                "Type" => "Annot",
                "Subtype" => "Text",
                "Rect" => vec![20.into(), 20.into(), 44.into(), 44.into()],
                "Contents" => Object::string_literal("Corner."),
            }],
            dictionary! { "Rotate" => 90 },
        );
        let comments = scan(&bytes, 1, None).expect("parse");
        let rect = comments.items[0].rect;
        assert!(
            rect[1] < 100.0,
            "a rotated page put the corner at y={} rather than near the top",
            rect[1]
        );
        // And the unrotated control, which is what makes the assertion above a
        // statement about the rotation rather than about the fixture.
        let flat = scan_annots(vec![dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![20.into(), 20.into(), 44.into(), 44.into()],
            "Contents" => Object::string_literal("Corner."),
        }]);
        assert!(
            flat.items[0].rect[1] > 700.0,
            "an unrotated page put the corner at y={} rather than near the bottom",
            flat.items[0].rect[1]
        );
    }

    /// One highlight with the `/QuadPoints` array given, and its quads.
    fn quads_from(points: Vec<Object>) -> Vec<f32> {
        let comments = scan_annots(vec![dictionary! {
            "Type" => "Annot",
            "Subtype" => "Highlight",
            "Rect" => vec![100.into(), 700.into(), 300.into(), 720.into()],
            "QuadPoints" => points,
            "Contents" => Object::string_literal(""),
        }]);
        assert_eq!(comments.items.len(), 1, "limits {:?}", comments.limits);
        comments.items[0].quads.clone()
    }

    /// The eight numbers of one quad, in the specification's corner order.
    fn spec_order(left: i64, bottom: i64, right: i64, top: i64) -> Vec<Object> {
        vec![
            left.into(),
            top.into(),
            right.into(),
            top.into(),
            left.into(),
            bottom.into(),
            right.into(),
            bottom.into(),
        ]
    }

    #[test]
    fn the_corners_of_a_quad_are_read_as_a_set_and_not_as_an_order() {
        // The load-bearing one. §12.5.6.10 gives the corners as upper-left,
        // upper-right, lower-left, lower-right; Acrobat has written them in a
        // different order for years, and `save.rs` writes the specification's
        // order exactly -- so a reader that trusted the order would agree with
        // our own writer on every fixture in this repository and mangle
        // everybody else's file. The two arrays below name the same four points
        // and must produce the same rectangle.
        let ours = quads_from(spec_order(100, 700, 300, 720));
        let theirs = quads_from(vec![
            // Lower-left, lower-right, upper-left, upper-right: one of the
            // orders found in the wild, and a straight read of it as
            // `[x1 y1 x2 y2]` yields a rectangle of zero height.
            100.into(),
            700.into(),
            300.into(),
            700.into(),
            100.into(),
            720.into(),
            300.into(),
            720.into(),
        ]);
        assert_eq!(ours.len(), 4, "one quad is four values");
        assert_eq!(ours, theirs, "the corner order changed the rectangle");
        // And that the rectangle is the right one rather than merely a stable
        // one: a reader taking `values[0..4]` verbatim passes the equality
        // above on a symmetric fixture and fails this.
        assert!(
            ours[2] - ours[0] > 190.0 && ours[3] - ours[1] > 19.0,
            "a 200x20 quad came back as {ours:?}"
        );
    }

    #[test]
    fn a_quad_array_that_is_not_whole_quads_is_declined() {
        // Not truncated to the whole quads it does contain: a producer that
        // wrote seven numbers has told us nothing reliable about the four
        // corners of anything, and half a rectangle placed on the page is worse
        // than no rectangle at all.
        assert!(quads_from(vec![1.into(); 7]).is_empty());
        assert!(quads_from(vec![1.into(); 9]).is_empty());
        assert!(quads_from(Vec::new()).is_empty());
        // And eight is accepted, or the three above are satisfied by a reader
        // that declines everything.
        assert_eq!(quads_from(vec![1.into(); 8]).len(), 4);
    }

    /// `quads_of` called directly, with no file written or re-parsed.
    ///
    /// The malformed cases below have to be built in memory rather than round
    /// tripped through `document_with`, and finding out why is worth the
    /// paragraph: `lopdf` **cannot re-read its own output** for two of them.
    /// `Object::string_literal` inside a number array writes `300(not a
    /// number)`, which is legal --- a `(` is a delimiter and ends the number
    /// token --- and its parser wants whitespace, so the whole annotation comes
    /// back `unreadable` and the assertion passes for a reason that has nothing
    /// to do with the code under test. `Object::Real(f32::INFINITY)` writes
    /// `inf`, which is not a PDF number at all, and does the same thing. Both
    /// would have been green.
    fn quads_direct(points: Vec<Object>) -> Vec<f32> {
        let annot = dictionary! {
            "Type" => "Annot",
            "Subtype" => "Highlight",
            "QuadPoints" => points,
        };
        quads_of(
            &annot,
            &Document::new(),
            Frame {
                width: 595.0,
                height: 842.0,
                turns: 0,
                origin: (0.0, 0.0),
            },
        )
    }

    #[test]
    fn a_quad_with_a_value_that_is_not_a_number_is_declined() {
        // The whole array, not the seven values that did parse. A producer that
        // wrote a non-number among the corners has told us nothing reliable
        // about any of them, and a rectangle built from the rest lands
        // somewhere plausible and wrong.
        let mut named = spec_order(100, 700, 300, 720);
        named[3] = Object::Name(b"Top".to_vec());
        assert!(quads_direct(named).is_empty());

        let mut null = spec_order(100, 700, 300, 720);
        null[3] = Object::Null;
        assert!(quads_direct(null).is_empty());

        let mut infinite = spec_order(100, 700, 300, 720);
        infinite[3] = Object::Real(f32::INFINITY);
        assert!(quads_direct(infinite).is_empty());

        // The control, through the same door: without it every assertion above
        // is satisfied by a `quads_direct` that returns nothing for anything.
        assert_eq!(quads_direct(spec_order(100, 700, 300, 720)).len(), 4);
    }

    #[test]
    fn quads_are_kept_only_for_the_kinds_that_mark_words() {
        // A `/Text` note with a stray `/QuadPoints` -- legal to write, and
        // meaningless, because a note is a point on the page rather than a span
        // of words. The keying is on the kind and not on the key's presence,
        // which is also what keeps a `/Redact`'s quads out: those name the words
        // somebody marked for deletion.
        for subtype in ["Text", "Square", "Ink", "Redact"] {
            let comments = scan_annots(vec![dictionary! {
                "Type" => "Annot",
                "Subtype" => subtype,
                "Rect" => vec![100.into(), 700.into(), 300.into(), 720.into()],
                "QuadPoints" => spec_order(100, 700, 300, 720),
                "Contents" => Object::string_literal(""),
            }]);
            assert!(
                comments.items[0].quads.is_empty(),
                "{subtype} kept quads it does not mark words with"
            );
        }
        for subtype in ["Highlight", "Underline", "Squiggly", "StrikeOut"] {
            let comments = scan_annots(vec![dictionary! {
                "Type" => "Annot",
                "Subtype" => subtype,
                "Rect" => vec![100.into(), 700.into(), 300.into(), 720.into()],
                "QuadPoints" => spec_order(100, 700, 300, 720),
                "Contents" => Object::string_literal(""),
            }]);
            assert_eq!(
                comments.items[0].quads.len(),
                4,
                "{subtype} marks words and lost its quads"
            );
        }
    }

    #[test]
    fn a_quad_array_past_the_bound_is_cut_to_it() {
        let mut points = Vec::new();
        for _ in 0..MAX_QUADS + 10 {
            points.extend(spec_order(100, 700, 300, 720));
        }
        assert_eq!(quads_from(points).len(), MAX_QUADS * 4);
    }

    #[test]
    fn a_rotated_page_places_quads_exactly_where_it_places_the_rect() {
        // The reason `place` is one function. A quad and a `/Rect` naming the
        // same region must land on the same pixels, or a panel would highlight
        // one thing and the words under it would be another -- and only a
        // rotated page can tell the two apart, since with `/Rotate 0` a second
        // implementation that forgot the turn agrees with this one.
        let bytes = document_with(
            vec![dictionary! {
                "Type" => "Annot",
                "Subtype" => "Highlight",
                "Rect" => vec![100.into(), 700.into(), 300.into(), 720.into()],
                "QuadPoints" => spec_order(100, 700, 300, 720),
                "Contents" => Object::string_literal(""),
            }],
            dictionary! { "Rotate" => 90 },
        );
        let comments = scan(&bytes, 1, None).expect("parse");
        let item = &comments.items[0];
        assert_eq!(
            item.quads.as_slice(),
            item.rect.as_slice(),
            "the quad and the rect name one region and were placed differently"
        );
        // The control: without it the equality above is satisfied by two
        // implementations that both ignore the rotation.
        let flat = scan_annots(vec![dictionary! {
            "Type" => "Annot",
            "Subtype" => "Highlight",
            "Rect" => vec![100.into(), 700.into(), 300.into(), 720.into()],
            "QuadPoints" => spec_order(100, 700, 300, 720),
            "Contents" => Object::string_literal(""),
        }]);
        assert_ne!(
            flat.items[0].quads, item.quads,
            "the rotation moved nothing, so this fixture cannot tell a turn from an identity"
        );
    }

    #[test]
    fn an_entry_that_cannot_be_read_is_counted_rather_than_skipped() {
        let comments = scan_annots(vec![dictionary! {
            "Type" => "Annot",
            "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "Contents" => Object::string_literal("I have no subtype."),
        }]);
        assert!(comments.items.is_empty());
        assert_eq!(comments.limits.unreadable, 1);
    }

    #[test]
    fn limits_report_nothing_when_nothing_was_cut() {
        assert!(!Limits::default().any());
        assert!(Limits {
            cycles: 1,
            ..Default::default()
        }
        .any());
    }

    /// No [`Comment`] field may carry a URL, and adding one must not compile.
    ///
    /// The counterpart of `outline.rs`'s `no_target_variant_may_carry_a_url`,
    /// and the reasoning is identical: `scripts/check_webview_sinks.py` proves
    /// the frontend cannot turn a string into markup, a navigation or a script,
    /// and that proof is *sufficient* only because no attacker-controlled URL
    /// arrives for it to turn into anything. A comment is the largest body of
    /// document-chosen text tpdf has ever put on screen, so this is where that
    /// would break first.
    ///
    /// The destructuring below is **exhaustive and deliberately not a
    /// wildcard**: a new field is a compile error here, which is the strongest
    /// verdict available.
    #[test]
    fn no_comment_field_may_carry_a_url() {
        let comment = Comment {
            id: 1,
            page: 2,
            kind: Kind::Text,
            author: "Timo".into(),
            body: "Have a look at https://example.invalid/ when you can.".into(),
            subject: "Figure 3".into(),
            date: Some("2026-08-12 10:15".into()),
            rect: [1.0, 2.0, 3.0, 4.0],
            quads: vec![1.0, 2.0, 3.0, 4.0],
            reply_to: Some(0),
            hidden: false,
            object: Some((7, 0)),
        };

        let Comment {
            id,
            page,
            kind,
            author,
            body,
            subject,
            date,
            rect,
            quads,
            reply_to,
            hidden,
            object,
        } = comment;

        // The three fields carrying document text. They are *shown*, never
        // resolved: the frontend puts them in `textContent` and the sinks gate
        // is what keeps it that way. What matters here is that they are the
        // only ones --- a URL inside a body is a string a reader can see, not a
        // link anything will follow.
        let shown = [author, body, subject];
        assert!(
            shown.iter().any(|value| value.contains("://")),
            "the fixture must actually contain a URL, or this proves nothing"
        );

        // Everything else is ours or is a number, and none of it can be a URL.
        let derived = [
            id.to_string(),
            page.to_string(),
            format!("{kind:?}"),
            // Two integers the *file* chose, which is worth one line rather than
            // a shrug: a document controls its own object numbers, so this is
            // attacker-influenced like everything else here. It cannot be a URL
            // because it cannot be text at all --- the type is the argument, and
            // the `Debug` below is what the assertion actually reads.
            format!("{object:?}"),
            date.unwrap_or_default(),
            format!("{rect:?}"),
            // Geometry the document chose the *numbers* of and this file chose
            // the type of: an `f32` that failed to parse never reaches here,
            // and one that did cannot spell anything.
            format!("{quads:?}"),
            reply_to.map(|id| id.to_string()).unwrap_or_default(),
            hidden.to_string(),
        ];
        for value in derived {
            assert!(
                !value.contains("://") && !value.starts_with("javascript:"),
                "a field the document does not choose looked like a URL: {value:?}"
            );
        }

        // And the kind is one of ours whatever the file said.
        assert_eq!(Kind::of(b"Text"), Some(Kind::Text));
        assert_eq!(Kind::of(b"javascript:alert(1)"), None);
    }
}
