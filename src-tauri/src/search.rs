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
//!
//! ## The two options
//!
//! [`Options`] turns off half of the fold and adds a boundary test. Both default
//! to off, which is the behaviour above and the behaviour a reader who has never
//! opened the find bar's menu gets.
//!
//! **Matching case** stops the `to_lowercase` pass and nothing else: whitespace
//! still collapses and soft hyphens still disappear, because neither of those is
//! about case and a reader who wants `Raster` rather than `raster` has not asked
//! for a phrase to stop matching across a line break.
//!
//! **Whole word** requires a word boundary at each end of a hit, in the sense
//! `\b` has everywhere else: a boundary sits between two characters when one of
//! them is a word character and the other is not. It is applied to the *folded*
//! sequence, which is what makes a soft hyphen not break a word --- it is gone
//! by then --- and what makes a line break count as a boundary.

use crate::text::PageText;

/// How a query is matched. Both off is the default described in the module docs.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, Hash,
)]
#[serde(rename_all = "camelCase", default)]
pub struct Options {
    /// Distinguish `Raster` from `raster`.
    pub match_case: bool,
    /// Require a word boundary at both ends of a hit.
    pub whole_word: bool,
}

/// A run of characters matching a query, as half-open character indices into
/// the page's `codes` --- the same indices [`crate::text::PageText`] keys its
/// boxes by, which is what makes a hit paintable without a lookup table.
///
/// It also carries the words around itself, for a results list. That is built
/// here rather than by the caller because the page's characters are already in
/// hand at this point and are dropped again the moment this returns: a frontend
/// assembling its own snippets would have to re-fetch every page a hit is on,
/// which on a 775-page document is the entire text of the document in order to
/// show a dozen lines of it.
///
/// **Three strings rather than one and two offsets.** An offset into a snippet
/// is a third index space --- alongside the page's code points and JavaScript's
/// UTF-16 --- and this module exists because two of those already disagree in
/// ways no test catches. Concatenating three strings cannot be got wrong.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Match {
    pub page: u32,
    pub start: u32,
    pub end: u32,
    /// Text immediately before the hit, whitespace collapsed.
    pub before: String,
    /// The matched text itself, exactly as the page spells it.
    pub hit: String,
    /// Text immediately after the hit, whitespace collapsed.
    pub after: String,
}

/// Characters of context taken on each side of a hit.
///
/// Two of these plus the hit is about a line of a results panel 260 px wide. The
/// cost is real and worth stating: a query matching 5,712 times ships roughly
/// 900 kB of snippets rather than 140 kB of bare ranges, arriving one page at a
/// time as the scan walks.
const CONTEXT_CHARS: usize = 40;

/// The characters of `codes` in `range`, with runs of whitespace collapsed.
///
/// Collapsed because a snippet is for reading in a list one line high, and PDF
/// text is full of line breaks that would otherwise arrive as blanks in the
/// middle of it. The hit itself is **not** collapsed --- it is what the page
/// says, and a results row that disagrees with the highlight it scrolls to is
/// worse than an ugly one.
fn slice_of(codes: &[u32], range: std::ops::Range<usize>) -> String {
    let mut out = String::new();
    for code in &codes[range] {
        let Some(ch) = char::from_u32(*code) else {
            continue;
        };
        if ch.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        out.push(ch);
    }
    out
}

/// The exact characters of `range`, unaltered.
fn exact_of(codes: &[u32], range: std::ops::Range<usize>) -> String {
    codes[range]
        .iter()
        .filter_map(|c| char::from_u32(*c))
        .collect()
}

/// What one page contributed to a search.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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

/// Whether a character is one a word is made of.
///
/// Letters, digits and the underscore, which is `\w` minus the locale
/// arguments. **Combining marks are not included**, and that is a real
/// divergence from `src/lib/text.ts`'s `classOf`, which counts `\p{M}` so that
/// double-clicking a decomposed `café` takes the accent with it. The standard
/// library exposes no general-category data, and pulling a Unicode crate in for
/// this one predicate is a dependency and a licence check for a case where the
/// consequence is that a whole-word search for `cafe` still matches a decomposed
/// `café` --- which is what the unrestricted search does anyway.
fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Whether a word boundary sits between two adjacent characters.
///
/// A missing character --- the start or the end of the page --- is always a
/// boundary, which is why the ends of a page are not a special case below.
fn boundary(left: Option<char>, right: Option<char>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => !(is_word(left) && is_word(right)),
        _ => true,
    }
}

impl Folded {
    /// Folds a sequence of `(index, char)` pairs. See the module docs.
    fn build(input: impl Iterator<Item = (u32, char)>, match_case: bool) -> Self {
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
            if match_case {
                chars.push(ch);
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

    fn of_page(text: &PageText, match_case: bool) -> Self {
        Self::build(
            text.codes
                .iter()
                .enumerate()
                .filter_map(|(index, code)| Some((index as u32, char::from_u32(*code)?))),
            match_case,
        )
    }

    fn of_query(query: &str, match_case: bool) -> Self {
        // Source indices are meaningless for a query and are never read.
        Self::build(query.chars().map(|ch| (0, ch)), match_case)
    }
}

/// Finds every non-overlapping occurrence of `query` in a page's characters.
///
/// An empty query matches nothing rather than matching everywhere, and so does
/// a query of only whitespace --- see the comment on that guard.
pub fn find_in(text: &PageText, page: u32, query: &str, options: Options) -> Vec<Match> {
    let needle = Folded::of_query(query, options.match_case);
    // A query of only whitespace is refused rather than run. The fold collapses
    // runs, so two spaces and one space are the same query here, and the only
    // distinction such a query could be trying to draw is exactly the one that
    // has just been destroyed --- answering it with every gap in the document
    // would be confidently wrong rather than merely useless.
    if needle.chars.iter().all(|ch| *ch == ' ') {
        return Vec::new();
    }

    let hay = Folded::of_page(text, options.match_case);
    let mut matches = Vec::new();
    let mut at = 0usize;

    while at + needle.chars.len() <= hay.chars.len() {
        let end = at + needle.chars.len();
        if hay.chars[at..end] != needle.chars[..] {
            at += 1;
            continue;
        }

        if options.whole_word
            && !(boundary(at.checked_sub(1).map(|i| hay.chars[i]), Some(hay.chars[at]))
                && boundary(Some(hay.chars[end - 1]), hay.chars.get(end).copied()))
        {
            // One character, not the needle's length. A rejected candidate is
            // not a match, and the next one may start inside it: `ab-a` occurs
            // twice in `ab-ab-a`, overlapping, and only the second is a whole
            // word. Skipping the span would walk past it.
            at += 1;
            continue;
        }

        // Back through the source map rather than by arithmetic: folding can
        // turn one character into two, and collapse several into one.
        let start = hay.source[at] as usize;
        let stop = hay.source[end - 1] as usize + 1;
        matches.push(Match {
            page,
            start: start as u32,
            end: stop as u32,
            before: slice_of(&text.codes, start.saturating_sub(CONTEXT_CHARS)..start),
            hit: exact_of(&text.codes, start..stop),
            after: slice_of(
                &text.codes,
                stop..(stop + CONTEXT_CHARS).min(text.codes.len()),
            ),
        });

        // Non-overlapping, which is what a reader counting hits expects: `aa`
        // occurs once in `aaa`, not twice.
        at += needle.chars.len();
    }

    matches
}

/// Searches one page.
pub fn search_page(text: &PageText, page: u32, query: &str, options: Options) -> PageMatches {
    PageMatches {
        page,
        matches: find_in(text, page, query, options),
        chars: text.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Neither option, which is what the module docs describe.
    const PLAIN: Options = Options {
        match_case: false,
        whole_word: false,
    };
    /// Case distinguished, everything else as `PLAIN`.
    const CASED: Options = Options {
        match_case: true,
        whole_word: false,
    };
    /// A boundary required at both ends, everything else as `PLAIN`.
    const WORDS: Options = Options {
        match_case: false,
        whole_word: true,
    };

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
    ///
    /// Taken from the *page*, not from the match's own `hit` field: a snippet
    /// the matcher wrote cannot say whether the indices it reported are right,
    /// and the indices are what the highlight is drawn from.
    fn covered(source: &str, m: &Match) -> String {
        source
            .chars()
            .skip(m.start as usize)
            .take((m.end - m.start) as usize)
            .collect()
    }

    #[test]
    fn a_match_is_found_where_it_is() {
        let text = "the raster appearance";
        let found = find_in(&page(text), 3, "raster", PLAIN);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].page, 3);
        assert_eq!(covered(text, &found[0]), "raster");
    }

    #[test]
    fn case_is_ignored_in_both_directions() {
        let text = "Kerning KERNING kerning";
        assert_eq!(find_in(&page(text), 0, "kerning", PLAIN).len(), 3);
        assert_eq!(find_in(&page(text), 0, "KERNING", PLAIN).len(), 3);
        assert_eq!(find_in(&page(text), 0, "KeRnInG", PLAIN).len(), 3);
    }

    #[test]
    fn a_phrase_matches_across_a_line_break() {
        // PDFium reports the break as its own character, so without collapsing
        // this is the common case of a search that should hit and does not.
        let text = "raster\r\nappearance";
        let found = find_in(&page(text), 0, "raster appearance", PLAIN);
        assert_eq!(found.len(), 1);
        assert_eq!(covered(text, &found[0]), text);
    }

    #[test]
    fn a_run_of_spaces_matches_one_space() {
        let text = "raster   appearance";
        assert_eq!(find_in(&page(text), 0, "raster appearance", PLAIN).len(), 1);
    }

    #[test]
    fn a_soft_hyphen_is_not_a_character() {
        let text = "ras\u{00ad}ter";
        let found = find_in(&page(text), 0, "raster", PLAIN);
        assert_eq!(found.len(), 1);
        // The hyphen is inside the match's span even though it matched nothing,
        // because a highlight that skipped it would be two rectangles.
        assert_eq!(covered(text, &found[0]), text);
    }

    #[test]
    fn a_multi_character_lowercase_still_maps_back() {
        // `İ` lowercases to two characters, `i` plus a combining dot, so one
        // source character becomes two folded ones and an end index computed as
        // start plus the query's length would be one past the page.
        let text = "\u{0130}b";
        let found = find_in(&page(text), 0, "i\u{0307}b", PLAIN);
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
        let found = find_in(&page(text), 0, "i", PLAIN);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start, 0);
        assert_eq!(found[0].end, 1);
    }

    #[test]
    fn matches_do_not_overlap() {
        let found = find_in(&page("aaaa"), 0, "aa", PLAIN);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].start, 0);
        assert_eq!(found[1].start, 2);
    }

    #[test]
    fn a_match_at_the_very_end_is_found() {
        let text = "journal catalog";
        let found = find_in(&page(text), 0, "catalog", PLAIN);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].end as usize, text.chars().count());
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert!(find_in(&page("catalog"), 0, "", PLAIN).is_empty());
        // Whitespace folds to a space rather than to nothing, so this is a
        // separate guard and not the same one twice: without it the query
        // matches every gap in the document.
        assert!(find_in(&page("a b c"), 0, "\n\n", PLAIN).is_empty());
        assert!(find_in(&page("a b c"), 0, " ", PLAIN).is_empty());
    }

    #[test]
    fn a_query_longer_than_the_page_matches_nothing() {
        assert!(find_in(&page("cat"), 0, "catalog", PLAIN).is_empty());
    }

    #[test]
    fn matching_case_distinguishes_what_ignoring_it_conflated() {
        let text = "Kerning KERNING kerning";
        assert_eq!(find_in(&page(text), 0, "kerning", CASED).len(), 1);
        assert_eq!(find_in(&page(text), 0, "KERNING", CASED).len(), 1);
        assert!(find_in(&page(text), 0, "KeRnInG", CASED).is_empty());
    }

    #[test]
    fn matching_case_leaves_the_rest_of_the_fold_alone() {
        // The two halves of the fold that are not about case. A reader who wants
        // `Raster` rather than `raster` has not asked for a phrase to stop
        // matching across a line break, nor for a soft hyphen to become a
        // character.
        assert_eq!(
            find_in(&page("Raster\r\nappearance"), 0, "Raster appearance", CASED).len(),
            1
        );
        assert_eq!(
            find_in(&page("Ras\u{00ad}ter"), 0, "Raster", CASED).len(),
            1
        );
    }

    #[test]
    fn a_whole_word_search_skips_the_word_it_is_part_of() {
        let text = "cat catalog concatenate cat.";
        let found = find_in(&page(text), 0, "cat", WORDS);
        assert_eq!(found.len(), 2, "found {found:?}");
        assert_eq!(found[0].start, 0);
        // The one before the full stop: punctuation is a boundary, a letter is
        // not. Without the option all four occurrences match.
        assert_eq!(covered(text, &found[1]), "cat");
        assert_eq!(found[1].start, 24);
        assert_eq!(find_in(&page(text), 0, "cat", PLAIN).len(), 4);
    }

    #[test]
    fn a_whole_word_search_bounds_both_ends_independently() {
        // One assertion per end, because a check that only ever tests the left
        // boundary passes with the right one deleted.
        assert!(find_in(&page("xcat"), 0, "cat", WORDS).is_empty());
        assert!(find_in(&page("catx"), 0, "cat", WORDS).is_empty());
        assert_eq!(find_in(&page("cat"), 0, "cat", WORDS).len(), 1);
    }

    #[test]
    fn a_word_may_end_at_the_page_rather_than_at_a_boundary() {
        // The ends of the page are boundaries. Without that, a document whose
        // last word is the query never matches, which is the failure nobody
        // notices because it only happens on the last word.
        assert_eq!(find_in(&page("a cat"), 0, "cat", WORDS).len(), 1);
        assert_eq!(find_in(&page("cat a"), 0, "cat", WORDS).len(), 1);
    }

    #[test]
    fn a_line_break_bounds_a_word_and_a_soft_hyphen_does_not() {
        // Both are about the boundary being tested on the *folded* sequence: the
        // break has become a space by then, and the hyphen has become nothing.
        assert_eq!(find_in(&page("a\ncat\nb"), 0, "cat", WORDS).len(), 1);
        assert!(find_in(&page("con\u{00ad}cat"), 0, "cat", WORDS).is_empty());
        assert_eq!(
            find_in(&page("a con\u{00ad}cat"), 0, "concat", WORDS).len(),
            1
        );
    }

    #[test]
    fn a_rejected_candidate_does_not_hide_the_one_overlapping_it() {
        // `ab-a` occurs twice in `ab-ab-a`, overlapping at offset 3, and only the
        // second is a whole word: the first is followed by `b`. Advancing past
        // the rejected span rather than by one character walks straight past it
        // and the search reports nothing.
        let text = "ab-ab-a";
        let found = find_in(&page(text), 0, "ab-a", WORDS);
        assert_eq!(found.len(), 1, "found {found:?}");
        assert_eq!(found[0].start, 3);
    }

    #[test]
    fn the_two_options_are_independent() {
        let text = "Cat cat Catalog";
        assert_eq!(
            find_in(
                &page(text),
                0,
                "cat",
                Options {
                    match_case: true,
                    whole_word: true
                }
            )
            .len(),
            1
        );
        assert_eq!(find_in(&page(text), 0, "cat", CASED).len(), 1);
        assert_eq!(find_in(&page(text), 0, "cat", WORDS).len(), 2);
    }

    #[test]
    fn a_hit_carries_the_words_on_either_side_of_it() {
        let text = "the raster appearance of a page";
        let found = find_in(&page(text), 0, "appearance", PLAIN);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].before, "the raster ");
        assert_eq!(found[0].hit, "appearance");
        assert_eq!(found[0].after, " of a page");
        // The three concatenate to the page around the hit, which is the only
        // property a caller can rely on -- it pastes them together and emboldens
        // the middle.
        let joined = format!("{}{}{}", found[0].before, found[0].hit, found[0].after);
        assert_eq!(joined, text);
    }

    #[test]
    fn context_stops_at_the_ends_of_the_page() {
        // Both ends, because a saturating subtraction and a clamped addition are
        // separate mistakes and either one alone panics on a real document.
        let found = find_in(&page("cat"), 0, "cat", PLAIN);
        assert_eq!(found[0].before, "");
        assert_eq!(found[0].after, "");
    }

    #[test]
    fn context_is_bounded_and_the_hit_is_not() {
        let long = "z".repeat(500);
        let text = format!("{long}catalog{long}");
        let found = find_in(&page(&text), 0, "catalog", PLAIN);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].before.chars().count(), CONTEXT_CHARS);
        assert_eq!(found[0].after.chars().count(), CONTEXT_CHARS);
        // The hit is whatever matched, however long. A query is not the place to
        // truncate: the row would show something the page does not say.
        let whole = find_in(&page(&text), 0, &text, PLAIN);
        assert_eq!(whole[0].hit.chars().count(), text.chars().count());
    }

    #[test]
    fn context_collapses_line_breaks_but_the_hit_keeps_them() {
        // A snippet is one line in a list, so the breaks around it become
        // spaces. The hit itself is not touched, because the row has to agree
        // with the highlight the reader lands on.
        let text = "a\n\n\nraster\nappearance\n\nb";
        let found = find_in(&page(text), 0, "raster appearance", PLAIN);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].before, "a ");
        assert_eq!(found[0].hit, "raster\nappearance");
        assert_eq!(found[0].after, " b");
    }

    #[test]
    fn the_hit_is_the_page_text_and_not_the_query() {
        // Case is folded for matching and must not be folded for display: a
        // results row spelling a word differently from the page it points at is
        // the row being wrong about the document.
        let found = find_in(&page("Kerning"), 0, "KERNING", PLAIN);
        assert_eq!(found[0].hit, "Kerning");
        // And a soft hyphen inside the span survives into the hit, for the same
        // reason the span covers it: there is one run of glyphs on the page.
        let found = find_in(&page("ras\u{00ad}ter"), 0, "raster", PLAIN);
        assert_eq!(found[0].hit, "ras\u{00ad}ter");
    }

    #[test]
    fn a_page_with_no_text_reports_it_rather_than_no_matches() {
        let result = search_page(&PageText::default(), 7, "catalog", PLAIN);
        assert!(result.matches.is_empty());
        assert_eq!(result.chars, 0);
        assert_eq!(result.page, 7);
    }

    #[test]
    fn a_page_with_text_and_no_hit_is_not_a_page_with_no_text() {
        let result = search_page(&page("journal"), 0, "catalog", PLAIN);
        assert!(result.matches.is_empty());
        assert_eq!(result.chars, 7);
    }
}
