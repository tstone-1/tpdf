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
//!   so `Ä` matches `ä` and `ΔΕΛΤΑ` matches `δελτα`. Lowercasing, note, and not
//!   case folding --- see below for the three things that costs.
//! - **Runs of whitespace collapse to one space.** A phrase that spans a line
//!   break is one phrase; PDFium reports the break as its own character, and a
//!   reader who types `raster appearance` does not know there is a newline in it.
//! - **Soft hyphens disappear.** They are a hint about where a word *may* break,
//!   not a character in the word.
//!
//! Because folding can change a character's length, the folded sequence carries
//! the source index each of its characters came from, and a match is translated
//! back through that rather than by arithmetic. The case that does it is a
//! **Turkish dotted capital**: `İ` lowercases to `i` followed by U+0307, so a hit
//! on `stanbul` inside `İstanbul` starts one source character further along than
//! its folded position says.
//!
//! This said `ß` lowercases to `ss` until 2026-08-01, and that is simply false ---
//! `ß` *upper*cases to `SS` and lowercases to itself, because it is already
//! lowercase. Nothing was wrong with the code; the example was, and it stood for
//! days because both halves of the sentence beside it are true. What it cost is
//! recorded below.
//!
//! ## What it deliberately does not do
//!
//! So that a search result can be trusted to be the text on the page: it does not
//! normalise ligatures (`ﬁ` is not `fi`), does not strip accents, and does not
//! rejoin a word that a hyphen broke across two lines. Each of those is a real
//! feature; each also makes the highlight cover characters the query did not
//! contain, and none is guessed at here.
//!
//! It also does not **case-fold**, which is a different operation from lowercasing
//! and is the one a search arguably wants. Measured on
//! `testdata/multilingual.pdf`, three consequences a reader would notice, all with
//! the same cause:
//!
//! - `strasse` finds `STRASSE` and **not** `Straße`.
//! - `odos` in Greek: `ΟΔΟΣ` lowercases to `οδοσ` with a medial sigma, so it is
//!   not found by the final-sigma spelling `οδος` a reader would type.
//! - `istanbul` does not find `İstanbul`, because the fold leaves the combining
//!   dot U+0307 between the `i` and the `s`.
//!
//! Case folding fixes all three in one move --- `ß` folds to `ss`, `ς` and `σ` fold
//! together --- and Rust's standard library does not offer it, so it means a
//! dependency. It is not taken here silently, because the same operation also
//! folds `ﬁ` to `fi`, which the paragraph above says outright that this does not
//! do. Changing that is a decision about what a highlight is allowed to cover,
//! not a bug fix, and `examples/search_probe.rs` states each of the three counts
//! above as a *decision* so that changing one has to be argued for.
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
    /// Read the query as a regular expression rather than as literal text.
    ///
    /// See [`find_in`] for what the pattern is matched *against*, which is the
    /// part that is easy to get wrong: the folded sequence, not the page's raw
    /// characters. A pattern that does not compile is reported rather than
    /// quietly matching nothing --- see [`PageMatches::problem`].
    pub regex: bool,
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
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub page: u32,
    pub start: u32,
    /// Exclusive end, on [`Self::end_page`] when there is one and on
    /// [`Self::page`] otherwise.
    pub end: u32,
    /// The page the hit *finishes* on, when that is not the one it starts on.
    ///
    /// A phrase can run over a page break --- "the raster" at the foot of one
    /// page and "appearance" at the head of the next --- and a reader who types
    /// it does not know there is a break in it. Such a hit is anchored on the
    /// page it starts on, because that is where the search should take them,
    /// and carries the second half here.
    ///
    /// `None` for every hit inside one page, which is nearly all of them, and
    /// the field is omitted from the wire in that case.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub end_page: Option<u32>,
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

/// Characters carried off the end of a page so a phrase can span the break.
///
/// The bound is on *source* characters, and it is generous relative to what it
/// has to hold because the fold shrinks: a page ending in fifty spaces
/// contributes one folded character, so a carry sized to the query in source
/// characters could arrive with nothing of the query in it.
const CARRY_CHARS: usize = 256;

/// The longest query a page break is looked across for, in folded characters.
///
/// Half the carry, so the characters *before* a straddling hit are always
/// present too --- which is what the whole-word test on the left-hand end reads.
/// A query longer than this is matched within each page, as it was before.
const CARRY_LONGEST_QUERY: usize = CARRY_CHARS / 2;

/// Source index standing for the page break itself, which belongs to no page.
///
/// A folded character carrying this came from the boundary rather than from
/// either side of it, so a hit that begins or ends on it is not a hit that
/// straddles: it lies wholly in one page, and that page's own reply reports it.
const BREAK: u32 = u32::MAX;

/// The tail of a page, for the request that asks about the next one.
///
/// Handed back to the caller and handed straight to the following
/// [`search_page`], which is what makes a cross-page hit findable without the
/// backend holding two pages at once or extracting either of them twice. The
/// walk is sequential anyway --- see `search.ts` --- so the caller has it.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Carry {
    /// The page these characters came from.
    pub page: u32,
    /// Index in that page of the first of them.
    pub from: u32,
    /// The characters, in order.
    pub codes: Vec<u32>,
}

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
    /// Why the query could not be run at all, if it could not.
    ///
    /// Only a regular expression can fail to be a query. The distinction is the
    /// same one `chars` exists for and matters more here, because a reader
    /// typing a pattern is *expecting* to get it wrong: `foo(` finds nothing,
    /// and "no matches" for it is a lie of omission that reads as a working
    /// search over a document that does not contain `foo(`. There is no third
    /// state to invent --- a page with a problem reports no matches and the
    /// reason, and the find bar shows the reason instead of the counter.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub problem: Option<String>,
    /// This page's last characters, for the request about the next one.
    ///
    /// Absent when the query cannot span a break anyway --- see
    /// [`carry_for`] --- so the common single-word search ships nothing extra.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tail: Option<Carry>,
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

    /// The folded characters as a string, with a byte offset to char index map.
    ///
    /// Only the regex path needs this. `regex` reports byte offsets into a
    /// `&str` and everything else here counts characters, so without the map a
    /// pattern that matched after any non-ASCII character on the page would
    /// resolve to the wrong source index --- silently, and only on the documents
    /// that most need searching.
    ///
    /// The map has one entry per byte plus one, and a byte in the middle of a
    /// character carries the same index as the byte that started it. A match
    /// boundary can only fall on a character boundary, so the interior entries
    /// are never read; they are there so the lookup needs no branch.
    fn as_str(&self) -> (String, Vec<u32>) {
        let mut text = String::with_capacity(self.chars.len());
        let mut at_byte: Vec<u32> = Vec::new();
        for (index, ch) in self.chars.iter().enumerate() {
            text.push(*ch);
            at_byte.resize(text.len(), index as u32);
        }
        at_byte.resize(text.len() + 1, self.chars.len() as u32);
        (text, at_byte)
    }
}

/// How large a compiled pattern may get, in bytes.
///
/// The `regex` crate matches in time linear in the input, so there is no
/// catastrophic backtracking to defend against and this is not that guard. It
/// bounds *space*: a pattern like `a{1000}{1000}` is small to type and large to
/// compile, and a reader who types one should get a refusal rather than a
/// window that stops responding. The default is 10 MB; a search box does not
/// need it.
const PATTERN_SIZE_LIMIT: usize = 1 << 20;

/// Compiles a reader's pattern against the folded haystack's conventions.
///
/// This said case was handled by the fold rather than by the `i` flag, *"with the
/// fold already lowercasing both sides"* --- and both sides is exactly what it does
/// not do. [`Folded::of_query`] folds a **literal** query; a pattern is handed here
/// raw, because a regex source is not text and cannot be lowercased safely: doing
/// so would turn `\S` into `\s`, `\D` into `\d`, `\B` into `\b` and `[A-Z]` into
/// `[a-z]`, each silently meaning the opposite of what was typed.
///
/// So with `match_case` off the haystack was lowercase and the pattern was not, and
/// **any uppercase letter in a pattern matched nothing at all**. A reader with the
/// regex option on and match-case off, typing `Encoding`, got no results on a page
/// that plainly contains it.
///
/// It survived because the comment above asserted the invariant it was breaking,
/// and because every corpus until `encodings.pdf` had lowercase text in it ---
/// `viewer_check.py` builds its pattern out of a word taken from the page, so the
/// pattern was lowercase too and the two agreed by accident. The corpus that found
/// it did so because its garbage text happens to be uppercase.
///
/// The `i` flag is the fix rather than folding the pattern: it composes with a
/// haystack that is already lowercase, it leaves every class and escape alone, and
/// it is what a reader means by "ignore case" on a pattern.
fn compile(pattern: &str, match_case: bool) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(!match_case)
        .size_limit(PATTERN_SIZE_LIMIT)
        .dfa_size_limit(PATTERN_SIZE_LIMIT)
        .build()
        .map_err(|e| {
            // The crate's own message is several lines with a caret diagram in
            // it, which is right for a terminal and wrong for a one-line find
            // bar. The first line is the sentence a reader needs.
            e.to_string()
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("not a valid pattern")
                .trim()
                .to_string()
        })
}

/// Finds every non-overlapping occurrence of `query` in a page's characters.
///
/// An empty query matches nothing rather than matching everywhere, and so does
/// a query of only whitespace --- see the comment on that guard.
///
/// Fails only when [`Options::regex`] is set and the pattern does not compile.
///
/// ## What a pattern is matched against
///
/// The **folded** sequence, which is the same haystack a literal query gets and
/// is worth stating because it is not what a reader might assume:
///
/// - Runs of whitespace are already one space, so `\n` never occurs and a
///   pattern written with one matches nothing. `^` and `$` anchor to the page,
///   not to a printed line --- there are no lines left by then. That is the same
///   bargain the literal path makes, and the reason a phrase matches across a
///   line break at all.
/// - Soft hyphens are already gone, so `ras\u{00ad}ter` is `raster` to a pattern
///   as well as to a literal.
/// - Case is folded by [`Options::match_case`] rather than by the `i` flag, so
///   the two query kinds mean the same thing by the same switch.
///
/// **A zero-length match is not a match.** `a*` matches the empty string at
/// every position; reporting those would fill the results list with hits that
/// highlight nothing and give the reader a count of the page's characters. They
/// are skipped, and the scan still advances, which is also what stops the walk
/// looping forever on one.
pub fn find_in(
    text: &PageText,
    page: u32,
    query: &str,
    options: Options,
) -> Result<Vec<Match>, String> {
    let needle = Folded::of_query(query, options.match_case);
    // A query of only whitespace is refused rather than run. The fold collapses
    // runs, so two spaces and one space are the same query here, and the only
    // distinction such a query could be trying to draw is exactly the one that
    // has just been destroyed --- answering it with every gap in the document
    // would be confidently wrong rather than merely useless.
    //
    // A *pattern* of only whitespace is a different thing --- `\s+` is spaces to
    // look at and not spaces to match --- so the guard reads the query as typed
    // in that case, which for a pattern is only empty when it is empty.
    // An empty needle first, and on its own, because the literal walk below
    // advances by the needle's length and would therefore **never terminate** on
    // one. The whitespace guard beneath happens to cover that today --- `all` is
    // true of an empty sequence --- and a termination argument that leans on
    // another guard's implementation is one edit away from a hang. It was:
    // deleting the whitespace guard as a mutation turned a readable red into a
    // run with no result at all.
    if needle.chars.is_empty() {
        return Ok(Vec::new());
    }
    if options.regex {
        if query.is_empty() {
            return Ok(Vec::new());
        }
    } else if needle.chars.iter().all(|ch| *ch == ' ') {
        return Ok(Vec::new());
    }

    let hay = Folded::of_page(text, options.match_case);

    // Whether a hit has a word boundary at both ends. A closure rather than a
    // filter over the collected spans, because *where* the test happens is
    // load-bearing on the literal path and only there --- see the walk below.
    let whole = |at: usize, end: usize| -> bool {
        !options.whole_word
            || (boundary(at.checked_sub(1).map(|i| hay.chars[i]), Some(hay.chars[at]))
                && boundary(Some(hay.chars[end - 1]), hay.chars.get(end).copied()))
    };

    // Accepted hits, as half-open indices into the folded sequence.
    let mut spans: Vec<(usize, usize)> = Vec::new();

    if options.regex {
        let pattern = compile(query, options.match_case)?;
        let (haystack, at_byte) = hay.as_str();
        for found in pattern.find_iter(&haystack) {
            if found.range().is_empty() {
                continue;
            }
            let at = at_byte[found.start()] as usize;
            let end = at_byte[found.end()] as usize;
            // Rejected outright rather than retried one character along, which
            // is the opposite of the literal path below and is right here:
            // `find_iter` has already chosen where the hits are, and a reader
            // who wants a boundary inside a pattern can write `\b`.
            if whole(at, end) {
                spans.push((at, end));
            }
        }
    } else {
        let mut at = 0usize;
        while at + needle.chars.len() <= hay.chars.len() {
            let end = at + needle.chars.len();
            if hay.chars[at..end] != needle.chars[..] {
                at += 1;
                continue;
            }
            if !whole(at, end) {
                // One character, not the needle's length. A rejected candidate
                // is not a match, and the next one may start inside it: `ab-a`
                // occurs twice in `ab-ab-a`, overlapping, and only the second is
                // a whole word. Skipping the span would walk past it.
                //
                // Collecting the spans first and filtering them afterwards
                // loses exactly this, and did: the test named for it is what
                // caught the restructure that introduced the regex path.
                at += 1;
                continue;
            }
            spans.push((at, end));
            // Non-overlapping, which is what a reader counting hits expects:
            // `aa` occurs once in `aaa`, not twice.
            at += needle.chars.len();
        }
    }

    let mut matches = Vec::new();
    for (at, end) in spans {
        // Back through the source map rather than by arithmetic: folding can
        // turn one character into two, and collapse several into one.
        let start = hay.source[at] as usize;
        let stop = hay.source[end - 1] as usize + 1;
        matches.push(Match {
            page,
            start: start as u32,
            end: stop as u32,
            end_page: None,
            before: slice_of(&text.codes, start.saturating_sub(CONTEXT_CHARS)..start),
            hit: exact_of(&text.codes, start..stop),
            after: slice_of(
                &text.codes,
                stop..(stop + CONTEXT_CHARS).min(text.codes.len()),
            ),
        });
    }

    Ok(matches)
}

/// Whether a page break is worth looking across for this query, and how much of
/// the previous page it would take.
///
/// `None` for the cases where a carry would be dead weight or wrong:
///
/// - a **pattern**, because `^` and `$` already mean the page and stitching two
///   pages into one haystack would quietly make them mean something else;
/// - a query of one folded character, which cannot straddle anything;
/// - a query longer than [`CARRY_LONGEST_QUERY`], where the carry could not
///   hold both the hit's left half and the character before it.
fn carry_len(query: &str, options: Options) -> Option<usize> {
    if options.regex {
        return None;
    }
    let folded = Folded::of_query(query, options.match_case).chars.len();
    if !(2..=CARRY_LONGEST_QUERY).contains(&folded) {
        return None;
    }
    Some(CARRY_CHARS)
}

/// The tail this page should hand to the request about the next one.
fn carry_for(text: &PageText, page: u32, query: &str, options: Options) -> Option<Carry> {
    let want = carry_len(query, options)?;
    let from = text.codes.len().saturating_sub(want);
    Some(Carry {
        page,
        from: from as u32,
        codes: text.codes[from..].to_vec(),
    })
}

/// Finds hits that begin on the carried page and finish on this one.
///
/// Only those. Everything inside this page is [`find_in`]'s job and reporting it
/// twice would double every count in the document.
fn find_across(
    text: &PageText,
    page: u32,
    carry: &Carry,
    query: &str,
    options: Options,
) -> Vec<Match> {
    let Some(_) = carry_len(query, options) else {
        return Vec::new();
    };
    let needle = Folded::of_query(query, options.match_case);
    if needle.chars.is_empty() {
        return Vec::new();
    }

    // One haystack, indexed continuously --- with a space in the middle that no
    // page contains.
    //
    // That space is the whole difference between this working and not. A page's
    // extracted characters do not end with whitespace: "raster" is the last
    // thing on one page and "appearance" the first on the next, so a plain
    // concatenation reads `rasterappearance` and the phrase a reader typed
    // matches nothing. The break *is* whitespace --- it is a line break with a
    // sheet of paper in it --- and the fold then collapses it against any
    // whitespace either side of it, exactly as it does a line break inside a
    // page. This cost two tests to find and is the reason they exist.
    let joined: Vec<u32> = carry
        .codes
        .iter()
        .chain(text.codes.iter())
        .copied()
        .collect();
    let split = carry.codes.len();
    let mut items: Vec<(u32, char)> = Vec::with_capacity(joined.len() + 1);
    for (index, code) in joined.iter().enumerate() {
        if index == split {
            items.push((BREAK, '\n'));
        }
        if let Some(ch) = char::from_u32(*code) {
            items.push((index as u32, ch));
        }
    }
    let hay = Folded::build(items.into_iter(), options.match_case);

    let mut matches = Vec::new();
    let mut at = 0usize;
    while at + needle.chars.len() <= hay.chars.len() {
        let end = at + needle.chars.len();
        if hay.chars[at..end] != needle.chars[..] {
            at += 1;
            continue;
        }
        let (first, last) = (hay.source[at], hay.source[end - 1]);
        // Straddling, and nothing else. A hit wholly in the carry belongs to the
        // previous page's own reply, and one wholly in this page to this reply's.
        // An end on the break itself is the first of those and a start on it the
        // second, which is why both are dropped here rather than resolved.
        if first == BREAK || last == BREAK {
            at += 1;
            continue;
        }
        let (first, last) = (first as usize, last as usize);
        if !(first < split && last >= split) {
            at += 1;
            continue;
        }
        if options.whole_word
            && !(boundary(at.checked_sub(1).map(|i| hay.chars[i]), Some(hay.chars[at]))
                && boundary(Some(hay.chars[end - 1]), hay.chars.get(end).copied()))
        {
            at += 1;
            continue;
        }

        let stop = last + 1;
        matches.push(Match {
            page: carry.page,
            start: carry.from + first as u32,
            end: (stop - split) as u32,
            end_page: Some(page),
            // Context from the joined text on the left and from this page on the
            // right, which is what a reader sees either side of the break.
            before: slice_of(&joined, first.saturating_sub(CONTEXT_CHARS)..first),
            // A space where the break is, because that is what the fold matched
            // against and what the reader sees --- the two pages' characters
            // concatenated would read `rasterappearance`, which is a snippet of
            // a word that occurs nowhere.
            hit: format!(
                "{} {}",
                exact_of(&joined, first..split),
                exact_of(&joined, split..stop)
            ),
            after: slice_of(
                &text.codes,
                (stop - split)..(stop - split + CONTEXT_CHARS).min(text.codes.len()),
            ),
        });
        at += needle.chars.len();
    }
    matches
}

/// Searches one page, and the break before it when there is a carry.
///
/// A pattern that does not compile is reported on every page rather than once,
/// because there is no "once" here: the walk asks page by page and each reply
/// stands alone. The frontend shows the first one it gets and stops the scan.
pub fn search_page(
    text: &PageText,
    page: u32,
    query: &str,
    options: Options,
    carry: Option<&Carry>,
) -> PageMatches {
    let tail = carry_for(text, page, query, options);
    match find_in(text, page, query, options) {
        Ok(mut matches) => {
            if let Some(carry) = carry {
                // Ahead of this page's own hits, because the walk keeps matches
                // in the order they are found and a hit that begins on the
                // previous page comes first in reading order.
                let mut across = find_across(text, page, carry, query, options);
                across.append(&mut matches);
                matches = across;
            }
            PageMatches {
                page,
                matches,
                chars: text.len() as u32,
                problem: None,
                tail,
            }
        }
        Err(problem) => PageMatches {
            page,
            matches: Vec::new(),
            chars: text.len() as u32,
            problem: Some(problem),
            tail: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Neither option, which is what the module docs describe.
    const PLAIN: Options = Options {
        match_case: false,
        whole_word: false,
        regex: false,
    };
    /// Case distinguished, everything else as `PLAIN`.
    const CASED: Options = Options {
        match_case: true,
        whole_word: false,
        regex: false,
    };
    /// A boundary required at both ends, everything else as `PLAIN`.
    const WORDS: Options = Options {
        match_case: false,
        whole_word: true,
        regex: false,
    };
    /// The query is a pattern, everything else as `PLAIN`.
    const PATTERN: Options = Options {
        match_case: false,
        whole_word: false,
        regex: true,
    };

    /// Finds, and asserts the query was runnable at all.
    ///
    /// Every test but the ones about a bad pattern goes through this, so a
    /// pattern that stopped compiling could not be mistaken for one that stopped
    /// matching --- which is the whole reason the error is not swallowed.
    fn find_in(text: &PageText, page: u32, query: &str, options: Options) -> Vec<Match> {
        super::find_in(text, page, query, options).expect("the query compiles")
    }

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
        assert!(
            m.end_page.is_none(),
            "a hit that ends on another page needs `covered_across`; \
             this one would subtract two pages' indices from each other"
        );
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
                    whole_word: true,
                    regex: false
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
        let result = search_page(&PageText::default(), 7, "catalog", PLAIN, None);
        assert!(result.matches.is_empty());
        assert_eq!(result.chars, 0);
        assert_eq!(result.page, 7);
    }

    #[test]
    fn a_page_with_text_and_no_hit_is_not_a_page_with_no_text() {
        let result = search_page(&page("journal"), 0, "catalog", PLAIN, None);
        assert!(result.matches.is_empty());
        assert_eq!(result.chars, 7);
    }

    /// The two halves a cross-page hit covers, as each page would paint it.
    fn covered_across(first: &str, second: &str, m: &Match) -> (String, String) {
        (
            first.chars().skip(m.start as usize).collect(),
            second.chars().take(m.end as usize).collect(),
        )
    }

    /// Searches `second` with `first`'s tail carried into it, as the walk does.
    ///
    /// Goes through `search_page` both times rather than calling `find_across`,
    /// so what is exercised is the handover the frontend actually performs ---
    /// the tail one reply produces is the carry the next request consumes, and
    /// a test that built the carry itself could not catch the two disagreeing.
    fn across(first: &str, second: &str, query: &str, options: Options) -> Vec<Match> {
        let tail = search_page(&page(first), 0, query, options, None).tail;
        search_page(&page(second), 1, query, options, tail.as_ref()).matches
    }

    #[test]
    fn a_phrase_matches_across_a_page_break() {
        let found = across(
            "at the foot: raster",
            "appearance, at the head",
            "raster appearance",
            PLAIN,
        );
        assert_eq!(found.len(), 1);
        let hit = &found[0];
        // Anchored where it starts, which is where the reader has to be taken.
        assert_eq!(hit.page, 0);
        assert_eq!(hit.end_page, Some(1));
        // Each half lands on the characters its own page would highlight, which
        // is what the two index spaces have to mean.
        let (left, right) = covered_across("at the foot: raster", "appearance, at the head", hit);
        assert_eq!(left, "raster");
        assert_eq!(right, "appearance");
        assert_eq!(hit.hit, "raster appearance");
    }

    #[test]
    fn a_break_collapses_like_any_other_line_break() {
        // The join is folded with everything else, so trailing and leading
        // whitespace at the break is one space --- the same bargain that makes a
        // phrase match across a line break inside a page.
        let found = across("raster  \n", "\n  appearance", "raster appearance", PLAIN);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn a_hit_inside_one_page_is_not_reported_twice() {
        // Both pages contain the phrase outright. Each reply must report its
        // own and neither must report the other's, or every count in a document
        // that repeats a phrase is doubled.
        let found = across(
            "raster appearance",
            "raster appearance",
            "raster appearance",
            PLAIN,
        );
        assert_eq!(found.len(), 1, "the second page's own hit, once: {found:?}");
        assert_eq!(found[0].page, 1);
        assert_eq!(found[0].end_page, None);
    }

    #[test]
    fn a_break_is_only_looked_across_when_the_pages_are_adjacent() {
        // The carry names the page it came from, and the walk is what decides
        // whether to pass it. What this pins is the other half: given no carry,
        // nothing straddles.
        let found = search_page(&page("appearance"), 1, "raster appearance", PLAIN, None);
        assert!(found.matches.is_empty());
    }

    #[test]
    fn whole_word_reads_the_characters_either_side_of_the_break() {
        // The left-hand boundary is on the *previous* page, which is the whole
        // reason the carry is longer than the query: without those characters
        // the test would see the start of the carry and call it a boundary.
        let joined = across("a graster", "appearance b", "raster appearance", WORDS);
        assert!(
            joined.is_empty(),
            "`graster appearance` is not a whole word: {joined:?}"
        );
        let clean = across("a raster", "appearance b", "raster appearance", WORDS);
        assert_eq!(clean.len(), 1);
    }

    #[test]
    fn a_pattern_is_not_matched_across_a_break() {
        // Deliberate, not missing: `^` and `$` mean the page, and a haystack
        // stitched from two pages would quietly make them mean something else.
        let found = across("raster", " appearance", "raster.appearance", PATTERN);
        assert!(found.is_empty());
        // The tail is not even produced for a pattern, so nothing is shipped
        // for a feature that is switched off.
        assert!(search_page(&page("raster"), 0, "r.ster", PATTERN, None)
            .tail
            .is_none());
    }

    #[test]
    fn a_query_that_cannot_straddle_ships_no_tail() {
        // One character cannot start on one page and finish on the next, and a
        // single-word search is the common case: it must not pay 256 characters
        // a page for a feature it cannot use.
        assert!(search_page(&page("raster"), 0, "r", PLAIN, None)
            .tail
            .is_none());
        assert!(search_page(&page("raster"), 0, "ra", PLAIN, None)
            .tail
            .is_some());
        let long = "x".repeat(CARRY_LONGEST_QUERY + 1);
        assert!(search_page(&page("raster"), 0, &long, PLAIN, None)
            .tail
            .is_none());
    }

    #[test]
    fn the_tail_is_the_end_of_the_page_and_says_where_it_started() {
        let text = "abcdefghij";
        let tail = search_page(&page(text), 4, "ij", PLAIN, None)
            .tail
            .expect("a two-character query carries a tail");
        assert_eq!(tail.page, 4);
        // Shorter than the bound, so it is the whole page and starts at zero.
        assert_eq!(tail.from, 0);
        assert_eq!(tail.codes.len(), text.chars().count());
    }

    #[test]
    fn a_long_page_carries_only_its_end() {
        let text = "z".repeat(CARRY_CHARS * 2);
        let tail = search_page(&page(&text), 0, "z z", PLAIN, None)
            .tail
            .expect("a tail");
        assert_eq!(tail.codes.len(), CARRY_CHARS);
        assert_eq!(tail.from, (CARRY_CHARS * 2 - CARRY_CHARS) as u32);
        // And an index built from it points at the right character. `from` is
        // what makes that work: an offset into the carry is not an offset into
        // the page, and reporting the first would put every hit on a long page
        // 256 characters early.
        let found = across(&text, "zz", "z z", PLAIN);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start, (CARRY_CHARS * 2 - 1) as u32);
        assert_eq!(found[0].end, 1);
    }

    #[test]
    fn a_word_the_break_splits_is_not_rejoined() {
        // The break reads as whitespace, so `appear` at the foot of one page and
        // `ance` at the head of the next is two words, not one. That is the same
        // answer the module already gives for a word a *line* break splits ---
        // "it does not rejoin a word that a hyphen broke across two lines" ---
        // and the alternative would make a hit out of two unrelated words
        // whenever a page happens to end mid-syllable.
        assert!(across("to appear", "ance of it", "appearance", PLAIN).is_empty());
        // The control: the same two pages find the phrase that really is there.
        assert_eq!(
            across("to appear", "ance of it", "appear ance", PLAIN).len(),
            1
        );
    }

    #[test]
    fn a_pattern_matches_what_a_literal_cannot() {
        let text = "raster ruster roster";
        assert_eq!(find_in(&page(text), 0, "r[au]ster", PATTERN).len(), 2);
        // The control: as a literal it is the text `r[au]ster`, which is not on
        // the page. Without this the test passes for a matcher that quietly
        // treats every query as a pattern.
        assert_eq!(find_in(&page(text), 0, "r[au]ster", PLAIN).len(), 0);
    }

    #[test]
    fn a_pattern_reads_the_folded_text_and_not_the_page() {
        // Three properties of the fold at once, each of which a pattern would
        // see differently if it ran against the raw characters: the line break
        // is one space, the soft hyphen is gone, and `.` therefore matches
        // across both.
        let found = find_in(
            &page("ras\u{00ad}ter\r\nappearance"),
            0,
            "raster.appear",
            PATTERN,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start, 0);
    }

    #[test]
    fn an_anchor_is_the_page_and_not_a_printed_line() {
        // Worth pinning rather than leaving to be discovered: after the fold
        // there are no newlines left, so `^` cannot mean what it means in a
        // text editor. A reader who assumes otherwise gets no matches, and the
        // module docs say so.
        let text = "alpha\nbeta";
        assert_eq!(find_in(&page(text), 0, "^alpha", PATTERN).len(), 1);
        assert_eq!(find_in(&page(text), 0, "^beta", PATTERN).len(), 0);
        assert_eq!(find_in(&page(text), 0, "beta$", PATTERN).len(), 1);
    }

    #[test]
    fn a_zero_length_match_is_not_a_match() {
        // `x*` matches the empty string at every position, which would report a
        // hit per character, each highlighting nothing.
        assert_eq!(find_in(&page("abc"), 0, "x*", PATTERN).len(), 0);
        // And a pattern that can match empty still finds what it really matches.
        assert_eq!(find_in(&page("abc"), 0, "b*", PATTERN).len(), 1);
    }

    #[test]
    fn a_hit_after_a_wide_character_lands_on_the_right_character() {
        // The one test that can only pass if the byte-offset map is right.
        // `regex` reports byte offsets and everything else here counts
        // characters: `äöü ` is four characters and seven bytes, so a matcher
        // that used the byte offset as an index would report the hit three
        // characters late --- past the end of `raster`, still inside the page,
        // and with a plausible-looking highlight three letters to the right.
        let text = "äöü raster";
        let found = find_in(&page(text), 0, "raster", PATTERN);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start, 4);
        assert_eq!(covered(text, &found[0]), "raster");
    }

    #[test]
    fn a_pattern_folds_case_with_the_same_switch_a_literal_does() {
        let text = "Raster raster";
        assert_eq!(find_in(&page(text), 0, "raster", PATTERN).len(), 2);
        let cased = Options {
            regex: true,
            ..CASED
        };
        assert_eq!(find_in(&page(text), 0, "raster", cased).len(), 1);
    }

    #[test]
    fn an_upper_case_pattern_matches_lower_case_text() {
        // The direction the test above cannot reach. It uses a *lowercase* pattern
        // against mixed-case text, which agrees with a lowercased haystack whether
        // or not the pattern is compiled case-insensitively --- so an uppercase
        // pattern matching nothing at all went unnoticed until a corpus turned up
        // whose text was uppercase.
        let text = "raster appearance";
        assert_eq!(find_in(&page(text), 0, "RASTER", PATTERN).len(), 1);
        assert_eq!(find_in(&page(text), 0, "R.STER", PATTERN).len(), 1);
        // And a class, which is why the pattern is not simply lowercased before
        // compiling: `[A-Z]` lowercased is `[a-z]`, and `\S` becomes `\s`.
        assert_eq!(find_in(&page(text), 0, "[A-Z]aster", PATTERN).len(), 1);
    }

    #[test]
    fn an_upper_case_pattern_still_distinguishes_case_when_asked() {
        // The other half of the switch: with `match_case` on, neither side is
        // lowercased and an uppercase pattern must mean uppercase again.
        let cased = Options {
            regex: true,
            ..CASED
        };
        assert_eq!(find_in(&page("Raster raster"), 0, "R.ster", cased).len(), 1);
        assert_eq!(find_in(&page("raster"), 0, "R.ster", cased).len(), 0);
    }

    #[test]
    fn a_class_that_negates_is_not_inverted_by_ignoring_case() {
        // `\S` must still mean "not whitespace" with the `i` flag on. A fold that
        // lowercased the pattern source would have turned it into `\s` and matched
        // the space instead of the letters, which is the opposite of what was typed
        // and would look like a match rather than an error.
        let hits = find_in(&page("ab cd"), 0, r"\S\S", PATTERN);
        assert_eq!(hits.len(), 2);
        assert_eq!(covered("ab cd", &hits[0]), "ab");
    }

    #[test]
    fn whole_word_still_applies_to_a_pattern() {
        let text = "cat cathode";
        let words = Options {
            regex: true,
            ..WORDS
        };
        assert_eq!(find_in(&page(text), 0, "c.t", words).len(), 1);
        assert_eq!(find_in(&page(text), 0, "c.t", PATTERN).len(), 2);
    }

    #[test]
    fn a_pattern_that_does_not_compile_is_reported_and_not_answered() {
        // The distinction the whole `problem` field exists for: a reader typing
        // a pattern expects to get it wrong, and "no matches" for `foo(` reads
        // as a working search over a document that does not contain it.
        let broken = super::find_in(&page("foo(bar"), 0, "foo(", PATTERN);
        assert!(broken.is_err(), "expected a refusal, got {broken:?}");
        let reported = search_page(&page("foo(bar"), 2, "foo(", PATTERN, None);
        assert!(reported.matches.is_empty());
        assert!(reported.problem.is_some());
        // One line, not the crate's multi-line caret diagram, because the find
        // bar has one line to show it in.
        let problem = reported.problem.unwrap_or_default();
        assert!(!problem.contains('\n'), "not one line: {problem:?}");
        // And the same text as a literal is a perfectly good query, which is
        // what says the refusal is about the pattern and not about the page.
        assert_eq!(find_in(&page("foo(bar"), 0, "foo(", PLAIN).len(), 1);
    }

    #[test]
    fn a_pattern_too_large_to_compile_is_refused_rather_than_built() {
        // Small to type, large to compile. The engine is linear-time, so this
        // is a bound on space rather than on backtracking.
        let huge = "a{1000}{1000}{1000}";
        assert!(super::find_in(&page("aaa"), 0, huge, PATTERN).is_err());
    }

    #[test]
    fn an_empty_pattern_matches_nothing() {
        assert_eq!(find_in(&page("abc"), 0, "", PATTERN).len(), 0);
        // A pattern *of* whitespace is a real query, unlike a literal one: the
        // literal guard exists because the fold has already destroyed the only
        // distinction two spaces could be drawing, and `\s+` is not that.
        assert_eq!(find_in(&page("a b"), 0, "\\s+", PATTERN).len(), 1);
        assert_eq!(find_in(&page("a b"), 0, " ", PLAIN).len(), 0);
    }
}
