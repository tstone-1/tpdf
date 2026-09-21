//! Whether a file can be certified clean, and what the answer rests on.
//!
//! `docs/PLAN.md` §6 makes verification mandatory after a redaction and forbids
//! a bare success: the result is *verified*, or *not verified* with specifics.
//! The rule it states for getting there is deny by default --- anything the
//! sanitizer does not understand is a failure rather than a shrug --- and spike
//! 0.4 found that rule, taken literally, refuses almost every real document.
//!
//! **The refusal rate is the finding, not a detail.** `lopdf` decodes
//! `FlateDecode`, `LZWDecode` and `ASCII85Decode`, and answers
//! `Unimplemented("decompression algorithms")` for everything else. Everything
//! else includes `/DCTDecode`, `/CCITTFaxDecode`, `/JBIG2Decode` and
//! `/JPXDecode`, which is what every scanner on earth emits. On the `filters`
//! fixture all six of the spike's rewrite routes reported *not verified*,
//! QPDF's own output among them.
//!
//! **So the split is by remedy, and deliberately not by verdict.** This is the
//! part that is easy to get wrong in the direction that ships a lie: calling an
//! image carrier "fine" would make a scanned document certifiable without
//! anything having read it. It is not fine, and it is not the same as a stream
//! nothing can decode either --- one needs a different instrument, the other
//! needs a different file. A [`Report`] therefore carries two lists, and both
//! of them withhold certification:
//!
//! * [`Report::blind`] --- nothing here can account for these bytes. There is no
//!   instrument that would change the answer.
//! * [`Report::deferred`] --- a raster image. Its *encoded bytes* were scanned
//!   like every other byte in the file, so a needle sitting literally in the
//!   stream is found; what was not read is the **picture**, and text that exists
//!   only as pixels is exactly what OCR is for.
//!
//! **That instrument exists now, and it is narrower than this list.**
//! [`crate::ocr_gate`] renders the regions a redaction removed from and has an
//! engine read them, so a *region* whose carrier is a picture is answered. A
//! deferred image sitting anywhere else in the file is still exactly what this
//! bullet says it is: bytes nobody read, reported rather than waved through.
//!
//! A caller that wants a single word gets [`Verdict`], which says *not verified*
//! for either list. What the split buys is that the reason names the next step
//! rather than ending the conversation.
//!
//! **Since 2026-09-21 a hit carries a page, when one can be earned.** Until
//! then a report said *"4711-0815 is still in the file"* and nothing more, which
//! a reader cannot act on: a removal that failed and a second copy on a page
//! nobody marked produce the identical sentence. [`Located`] is the answer, and
//! it has **three** values rather than two because the middle one is where a
//! two-valued version would lie --- an object more than one page draws belongs
//! to no page, and naming the first page that reached it would turn *still in
//! the file* into *on a page you did not mark*. So: placed on a page set, shared
//! between pages and therefore unplaceable, or reached by no page at all. A
//! bound tripping anywhere withholds every answer rather than shortening one,
//! because a truncated walk does not lose an answer, it invents a wrong one.
//!
//! What that does **not** change is the verdict. A word still in the file is
//! still a leak whatever page it is on, and this module will not certify one:
//! the location makes the finding actionable, not forgivable.
//!
//! **Two things the byte scan can see and the graph walk cannot**, both from
//! spike 0.4 and both preserved here: bytes past the last `%%EOF` belong to no
//! object at all, and a file with more than one `%%EOF` has revisions that no
//! parser resolves --- an object a later revision *overwrote* sits at its old
//! offset addressable by nothing, so it is invisible to any graph walk and, if
//! it is compressed, to the byte scan as well. Such a file cannot be certified;
//! it can only be rewritten and then certified.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Document, Object};

/// The ceiling on any single decoded stream, which every other scan uses too.
///
/// **This module declared its own literal from 2026-08-26 until 2026-08-31**,
/// with a comment saying it was stated rather than imported so that the bound
/// would be visible where it is enforced --- and citing `save::MAX_DECODE`,
/// which is not a definition but `save.rs`'s import of this one. Two copies of a
/// security bound with nothing asserting they agree, one day after
/// `encoding.rs` recorded that having six of them was the defect. Visibility is
/// what a `use` line is for; agreement is what one value is for.
use crate::encoding::MAX_DECODE;

/// The filters `lopdf` 0.44 can actually decode.
///
/// Read out of `Stream::decode_filters`, which dispatches on exactly these three
/// and returns `Unimplemented` otherwise. Written down rather than discovered at
/// run time because the classification below has to distinguish "this failed to
/// decode" from "this was never going to decode", and only the second is a fact
/// about the library.
const DECODABLE: &[&[u8]] = &[b"FlateDecode", b"LZWDecode", b"ASCII85Decode"];

/// How many per-object reasons one report carries before it summarises the rest.
///
/// **A bound that had no reason to exist while the scan ran here, and has one
/// now that it runs in a worker.** A [`Report`] is the reply
/// [`crate::worker_proto::Reply::Verified`] carries, and a reply is read under
/// [`crate::worker_proto::MAX_REPLY_BYTES`], which is 32 MB. Each per-object
/// reason is about a hundred bytes and there is one per object, so a file with a
/// few hundred thousand undecodable objects --- reachable inside 32 MB of PDF,
/// since an object with a filter and an empty stream costs about forty bytes ---
/// produces a report that will not fit down the pipe. The reader would then be
/// told the verification *failed*, which is a different and much worse sentence
/// than the one the file has earned.
///
/// It bounds the worker's own memory for the same input, which the coordinator
/// never bounded either.
///
/// **The suppressed reasons still withhold certification.** What replaces them
/// is one more blind reason saying how many there were, so the verdict is
/// unchanged and only the enumeration is shortened. That is the direction this
/// module is built around: a report may be less specific than the file deserves,
/// and it may never be more reassuring.
///
/// A thousand rather than a hundred because the list is a reader's evidence, and
/// rather than a million because nobody reads the millionth line either.
const MAX_OBJECT_REASONS: usize = 1_000;

/// Filters whose content is a raster image rather than bytes worth scanning.
///
/// Not a list of things we forgive. A needle cannot be *found* in a JPEG by
/// looking at its decoded output, because the decoded output is pixels --- so
/// even a decoder for these would not answer the question the byte scan asks.
/// That is what makes them a different instrument's problem rather than a
/// weaker version of the same one.
const IMAGE: &[&[u8]] = &[
    b"DCTDecode",
    b"JPXDecode",
    b"JBIG2Decode",
    b"CCITTFaxDecode",
];

/// What a stream's contents rest on, once the scan has done what it can.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Carrier {
    /// Decoded and scanned. The only outcome that accounts for the bytes.
    Scanned,
    /// A raster image: its encoded bytes were scanned, its picture was not read.
    Image {
        /// The filter that says so, for a report a person reads.
        filter: String,
    },
    /// An encoding this build understands to exist and cannot decode.
    ///
    /// `/ASCIIHexDecode` and `/RunLengthDecode` are the common ones, and they
    /// carry arbitrary bytes --- including text --- which is what separates them
    /// from [`Carrier::Image`].
    Undecodable {
        /// The filter that stopped it.
        filter: String,
    },
    /// A filter nothing here recognises at all.
    ///
    /// Distinct from [`Carrier::Undecodable`] because the remedies differ: one
    /// is a decoder this build lacks, the other is a construct nobody has looked
    /// at. Deny by default covers both, and a report that conflated them would
    /// hide the second behind the first.
    Unrecognised {
        /// The filter as the file spells it.
        filter: String,
    },
}

/// What a stream's filter chain means for the scan.
///
/// **Classified by the LAST filter**, which is the one that decides what the
/// content is: `/Filter [/ASCII85Decode /DCTDecode]` is an ASCII-armoured JPEG,
/// and it is the JPEG that matters. `lopdf` decodes the chain in order and
/// stops at the first it cannot do, so a chain whose earlier entries are
/// decodable and whose last is not still reaches this function's answer.
///
/// An empty chain is [`Carrier::Scanned`]: a stream with no `/Filter` is stored
/// as it is, and `lopdf` returns its content unchanged.
#[must_use]
pub fn classify(filters: &[&[u8]]) -> Carrier {
    let Some(last) = filters.last() else {
        return Carrier::Scanned;
    };
    let name = || String::from_utf8_lossy(last).into_owned();
    if IMAGE.contains(last) {
        return Carrier::Image { filter: name() };
    }
    if DECODABLE.contains(last) {
        return Carrier::Scanned;
    }
    // Known to the format, absent from this build's decoder. The two are worth
    // separating even though both withhold certification, because "we lack a
    // decoder" is a thing that can be fixed here and "we have never heard of
    // this" is a thing that has to be read first.
    const KNOWN: &[&[u8]] = &[b"ASCIIHexDecode", b"RunLengthDecode", b"Crypt"];
    if KNOWN.contains(last) {
        Carrier::Undecodable { filter: name() }
    } else {
        Carrier::Unrecognised { filter: name() }
    }
}

/// How deep one page's reachability walk may follow references.
///
/// A page reaches its content stream at depth 1 and a form's resources two
/// levels further down, so a document nesting forms eight deep --- which is the
/// bound `crate::textedit::forms` enforces for the editor --- sits at about 17.
/// Thirty-two leaves room for that and stops a reference cycle that the
/// per-page visited set somehow did not.
const MAX_REACH_DEPTH: usize = 32;

/// How many distinct objects one page's walk may reach.
///
/// Per page rather than per document, because the question the walk answers is
/// per page: a document of ten thousand pages is not more suspicious than one
/// page, but a single page that reaches a hundred thousand objects is.
const MAX_REACH_OBJECTS: usize = 100_000;

/// How much work the whole walk may do, across every page.
///
/// The per-page bound above cannot bound the document: a hostile file can pay
/// its cost once per page. This is the one that makes the walk's total cost a
/// function of nothing the file controls without limit.
const MAX_REACH_STEPS: usize = 4_000_000;

/// How many objects may carry one needle before the walk stops placing it.
///
/// Not a performance bound --- it is the point at which an answer stops being
/// one. A word in a thousand objects is not *on* a page, and the honest answer
/// for it is [`Located::Unplaced`] rather than a list nobody can read.
const MAX_CARRIERS: usize = 1_000;

/// How many pages one answer names before it counts the rest.
///
/// [`MAX_OBJECT_REASONS`]'s rule, in the place the same pressure shows up: the
/// report crosses a pipe under [`crate::worker_proto::MAX_REPLY_BYTES`], and a
/// needle on every page of a ten-thousand-page file would otherwise put ten
/// thousand numbers in it, once per needle. The enumeration is shortened and
/// the count survives; the verdict is untouched either way.
const MAX_LOCATED_PAGES: usize = 64;

/// Dictionary keys a page's walk must not follow, because they leave the page.
///
/// **The whole soundness of attribution is this list.** A page dictionary names
/// its `/Parent`, an annotation names its `/P`, a link names a `/Dest` on some
/// other page, and an outline entry names its `/Next`. Following any one of
/// them walks out of this page and into the rest of the document, and the walk
/// would then answer *every page* for every object --- which is not a weaker
/// answer than none, it is a **wrong** one, and a wrong attribution turns
/// *"still in the file"* into *"on a page you did not mark"*.
///
/// So this errs towards reaching too little. An appearance stream under `/D`
/// (an annotation's *down* look) is skipped with the actions that share the
/// key, and a needle sitting only there is reported [`Located::Unplaced`]
/// rather than placed --- which withholds an answer instead of inventing one.
/// That is the direction this module is built around.
const NOT_CONTENT: &[&[u8]] = &[
    // Up and across the page tree.
    b"Parent",
    b"Kids",
    b"Root",
    b"Pages",
    b"PageLabels",
    // An annotation's page, and a reply's antecedent.
    b"P",
    b"IRT",
    // Destinations and actions, which name another page by design.
    b"Dest",
    b"D",
    b"A",
    b"AA",
    b"OpenAction",
    b"Names",
    // The outline and the article threads, which are document-wide chains.
    b"Outlines",
    b"First",
    b"Last",
    b"Next",
    b"Prev",
    b"Threads",
    b"B",
    // The structure tree, which reaches every page from any element.
    b"StructTreeRoot",
    b"StructParent",
    b"StructParents",
    b"K",
    // The form, whose field tree spans the document.
    b"AcroForm",
];

/// Pages an answer names, with however many it did not name.
///
/// Slots are **0-based positions in the file that was scanned**, which for a
/// redaction is the file that was just written --- not page numbers of the
/// document the reader opened, and not the baseline numbers the plan is in
/// terms of. [`Placed::sentence`] is the only thing that turns one into a page
/// number a person reads, and it is the only place the `+ 1` happens.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Placed {
    /// Ascending, at most [`MAX_LOCATED_PAGES`] of them.
    pub pages: Vec<u32>,
    /// How many further pages there were.
    pub more: usize,
}

impl Placed {
    /// Takes the first [`MAX_LOCATED_PAGES`] and counts the rest.
    #[must_use]
    pub fn of(pages: &BTreeSet<u32>) -> Self {
        Placed {
            pages: pages.iter().copied().take(MAX_LOCATED_PAGES).collect(),
            more: pages.len().saturating_sub(MAX_LOCATED_PAGES),
        }
    }

    /// The pages as a person reads them, one-based.
    #[must_use]
    pub fn sentence(&self) -> String {
        let numbers: Vec<String> = self
            .pages
            .iter()
            .map(|slot| (slot + 1).to_string())
            .collect();
        if self.more > 0 {
            return format!("pages {}, and {} more", numbers.join(", "), self.more);
        }
        match numbers.as_slice() {
            [] => "no page".to_string(),
            [one] => format!("page {one}"),
            [first, second] => format!("pages {first} and {second}"),
            [rest @ .., last] => format!("pages {} and {last}", rest.join(", ")),
        }
    }
}

/// Where a needle the scan found still sits, as far as the walk can prove.
///
/// **Three answers rather than two, and the third is the point.** A scan that
/// could only say *found* or *not found* was what made *"4711-0815 is still in
/// the file"* unactionable: a reader could not tell a removal that failed from
/// a second copy on a page nobody marked. Two answers would have been *placed*
/// and *not placed*, and that is the version that ships a lie --- an object
/// more than one page draws has no page, and calling it the first page that
/// reached it is exactly the wrong attribution this type exists to refuse.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Located {
    /// Every object carrying it is reached by exactly one page, and these are
    /// those pages.
    Pages(Placed),
    /// Something carrying it is drawn by more than one page --- a shared form,
    /// a shared font --- so no page owns it. The pages that share it.
    Shared(Placed),
    /// Something carrying it is reached by no page at all: bytes outside the
    /// page tree, the file's own metadata, or a hit the byte scan found and the
    /// graph walk never accounted for.
    Unplaced,
}

/// Which pages of a document reach which objects.
///
/// Built once per scan and only when there is something to place. The `Vec` per
/// object is exact rather than a set because the outer loop runs slots in
/// ascending order and visits each object at most once per page, so pushing
/// when the last entry is not this slot is a deduplication.
///
/// **Keyed by the whole [`lopdf::ObjectId`], generation included, and not by the
/// object number.** The number is what a report *prints* --- every `object N`
/// message in this module drops the generation --- and for a message that is
/// fine. For attribution it is not: `lopdf` keys `Document::objects` by the
/// pair, so a file defining both `5 0 obj` and `5 1 obj` holds two objects here,
/// and a walk keyed on `5` would hand one of them the other's pages. That is a
/// *wrong* page, which is the one answer this whole walk exists to refuse.
///
/// **Defensive rather than demonstrated**, and worth saying so: an ordinary
/// cross-reference table carries one generation per number, and `lopdf`'s writer
/// emits generation 0 for everything, so no fixture here can be built with the
/// collision in it. It costs nothing, and the direction it fails in if it ever
/// does fire is the expensive one.
struct Reach {
    by_object: std::collections::HashMap<lopdf::ObjectId, Vec<u32>>,
}

/// Every page's reachable objects, or `None` when a bound stopped the walk.
///
/// **A bound anywhere withholds attribution everywhere, and that is the safe
/// direction rather than a convenience.** A walk truncated on page 400 has not
/// merely lost page 400's answer: an object it would have reached there is now
/// recorded as reached by page 1 alone, and [`locate`] would place a needle on
/// page 1 that sits on both. Under-claiming is what this module is for, so a
/// truncated walk answers nothing at all.
fn reach(doc: &Document) -> Option<Reach> {
    let pages = crate::pagetree::ordered_pages(doc);
    let mut by_object: std::collections::HashMap<lopdf::ObjectId, Vec<u32>> =
        std::collections::HashMap::new();
    let mut steps = 0usize;
    for (slot, page) in pages.iter().enumerate() {
        let slot = u32::try_from(slot).ok()?;
        let mut seen: BTreeSet<lopdf::ObjectId> = BTreeSet::new();
        let mut stack: Vec<(lopdf::ObjectId, usize)> = vec![(*page, 0)];
        while let Some((id, depth)) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if seen.len() > MAX_REACH_OBJECTS {
                return None;
            }
            steps += 1;
            if steps > MAX_REACH_STEPS {
                return None;
            }
            let reached = by_object.entry(id).or_default();
            if reached.last() != Some(&slot) {
                reached.push(slot);
            }
            let Ok(object) = doc.get_object(id) else {
                // Not a truncation: a dangling reference reaches nothing, and a
                // page that names one is a page with a hole in it rather than a
                // page this walk failed to read. Anything carried by the object
                // that is not there cannot be found by the scan either.
                continue;
            };
            if !push_refs(object, depth, &mut stack, &mut steps) {
                return None;
            }
        }
    }
    Some(Reach { by_object })
}

/// Pushes every reference inside one object, skipping the keys that leave the page.
///
/// Returns `false` when a bound tripped, which the caller turns into no
/// attribution at all. The inner walk is a worklist rather than recursion
/// because its depth is the file's to choose: a directly nested array is not a
/// reference and so is not bounded by [`MAX_REACH_DEPTH`], and recursing on it
/// would put the file in charge of this process's stack.
fn push_refs(
    object: &Object,
    depth: usize,
    stack: &mut Vec<(lopdf::ObjectId, usize)>,
    steps: &mut usize,
) -> bool {
    let mut inner: Vec<&Object> = vec![object];
    while let Some(value) = inner.pop() {
        *steps += 1;
        if *steps > MAX_REACH_STEPS {
            return false;
        }
        match value {
            Object::Reference(id) => {
                if depth + 1 > MAX_REACH_DEPTH {
                    return false;
                }
                stack.push((*id, depth + 1));
            }
            Object::Array(items) => inner.extend(items.iter()),
            Object::Dictionary(dict) => push_entries(dict, &mut inner),
            Object::Stream(stream) => push_entries(&stream.dict, &mut inner),
            _ => {}
        }
    }
    true
}

/// A dictionary's values, minus the ones [`NOT_CONTENT`] names.
///
/// **One function for the two arms above, so the skip list has one call site.**
/// The obvious shape is the loop written twice, and it is what this was; a
/// content stream's dictionary carries none of these keys in any fixture here,
/// so the second copy of the guard was one no test could reach and no mutation
/// could aim at. `docs/TRAPS.md` has the general form more than once --- a check
/// bound to one caller covers only that caller, and two copies of one rule
/// drift.
fn push_entries<'a>(dict: &'a lopdf::Dictionary, inner: &mut Vec<&'a Object>) {
    for (key, value) in dict {
        if NOT_CONTENT.contains(&key.as_slice()) {
            continue;
        }
        inner.push(value);
    }
}

/// Which objects carry one needle, and whether there were too many to say.
#[derive(Default)]
struct Carriers {
    objects: BTreeSet<lopdf::ObjectId>,
    /// More than [`MAX_CARRIERS`] of them, so the list is not an answer.
    overflowed: bool,
}

impl Carriers {
    fn note(&mut self, object: lopdf::ObjectId) {
        if self.objects.len() >= MAX_CARRIERS {
            self.overflowed = true;
            return;
        }
        self.objects.insert(object);
    }
}

/// Turns each needle's carriers into the pages that reach them.
///
/// **The weakest carrier decides.** A needle carried by one page-1-only object
/// and one object nobody reaches is not on page 1: it is somewhere this walk
/// cannot account for, and saying *page 1* would be the wrong attribution. So
/// [`Located::Unplaced`] outranks [`Located::Shared`], which outranks
/// [`Located::Pages`], and a needle only gets the specific answer when every
/// one of its carriers earned it.
fn locate(
    doc: &Document,
    found: &BTreeSet<String>,
    carriers: &BTreeMap<String, Carriers>,
) -> BTreeMap<String, Located> {
    let Some(reach) = reach(doc) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for needle in found {
        // No object carries it and the byte scan found it anyway: it is in
        // bytes no page reaches. That is the one case that can never become an
        // answer, however good the walk gets.
        let Some(carried) = carriers.get(needle) else {
            out.insert(needle.clone(), Located::Unplaced);
            continue;
        };
        if carried.overflowed || carried.objects.is_empty() {
            out.insert(needle.clone(), Located::Unplaced);
            continue;
        }
        let mut pages: BTreeSet<u32> = BTreeSet::new();
        let mut shared: BTreeSet<u32> = BTreeSet::new();
        let mut unplaced = false;
        for object in &carried.objects {
            match reach.by_object.get(object) {
                None => unplaced = true,
                Some(slots) => {
                    if slots.len() > 1 {
                        shared.extend(slots.iter().copied());
                    }
                    pages.extend(slots.iter().copied());
                }
            }
        }
        let answer = if unplaced {
            Located::Unplaced
        } else if !shared.is_empty() {
            Located::Shared(Placed::of(&shared))
        } else {
            Located::Pages(Placed::of(&pages))
        };
        out.insert(needle.clone(), answer);
    }
    out
}

/// One word for a caller that needs one, and never a bare success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Every carrier accounted for, and nothing that should be gone is present.
    Verified,
    /// With the specifics. `docs/PLAN.md` §6 forbids reporting this any other
    /// way: a redaction that cannot be proved clean is a confident lie, and the
    /// reasons are what tell a reader whether the next step is OCR, a rewrite,
    /// or giving up on the file.
    NotVerified(Vec<String>),
}

/// What a scan of one file found, and what it could not look at.
///
/// **Serialised, because the scan runs in a worker.** This is the answer
/// [`crate::worker_proto::Reply::Verified`] carries back, and it is the whole of
/// what crosses: the bytes it was read from stay behind the boundary. That is
/// also why the per-object lists are bounded --- see [`MAX_OBJECT_REASONS`].
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Report {
    /// Needles still present. For a redaction, any of these is a leak.
    pub found: BTreeSet<String>,
    /// Where each of them is, for the ones the walk could place.
    ///
    /// **Absent is not a page, and an entry is not an excuse.** A needle with no
    /// entry here was never attributed --- either nothing was found, or the walk
    /// was withheld --- and [`Report::verdict`] then says exactly what it said
    /// before this existed. A needle with an entry is still a leak; the entry
    /// only says where, which is what makes the finding actionable.
    #[serde(default)]
    pub located: BTreeMap<String, Located>,
    /// Bytes nothing here can account for. No instrument would change this.
    pub blind: Vec<String>,
    /// Pictures nobody read. OCR is the instrument that would.
    pub deferred: Vec<String>,
    /// How many objects the graph walk reached.
    pub objects: usize,
    /// `%%EOF` markers, so more than one revision is visible.
    pub eofs: usize,
    /// Non-whitespace bytes after the last `%%EOF`.
    pub trailing: usize,
    /// The file's length, so a report says what it was about.
    pub bytes: u64,
}

impl Report {
    /// The one-word answer, with every reason that withheld it.
    ///
    /// **Order matters to a reader and not to the verdict.** A leak is reported
    /// first because it is the finding that makes the others moot: a file with a
    /// needle still in it is not going to become clean by running OCR.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        let mut why = Vec::new();
        for needle in &self.found {
            why.push(match self.located.get(needle) {
                // What this said before attribution existed, and what it still
                // says whenever the walk did not run or would not answer.
                None => format!("{needle} is still in the file"),
                Some(Located::Pages(placed)) => {
                    format!("{needle} is still in the file, on {}", placed.sentence())
                }
                Some(Located::Shared(placed)) => format!(
                    "{needle} is still in the file, carried by something more than one page \
                     draws ({}), so which page it is on could not be established",
                    placed.sentence()
                ),
                Some(Located::Unplaced) => format!(
                    "{needle} is still in the file, in something no page of it reaches, so \
                     which page it is on could not be established"
                ),
            });
        }
        why.extend(self.blind.iter().cloned());
        why.extend(self.deferred.iter().cloned());
        if why.is_empty() {
            Verdict::Verified
        } else {
            Verdict::NotVerified(why)
        }
    }

    /// Whether every word the scan found was placed on a page set.
    ///
    /// **The precondition for anything a caller says about marked pages.** A
    /// caller knows which slots the reader marked and this knows where the
    /// words are; only both together make *"none of them is on a page you
    /// marked"* a provable sentence. One needle the walk would not place makes
    /// the whole comparison unsound, so this is `all` and not `any`.
    ///
    /// False for a report that found nothing, deliberately: there is nothing to
    /// compare, and a caller reading this as *"the pages you marked are clean"*
    /// would be reading a claim out of an empty set.
    #[must_use]
    pub fn placed(&self) -> bool {
        !self.found.is_empty()
            && self
                .found
                .iter()
                .all(|needle| matches!(self.located.get(needle), Some(Located::Pages(_))))
    }

    /// Every page a placed needle sits on, as slots in the file that was scanned.
    #[must_use]
    pub fn placed_pages(&self) -> BTreeSet<u32> {
        self.located
            .values()
            .filter_map(|where_| match where_ {
                Located::Pages(placed) => Some(placed.pages.iter().copied()),
                Located::Shared(_) | Located::Unplaced => None,
            })
            .flatten()
            .collect()
    }
}

/// What is certainly wrong with a file this build just wrote.
///
/// **The narrow half of `docs/PLAN.md` §6 step 5, and the narrowness is the
/// finding.** That step asks for an independent parser to re-check a rewrite,
/// on the strength of spike 0.4 catching a `/Size` that claimed more objects
/// than the file held --- PDFium rendered it pixel-perfect, `qpdf --check`
/// named it. Four readers were put to that same defect on 2026-08-26, and the
/// result decided this function's shape:
///
/// | reader | stale `/Size` |
/// |---|---|
/// | this byte scan | silent |
/// | `lopdf`'s loader | *OK, 8 pages* |
/// | PDFKit (`print_macos::read`) | *OK, 8 pages*, in 0.2 ms |
/// | `qpdf --check` | **exit 3** |
///
/// So there is no in-app parser that catches it, and the obvious repair --- our
/// own rule that `/Size` must equal the cross-reference table's entry count ---
/// was written, run, and **condemned a healthy file**: a swept rewrite of
/// `links.pdf` has 91 entries in three subsections against `/Size 102`, because
/// object numbers go sparse when a sweep removes objects and an unlisted number
/// is free. `qpdf --check` passes it. Every `incr-*.pdf` fixture fails the same
/// rule for the same reason from the other direction, since an incremental
/// file's `/Size` counts every revision's objects and its last section lists
/// only what changed.
///
/// A validator that fires on correct input is worse than none, so this checks
/// **only what cannot be legitimate in a file we wrote a moment ago**, and says
/// so rather than implying it covers structure. Real cross-reference validation
/// is qpdf's, it is not here, and `docs/PLAN.md` §6 keeps the note that QPDF
/// still has a place.
///
/// The two revision rules are §6's own words --- *assert exactly one logical
/// revision and no trailing data* --- and they are meaningful precisely because
/// this is our output: a **source** document may legitimately have many
/// revisions, and this is never pointed at one.
///
/// Costs a scan of the bytes and nothing else: 65.8 ms on the 321 MB fixture,
/// 0.34 ms on a 1.3 MB one.
#[must_use]
pub fn structure(bytes: &[u8]) -> Vec<String> {
    let mut wrong = Vec::new();

    if !bytes.starts_with(b"%PDF-") {
        wrong.push("the file does not begin with a PDF header".to_string());
    }

    let eofs = count(bytes, b"%%EOF");
    match eofs {
        0 => wrong.push("the file has no %%EOF marker".to_string()),
        1 => {}
        many => wrong.push(format!(
            "the file has {many} %%EOF markers, so it holds more than one revision \u{2014} a rewrite writes exactly one, and an earlier revision is content no parser will show and no scan can decode"
        )),
    }

    if let Some(last) = rfind(bytes, b"%%EOF") {
        let trailing = bytes[last + 5..]
            .iter()
            .filter(|byte| !byte.is_ascii_whitespace())
            .count();
        if trailing > 0 {
            wrong.push(format!(
                "{trailing} byte(s) follow the last %%EOF, which belong to no object and which nothing here put there"
            ));
        }
    }

    // Not "is there a `startxref`" --- `rfind` would find the one inside a
    // string or a comment just as happily. What makes this worth checking is
    // the *offset*: a rewrite computes it, and one pointing past the end of the
    // file is a file no reader can open, which is the failure this is between
    // the reader and.
    match rfind(bytes, b"startxref") {
        None => wrong.push("the file has no startxref".to_string()),
        Some(at) => match start_offset(&bytes[at + 9..]) {
            None => wrong.push("the file's startxref has no offset after it".to_string()),
            Some(offset) if offset >= bytes.len() => wrong.push(format!(
                "startxref points at byte {offset} of a {}-byte file",
                bytes.len()
            )),
            Some(_) => {}
        },
    }

    wrong
}

/// The decimal number after a `startxref`, skipping the whitespace before it.
///
/// Returns `None` for no digits at all, and for a number too large to be a file
/// offset --- which is the same answer for the purpose here, since both mean the
/// offset cannot be believed.
fn start_offset(after: &[u8]) -> Option<usize> {
    let digits: Vec<u8> = after
        .iter()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .take_while(|byte| byte.is_ascii_digit())
        .copied()
        .collect();
    if digits.is_empty() {
        return None;
    }
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

/// Scans a file for every needle, at the byte level and through the object graph.
///
/// Both are needed and neither is sufficient. The byte scan is the only thing
/// that can see content outside the object graph --- trailing bytes past `%%EOF`
/// belong to no object at all --- and the graph walk is the only thing that can
/// see inside a compressed stream.
///
/// Promoted from `examples/sanitize_rewrite.rs` on 2026-08-26, which now calls
/// this rather than carrying its own copy: two implementations of what counts as
/// clean is the drift this repository keeps finding in other forms.
///
/// # Errors
///
/// The file cannot be read. A file that cannot be *parsed* is not an error ---
/// it is a blind spot, reported as one, which is the whole point of the type.
pub fn scan(bytes: &[u8], needles: &[String], password: Option<&str>) -> Report {
    let mut report = Report {
        eofs: count(bytes, b"%%EOF"),
        bytes: bytes.len() as u64,
        ..Default::default()
    };

    for needle in needles {
        if find(bytes, needle.as_bytes()) {
            report.found.insert(needle.clone());
        }
    }

    if report.eofs > 1 {
        report.blind.push(format!(
            "the file has {} %%EOF markers, so earlier revisions exist that no parser will \
             resolve and no scan can decode",
            report.eofs
        ));
    }

    match rfind(bytes, b"%%EOF") {
        Some(last) => {
            report.trailing = bytes[last + 5..]
                .iter()
                .filter(|byte| !byte.is_ascii_whitespace())
                .count();
        }
        None => report
            .blind
            .push("the file has no %%EOF marker".to_string()),
    }

    match Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            password: password.map(str::to_string),
            ..Default::default()
        },
    ) {
        Err(why) => report.blind.push(format!(
            "the file could not be parsed, so nothing in it was accounted for: {why}"
        )),
        Ok(doc) => {
            report.objects = doc.objects.len();
            // **Before the walk, because an empty walk finds nothing and that
            // reads exactly like a clean file.** `lopdf` answers `Ok` for a
            // document it could not authenticate, having parsed no objects at
            // all --- so the loop below would compare every needle against
            // nothing, `found` would stay empty, and `verdict` would answer
            // *Verified* about bytes this build never decoded. An absence and a
            // lock are the same reading here, and the reassuring one is wrong.
            //
            // `docs/PLAN.md` §6 forbids reporting a redaction clean that was not
            // proved clean, and this is the one scanner that claim rests on.
            if doc.is_encrypted() {
                report.blind.push(
                    "the file is encrypted and no password opened it, so nothing in it was \
                     decoded and nothing it contains was checked"
                        .to_string(),
                );
            } else if doc.objects.is_empty() {
                report.blind.push(
                    "the file parsed to no objects at all, so nothing in it was accounted for"
                        .to_string(),
                );
            }
            // Counted in full and listed in part: see `MAX_OBJECT_REASONS`.
            // `blind` also carries reasons that are about the *file* rather than
            // about an object, and those are never suppressed --- which is why
            // this is a counter and not `report.blind.len()`.
            let mut blind_objects = 0usize;
            let mut deferred_objects = 0usize;
            // Which objects carry which needle, so the walk below can turn that
            // into pages. Recorded here because this is the only loop that
            // decodes anything, and only for needles that actually match, so a
            // clean file pays nothing for it.
            let mut carriers: BTreeMap<String, Carriers> = BTreeMap::new();
            for (id, object) in &doc.objects {
                let strings = flatten_strings(object);
                for needle in needles {
                    if find(&strings, needle.as_bytes()) {
                        report.found.insert(needle.clone());
                        carriers.entry(needle.clone()).or_default().note(*id);
                    }
                }
                let Object::Stream(stream) = object else {
                    continue;
                };
                let filters = stream.filters().unwrap_or_default();
                match classify(&filters) {
                    Carrier::Image { filter } => {
                        deferred_objects += 1;
                        if deferred_objects <= MAX_OBJECT_REASONS {
                            report.deferred.push(format!(
                                "object {} is a {filter} image, so its encoded bytes were \
                                 scanned and its picture was not read \u{2014} text visible only as \
                                 pixels needs OCR",
                                id.0
                            ));
                        }
                    }
                    Carrier::Undecodable { filter } => {
                        blind_objects += 1;
                        if blind_objects <= MAX_OBJECT_REASONS {
                            report.blind.push(format!(
                                "object {} is {filter}, which this build cannot decode, so its \
                                 contents are unknown",
                                id.0
                            ));
                        }
                    }
                    Carrier::Unrecognised { filter } => {
                        blind_objects += 1;
                        if blind_objects <= MAX_OBJECT_REASONS {
                            report.blind.push(format!(
                                "object {} uses {filter}, which nothing here recognises",
                                id.0
                            ));
                        }
                    }
                    Carrier::Scanned => match stream.decompressed_content_with_limit(MAX_DECODE) {
                        Ok(decoded) => {
                            for needle in needles {
                                if find(&decoded, needle.as_bytes()) {
                                    report.found.insert(needle.clone());
                                    carriers.entry(needle.clone()).or_default().note(*id);
                                }
                            }
                        }
                        // Classified as decodable and then would not decode: a
                        // bomb over the bound, or damage. Blind either way, and
                        // it must not be mistaken for the filter cases above ---
                        // those are decided before any decoding is attempted.
                        Err(why) => {
                            blind_objects += 1;
                            if blind_objects <= MAX_OBJECT_REASONS {
                                report.blind.push(format!(
                                    "object {} could not be decoded, so its contents are \
                                     unknown: {why}",
                                    id.0
                                ));
                            }
                        }
                    },
                }
            }
            // The suppressed ones, said once. A count is still a reason, so the
            // verdict a file with a million bad objects earns is the verdict it
            // gets --- only the enumeration is shortened.
            if deferred_objects > MAX_OBJECT_REASONS {
                report.deferred.push(format!(
                    "{} further images were not read, and are not listed individually",
                    deferred_objects - MAX_OBJECT_REASONS
                ));
            }
            if blind_objects > MAX_OBJECT_REASONS {
                report.blind.push(format!(
                    "{} further objects could not be accounted for, and are not listed \
                     individually",
                    blind_objects - MAX_OBJECT_REASONS
                ));
            }

            // **Only when there is something to place, and only in a file the
            // walk can account for.**
            //
            // The first half is cost: a clean scan --- the common one --- never
            // walks a page at all.
            //
            // The second is soundness, and it is this module's opening
            // paragraph arriving as a guard. A file with more than one `%%EOF`
            // holds revisions no parser resolves: an object a later revision
            // overwrote sits at its old offset, addressable by nothing, so the
            // graph walk cannot see it and neither can the byte scan if it is
            // compressed. Attributing a needle to page 5 in such a file would
            // be a claim about the *live* graph offered as a claim about the
            // file, with a dead copy of the same word invisible beside it.
            // `blind` already says the file is uncertifiable; this says the
            // location is unknowable too, rather than answering anyway.
            if !report.found.is_empty() && report.eofs <= 1 {
                report.located = locate(&doc, &report.found, &carriers);
            }
        }
    }

    report.blind.sort();
    report.blind.dedup();
    report.deferred.sort();
    report.deferred.dedup();
    report
}

/// How many times `needle` occurs in `haystack`.
fn count(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

/// Whether `needle` occurs in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Where `needle` last occurs in `haystack`.
fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

/// Every string in an object, flattened, so a needle in one is found.
fn flatten_strings(object: &Object) -> Vec<u8> {
    let mut out = Vec::new();
    collect_strings(object, &mut out);
    out
}

fn collect_strings(object: &Object, out: &mut Vec<u8>) {
    match object {
        Object::String(bytes, _) => {
            out.extend_from_slice(bytes);
            out.push(b'\n');
        }
        Object::Array(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        Object::Dictionary(dict) => {
            for (_, value) in dict {
                collect_strings(value, out);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in &stream.dict {
                collect_strings(value, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {

    use super::structure;
    use super::{classify, Carrier, Located, Report, Verdict};

    /// A minimal file with the shape `structure` expects, to perturb.
    ///
    /// Built by hand rather than serialised, and it has to be: `lopdf` writes a
    /// well-formed file for every document it will accept --- an empty one comes
    /// out as 125 valid bytes --- so a fixture from the writer cannot carry any
    /// of the defects below. `docs/TRAPS.md` records building the malformed
    /// fixture by hand as the answer when the model forbids the input.
    fn well_formed() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.7\n1 0 obj\n<</Type/Catalog>>\nendobj\n");
        let start = bytes.len();
        bytes.extend_from_slice(b"xref\n0 2\n");
        bytes.extend_from_slice(b"0000000000 65535 f \n0000000009 00000 n \n");
        bytes.extend_from_slice(b"trailer\n<</Size 2/Root 1 0 R>>\n");
        bytes.extend_from_slice(format!("startxref\n{start}\n%%EOF\n").as_bytes());
        bytes
    }

    /// The control, and without it every check below is satisfied by a function
    /// that complains about everything.
    #[test]
    fn a_well_formed_file_draws_no_complaint() {
        assert_eq!(structure(&well_formed()), Vec::<String>::new());
    }

    /// **The corpus control is in `save.rs`, deliberately, and it is the one
    /// that matters.** A hand-built fixture agrees with whatever the writer of
    /// the check had in mind, which is the writer-and-its-own-reader shape; what
    /// catches a rule that is wrong *about PDF* is real documents. The first
    /// draft of a `/Size` rule was killed exactly that way, condemning a healthy
    /// swept rewrite of `links.pdf` that `qpdf --check` passes.
    ///
    /// It sweeps rewritten **output** rather than source documents, because that
    /// is the only population this function is ever pointed at. Sweeping sources
    /// was tried first and reported `hostile-trailing.pdf` --- correctly, since
    /// that fixture exists to carry 84 bytes past its `%%EOF`. A control whose
    /// population includes deliberately malformed files has to exclude them, and
    /// an exclusion list is a thing that rots; changing the population removes
    /// the question. See `every_rewritten_fixture_is_structurally_sound`.
    #[test]
    fn a_file_that_does_not_begin_with_a_pdf_header_is_refused() {
        let mut bytes = well_formed();
        bytes.splice(0..0, *b"junk");
        let wrong = structure(&bytes);
        assert!(
            wrong.iter().any(|why| why.contains("PDF header")),
            "{wrong:?}"
        );
    }

    #[test]
    fn a_file_with_no_eof_marker_is_refused() {
        let bytes = well_formed();
        let at = bytes.len() - 6;
        let wrong = structure(&bytes[..at]);
        assert!(
            wrong.iter().any(|why| why.contains("no %%EOF")),
            "{wrong:?}"
        );
    }

    /// `docs/PLAN.md` §6: a rewrite writes exactly one logical revision.
    #[test]
    fn a_second_revision_is_refused_and_the_count_is_reported() {
        let mut bytes = well_formed();
        let again = bytes.clone();
        bytes.extend_from_slice(&again);
        let wrong = structure(&bytes);
        assert!(
            wrong.iter().any(|why| why.contains("2 %%EOF markers")),
            "{wrong:?}"
        );
    }

    /// §6 again: no trailing data.
    #[test]
    fn bytes_after_the_last_eof_are_refused_and_whitespace_is_not() {
        let mut bytes = well_formed();
        bytes.extend_from_slice(b"\n\r\t   \n");
        assert_eq!(
            structure(&bytes),
            Vec::<String>::new(),
            "whitespace after %%EOF is how every writer ends a file"
        );
        bytes.extend_from_slice(b"leftover");
        let wrong = structure(&bytes);
        assert!(
            wrong.iter().any(|why| why.contains("8 byte(s) follow")),
            "{wrong:?}"
        );
    }

    #[test]
    fn a_file_with_no_startxref_is_refused() {
        let bytes = well_formed();
        let text = String::from_utf8(bytes).expect("ascii fixture");
        let wrong = structure(text.replace("startxref", "startxxxx").as_bytes());
        assert!(
            wrong.iter().any(|why| why.contains("no startxref")),
            "{wrong:?}"
        );
    }

    /// The offset is what makes this worth checking rather than the word.
    #[test]
    fn a_startxref_pointing_past_the_end_of_the_file_is_refused() {
        let bytes = well_formed();
        let text = String::from_utf8(bytes).expect("ascii fixture");
        let at = text.rfind("startxref").expect("the fixture has one");
        let broken = format!("{}startxref\n999999999\n%%EOF\n", &text[..at]);
        let wrong = structure(broken.as_bytes());
        assert!(
            wrong
                .iter()
                .any(|why| why.contains("startxref points at byte 999999999")),
            "{wrong:?}"
        );
    }

    #[test]
    fn a_startxref_with_no_number_after_it_is_refused() {
        let bytes = well_formed();
        let text = String::from_utf8(bytes).expect("ascii fixture");
        let at = text.rfind("startxref").expect("the fixture has one");
        let broken = format!("{}startxref\n%%EOF\n", &text[..at]);
        let wrong = structure(broken.as_bytes());
        assert!(
            wrong.iter().any(|why| why.contains("no offset after it")),
            "{wrong:?}"
        );
    }

    /// An offset too large for a `usize` is unbelievable rather than large.
    #[test]
    fn a_startxref_offset_too_large_to_be_an_offset_is_refused() {
        let bytes = well_formed();
        let text = String::from_utf8(bytes).expect("ascii fixture");
        let at = text.rfind("startxref").expect("the fixture has one");
        let broken = format!(
            "{}startxref\n99999999999999999999999999\n%%EOF\n",
            &text[..at]
        );
        let wrong = structure(broken.as_bytes());
        assert!(
            wrong.iter().any(|why| why.contains("no offset after it")),
            "{wrong:?}"
        );
    }

    /// A stream with no `/Filter` is stored as it is, so it is scannable.
    #[test]
    fn a_stream_with_no_filter_is_scanned() {
        assert_eq!(classify(&[]), Carrier::Scanned);
    }

    /// The three `lopdf` actually implements, read out of its dispatch.
    #[test]
    fn the_filters_lopdf_implements_are_scanned() {
        for filter in [&b"FlateDecode"[..], b"LZWDecode", b"ASCII85Decode"] {
            assert_eq!(classify(&[filter]), Carrier::Scanned, "{filter:?}");
        }
    }

    /// What every scanner emits, and the reason this module exists.
    #[test]
    fn the_raster_filters_are_deferred_to_a_different_instrument() {
        for filter in [
            &b"DCTDecode"[..],
            b"JPXDecode",
            b"JBIG2Decode",
            b"CCITTFaxDecode",
        ] {
            assert!(
                matches!(classify(&[filter]), Carrier::Image { .. }),
                "{filter:?} should be an image carrier"
            );
        }
    }

    /// The last filter decides, because it is the one that produces the content.
    ///
    /// `/Filter [/ASCII85Decode /DCTDecode]` is an ASCII-armoured JPEG: the
    /// chain is applied in order, so what comes out at the end is a picture.
    /// Classifying by the *first* would call this scannable and then hand the
    /// byte scan a decoded JPEG to look for words in.
    #[test]
    fn an_armoured_image_is_still_an_image() {
        assert!(matches!(
            classify(&[b"ASCII85Decode", b"DCTDecode"]),
            Carrier::Image { .. }
        ));
    }

    /// Understood by the format, absent from this build's decoder.
    ///
    /// Separated from an image because these carry arbitrary bytes --- text
    /// included --- so a decoder for them *would* answer the byte scan's
    /// question, where a decoder for a JPEG would not. Both withhold
    /// certification; only one of them could stop doing so.
    #[test]
    fn a_filter_we_cannot_decode_is_blind_rather_than_deferred() {
        for filter in [&b"ASCIIHexDecode"[..], b"RunLengthDecode", b"Crypt"] {
            assert!(
                matches!(classify(&[filter]), Carrier::Undecodable { .. }),
                "{filter:?}"
            );
        }
    }

    /// Deny by default, and it names the filter so a reader can go and look.
    #[test]
    fn a_filter_nothing_recognises_is_its_own_answer() {
        let Carrier::Unrecognised { filter } = classify(&[b"SomeVendorDecode"]) else {
            panic!("an unknown filter must not be classified as anything else");
        };
        assert_eq!(filter, "SomeVendorDecode");
    }

    #[test]
    fn a_clean_report_is_the_only_thing_that_verifies() {
        assert_eq!(Report::default().verdict(), Verdict::Verified);
    }

    /// **The keystone.** An image carrier withholds certification.
    ///
    /// This is the whole "split by remedy, not by verdict" claim, and it is the
    /// one that would ship a lie if it went the other way: a scanned document is
    /// nothing *but* image carriers, so a `deferred` list that certified would
    /// hand a reader the word "verified" for a file where nothing read the only
    /// thing in it.
    #[test]
    fn an_image_carrier_does_not_certify() {
        let report = Report {
            deferred: vec!["object 4 is a DCTDecode image".to_string()],
            ..Default::default()
        };
        let Verdict::NotVerified(why) = report.verdict() else {
            panic!("a picture nobody read must not verify");
        };
        assert_eq!(why.len(), 1);
        assert!(why[0].contains("DCTDecode"), "{why:?}");
    }

    /// A needle still present is reported first, because it makes the rest moot.
    #[test]
    fn a_leak_is_reported_before_the_things_that_could_still_be_looked_at() {
        let mut report = Report {
            blind: vec!["object 9 is ASCIIHexDecode".to_string()],
            deferred: vec!["object 4 is a DCTDecode image".to_string()],
            ..Default::default()
        };
        report.found.insert("SECRET".to_string());
        let Verdict::NotVerified(why) = report.verdict() else {
            panic!("a leak must not verify");
        };
        assert_eq!(why.len(), 3);
        assert!(
            why[0].contains("SECRET") && why[0].contains("still in the file"),
            "the leak comes first: {why:?}"
        );
    }

    /// A scan that decoded nothing must never be the reassuring answer.
    ///
    /// **The one claim `docs/PLAN.md` §6 will not let this build make.** `lopdf`
    /// answers `Ok` for a document it could not authenticate, having decoded
    /// none of it --- so every needle is compared against nothing, `found` stays
    /// empty, and without the guard `verdict()` says *Verified* about bytes this
    /// build never read. Measured on the fixture below: one blind entry, which
    /// is the guard's own, so the verdict really was `Verified` before it.
    ///
    /// A report lists at most [`MAX_OBJECT_REASONS`] objects, and counts the rest.
    ///
    /// **The bound exists because the report now crosses a pipe**, and a check
    /// that only asserted the cap would be satisfied by a scan that stopped
    /// looking. So the assertion is in two halves: the list is bounded, *and*
    /// the count in the summary line accounts for every object that was not
    /// listed. A scan that gave up at the cap would fail the second.
    ///
    /// Built rather than taken from `testdata/`: no fixture has a thousand
    /// objects nothing can decode, and one written to have them would be a
    /// fixture for this test alone.
    #[test]
    fn a_report_lists_a_bounded_number_of_objects_and_counts_the_rest() {
        use lopdf::{dictionary, Document, Object, Stream};

        // Two hundred over the cap, so the summary's number is a value only the
        // full walk could produce --- an off-by-one or a cap-and-stop both miss
        // it, and a rounder excess would not.
        const EXTRA: usize = 200;

        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        for _ in 0..super::MAX_OBJECT_REASONS + EXTRA {
            let mut stream = Stream::new(
                dictionary! { "Filter" => "AFilterNothingRecognises" },
                b"..".to_vec(),
            );
            stream.allows_compression = false;
            document.add_object(stream);
        }
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("the fixture must save");

        let report = super::scan(&bytes, &[], None);
        let listed = report
            .blind
            .iter()
            .filter(|reason| reason.starts_with("object "))
            .count();
        assert_eq!(
            listed,
            super::MAX_OBJECT_REASONS,
            "the per-object list is bounded: {} reasons in all",
            report.blind.len()
        );
        assert!(
            report.blind.iter().any(|reason| reason
                == &format!(
                    "{EXTRA} further objects could not be accounted for, and are not listed \
                     individually"
                )),
            "the summary must account for every object the list left out: {:?}",
            report
                .blind
                .iter()
                .filter(|reason| !reason.starts_with("object "))
                .collect::<Vec<_>>()
        );
        // And the verdict is unchanged by the shortening, which is the whole
        // point: a file with a thousand objects nothing can read is not
        // certifiable, however few of them are named.
        assert!(
            matches!(report.verdict(), Verdict::NotVerified(_)),
            "a shortened report is still a refusal"
        );
    }

    /// **Two subjects, because there are two rules.** The encrypted document
    /// parses to **one** object rather than none --- so an `objects.is_empty()`
    /// guard, which is the obvious one to write, would not fire here at all. It
    /// is `is_encrypted()` that answers, and the emptiness rule needs a file of
    /// its own or neither is falsifiable.
    #[test]
    fn a_scan_that_decoded_no_object_is_not_verified() {
        let needles = vec!["a-string-that-is-in-no-document".to_string()];

        let path = std::path::Path::new("../testdata/incr-encrypted-pw.pdf");
        if !path.exists() {
            println!(
                "[SKIP] a_scan_that_decoded_no_object_is_not_verified: generate testdata/ (BUILD.md)"
            );
            return;
        }
        let bytes = std::fs::read(path).expect("read the fixture");

        let locked = super::scan(&bytes, &needles, None);
        assert_eq!(
            locked.objects, 1,
            "the fixture must be one `lopdf` opens without decoding it --- if this ever \
             becomes 0 the emptiness rule below is what catches it, and this test is measuring \
             the wrong thing"
        );
        assert!(
            locked.found.is_empty(),
            "the needle is in no document --- the point is that looking for it succeeded \
             without anything having been looked at"
        );
        let Verdict::NotVerified(why) = locked.verdict() else {
            panic!("a scan that decoded no object must never verify");
        };
        assert!(
            why.iter()
                .any(|reason| reason.contains("no password opened it")),
            "the verdict has to say the file was never decoded: {why:?}"
        );

        // The control for that one. With the key the walk happens, so the guard
        // is about what was read rather than about encryption as such.
        let seeing = super::scan(&bytes, &needles, Some("swordfish"));
        assert!(
            seeing.objects > 1,
            "with the password the scan must reach the object graph, not just its wrapper"
        );
        assert!(
            !seeing
                .blind
                .iter()
                .any(|reason| reason.contains("no password opened it")),
            "a scan that decoded the file must not report it as undecoded: {:?}",
            seeing.blind
        );

        // **The second subject.** A file that parses cleanly and holds nothing:
        // not encrypted, so the rule above cannot reach it. Built by hand for
        // `well_formed`'s reason --- `lopdf` writes at least a catalog for every
        // document it will serialise, so no fixture from the writer has this
        // shape.
        let mut hollow = Vec::new();
        hollow.extend_from_slice(b"%PDF-1.7\n");
        let start = hollow.len();
        hollow.extend_from_slice(b"xref\n0 1\n0000000000 65535 f \n");
        hollow.extend_from_slice(b"trailer\n<</Size 1>>\n");
        hollow.extend_from_slice(format!("startxref\n{start}\n%%EOF\n").as_bytes());
        let empty = super::scan(&hollow, &needles, None);
        assert_eq!(empty.objects, 0, "the hand-built file holds no objects");
        let Verdict::NotVerified(why) = empty.verdict() else {
            panic!("a scan of a document with no objects in it must never verify");
        };
        assert!(
            why.iter()
                .any(|reason| reason.contains("no objects at all")),
            "the emptiness rule has its own reason, and this is the only subject that \
             reaches it: {why:?}"
        );
    }

    /// A document of one content stream per page, serialised as the writer does.
    ///
    /// **Built here rather than taken from `testdata/`**, for the reason
    /// `well_formed` gives and one more: these fixtures differ from each other
    /// by exactly one property --- which page prints a word, whether a form is
    /// shared, whether a link points somewhere --- and a tracked corpus cannot
    /// be varied one property at a time.
    fn document(pages: &[&str]) -> Vec<u8> {
        pieces(pages, &[], false)
    }

    /// The same, with optional extras: a form every page draws, and a link from
    /// page 1 to the last page.
    fn pieces(pages: &[&str], form: &[u8], link: bool) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object, Stream};

        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        // One form object, drawn by every page, so a needle inside it is
        // carried by something no single page owns.
        let shared = (!form.is_empty()).then(|| {
            doc.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                },
                form.to_vec(),
            ))
        });
        let mut kids: Vec<Object> = Vec::new();
        let mut ids: Vec<lopdf::ObjectId> = Vec::new();
        for text in pages {
            let content = doc.add_object(Stream::new(
                dictionary! {},
                format!("BT /F1 12 Tf 72 700 Td ({text}) Tj ET").into_bytes(),
            ));
            let mut page = dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Contents" => content,
            };
            if let Some(form) = shared {
                page.set(
                    "Resources",
                    dictionary! { "XObject" => dictionary! { "X1" => form } },
                );
            }
            let id = doc.add_object(page);
            ids.push(id);
            kids.push(Object::Reference(id));
        }
        // A `GoTo` from page one to the last page. Following `/A` or `/D` would
        // walk page one's attribution into that page's content; this is the
        // control that says it does not.
        if link {
            if let (Some(first), Some(last)) = (ids.first().copied(), ids.last().copied()) {
                let annot = doc.add_object(dictionary! {
                    "Type" => "Annot",
                    "Subtype" => "Link",
                    "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                    "P" => first,
                    "A" => dictionary! {
                        "S" => "GoTo",
                        "D" => vec![Object::Reference(last), "Fit".into()],
                    },
                });
                if let Ok(Object::Dictionary(page)) = doc.get_object_mut(first) {
                    page.set("Annots", vec![Object::Reference(annot)]);
                }
            }
        }
        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => count,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise the fixture");
        bytes
    }

    /// The reason a report gave for one needle, so a test asserts the sentence.
    fn reason(report: &Report, needle: &str) -> String {
        let Verdict::NotVerified(why) = report.verdict() else {
            panic!("a report that found {needle} must never verify");
        };
        why.into_iter()
            .find(|one| one.starts_with(needle))
            .unwrap_or_else(|| panic!("no reason named {needle}"))
    }

    /// **The measurement the 2026-09-20 redaction narrowing rests on, and there
    /// is no inserted page anywhere in it.**
    ///
    /// `Refusal::RedactionBesideImportedPages` refused a redaction for the whole
    /// document while any page came from another file, and one of the four
    /// reasons it gave was this scan: *"the scan in particular reads the whole
    /// output, so imported text that happens to repeat a removed word reads as
    /// a leak"*. Every word of that is true and none of it is about imports.
    /// This scans a two-page document that never left the opened file --- the
    /// word is gone from page 1 and still printed on page 2 --- and the answer
    /// is the same *still in the file*.
    ///
    /// So the ambiguity is a property of a whole-file scan and is already
    /// shipped: an inserted page is exactly as opaque to it as page two, which
    /// makes it a reason to say so in the report
    /// ([`crate::redact::inserted_pages_note`]) and not a reason to refuse.
    ///
    /// ⚠ **The sentence it ends on changed on 2026-09-21, and the test's old
    /// last assertion said the opposite of what is now true.** It read *"the
    /// reason names the word, not the page --- which is exactly what it cannot
    /// say"*, and the walk this file gained makes it say it: the answer here is
    /// [`Located::Pages`] naming page two. What has **not** changed is the
    /// verdict, which is the half the redaction narrowing actually rests on ---
    /// a word still in the file is still *not verified*, wherever it is.
    ///
    /// The two needles are the control pair: one the writer put on the second
    /// page only, one on neither. A scan that "found" both, or neither, would
    /// pass an assertion about only the first.
    #[test]
    fn a_needle_on_another_page_reads_as_still_in_the_file() {
        const REPEATED: &str = "SECRET-4711";
        const ABSENT: &str = "NOT-IN-THIS-FILE-AT-ALL";

        let bytes = document(&["this page was redacted", REPEATED]);
        let needles = vec![REPEATED.to_string(), ABSENT.to_string()];
        let report = super::scan(&bytes, &needles, None);
        assert!(
            report.found.contains(REPEATED),
            "a word still printed on another page of the same document is reported as still \
             in the file: {:?}",
            report.found
        );
        assert!(
            !report.found.contains(ABSENT),
            "and the control says the scan is discriminating rather than agreeable: {:?}",
            report.found
        );
        assert!(
            matches!(report.verdict(), Verdict::NotVerified(_)),
            "a report that found a needle must never verify, however well it is placed"
        );
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Pages(super::Placed {
                pages: vec![1],
                more: 0
            })),
            "the walk places it on the page that prints it, and on no other"
        );
        assert_eq!(
            reason(&report, REPEATED),
            format!("{REPEATED} is still in the file, on page 2"),
            "and the reason a reader is shown names that page"
        );
        assert!(
            !report.located.contains_key(ABSENT),
            "nothing is placed for a word that was never found"
        );
    }

    /// The word is on the page the reader marked, which is the loud failure.
    ///
    /// **The control that makes the test above mean something.** Both fixtures
    /// are two pages and one needle; only *which* page prints it differs, and
    /// the two answers have to differ with it. A walk that attributed
    /// everything to page one would pass this and fail that, and one that
    /// followed `/Parent` up to `/Pages` and back down through `/Kids` --- the
    /// route [`NOT_CONTENT`] exists to cut --- would answer *both pages* here
    /// and fail neither assertion about *a* page.
    #[test]
    fn a_needle_on_the_page_it_was_removed_from_is_placed_there() {
        const REPEATED: &str = "SECRET-4711";

        let bytes = document(&[REPEATED, "this page was never marked"]);
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Pages(super::Placed {
                pages: vec![0],
                more: 0
            })),
            "page one, and not both pages --- the page tree is not a route between them"
        );
        assert_eq!(
            reason(&report, REPEATED),
            format!("{REPEATED} is still in the file, on page 1")
        );
    }

    /// A word on two pages is placed on both, each by its own carrier.
    ///
    /// Distinct from [`Located::Shared`], which is one carrier that two pages
    /// draw. Here there are two carriers and each is owned, so the answer is
    /// specific --- and the sentence has to read as a list rather than as a
    /// singular page.
    #[test]
    fn a_needle_on_two_pages_is_placed_on_both() {
        const REPEATED: &str = "SECRET-4711";

        let bytes = document(&[REPEATED, "clean", REPEATED]);
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Pages(super::Placed {
                pages: vec![0, 2],
                more: 0
            }))
        );
        assert_eq!(
            reason(&report, REPEATED),
            format!("{REPEATED} is still in the file, on pages 1 and 3")
        );
    }

    /// **One object that two pages draw has no page, and says so.**
    ///
    /// The middle of the three answers, and the one a two-valued version would
    /// get wrong: a shared form reached by page one and page two would be
    /// reported as *on page one* by anything that took the first answer it
    /// found, and a reader comparing that against the page they marked would
    /// act on a fabrication. The verdict is unchanged --- the word is still in
    /// the file --- and the sentence withholds the page instead of inventing
    /// one.
    #[test]
    fn a_needle_in_something_two_pages_draw_is_not_placed_on_either() {
        const REPEATED: &str = "SECRET-4711";

        let form = format!("BT /F1 12 Tf 10 10 Td ({REPEATED}) Tj ET").into_bytes();
        let bytes = pieces(&["one", "two"], &form, false);
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert!(
            report.found.contains(REPEATED),
            "the scan reads inside the form, which is the precondition for the rest"
        );
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Shared(super::Placed {
                pages: vec![0, 1],
                more: 0
            })),
            "shared, and naming the pages that share it"
        );
        assert_eq!(
            reason(&report, REPEATED),
            format!(
                "{REPEATED} is still in the file, carried by something more than one page \
                 draws (pages 1 and 2), so which page it is on could not be established"
            )
        );
        assert!(
            !report.placed(),
            "and a caller must not compare this against the pages a reader marked"
        );
    }

    /// A link is not a route: following `/A` would put page three's words on page one.
    ///
    /// **The [`NOT_CONTENT`] list's own control, and the only one of its
    /// entries a plain two-page fixture does not already exercise.** A `GoTo`
    /// action's destination array names another page's object, so a walk that
    /// followed it would reach that page's content stream from page one --- and
    /// the needle would come back [`Located::Shared`] between pages one and
    /// three, which is a *softer* wrong answer than a wrong page and still a
    /// wrong answer. The assertion is therefore the exact `Pages([2])`, not
    /// merely that page three is in the set.
    #[test]
    fn a_link_to_another_page_does_not_reach_that_page_s_words() {
        const REPEATED: &str = "SECRET-4711";

        let bytes = pieces(&["one", "two", REPEATED], &[], true);
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Pages(super::Placed {
                pages: vec![2],
                more: 0
            })),
            "the link's own page reaches the annotation, and stops there"
        );
    }

    /// Bytes past the last `%%EOF` belong to no object, so they belong to no page.
    ///
    /// **The third answer, and the one no walk will ever improve on.** The byte
    /// scan finds the word and the graph walk never accounts for it, which is
    /// the module's opening paragraph arriving as an attribution: there is no
    /// page to name because there is no object.
    #[test]
    fn a_needle_no_object_carries_is_not_placed() {
        const REPEATED: &str = "SECRET-4711";

        let mut bytes = document(&["clean", "also clean"]);
        bytes.extend_from_slice(REPEATED.as_bytes());
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert!(report.found.contains(REPEATED), "the byte scan sees it");
        assert_eq!(report.located.get(REPEATED), Some(&Located::Unplaced));
        assert_eq!(
            reason(&report, REPEATED),
            format!(
                "{REPEATED} is still in the file, in something no page of it reaches, so which \
                 page it is on could not be established"
            )
        );
    }

    /// The same answer for an object the page tree does not reach at all.
    ///
    /// A different subject from the one above and it needs its own fixture: the
    /// word here *is* in an object, and the object is the document's own
    /// `/Info` --- which `save::apply_redactions` scrubs precisely because a
    /// redacted word can sit in it. A walk keyed on pages cannot place it, and
    /// the honest answer is the same one trailing bytes get.
    #[test]
    fn a_needle_in_the_file_s_own_metadata_is_not_placed() {
        use lopdf::{dictionary, Document, Object};

        const REPEATED: &str = "SECRET-4711";

        // Rebuilt through `lopdf` rather than patched into the bytes, so the
        // `/Info` really is an object in the graph rather than loose text.
        let mut doc =
            Document::load_mem(&document(&["clean", "also clean"])).expect("the fixture loads");
        let info = doc.add_object(Object::Dictionary(dictionary! {
            "Title" => Object::string_literal(REPEATED),
        }));
        doc.trailer.set("Info", info);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise");

        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert!(report.found.contains(REPEATED));
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Unplaced),
            "an object no page reaches cannot be attributed to a page"
        );
    }

    /// More than one revision withholds every answer, not just the ambiguous one.
    ///
    /// **This module's opening paragraph as a guard.** An object a later
    /// revision overwrote sits at its old offset addressable by nothing: the
    /// graph walk cannot see it, and nor can the byte scan if it is compressed.
    /// So the live graph can honestly answer *page two* for a file that also
    /// holds a dead copy of the same word --- an answer about the graph offered
    /// as an answer about the file. The needle is still reported; only the
    /// place is withheld.
    #[test]
    fn a_file_with_more_than_one_revision_places_nothing() {
        const REPEATED: &str = "SECRET-4711";

        let one = document(&["clean", REPEATED]);
        let report = super::scan(&one, &[REPEATED.to_string()], None);
        assert!(
            report.located.contains_key(REPEATED),
            "the control: one revision, and the walk answers"
        );

        let mut two = one.clone();
        two.extend_from_slice(&one);
        let report = super::scan(&two, &[REPEATED.to_string()], None);
        assert!(report.eofs > 1, "the fixture really does hold two of them");
        assert!(report.found.contains(REPEATED), "the finding survives");
        assert!(
            report.located.is_empty(),
            "and nothing is placed: {:?}",
            report.located
        );
        assert_eq!(
            reason(&report, REPEATED),
            format!("{REPEATED} is still in the file"),
            "the sentence falls back to the one it gave before attribution existed"
        );
    }

    /// A word in more objects than [`MAX_CARRIERS`] is not *on* a page.
    ///
    /// The list stops being an answer long before it stops being affordable, so
    /// this is not a performance bound. Two hundred over the cap, for the reason
    /// [`MAX_OBJECT_REASONS`]'s test gives: a round excess would not tell an
    /// off-by-one from a cap-and-stop.
    #[test]
    fn a_needle_in_more_objects_than_the_cap_is_not_placed() {
        use lopdf::{Document, Object};

        const REPEATED: &str = "SECRET-4711";
        const EXTRA: usize = 200;

        let mut doc = Document::load_mem(&document(&["clean"])).expect("the fixture loads");
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let mut carried: Vec<Object> = Vec::new();
        for _ in 0..super::MAX_CARRIERS + EXTRA {
            carried.push(Object::Reference(
                doc.add_object(Object::string_literal(REPEATED)),
            ));
        }
        // Hung off the page so every one of them is reachable: the point is the
        // number of carriers, not whether they could have been placed.
        if let Ok(Object::Dictionary(page)) = doc.get_object_mut(page) {
            page.set("Resources", carried);
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("serialise");

        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        assert!(report.found.contains(REPEATED));
        assert_eq!(
            report.located.get(REPEATED),
            Some(&Located::Unplaced),
            "a thousand carriers is not a location"
        );
    }

    /// The page list is shortened and the count survives it.
    ///
    /// [`MAX_OBJECT_REASONS`]'s rule in the place the same pressure shows up.
    /// Asserted in two halves for that test's reason: the list is bounded *and*
    /// the remainder accounts for every page it left out, so a walk that gave
    /// up at the cap fails the second.
    #[test]
    fn a_placed_needle_names_a_bounded_number_of_pages_and_counts_the_rest() {
        const REPEATED: &str = "SECRET-4711";
        const EXTRA: usize = 3;

        let pages = vec![REPEATED; super::MAX_LOCATED_PAGES + EXTRA];
        let bytes = document(&pages);
        let report = super::scan(&bytes, &[REPEATED.to_string()], None);
        let Some(Located::Pages(placed)) = report.located.get(REPEATED) else {
            panic!("every page owns its own carrier, so this is placed: {report:?}");
        };
        assert_eq!(placed.pages.len(), super::MAX_LOCATED_PAGES);
        assert_eq!(placed.more, EXTRA);
        assert!(
            placed.sentence().ends_with(", and 3 more"),
            "{}",
            placed.sentence()
        );
    }

    /// A chain of references deeper than [`MAX_REACH_DEPTH`] stops the walk.
    ///
    /// **Called against [`reach`] rather than through [`scan`], and that is what
    /// makes the three bound tests affordable.** Each of them needs a document
    /// at the bound, and serialising a hundred thousand objects to exercise a
    /// limit on how many are *visited* would spend the whole cost on the one
    /// step that is not the subject. The `None`/`Some` pair is the control:
    /// without the second, a `reach` that returned `None` for everything would
    /// pass all three.
    #[test]
    fn a_reference_chain_past_the_depth_bound_stops_the_walk() {
        assert!(
            super::reach(&chain(super::MAX_REACH_DEPTH - 2)).is_some(),
            "a chain inside the bound is walked"
        );
        assert!(
            super::reach(&chain(super::MAX_REACH_DEPTH + 2)).is_none(),
            "and one past it withholds every answer, not just its own"
        );
    }

    /// A page reaching more objects than [`MAX_REACH_OBJECTS`] stops the walk.
    #[test]
    fn a_page_reaching_past_the_object_bound_stops_the_walk() {
        assert!(
            super::reach(&fan(super::MAX_REACH_OBJECTS - 10)).is_some(),
            "a page inside the bound is walked"
        );
        assert!(super::reach(&fan(super::MAX_REACH_OBJECTS + 10)).is_none());
    }

    /// Work past [`MAX_REACH_STEPS`] stops the walk, however it was spent.
    ///
    /// **Direct objects, which is the half the per-page object bound cannot
    /// reach.** A hostile array of two million integers costs two million steps
    /// and one object, so a walk bounded only by objects visited would grind
    /// through it. The bound is on the work rather than on the graph.
    ///
    /// See [`heap`] for why its page has no content stream: with one, this test
    /// passed for the wrong reason and the mutation that deletes the guard it
    /// is named for survived.
    #[test]
    fn work_past_the_step_bound_stops_the_walk() {
        assert!(
            super::reach(&heap(super::MAX_REACH_STEPS / 2)).is_some(),
            "half the budget is spent and answered"
        );
        assert!(super::reach(&heap(super::MAX_REACH_STEPS + 10)).is_none());
    }

    /// One page whose `/Resources` is a chain of `deep` references.
    fn chain(deep: usize) -> lopdf::Document {
        use lopdf::{dictionary, Document, Object};

        let mut doc = Document::load_mem(&document(&["clean"])).expect("the fixture loads");
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let mut last = doc.add_object(Object::Null);
        for _ in 0..deep {
            last = doc.add_object(dictionary! { "Link" => last });
        }
        if let Ok(Object::Dictionary(page)) = doc.get_object_mut(page) {
            page.set("Resources", last);
        }
        doc
    }

    /// One page whose `/Resources` references `wide` separate objects.
    fn fan(wide: usize) -> lopdf::Document {
        use lopdf::{Document, Object};

        let mut doc = Document::load_mem(&document(&["clean"])).expect("the fixture loads");
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let spread: Vec<Object> = (0..wide)
            .map(|_| Object::Reference(doc.add_object(Object::Null)))
            .collect();
        if let Ok(Object::Dictionary(page)) = doc.get_object_mut(page) {
            page.set("Resources", spread);
        }
        doc
    }

    /// One page whose `/Resources` is a direct array of `many` integers, and
    /// which references **nothing** --- not even its own content stream.
    ///
    /// ⚠ **The `/Contents` had to go, and finding out why is the interesting
    /// part.** There are two step checks: one in [`push_refs`], which stops a
    /// single object from burning the whole budget, and one in [`reach`]'s own
    /// loop. They share a counter, so with a content stream still on the page
    /// the stack was not empty when `push_refs` returned --- `reach` popped it,
    /// incremented the shared counter past the bound and answered `None`
    /// anyway. The mutation that deletes the inner check therefore **survived**
    /// on the first run of it: the walk still truncated, for the other reason.
    ///
    /// A guard that cannot be shown to fire is not a guard, and the failure
    /// here is not hypothetical --- it is exactly what an object holding a
    /// billion direct values would exploit, spending a billion steps before the
    /// outer check gets a turn. Stripping every reference from the page is what
    /// leaves the inner check as the only thing standing between the fixture
    /// and an answer.
    fn heap(many: usize) -> lopdf::Document {
        use lopdf::{Document, Object};

        let mut doc = Document::load_mem(&document(&["clean"])).expect("the fixture loads");
        let page = crate::pagetree::ordered_pages(&doc)[0];
        if let Ok(Object::Dictionary(page)) = doc.get_object_mut(page) {
            page.remove(b"Contents");
            page.set("Resources", vec![Object::Integer(0); many]);
        }
        doc
    }
}
