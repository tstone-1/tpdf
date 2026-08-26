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

use lopdf::content::Content;
use lopdf::{Document, ObjectId};

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
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[must_use]
fn overlaps(a: Rect, b: Rect) -> bool {
    let a = normalised(a);
    let b = normalised(b);
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// What a removal did, so a caller can report it rather than assume it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// How many show operators the page had before.
    pub shows_before: usize,
    /// How many were removed.
    pub removed: usize,
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
    for at in positions.into_iter().rev() {
        content.operations.remove(at);
    }

    let encoded = content
        .encode()
        .map_err(|why| format!("the rewritten content stream will not encode: {why}"))?;
    doc.change_page_content(page, encoded)
        .map_err(|why| format!("the page's content could not be replaced: {why}"))?;

    Ok(Removed {
        shows_before: shows.len(),
        removed,
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

#[cfg(test)]
mod tests {
    use super::{covered, is_show, overlaps, remove_shows, PageObject, Plan, Unhandled};
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
}
