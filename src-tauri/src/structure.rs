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
//! Bounding the *walk* is not the same as bounding the *work*, and the tree
//! multiplies two things the walk does not visit. Each element states how many
//! marked-content ids it has, and following one costs a scan of every character
//! on the page --- so elements x marks x characters is the real cost and the walk
//! bounds only the first factor. [`MAX_MARKS`] and [`MAX_RUNS`] bound the other
//! two, page-wide rather than per element, and both report through the same
//! truncation flag: a bound is a bound whichever one was reached.
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

/// Most marked-content ids the walk will follow, over the whole page.
///
/// A budget for the page rather than a limit per element, and that is the whole
/// of it: following one id costs a full [`spans_of`] scan of the page's
/// characters, so a per-element limit would still be multiplied by
/// [`MAX_ELEMENTS`] and the expensive product --- elements x marks x characters
/// --- would stay unbounded with every factor in it named. The count an element
/// reports is the file's to choose and nothing else checks it.
///
/// Ten thousand is generous against a real page: a tagged paragraph is one id,
/// a table cell is one, and a dense page spends a few hundred.
const MAX_MARKS: usize = 10_000;

/// Most runs the walk will keep.
///
/// Memory rather than time, and it needs its own bound because the two are not
/// the same limit: one id can name characters all over the page, so a single
/// mark within the budget above can produce arbitrarily many spans, and each one
/// clones the path it sits under --- a `Vec<String>` per run. Nothing honest has
/// ten thousand separately-tagged fragments on one page.
const MAX_RUNS: usize = 10_000;

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

impl PageStructure {
    /// The runs, but only if the walk finished.
    ///
    /// The invariant a consumer gets to rely on: **runs present means runs
    /// complete.** A partial reading order is not a reading order, and the
    /// fallback for one is the same as the fallback for no tags at all --- so
    /// truncation is collapsed into emptiness *here*, once, rather than at each
    /// call site where forgetting it produces a document that reads correctly
    /// for a while and then stops.
    ///
    /// Separate from [`read`] and taking no PDFium handle so it can be tested
    /// without one; the condition it encodes is the whole of what a caller has
    /// to get right.
    pub fn complete_runs(self) -> Vec<TaggedRun> {
        if self.truncated {
            return Vec::new();
        }
        self.runs
    }
}

/// Reads a page's structure, or reports that it has none.
///
/// Never fails on a document that merely lacks tags: that is an empty
/// [`PageStructure`], not an error. An `Err` means the page's text could not be
/// read at all, which is a different thing and is the caller's to report.
pub fn read(page: &RawPage<'_>) -> Result<PageStructure, String> {
    let text = RawTextPage::load(page)?;
    read_using(page, &text)
}

/// [`read`], for a caller that has already loaded the page's text.
///
/// `text::extract` has one, and loading a second would mean PDFium building a
/// second character index for the same page --- work proportional to the text on
/// it, on every page of every document, to obtain something already in hand.
/// Splitting it here rather than making [`read`] take one keeps the probe and the
/// tests calling a function with a single argument.
pub fn read_using(page: &RawPage<'_>, text: &RawTextPage<'_>) -> Result<PageStructure, String> {
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
        found: Found {
            of_char: &of_char,
            codes: &codes,
            runs: Vec::new(),
            marks: 0,
            truncated: false,
        },
        visited: 0,
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

    let Found {
        runs, truncated, ..
    } = walk.found;
    Ok(PageStructure {
        untagged_chars: untagged(chars, &runs),
        runs,
        chars,
        truncated,
    })
}

/// Characters no run claims.
///
/// The sum is `u64` and the subtraction saturates, because neither operand is
/// ours: a run is as wide as the document's own marked content and there can be
/// [`MAX_RUNS`] of them, and two elements naming the same id claim the same
/// characters twice, so the total can pass the index space the addends live in.
/// A `u32` sum panics there in debug and wraps in release --- and the wrap is the
/// one to spend a bound on, because a wrapped total reports a page as *more*
/// tagged than it is, which quietly removes text from a reading order rather
/// than obviously breaking one.
fn untagged(chars: u32, runs: &[TaggedRun]) -> u32 {
    let claimed: u64 = runs
        .iter()
        .map(|run| u64::from(run.end.saturating_sub(run.start)))
        .sum();
    chars.saturating_sub(u32::try_from(claimed).unwrap_or(u32::MAX))
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
    found: Found<'a>,
    visited: usize,
}

/// What the walk has found, and the two bounds on how much more it may find.
///
/// Split out of [`Walk`] because it holds no PDFium: a [`Walk`] carries
/// [`Bindings`], which is a loaded library, so a bound tested through one could
/// only be reached by finding a document that has ten thousand of something.
/// Everything the bounds govern happens in [`Found::mark`], and the tests drive
/// **that** function rather than a copy of it --- the same reason [`spans_of`]
/// is a free function.
struct Found<'a> {
    of_char: &'a [i32],
    codes: &'a [u32],
    runs: Vec<TaggedRun>,
    /// Marked-content ids followed so far, over the whole page. See [`MAX_MARKS`].
    marks: usize,
    truncated: bool,
}

impl Found<'_> {
    /// Emits the runs one marked-content id claims, or reports a bound stopping.
    ///
    /// `false` means this element has nothing further to contribute and its
    /// caller should stop asking. Both bounds are page-wide, so a `false` here is
    /// final rather than local --- every later call would answer the same way,
    /// and continuing to ask is only cheaper than not asking.
    ///
    /// The budget is charged before `mcid` is looked at, so an element declaring
    /// two billion ids that all name nothing is bounded too. That is the shape a
    /// hostile file would take: the run cap cannot fire on marks that claim no
    /// characters, and the loop asking for them is otherwise as long as the file
    /// says it is.
    fn mark(&mut self, mcid: i32, tag: &str, path: &[String]) -> bool {
        if self.marks >= MAX_MARKS {
            self.truncated = true;
            return false;
        }
        self.marks += 1;
        if mcid < 0 {
            return true;
        }
        for (start, end) in spans_of(self.of_char, self.codes, mcid) {
            if self.runs.len() >= MAX_RUNS {
                self.truncated = true;
                return false;
            }
            self.runs.push(TaggedRun {
                tag: tag.to_owned(),
                path: path.to_vec(),
                start,
                end,
            });
        }
        true
    }
}

impl Walk<'_> {
    /// Visits one element, emitting its runs and then its children's.
    ///
    /// Pre-order, because that is reading order: an element's own marked content
    /// comes before whatever is nested inside it, which is how a heading that
    /// contains a `/Span` reads.
    fn element(&mut self, element: FPDF_STRUCTELEMENT, path: &mut Vec<String>, depth: usize) {
        if depth >= MAX_DEPTH || self.visited >= MAX_ELEMENTS {
            self.found.truncated = true;
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
            // The count above is the document's claim and nothing bounds it, so
            // the loop is left to the budget rather than clamped here: the first
            // refusal ends it, and it is page-wide so a later element cannot
            // start a fresh one.
            if !self.found.mark(mcid, &tag, path) {
                break;
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

    /// A [`Found`] over a page, ready to be driven a mark at a time.
    ///
    /// The bounds are reached by calling the real [`Found::mark`], not by a
    /// document that has ten thousand of anything: what they bound is that
    /// function, and a fixture large enough to reach them through PDFium would
    /// mostly be testing the fixture generator.
    fn found<'a>(of_char: &'a [i32], codes: &'a [u32]) -> Found<'a> {
        Found {
            of_char,
            codes,
            runs: Vec::new(),
            marks: 0,
            truncated: false,
        }
    }

    #[test]
    fn the_mark_budget_stops_a_page_that_keeps_offering_them() {
        // Every mark here claims nothing, which is the case the run cap cannot
        // see: no run is pushed, so only the budget can end this. An element is
        // free to declare two billion marked-content ids, and each one costs a
        // scan of the page whether or not it turns out to name a character.
        let of_char = [3, 3, 3];
        let codes = ['x' as u32; 3];
        let mut found = found(&of_char, &codes);
        for spent in 0..MAX_MARKS {
            assert!(found.mark(7, "P", &[]), "stopped after {spent}");
        }
        assert!(!found.truncated, "nothing has been cut yet");
        assert!(!found.mark(7, "P", &[]), "the budget is spent");
        assert!(found.truncated, "and spending it is a truncation");
    }

    #[test]
    fn the_run_cap_stops_one_mark_that_fragments_the_whole_page() {
        // One id, alternating with characters belonging to another, so a single
        // call produces a span per pair. This is the memory bound rather than
        // the time one: each run clones the path it sits under, and the budget
        // above would let this happen ten thousand times over.
        let of_char: Vec<i32> = (0..(MAX_RUNS + 1) * 2)
            .map(|at| if at % 2 == 0 { 7 } else { 3 })
            .collect();
        let codes: Vec<u32> = of_char.iter().map(|_| 'x' as u32).collect();
        let mut found = found(&of_char, &codes);
        let path = vec!["Document".to_owned(), "P".to_owned()];
        assert!(!found.mark(7, "P", &path), "the cap stops it mid-mark");
        assert_eq!(found.runs.len(), MAX_RUNS);
        assert!(found.truncated);
    }

    #[test]
    fn a_mark_within_both_bounds_is_kept_whole() {
        // The control for the two above. A cap that fired early would satisfy
        // them just as well, and this is the page they must not touch.
        let of_char = [7, 7, 3, 7];
        let codes = ['x' as u32; 4];
        let mut found = found(&of_char, &codes);
        assert!(found.mark(7, "P", &["P".to_owned()]));
        assert_eq!(found.runs.len(), 2);
        assert!(!found.truncated);
    }

    #[test]
    fn a_claimed_total_past_the_index_space_does_not_wrap() {
        // Wider runs than a page can hold, and deliberately so: `untagged` adds
        // up numbers the document chose, over a count of runs the document
        // influences, and it must not depend on either being small. A `u32` sum
        // panics here in debug and wraps in release --- and the wrap is the
        // dangerous half, because it reports the page as *more* tagged than it
        // is, which removes text from a reading order without saying so.
        let run = |start, end| TaggedRun {
            tag: "P".to_owned(),
            path: Vec::new(),
            start,
            end,
        };
        assert_eq!(untagged(10, &[run(0, u32::MAX), run(0, u32::MAX)]), 0);
        // The control: it is still arithmetic, not a clamp to zero.
        assert_eq!(untagged(10, &[run(0, 4)]), 6);
    }

    /// A structure with `count` runs, truncated or not.
    fn structure(count: usize, truncated: bool) -> PageStructure {
        PageStructure {
            runs: (0..count)
                .map(|at| TaggedRun {
                    tag: "P".to_string(),
                    path: vec!["P".to_string()],
                    start: at as u32,
                    end: at as u32 + 1,
                })
                .collect(),
            chars: count as u32,
            untagged_chars: 0,
            truncated,
        }
    }

    #[test]
    fn a_finished_walks_runs_are_offered() {
        assert_eq!(structure(3, false).complete_runs().len(), 3);
    }

    #[test]
    fn a_truncated_walk_offers_nothing() {
        // Not "offers what it managed", which is the tempting reading and is
        // wrong: those runs are a reading order with an unknown amount of the
        // page missing from it, and a consumer cannot tell which part. Falling
        // back to geometry gives an order over *all* the text, which is worse in
        // the places the tags disagree and complete everywhere.
        assert!(structure(3, true).complete_runs().is_empty());
    }
}
