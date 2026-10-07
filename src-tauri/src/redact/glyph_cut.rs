//! Taking part of a show operator: the glyphs under a region, and no others.
//!
//! `redact.rs` removes the whole show operator that drew any glyph under a
//! region, which is `docs/PLAN.md` §6's route B. It is safe and it is blunt: a
//! producer that writes a line as one string loses the line when a reader
//! marks an account number in it. Reported from use on 2026-10-07, on a phone
//! company's invoice.
//!
//! This is route A for the case it can be proved in. The glyphs under the
//! region are cut out of the string, and a gap exactly as wide as they were is
//! put in their place, so every glyph that stays is drawn where it was.
//!
//! ## Nothing here knows a font's widths
//!
//! The width of what goes is the distance between two pen positions, and
//! PDFium reports the pen for every character (`objects::TextGlyphs`). So no
//! width table is read, no encoding is decoded, and character spacing, word
//! spacing and kerning are in the measurement without being named.
//!
//! ## When a show is cut, and when it still goes whole
//!
//! A show is cut only when **every** one of these holds, and goes whole
//! otherwise, which is what happened to all of them before:
//!
//! * PDFium placed its characters along one baseline, running forwards.
//! * The glyphs PDFium placed are as many as the codes in the string. PDFium
//!   never makes more glyphs than codes, so equal counts mean one glyph per
//!   code, in order.
//! * The font's codes have a known length: one byte, or two under
//!   `Identity-H`.
//! * Every glyph with no ink is white space. A glyph with no box cannot be
//!   said to be outside the region.
//! * When the last glyph goes and the next show starts where this one's pen
//!   stopped, PDFium placed that next show.

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, ObjectId};

use super::{
    clear_shadow_text, clear_struct_shadow_text, decode_whole, makes_text_object,
    replace_page_content, Plan, Rect, Removed, MAX_CONTENT_BYTES,
};
use crate::objects::{Glyph, TextGlyphs};

/// How much of a glyph's box a region has to cover for the glyph to go.
///
/// A tenth. A region that grazes the top of the line below it covers a few
/// percent of each glyph there; those glyphs stay, nearly all of each still
/// visible beside the fill. Before this rule the whole line went.
pub const UNDER: f32 = 0.1;

/// Part of one show operator to take out.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShowCut {
    /// Which of the page's text objects, by [`Plan::shows`]'s ordinals.
    pub ordinal: usize,
    /// The pen position of every glyph of the object, in thousandths of the
    /// font size. Its length is the guard: the writer refuses a string that
    /// does not hold this many codes.
    pub pens: Vec<f32>,
    /// Where the next text object starts, in the same unit.
    /// `objects::TextGlyphs::tail`, carried through.
    pub tail: Option<f32>,
    /// Which glyphs go, ascending.
    pub take: Vec<usize>,
}

/// What `lopdf` can say about one show operator that PDFium made text of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowFacts {
    /// How many codes its string holds.
    pub codes: usize,
    /// Whether the next show starts where this one's pen stops, with nothing
    /// in between that places it.
    pub carries_on: bool,
}

/// Which glyphs a region takes, or `None` when that cannot be said.
///
/// A glyph with ink goes when the region covers more than [`UNDER`] of its
/// box. A glyph with none --- a space --- goes when the inked glyphs on both
/// sides of it go, so the space inside a marked phrase goes with the phrase
/// and the space before it stays with the word before it.
fn taken(glyphs: &[Glyph], region: Rect) -> Option<Vec<usize>> {
    let region = [
        region[0].min(region[2]),
        region[1].min(region[3]),
        region[0].max(region[2]),
        region[1].max(region[3]),
    ];
    let mut under: Vec<Option<bool>> = Vec::with_capacity(glyphs.len());
    for glyph in glyphs {
        let inked = glyph
            .bounds
            .filter(|b| b.iter().all(|v| v.is_finite()) && b[2] > b[0] && b[3] > b[1]);
        match inked {
            Some(b) => {
                let wide = (b[2].min(region[2]) - b[0].max(region[0])).max(0.0);
                let high = (b[3].min(region[3]) - b[1].max(region[1])).max(0.0);
                let share = (wide * high) / ((b[2] - b[0]) * (b[3] - b[1]));
                under.push(Some(share > UNDER));
            }
            None if glyph.draws.chars().all(char::is_whitespace) => under.push(None),
            None => return None,
        }
    }
    let mut take = Vec::new();
    for (at, one) in under.iter().enumerate() {
        let goes = match one {
            Some(goes) => *goes,
            None => {
                let before = under[..at].iter().rev().find_map(|x| *x);
                let after = under[at + 1..].iter().find_map(|x| *x);
                match (before, after) {
                    (Some(a), Some(b)) => a && b,
                    (Some(one), None) | (None, Some(one)) => one,
                    (None, None) => false,
                }
            }
        };
        if goes {
            take.push(at);
        }
    }
    Some(take)
}

/// Turns the whole-show removals of a plan into cuts where a cut can be
/// proved, and returns what each cut takes, by ordinal.
///
/// A show the region covers all of stays in [`Plan::shows`]. One whose glyphs
/// are all outside the region leaves the plan: its box overlaps the region and
/// none of its ink does. Everything this cannot describe stays in
/// [`Plan::shows`] and goes whole, as before.
///
/// `glyphs` and `facts` are by text-object ordinal. A `facts` of `None` ---
/// the content could not be read, or its shows are not as many as PDFium's
/// text objects --- cuts nothing.
pub fn cut_within(
    plan: &mut Plan,
    glyphs: &[Option<TextGlyphs>],
    facts: Option<&[Option<ShowFacts>]>,
    region: Rect,
) -> Vec<(usize, String)> {
    let Some(facts) = facts else {
        return Vec::new();
    };
    let mut said = Vec::new();
    let mut whole = Vec::new();
    for ordinal in std::mem::take(&mut plan.shows) {
        let known = glyphs
            .get(ordinal)
            .and_then(Option::as_ref)
            .zip(facts.get(ordinal).copied().flatten())
            .filter(|(text, fact)| fact.codes == text.glyphs.len());
        let Some((text, fact)) = known else {
            whole.push(ordinal);
            continue;
        };
        let Some(take) = taken(&text.glyphs, region) else {
            whole.push(ordinal);
            continue;
        };
        if take.is_empty() {
            continue;
        }
        let to_the_end = take.last() == Some(&(text.glyphs.len() - 1));
        if take.len() == text.glyphs.len() || (to_the_end && fact.carries_on && text.tail.is_none())
        {
            whole.push(ordinal);
            continue;
        }
        said.push((
            ordinal,
            take.iter()
                .map(|at| text.glyphs[*at].draws.as_str())
                .collect(),
        ));
        plan.show_cuts.push(ShowCut {
            ordinal,
            pens: text.glyphs.iter().map(|glyph| glyph.pen).collect(),
            tail: text.tail,
            take,
        });
    }
    plan.shows = whole;
    said
}

/// Merges the cuts several regions make on one page.
///
/// Two regions over one line each cut the same show, and the show loses both
/// parts. One that another region takes whole (`shows`, sorted) is dropped,
/// and one whose parts add up to all of it joins `shows`.
pub fn merge_cuts(cuts: Vec<ShowCut>, shows: &mut Vec<usize>) -> Vec<ShowCut> {
    let mut merged: Vec<ShowCut> = Vec::new();
    for cut in cuts {
        if shows.binary_search(&cut.ordinal).is_ok() {
            continue;
        }
        match merged.iter_mut().find(|have| have.ordinal == cut.ordinal) {
            Some(have) => {
                have.take.extend(cut.take);
                have.take.sort_unstable();
                have.take.dedup();
            }
            None => merged.push(cut),
        }
    }
    merged.retain(|cut| {
        let all = cut.take.len() >= cut.pens.len();
        if all {
            shows.push(cut.ordinal);
        }
        !all
    });
    // Sorted again and not de-duplicated: an ordinal that joins here was not
    // in `shows`, or its cut would have been dropped above.
    shows.sort_unstable();
    merged.sort_by_key(|cut| cut.ordinal);
    merged
}

/// How many bytes one code of a font is, or `None` when this does not know.
///
/// One for a simple font. Two for a composite font under `Identity-H`, which
/// is what nearly every producer embeds a subset with. Any other CMap can mix
/// code lengths, and `Identity-V` writes down the page.
///
/// The page's own `/Resources` and no ancestor's once it has one, which is
/// what a renderer does (`print.rs` records what the merged lookup cost).
fn code_length(doc: &Document, page: ObjectId, font: &[u8]) -> Option<usize> {
    let (own, inherited) = doc.get_page_resources(page).ok()?;
    let resources = match own {
        Some(own) => own,
        None => doc.get_dictionary(*inherited.first()?).ok()?,
    };
    fn dict<'a>(doc: &'a Document, value: &'a Object) -> Option<&'a lopdf::Dictionary> {
        doc.dereference(value)
            .ok()
            .and_then(|(_, value)| value.as_dict().ok())
    }
    let fonts = dict(doc, resources.get(b"Font").ok()?)?;
    let font = dict(doc, fonts.get(font).ok()?)?;
    match font.get(b"Subtype").and_then(Object::as_name).ok()? {
        b"Type0" => match font.get(b"Encoding").ok()? {
            Object::Name(name) if name == b"Identity-H" => Some(2),
            _ => None,
        },
        b"Type1" | b"TrueType" | b"MMType1" | b"Type3" => Some(1),
        _ => None,
    }
}

/// The code length in force at each operation, `None` where no font is set or
/// its code length is not known.
///
/// The font is part of the graphics state, so `q` and `Q` save and restore it.
fn code_lengths(doc: &Document, page: ObjectId, operations: &[Operation]) -> Vec<Option<usize>> {
    let mut now: Option<usize> = None;
    let mut saved: Vec<Option<usize>> = Vec::new();
    operations
        .iter()
        .map(|operation| {
            match operation.operator.as_str() {
                "q" => saved.push(now),
                "Q" => now = saved.pop().flatten(),
                "Tf" => {
                    now = match operation.operands.first() {
                        Some(Object::Name(name)) => code_length(doc, page, name),
                        _ => None,
                    }
                }
                _ => {}
            }
            now
        })
        .collect()
}

/// How many codes a show's strings hold, at `length` bytes a code.
fn codes_in(operation: &Operation, length: usize) -> Option<usize> {
    let of = |object: &Object| match object {
        Object::String(bytes, _) if bytes.len() % length == 0 => Some(bytes.len() / length),
        _ => None,
    };
    match (operation.operator.as_str(), operation.operands.last()?) {
        ("TJ", Object::Array(parts)) => parts
            .iter()
            .filter(|part| matches!(part, Object::String(..)))
            .map(of)
            .sum(),
        ("Tj" | "'" | "\"", last) => of(last),
        _ => None,
    }
}

/// Whether the show after `at` starts where this one's pen stops.
///
/// `Td`, `TD`, `Tm` and `T*` place the next show from the start of the line,
/// not from the pen, and so do `'` and `"`. Only a `Tj` or a `TJ` reached
/// before any of them starts at the pen.
fn carries_on(operations: &[Operation], at: usize) -> bool {
    for operation in &operations[at + 1..] {
        match operation.operator.as_str() {
            "Tj" | "TJ" if makes_text_object(operation) => return true,
            "Td" | "TD" | "Tm" | "T*" | "'" | "\"" | "ET" | "BT" => return false,
            _ => {}
        }
    }
    false
}

/// What `lopdf` can say about each show PDFium made text of, by ordinal, or
/// `None` when the page's shows are not `expected` many or cannot be read.
///
/// An entry is `None` for a show whose font's code length is not known or
/// whose strings do not divide by it.
#[must_use]
pub fn show_facts(
    doc: &Document,
    page: ObjectId,
    expected: usize,
) -> Option<Vec<Option<ShowFacts>>> {
    let data = doc
        .get_page_content_with_limit(page, MAX_CONTENT_BYTES)
        .ok()?;
    let operations = Content::decode(&data).ok()?.operations;
    let lengths = code_lengths(doc, page, &operations);
    let facts: Vec<Option<ShowFacts>> = operations
        .iter()
        .enumerate()
        .filter(|(_, operation)| makes_text_object(operation))
        .map(|(at, operation)| {
            let codes = codes_in(operation, lengths[at]?)?;
            Some(ShowFacts {
                codes,
                carries_on: carries_on(&operations, at),
            })
        })
        .collect();
    (facts.len() == expected).then_some(facts)
}

/// One show with the glyphs of `cut` taken out, as the operations that replace
/// it.
///
/// Always a `TJ`: the strings that stay, and between them a number for each
/// run that went. A `'` or a `"` also moves to the next line and sets spacing,
/// and those come first as the operators they stand for.
fn cut_one(
    operation: &Operation,
    length: usize,
    cut: &ShowCut,
    continues: bool,
) -> Result<Vec<Operation>, String> {
    let wrong = || {
        "a line of text is not shaped the way it was when it was measured, so nothing was \
         removed"
            .to_string()
    };
    let mut out: Vec<Operation> = Vec::new();
    let parts: Vec<Object> = match (operation.operator.as_str(), &operation.operands[..]) {
        ("Tj", [text]) => vec![text.clone()],
        ("'", [text]) => {
            out.push(Operation::new("T*", Vec::new()));
            vec![text.clone()]
        }
        ("\"", [word, character, text]) => {
            out.push(Operation::new("Tw", vec![word.clone()]));
            out.push(Operation::new("Tc", vec![character.clone()]));
            out.push(Operation::new("T*", Vec::new()));
            vec![text.clone()]
        }
        ("TJ", [Object::Array(parts)]) => parts.clone(),
        _ => return Err(wrong()),
    };

    let mut shown: Vec<Object> = Vec::new();
    // The glyph a run that is being taken started at.
    let mut open: Option<usize> = None;
    let mut glyph = 0usize;
    for part in parts {
        match part {
            Object::String(bytes, format) => {
                if bytes.len() % length != 0 {
                    return Err(wrong());
                }
                let mut kept: Vec<u8> = Vec::new();
                for code in bytes.chunks(length) {
                    let pen = *cut.pens.get(glyph).ok_or_else(wrong)?;
                    if cut.take.binary_search(&glyph).is_ok() {
                        if open.is_none() {
                            open = Some(glyph);
                            if !kept.is_empty() {
                                shown.push(Object::String(std::mem::take(&mut kept), format));
                            }
                        }
                    } else {
                        if let Some(from) = open.take() {
                            // A `TJ` number moves the pen back by that many
                            // thousandths, so a gap forwards is negative.
                            shown.push(Object::Real(cut.pens[from] - pen));
                        }
                        kept.extend_from_slice(code);
                    }
                    glyph += 1;
                }
                if !kept.is_empty() {
                    shown.push(Object::String(kept, format));
                }
            }
            // A number inside a run that goes is part of the distance between
            // the two pens, which the gap already is.
            number @ (Object::Integer(_) | Object::Real(_)) => {
                if open.is_none() {
                    shown.push(number);
                }
            }
            _ => return Err(wrong()),
        }
    }
    if glyph != cut.pens.len() {
        return Err(wrong());
    }
    if let Some(from) = open {
        // The run reached the end. What follows needs the pen only when it
        // starts there.
        if continues {
            let tail = cut.tail.ok_or_else(wrong)?;
            shown.push(Object::Real(cut.pens[from] - tail));
        }
    }
    out.push(Operation::new("TJ", vec![Object::Array(shown)]));
    Ok(out)
}

/// Takes the glyphs each cut names out of its show operator.
///
/// The shows a page has, and which of them PDFium made text of, do not change:
/// a cut always leaves a glyph. So this runs before [`super::remove_shows`]
/// and the ordinals both are given mean the same operators.
///
/// The alternate text of every marked-content span around a cut show is
/// cleared, as it is around a removed one: it restates the words that went.
///
/// # Errors
///
/// What [`super::remove_shows`] refuses, and a show that no longer holds the
/// codes it was measured with. Nothing is written when it refuses.
pub fn cut_shows(
    doc: &mut Document,
    page: ObjectId,
    cuts: &[ShowCut],
    text_objects: usize,
) -> Result<Removed, String> {
    let data = doc
        .get_page_content_with_limit(page, MAX_CONTENT_BYTES)
        .map_err(|why| format!("the page's content stream could not be read: {why}"))?;
    let mut content = decode_whole(&data, "the page's content stream")?;
    let shows: Vec<usize> = content
        .operations
        .iter()
        .enumerate()
        .filter(|(_, operation)| makes_text_object(operation))
        .map(|(at, _)| at)
        .collect();
    if shows.len() != text_objects {
        return Err(format!(
            "the page has {} text-showing operator(s) and PDFium reported {text_objects} text \
             object(s). Cutting by position needs those to agree, so nothing was removed.",
            shows.len()
        ));
    }
    let lengths = code_lengths(doc, page, &content.operations);

    let mut work: Vec<(usize, &ShowCut)> = Vec::with_capacity(cuts.len());
    for cut in cuts {
        let at = *shows.get(cut.ordinal).ok_or_else(|| {
            format!(
                "there is no show operator {} on this page, which has {}",
                cut.ordinal,
                shows.len()
            )
        })?;
        if work.iter().any(|(have, _)| *have == at) {
            return Err(format!(
                "show operator {} is cut twice, so nothing was removed",
                cut.ordinal
            ));
        }
        work.push((at, cut));
    }
    work.sort_by_key(|(at, _)| *at);
    let positions: Vec<usize> = work.iter().map(|(at, _)| *at).collect();

    // Before the rewrite, for `remove_shows`'s reason: a span is addressed by
    // where its `BDC` sits, and a `'` that becomes two operations moves every
    // one after it.
    let carriers = clear_shadow_text(doc, page, &mut content.operations, &positions)?;

    // Backwards, so a show that becomes several operations does not move one
    // still to be cut.
    for (at, cut) in work.into_iter().rev() {
        let length = lengths[at].ok_or_else(|| {
            "a line of text is in a font whose codes this cannot count, so nothing was removed"
                .to_string()
        })?;
        let continues = carries_on(&content.operations, at);
        let replacement = cut_one(&content.operations[at], length, cut, continues)?;
        content.operations.splice(at..=at, replacement);
    }

    let struct_carriers = clear_struct_shadow_text(doc, page, &carriers.mcids);
    let encoded = content
        .encode()
        .map_err(|why| format!("the rewritten content stream will not encode: {why}"))?;
    replace_page_content(doc, page, encoded)?;
    Ok(Removed {
        shows_before: shows.len(),
        removed: cuts.len(),
        carriers: carriers.keys,
        struct_carriers,
    })
}

#[cfg(test)]
#[path = "glyph_cut_tests.rs"]
mod tests;
