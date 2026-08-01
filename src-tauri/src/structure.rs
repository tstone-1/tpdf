//! Reading order taken from a document's own tags, where it has them.
//!
//! `reading.rs` and `reading.ts` recover reading order from **geometry**, which
//! is what an untagged document forces and is what `docs/PLAN.md` recorded as a
//! real limitation rather than a missing nicety: a tagged PDF already carries a
//! `/StructTree` saying what is a heading, what is a table cell, and in what
//! order it should be read, and inferring all of that from character boxes is
//! strictly worse for a document that has bothered to say.
//!
//! ## Why this does not need a second extraction
//!
//! The obvious way to relate a structure tree to text is to parse the page's
//! content stream, find the marked-content operators, and correlate what they
//! contain with what the text extractor returned. That is the shape `text.rs`
//! opens by warning against --- two extractions with two index spaces that
//! disagree in ways no test catches, each self-consistent --- and it would have
//! been the third one in this codebase.
//!
//! PDFium makes it unnecessary. `FPDFText_GetTextObject` gives the page object a
//! character belongs to, and `FPDFPageObj_GetMarkedContentID` gives that object's
//! marked-content id. So a character index maps to an MCID directly, and an
//! element's MCIDs map back to ranges of **the same character indices** the
//! selection, the search and the accessibility tree already use. There is no
//! second index space, so there is nothing to get wrong between them.
//!
//! ## The tree is hostile input, like the outline
//!
//! `outline.rs` bounds its walk because a malformed document can present an
//! infinite one and PDFium documents that noticing is the caller's job. A
//! structure tree is the same kind of graph and gets the same treatment: bounded
//! depth, a bounded number of elements, and **the truncation is reported**, so a
//! document that hits a bound is not silently shown a partial reading order.
//!
//! ## What it does not do
//!
//! It reports the order and the element types. It does not interpret them --- a
//! `/TD` is reported as `TD` and what a table cell *means* is the consumer's
//! question --- and it does not invent structure for a document that has none.
//! An untagged page reports no runs at all, which is what tells a caller to fall
//! back to geometry rather than to show an empty document.

use pdfium_render::prelude::*;

use crate::progressive::{Bindings, RawPage};
use crate::text::RawTextPage;

/// Deepest the walk will follow `/K`, matching `outline.rs`'s bound.
const MAX_DEPTH: usize = 32;

/// Most elements the walk will visit.
///
/// The same order of magnitude as the outline's cap and for the same reason: it
/// bounds a hostile document without touching a real one. A 775-page manual's
/// structure tree is thousands of elements *in total*, and this is per page.
const MAX_ELEMENTS: usize = 10_000;

/// Longest element type accepted, in UTF-16 code units.
///
/// A type is `P`, `H1`, `Figure`, `TD`. Anything longer is a document saying
/// something strange, and the buffer for it is bounded rather than trusted.
const MAX_TYPE_UNITS: usize = 64;

/// One tagged run of text, as half-open character indices into the page.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaggedRun {
    /// The element's type as the document spells it: `P`, `H1`, `Note`, `TD`.
    ///
    /// Not normalised. A consumer that wants to know whether this is a heading
    /// should ask for the types it understands rather than be handed a guess,
    /// and a document using a type nobody has seen before should be visible as
    /// such rather than flattened into `P`.
    pub tag: String,
    /// Where this run sits in the tree, deepest last: `["Document", "Sect", "P"]`.
    ///
    /// Carried because the type alone loses what the element is *inside*, and a
    /// `/P` in a `/TD` is a table cell's text rather than a paragraph.
    pub path: Vec<String>,
    /// First character of the run.
    pub start: u32,
    /// One past the last character of the run.
    pub end: u32,
}

/// What a page's tags say about it.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageStructure {
    /// Runs in the order the document says they should be read.
    ///
    /// Empty for a page with no structure tree, which is the signal to fall back
    /// to geometry. It is deliberately not an `Option`: "untagged" and "tagged
    /// with nothing on this page" are the same answer to the only question a
    /// caller asks, and inventing a distinction would invite one of the two to
    /// be handled and the other forgotten.
    pub runs: Vec<TaggedRun>,
    /// Characters the page has at all.
    ///
    /// So "this page is untagged" can be told from "this page has no text",
    /// which is the same distinction `search::PageMatches::chars` exists for.
    pub chars: u32,
    /// Characters that no run claims.
    ///
    /// A tagged document can leave text out of its tree --- an artifact marked
    /// `/Artifact`, or a producer that simply missed some --- and a consumer that
    /// used the runs as *the* reading order would silently drop it. Reported so
    /// the decision to fall back is made on a number rather than on a hope.
    pub untagged_chars: u32,
    /// Whether a bound stopped the walk before it finished.
    ///
    /// A truncated reading order presented as a complete one is the failure
    /// `outline.rs` guards against, and it is worse here: the missing part is
    /// text a reader can see on the page.
    pub truncated: bool,
}

/// Reads a page's structure, or reports that it has none.
///
/// Never fails on a document that merely lacks tags: that is an empty
/// [`PageStructure`], not an error. An `Err` means the page's text could not be
/// read at all, which is a different thing and is the caller's to report.
pub fn read(page: &RawPage<'_>) -> Result<PageStructure, String> {
    let text = RawTextPage::load(page)?;
    let chars = text.count();
    let bindings = page.bindings();

    // The tree first, and the early return before anything per-character. An
    // untagged document is the common case and the mapping below is two FFI
    // calls *per character* --- thousands on a dense page --- so paying for it
    // before knowing whether there is a tree to relate it to would put that cost
    // on every page of every document that has no tags at all.
    //
    // SAFETY: the page handle is valid for the borrow.
    let tree = unsafe { bindings.FPDF_StructTree_GetForPage(page.handle()) };
    if tree.is_null() {
        return Ok(PageStructure {
            runs: Vec::new(),
            chars,
            untagged_chars: chars,
            truncated: false,
        });
    }
    let tree = TreeHandle {
        bindings,
        handle: tree,
    };

    // Character -> marked-content id, taken from the text objects themselves.
    let codes: Vec<u32> = (0..chars).map(|index| text.code(index)).collect();
    let mut of_char: Vec<i32> = Vec::with_capacity(chars as usize);
    for index in 0..chars {
        // SAFETY: the text page outlives this loop and `index` is in range.
        let object = unsafe { bindings.FPDFText_GetTextObject(text.handle(), index as i32) };
        of_char.push(if object.is_null() {
            -1
        } else {
            // SAFETY: `object` is a valid page object owned by the page.
            unsafe { bindings.FPDFPageObj_GetMarkedContentID(object) }
        });
    }

    let mut walk = Walk {
        bindings: tree.bindings,
        of_char: &of_char,
        codes: &codes,
        runs: Vec::new(),
        visited: 0,
        truncated: false,
    };

    // SAFETY: `tree.handle` is non-null and owned by `tree`.
    let roots = unsafe { tree.bindings.FPDF_StructTree_CountChildren(tree.handle) };
    let mut path: Vec<String> = Vec::new();
    for index in 0..roots.max(0) {
        // SAFETY: `index` is below the reported child count.
        let child = unsafe {
            tree.bindings
                .FPDF_StructTree_GetChildAtIndex(tree.handle, index)
        };
        if !child.is_null() {
            walk.element(child, &mut path, 0);
        }
    }

    let mut claimed = 0u32;
    for run in &walk.runs {
        claimed += run.end - run.start;
    }
    Ok(PageStructure {
        runs: walk.runs,
        chars,
        untagged_chars: chars.saturating_sub(claimed),
        truncated: walk.truncated,
    })
}

/// Owns the tree handle so it is closed on every path out, panics included.
struct TreeHandle {
    bindings: Bindings,
    handle: FPDF_STRUCTTREE,
}

impl Drop for TreeHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was non-null when this was constructed and is not
        // used after this point.
        unsafe { self.bindings.FPDF_StructTree_Close(self.handle) };
    }
}

/// The state a depth-first walk carries.
struct Walk<'a> {
    bindings: Bindings,
    of_char: &'a [i32],
    codes: &'a [u32],
    runs: Vec<TaggedRun>,
    visited: usize,
    truncated: bool,
}

impl Walk<'_> {
    /// Visits one element, emitting its runs and then its children's.
    ///
    /// Pre-order, because that is reading order: an element's own marked content
    /// comes before whatever is nested inside it, which is how a heading that
    /// contains a `/Span` reads.
    fn element(&mut self, element: FPDF_STRUCTELEMENT, path: &mut Vec<String>, depth: usize) {
        if depth >= MAX_DEPTH || self.visited >= MAX_ELEMENTS {
            self.truncated = true;
            return;
        }
        self.visited += 1;

        let tag = self.type_of(element);
        path.push(tag.clone());

        // SAFETY: `element` is non-null and owned by the tree.
        let marks = unsafe {
            self.bindings
                .FPDF_StructElement_GetMarkedContentIdCount(element)
        };
        for index in 0..marks.max(0) {
            // SAFETY: `index` is below the reported count.
            let mcid = unsafe {
                self.bindings
                    .FPDF_StructElement_GetMarkedContentIdAtIndex(element, index)
            };
            if mcid < 0 {
                continue;
            }
            for (start, end) in spans_of(self.of_char, self.codes, mcid) {
                self.runs.push(TaggedRun {
                    tag: tag.clone(),
                    path: path.clone(),
                    start,
                    end,
                });
            }
        }

        // SAFETY: as above.
        let children = unsafe { self.bindings.FPDF_StructElement_CountChildren(element) };
        for index in 0..children.max(0) {
            // SAFETY: `index` is below the reported child count.
            let child = unsafe {
                self.bindings
                    .FPDF_StructElement_GetChildAtIndex(element, index)
            };
            if !child.is_null() {
                self.element(child, path, depth + 1);
            }
        }
        path.pop();
    }

    /// The element's type, or `""` when the document does not give one.
    fn type_of(&self, element: FPDF_STRUCTELEMENT) -> String {
        // SAFETY: a null buffer with length 0 is the documented way to ask for
        // the size, and writes nothing.
        let needed = unsafe {
            self.bindings
                .FPDF_StructElement_GetType(element, std::ptr::null_mut(), 0)
        } as usize;
        // Two bytes per UTF-16 unit plus the terminator; PDFium reports bytes.
        if !(4..=(MAX_TYPE_UNITS + 1) * 2).contains(&needed) {
            return String::new();
        }
        let mut buffer = vec![0u8; needed];
        // SAFETY: the buffer is exactly the size PDFium asked for.
        let written = unsafe {
            self.bindings.FPDF_StructElement_GetType(
                element,
                buffer.as_mut_ptr().cast(),
                needed as std::os::raw::c_ulong,
            )
        } as usize;
        if written == 0 || written > buffer.len() {
            return String::new();
        }
        let units: Vec<u16> = buffer[..written]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }
}

/// The character ranges belonging to one marked-content id.
///
/// A list rather than one range: nothing requires a mark's characters to be
/// contiguous in extraction order, and a producer that interleaves two marks
/// would otherwise have one of them silently swallow the other's text.
///
/// ## Why the gaps have to be bridged
///
/// A paragraph is one marked-content id and, in the content stream, one text
/// object per line. PDFium reports a **generated** character between two text
/// objects --- the space or break that makes the extracted text read as prose ---
/// and that character belongs to no page object, so it has no mark. Taken
/// literally, a four-line paragraph therefore arrives as four separate runs with
/// three unclaimed characters between them, which is what the first run of
/// `structure_probe` reported: ten runs for four blocks.
///
/// So a gap is bridged when every character in it is unmarked **and**
/// whitespace. Both conditions matter. Unmarked alone would let a run swallow
/// visible text that the producer simply failed to tag, which is the one thing a
/// reading order must never do silently; whitespace alone would let it swallow a
/// *different* element's spaces and put them in the wrong block.
///
/// A free function taking slices, not a method, so the tests can call **this**
/// one. A test that reimplements the walk it is checking is a copy that drifts,
/// and `docs/TRAPS.md` records a mutation surviving for exactly that reason.
fn spans_of(of_char: &[i32], codes: &[u32], mcid: i32) -> Vec<(u32, u32)> {
    let mut spans: Vec<(u32, u32)> = Vec::new();
    let mut start: Option<u32> = None;
    for (index, owner) in of_char.iter().enumerate() {
        if *owner == mcid {
            start.get_or_insert(index as u32);
        } else if let Some(from) = start.take() {
            spans.push((from, index as u32));
        }
    }
    if let Some(from) = start {
        spans.push((from, of_char.len() as u32));
    }

    let bridgeable = |from: usize, to: usize| -> bool {
        (from..to).all(|index| {
            of_char.get(index) == Some(&-1)
                && char::from_u32(codes.get(index).copied().unwrap_or(0))
                    .is_some_and(char::is_whitespace)
        })
    };

    let mut joined: Vec<(u32, u32)> = Vec::with_capacity(spans.len());
    for span in spans {
        match joined.last_mut() {
            Some(last) if bridgeable(last.1 as usize, span.0 as usize) => last.1 = span.1,
            _ => joined.push(span),
        }
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real `spans_of`, with the marked-content id the fixtures use.
    ///
    /// Everything else in this module is a sequence of PDFium calls, and
    /// asserting those against a fake would only prove the fake. What the tree
    /// produces is checked end to end by `examples/structure_probe.rs` against a
    /// fixture a different program wrote.
    fn walk(of_char: &[i32]) -> Vec<(u32, u32)> {
        // Every character a letter, so nothing is bridgeable and these assert
        // the span-finding alone.
        let codes: Vec<u32> = of_char.iter().map(|_| 'x' as u32).collect();
        spans_of(of_char, &codes, 7)
    }

    /// The same, with the gap characters PDFium generates between text objects.
    fn walk_with_gaps(of_char: &[i32], text: &str) -> Vec<(u32, u32)> {
        let codes: Vec<u32> = text.chars().map(|ch| ch as u32).collect();
        assert_eq!(of_char.len(), codes.len(), "the fixture is inconsistent");
        spans_of(of_char, &codes, 7)
    }

    #[test]
    fn a_contiguous_mark_is_one_span() {
        assert_eq!(walk(&[-1, 7, 7, 7, -1]), vec![(1, 4)]);
    }

    #[test]
    fn a_mark_running_to_the_end_is_closed() {
        // The bug this catches is the loop that only closes a span when it sees
        // a character belonging to something else: a mark covering the last
        // character of the page then disappears entirely.
        assert_eq!(walk(&[-1, 7, 7]), vec![(1, 3)]);
    }

    #[test]
    fn an_interrupted_mark_is_two_spans() {
        // Nothing requires a mark's characters to be contiguous, and treating
        // the first and last as one range would swallow whatever is between.
        assert_eq!(walk(&[7, 7, 3, 7]), vec![(0, 2), (3, 4)]);
    }

    #[test]
    fn a_mark_that_claims_nothing_is_no_span() {
        assert_eq!(walk(&[-1, -1, 3]), Vec::<(u32, u32)>::new());
    }

    #[test]
    fn a_generated_space_inside_one_element_is_bridged() {
        // A two-line paragraph: PDFium reports a space between the two text
        // objects and that space belongs to no object, so it carries no mark.
        // Without bridging, one paragraph is two runs -- which is exactly what
        // the probe's first run reported, ten runs for four blocks.
        assert_eq!(walk_with_gaps(&[7, 7, -1, 7, 7], "ab cd"), vec![(0, 5)]);
    }

    #[test]
    fn visible_text_in_the_gap_is_not_swallowed() {
        // The condition that makes bridging safe. An untagged *word* between two
        // marked runs is text the producer failed to tag, and absorbing it into
        // whichever run happens to be adjacent is the one thing a reading order
        // must not do quietly: it would appear in the wrong place and nothing
        // would say so.
        assert_eq!(
            walk_with_gaps(&[7, -1, -1, -1, 7], "aXYZb"),
            vec![(0, 1), (4, 5)]
        );
    }

    #[test]
    fn another_elements_space_is_not_bridged_across() {
        // Whitespace alone is not enough either: the gap here is whitespace but
        // it is *marked*, so it belongs to element 3 and bridging over it would
        // move another element's characters into this one's run.
        assert_eq!(walk_with_gaps(&[7, 3, 7], "a b"), vec![(0, 1), (2, 3)]);
    }
}
