//! Finding a string in a page's characters.
//!
//! ## Why this does not call `FPDFText_FindStart`
//!
//! PDFium has a search API, it is what Chrome's Ctrl-F uses, and reaching for it
//! would have been shorter. It searches PDFium's *own* extracted text buffer and
//! returns positions in it --- which is a second extraction, with its own index
//! space, sitting beside the one [`crate::text`] exists to be the only one of.
//! `text.rs` opens by saying that three features reading three different
//! extractions disagree in ways no test catches, each being self-consistent;
//! search would have been the second of the three.
//!
//! So matching happens here, over the codes the selection and the accessibility
//! tree will read. A hit is a range of the same character indices the boxes are
//! keyed by, so highlighting it is the selection code with a different colour,
//! and no mapping between two index spaces exists to be got wrong.
//!
//! The cost of that choice is that Unicode-aware matching is ours to write: the
//! fold below is what a reader expects Ctrl-F to do, and nothing more.
//!
//! ## What the fold does
//!
//! A query matches text a reader would say is the same text, which is not the
//! same as an equal sequence of code points:
//!
//! - **Case is ignored.** `char::to_lowercase` rather than `to_ascii_lowercase`,
//!   so `STRASSE` matches `strasse` and `Ä` matches `ä`.
//! - **Runs of whitespace collapse to one space.** A phrase that spans a line
//!   break is one phrase; PDFium reports the break as its own character, and a
//!   reader who types `raster appearance` does not know there is a newline in it.
//! - **Soft hyphens disappear.** They are a hint about where a word *may* break,
//!   not a character in the word.
//!
//! Because folding can change a character's length --- `ß` lowercases to `ss` ---
//! the folded sequence carries the source index each of its characters came from,
//! and a match is translated back through that rather than by arithmetic.
//!
//! What it deliberately does not do, so that a search result can be trusted to be
//! the text on the page: it does not normalise ligatures (`ﬁ` is not `fi`), does
//! not strip accents, and does not rejoin a word that a hyphen broke across two
//! lines. Each of those is a real feature; each also makes the highlight cover
//! characters the query did not contain, and none is guessed at here.

use crate::text::PageText;

/// A run of characters matching a query, as half-open character indices into
/// the page's `codes` --- the same indices [`crate::text::PageText`] keys its
/// boxes by, which is what makes a hit paintable without a lookup table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Match {
    pub page: u32,
    pub start: u32,
    pub end: u32,
}

/// What one page contributed to a search.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PageMatches {
    pub page: u32,
    pub matches: Vec<Match>,
    /// Characters the page has at all.
    ///
    /// Carried so that a search which found nothing can say *why*. A scanned
    /// document has no extractable text, and reporting "no matches" for it is a
    /// lie of omission --- the query was never really tested against anything.
    /// docs/PLAN.md section 9 measured the A0 sheet at zero characters, which is
    /// the correct answer for it and the case this field exists for.
    pub chars: u32,
}

/// Characters left after folding, and where each came from.
struct Folded {
    chars: Vec<char>,
    /// `source[i]` is the character index in the original page that produced
    /// `chars[i]`. Several folded characters can share one source index.
    source: Vec<u32>,
}

/// The soft hyphen, which marks a permitted break rather than a character.
const SOFT_HYPHEN: char = '\u{00ad}';

impl Folded {
    /// Folds a sequence of `(index, char)` pairs. See the module docs.
    fn build(input: impl Iterator<Item = (u32, char)>) -> Self {
        let mut chars: Vec<char> = Vec::new();
        let mut source: Vec<u32> = Vec::new();

        for (index, ch) in input {
            if ch == SOFT_HYPHEN {
                continue;
            }
            // `is_whitespace` is the Unicode property, so this already covers
            // the non-breaking space and the exotic ones. An explicit `|| ch ==
            // '\u{00a0}'` was written here first and is exactly the guard no
            // mutation can break that AGENTS.md warns about.
            if ch.is_whitespace() {
                // One space for a run, so a line break inside a phrase is not a
                // reason for the phrase not to match.
                if chars.last() == Some(&' ') {
                    continue;
                }
                chars.push(' ');
                source.push(index);
                continue;
            }
            for lower in ch.to_lowercase() {
                chars.push(lower);
                source.push(index);
            }
        }

        Self { chars, source }
    }

    fn of_page(text: &PageText) -> Self {
        Self::build(
            text.codes
                .iter()
                .enumerate()
                .filter_map(|(index, code)| Some((index as u32, char::from_u32(*code)?))),
        )
    }

    fn of_query(query: &str) -> Self {
        // Source indices are meaningless for a query and are never read.
        Self::build(query.chars().map(|ch| (0, ch)))
    }
}

/// Finds every non-overlapping occurrence of `query` in a page's characters.
///
/// An empty query matches nothing rather than matching everywhere, and so does
/// a query of only whitespace --- see the comment on that guard.
pub fn find_in(text: &PageText, page: u32, query: &str) -> Vec<Match> {
    let needle = Folded::of_query(query);
    // A query of only whitespace is refused rather than run. The fold collapses
    // runs, so two spaces and one space are the same query here, and the only
    // distinction such a query could be trying to draw is exactly the one that
    // has just been destroyed --- answering it with every gap in the document
    // would be confidently wrong rather than merely useless.
    if needle.chars.iter().all(|ch| *ch == ' ') {
        return Vec::new();
    }

    let hay = Folded::of_page(text);
    let mut matches = Vec::new();
    let mut at = 0usize;

    while at + needle.chars.len() <= hay.chars.len() {
        if hay.chars[at..at + needle.chars.len()] != needle.chars[..] {
            at += 1;
            continue;
        }

        // Back through the source map rather than by arithmetic: folding can
        // turn one character into two, and collapse several into one.
        let start = hay.source[at];
        let end = hay.source[at + needle.chars.len() - 1] + 1;
        matches.push(Match { page, start, end });

        // Non-overlapping, which is what a reader counting hits expects: `aa`
        // occurs once in `aaa`, not twice.
        at += needle.chars.len();
    }

    matches
}

/// Searches one page.
pub fn search_page(text: &PageText, page: u32, query: &str) -> PageMatches {
    PageMatches {
        page,
        matches: find_in(text, page, query),
        chars: text.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page whose characters are `text`, with no geometry.
    ///
    /// Boxes are not populated: nothing here reads them, and the point of this
    /// module is that a match is expressed in indices that a *caller* resolves
    /// against boxes. A fixture carrying fake geometry would invite a test that
    /// asserts against the fake.
    fn page(text: &str) -> PageText {
        PageText {
            codes: text.chars().map(|c| c as u32).collect(),
            ..PageText::default()
        }
    }

    /// The characters a match covers, which is what a highlight would paint.
    fn covered(source: &str, m: Match) -> String {
        source
            .chars()
            .skip(m.start as usize)
            .take((m.end - m.start) as usize)
            .collect()
    }

    #[test]
    fn a_match_is_found_where_it_is() {
        let text = "the raster appearance";
        let found = find_in(&page(text), 3, "raster");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].page, 3);
        assert_eq!(covered(text, found[0]), "raster");
    }

    #[test]
    fn case_is_ignored_in_both_directions() {
        let text = "Kerning KERNING kerning";
        assert_eq!(find_in(&page(text), 0, "kerning").len(), 3);
        assert_eq!(find_in(&page(text), 0, "KERNING").len(), 3);
        assert_eq!(find_in(&page(text), 0, "KeRnInG").len(), 3);
    }

    #[test]
    fn a_phrase_matches_across_a_line_break() {
        // PDFium reports the break as its own character, so without collapsing
        // this is the common case of a search that should hit and does not.
        let text = "raster\r\nappearance";
        let found = find_in(&page(text), 0, "raster appearance");
        assert_eq!(found.len(), 1);
        assert_eq!(covered(text, found[0]), text);
    }

    #[test]
    fn a_run_of_spaces_matches_one_space() {
        let text = "raster   appearance";
        assert_eq!(find_in(&page(text), 0, "raster appearance").len(), 1);
    }

    #[test]
    fn a_soft_hyphen_is_not_a_character() {
        let text = "ras\u{00ad}ter";
        let found = find_in(&page(text), 0, "raster");
        assert_eq!(found.len(), 1);
        // The hyphen is inside the match's span even though it matched nothing,
        // because a highlight that skipped it would be two rectangles.
        assert_eq!(covered(text, found[0]), text);
    }

    #[test]
    fn a_multi_character_lowercase_still_maps_back() {
        // `İ` lowercases to two characters, `i` plus a combining dot, so one
        // source character becomes two folded ones and an end index computed as
        // start plus the query's length would be one past the page.
        let text = "\u{0130}b";
        let found = find_in(&page(text), 0, "i\u{0307}b");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start, 0);
        assert_eq!(found[0].end, 2);
    }

    #[test]
    fn matching_half_of_a_folded_character_still_covers_all_of_it() {
        // Only the `i` of `İ`'s two folded characters is matched. The highlight
        // has to cover the whole source character regardless --- there is one
        // glyph on the page and one box for it.
        let text = "\u{0130}b";
        let found = find_in(&page(text), 0, "i");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start, 0);
        assert_eq!(found[0].end, 1);
    }

    #[test]
    fn matches_do_not_overlap() {
        let found = find_in(&page("aaaa"), 0, "aa");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].start, 0);
        assert_eq!(found[1].start, 2);
    }

    #[test]
    fn a_match_at_the_very_end_is_found() {
        let text = "journal catalog";
        let found = find_in(&page(text), 0, "catalog");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].end as usize, text.chars().count());
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert!(find_in(&page("catalog"), 0, "").is_empty());
        // Whitespace folds to a space rather than to nothing, so this is a
        // separate guard and not the same one twice: without it the query
        // matches every gap in the document.
        assert!(find_in(&page("a b c"), 0, "\n\n").is_empty());
        assert!(find_in(&page("a b c"), 0, " ").is_empty());
    }

    #[test]
    fn a_query_longer_than_the_page_matches_nothing() {
        assert!(find_in(&page("cat"), 0, "catalog").is_empty());
    }

    #[test]
    fn a_page_with_no_text_reports_it_rather_than_no_matches() {
        let result = search_page(&PageText::default(), 7, "catalog");
        assert!(result.matches.is_empty());
        assert_eq!(result.chars, 0);
        assert_eq!(result.page, 7);
    }

    #[test]
    fn a_page_with_text_and_no_hit_is_not_a_page_with_no_text() {
        let result = search_page(&page("journal"), 0, "catalog");
        assert!(result.matches.is_empty());
        assert_eq!(result.chars, 7);
    }
}
