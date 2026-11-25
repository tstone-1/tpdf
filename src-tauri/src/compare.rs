//! What changed between two documents' words.
//!
//! **Text only, and it says so.** Two documents are compared as the sequence of
//! words the text extraction reads out of each, in the order it reads them ---
//! the same characters search, selection and the accessibility tree read
//! (`text.rs`). A change in a picture, a drawing, a colour, a font or the
//! position of a line is not a change of words, and nothing here can see it.
//! Every caller repeats that sentence to the reader rather than letting "no
//! differences" stand for "the same document".
//!
//! **Whitespace is not a word.** A word is a run of characters between
//! whitespace, so text that only moved to another line, or to the next page, is
//! the same text. A soft hyphen is a hint about where a word may break and is
//! left out of the word, as search leaves it out.
//!
//! **A page with no text is not a page with no words.** A scan holds words
//! nobody has read, so unmatched words standing where the other side has such
//! a page are not compared, rather than listed as removed or added
//! ([`Comparison::left_unread`]).
//!
//! **The words are compared exactly.** Case is a change: `Euro` against `euro`
//! is something a reader checking a contract wants to see.
//!
//! ## The algorithm
//!
//! A patience diff: the words that occur exactly once on each side are paired,
//! the longest run of those pairs that is in the same order on both sides is
//! kept as fixed points, and the stretches between them are compared the same
//! way. Common words at either end of a stretch are taken off before it is
//! looked at. A stretch with no unique word in common is aligned by a
//! longest-common-subsequence table when it is small enough
//! ([`MAX_TABLE_CELLS`]) and is otherwise reported as one change, whole. That
//! last rule is a bound and not a judgement: it only happens between two
//! stretches that share no word that is unique to both, which is text that has
//! been rewritten rather than edited.
//!
//! Patience rather than a plain minimum edit script because a document's most
//! common words --- `the`, `of`, a figure that recurs --- are the ones a minimum
//! script lines up across a long distance to save one edit, pairing a `the` in
//! one paragraph with a `the` three paragraphs on. A word that occurs once on
//! each side is almost always the same place in both, so the fixed points are
//! sound and the table only ever sees the short stretches between them.
//!
//! Everything is bounded: the words on each side ([`MAX_WORDS`]), the cells of a
//! table, the depth of the recursion, and the number of changes reported
//! ([`MAX_CHANGES`]). A bound that is reached is said in the answer, never
//! passed over.

use std::collections::{HashMap, HashSet};

/// The most words one side may have.
///
/// About 4,000 dense pages. A side with more is refused rather than cut, since
/// a comparison that stops part-way reads as two documents that agree from
/// there on.
pub const MAX_WORDS: usize = 2_000_000;

/// Maximum extracted characters per document, including whitespace.
pub const MAX_CHARACTERS: usize = 16_000_000;

/// Tokenized pages accumulated as the worker reads them. Plain CLI output
/// can release each page's character boxes immediately.
#[derive(Default)]
pub(crate) struct ComparisonInput {
    words: Vec<Word>,
    pages: Pages,
    characters: usize,
    continues: bool,
}

impl ComparisonInput {
    pub(crate) fn add(&mut self, page: u32, codes: &[u32], which: &str) -> Result<(), String> {
        if codes.len() > MAX_CHARACTERS - self.characters {
            return Err(said!(
                "the {which} document exceeds the character limit for comparison"
            ));
        }
        self.characters += codes.len();
        let before = self.words.len();
        let join = self.continues
            && codes
                .first()
                .and_then(|&c| char::from_u32(c))
                .is_some_and(char::is_alphanumeric);
        append_words(
            &mut self.words,
            page,
            codes,
            which,
            MAX_WORDS + usize::from(join),
        )?;
        let readable = self.words.len() > before;
        // PDFium can include trailing line breaks after a discretionary hyphen.
        self.continues = readable
            && self.words.last().is_some_and(|word| {
                let end = word.end as usize;
                codes[end - 1] == u32::from(SOFT_HYPHEN)
                    && codes[end..].iter().all(|c| matches!(c, 13 | 10))
            });
        if join {
            let tail = self.words.remove(before);
            self.pages
                .continuations
                .entry(before - 1)
                .or_default()
                .push(tail.span());
            self.words[before - 1].text.push_str(&tail.text);
        }
        self.pages.position.insert(page, self.pages.count);
        if !readable {
            self.pages.unread.push((self.pages.count, page));
        }
        self.pages.count += 1;
        Ok(())
    }

    pub(crate) fn page_count(&self) -> usize {
        self.pages.count
    }
}

/// The most changes one comparison reports.
///
/// Past this the documents are not two versions of one text in any sense a
/// list can help with; the answer says how many there were and lists these.
pub const MAX_CHANGES: usize = 10_000;

/// The largest stretch, in cells (`left words x right words`), aligned by a
/// table. 4,000,000 cells is 16 MB of `u32` and well under a second.
pub const MAX_TABLE_CELLS: usize = 4_000_000;

/// How deep the anchoring may recurse before a stretch is aligned by the table
/// or reported whole. Each level needs a word unique within a smaller stretch,
/// so real documents stop after a handful; the bound is for a crafted one.
const MAX_DEPTH: u32 = 48;

/// The most characters of a change's words quoted in its row.
pub const QUOTE_CHARS: usize = 160;

/// The words of context quoted before and after a change.
const CONTEXT_WORDS: usize = 4;

const SOFT_HYPHEN: char = '\u{00ad}';

/// One page's characters, as the text extraction gave them.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct PageCodes {
    /// The page, counted from 0, in whatever numbering the caller uses --- the
    /// window sends its slots and the command line its file pages. It is
    /// handed back unchanged in every [`Span`].
    pub page: u32,
    /// One Unicode scalar per character index, as `PageText::codes`.
    pub codes: Vec<u32>,
}

/// One word: where it is, and what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Word {
    page: u32,
    /// First character index.
    start: u32,
    /// Exclusive end.
    end: u32,
    /// The word's characters, soft hyphens left out.
    text: String,
}

impl Word {
    fn span(&self) -> Span {
        Span {
            page: self.page,
            start: self.start,
            end: self.end,
        }
    }
}

/// A run of characters on one page that a change covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Span {
    /// The page, in the caller's numbering.
    pub page: u32,
    /// First character index.
    pub start: u32,
    /// Exclusive end.
    pub end: u32,
}

/// Where a change sits on a side it has no words on: just after the last word
/// the two sides still share, so the reader can be shown the place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Place {
    /// The page, in the caller's numbering.
    pub page: u32,
    /// The character index the missing words would start at.
    pub index: u32,
}

/// What kind of change a [`Change`] is, from the left document to the right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Words on the left only.
    Removed,
    /// Words on the right only.
    Added,
    /// Words on the left that the right has others in place of.
    Replaced,
}

/// One change.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Change {
    pub kind: Kind,
    /// The left document's words, one span per page they are on; empty when
    /// the change is [`Kind::Added`].
    pub left: Vec<Span>,
    /// The right document's, likewise; empty when it is [`Kind::Removed`].
    pub right: Vec<Span>,
    /// Where the change is on the left, which for an addition is where the
    /// words would have been.
    pub left_at: Place,
    /// Where it is on the right, likewise.
    pub right_at: Place,
    /// The left words, joined by single spaces and cut at [`QUOTE_CHARS`]
    /// with an ellipsis; empty for an addition.
    pub removed: String,
    /// The right words, likewise; empty for a removal.
    pub added: String,
    /// Up to four shared words before the change, for its row.
    pub before: String,
    /// Up to four shared words after it.
    pub after: String,
}

/// Two documents compared.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Comparison {
    /// How many words the left document has.
    pub left_words: u32,
    /// And the right.
    pub right_words: u32,
    /// The left words the right does not have, counting replaced ones.
    pub removed_words: u32,
    /// The right words the left does not have, counting replacements.
    pub added_words: u32,
    /// The changes in reading order, at most [`MAX_CHANGES`].
    pub changes: Vec<Change>,
    /// How many changes there were; more than `changes.len()` only when the
    /// list was cut at [`MAX_CHANGES`].
    pub total_changes: u32,
    /// Compared stretches too large to align word by word, each counted as one
    /// replacement, including changes beyond the list limit.
    pub unaligned: u32,
    /// Left words not compared: they stand where the right document has a page
    /// with no text, so whether that page holds them cannot be known. Not in
    /// `changes`, `removed_words` or `added_words`.
    pub left_unread: u32,
    /// Right words not compared, likewise against a page of the left.
    pub right_unread: u32,
    /// Caller-supplied page numbers that yielded no words after tokenization.
    pub left_without_words: Vec<u32>,
    pub right_without_words: Vec<u32>,
}

/// Compares the words of two documents.
///
/// # Errors
///
/// A side with more than [`MAX_WORDS`] words, as one sentence.
pub fn compare(left: &[PageCodes], right: &[PageCodes]) -> Result<Comparison, String> {
    let collect = |pages: &[PageCodes], which: &str| {
        let mut input = ComparisonInput::default();
        for page in pages {
            input.add(page.page, &page.codes, which)?;
        }
        Ok::<_, String>(input)
    };
    Ok(compare_inputs(
        &collect(left, "first")?,
        &collect(right, "second")?,
    ))
}

pub(crate) fn compare_inputs(left: &ComparisonInput, right: &ComparisonInput) -> Comparison {
    let left_words = &left.words;
    let right_words = &right.words;
    // Interned, so the diff compares integers and the table holds `u32`.
    let mut ids: HashMap<&str, u32> = HashMap::new();
    let a = intern(&mut ids, left_words);
    let b = intern(&mut ids, right_words);

    let mut aligner = Aligner {
        a: &a,
        b: &b,
        pairs: Vec::new(),
        fallback_starts: Vec::new(),
    };
    aligner.align(0..a.len(), 0..b.len(), 0);
    let Aligner {
        mut pairs,
        fallback_starts,
        ..
    } = aligner;
    pairs.sort_unstable();

    let mut changes = Vec::new();
    let mut total = 0usize;
    let mut removed_words = 0usize;
    let mut added_words = 0usize;
    let mut left_unread = 0usize;
    let mut right_unread = 0usize;
    // Each gap lies between consecutive runs of shared words. Only those runs
    // can supply shared context, even when another edit lies a few words away.
    let mut unaligned = 0;
    let mut previous_run = 0;
    let mut ai = 0usize;
    let mut bi = 0usize;
    let runs = pairs.chunk_by(|&(a1, b1), &(a2, b2)| a2 == a1 + 1 && b2 == b1 + 1);
    for run in runs.chain(std::iter::once(&[][..])) {
        let (pa, pb) = run.first().copied().unwrap_or((a.len(), b.len()));
        // Words with no match, where the other side has a page with no text
        // in the same place, may be on that page: they are not compared, and
        // listing them as removed or added would say something nobody read.
        let mut removed = ai..pa;
        let mut added = bi..pb;
        if !removed.is_empty() && right.pages.unread_between(right_words, bi, pb) {
            left_unread += removed.len();
            removed.end = ai;
        }
        if !added.is_empty() && left.pages.unread_between(left_words, ai, pa) {
            right_unread += added.len();
            added.end = bi;
        }
        let fallback = fallback_starts.binary_search(&ai).is_ok();
        let mut record = |removed: std::ops::Range<usize>, added: std::ops::Range<usize>| {
            removed_words += removed.len();
            added_words += added.len();
            let before = if removed.start == ai && added.start == bi {
                previous_run
            } else {
                0
            };
            let after = if (removed.is_empty() || removed.end == pa)
                && (added.is_empty() || added.end == pb)
            {
                run.len()
            } else {
                0
            };
            total += 1;
            if changes.len() < MAX_CHANGES {
                changes.push(change(
                    left,
                    removed,
                    right,
                    added,
                    ai - before.min(CONTEXT_WORDS)..ai,
                    pa..pa + after.min(CONTEXT_WORDS),
                ));
            }
        };
        if fallback && removed.is_empty() && !added.is_empty() && ai < pa {
            left_unread -= compare_beside_scan(&b[bi..pb], &a[ai..pa], |r| {
                record(ai..ai, bi + r.start..bi + r.end)
            });
        } else if fallback && added.is_empty() && !removed.is_empty() && bi < pb {
            right_unread -= compare_beside_scan(&a[ai..pa], &b[bi..pb], |r| {
                record(ai + r.start..ai + r.end, bi..bi)
            });
        } else if !removed.is_empty() || !added.is_empty() {
            if fallback && !removed.is_empty() && !added.is_empty() {
                unaligned += 1;
            }
            record(removed, added);
        }
        if let Some(&(last_a, last_b)) = run.last() {
            ai = last_a + 1;
            bi = last_b + 1;
        }
        previous_run = run.len();
    }

    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    Comparison {
        left_words: count(left_words.len()),
        right_words: count(right_words.len()),
        removed_words: count(removed_words),
        added_words: count(added_words),
        changes,
        total_changes: count(total),
        unaligned,
        left_unread: count(left_unread),
        right_unread: count(right_unread),
        left_without_words: left.pages.unread.iter().map(|&(_, page)| page).collect(),
        right_without_words: right.pages.unread.iter().map(|&(_, page)| page).collect(),
    }
}

/// Mark only words absent from the opposite passage: an extra occurrence of a
/// repeated word cannot safely be located. Shared words count as compared only
/// if removing the side-specific words leaves identical sequences in order.
fn compare_beside_scan(
    words: &[u32],
    opposite: &[u32],
    mut record: impl FnMut(std::ops::Range<usize>),
) -> usize {
    let opposite_ids: HashSet<_> = opposite.iter().copied().collect();
    let mut start = None;
    let mut shared_ids = HashSet::new();
    let mut shared_count = 0;
    for (i, id) in words.iter().enumerate() {
        if opposite_ids.contains(id) {
            shared_ids.insert(*id);
            shared_count += 1;
            if let Some(from) = start.take() {
                record(from..i);
            }
        } else {
            start.get_or_insert(i);
        }
    }
    if let Some(from) = start {
        record(from..words.len());
    }
    let shared = words.iter().filter(|id| opposite_ids.contains(id));
    if shared.eq(opposite.iter().filter(|id| shared_ids.contains(id))) {
        shared_count
    } else {
        0
    }
}

/// One side's pages in the order given, and which of them have no words.
#[derive(Default)]
struct Pages {
    /// Each page number's position in the order given.
    position: HashMap<u32, usize>,
    /// Further page spans, only for words split across pages.
    continuations: HashMap<usize, Vec<Span>>,
    /// Positions of pages with no words, ascending, and their caller-supplied page IDs.
    unread: Vec<(usize, u32)>,
    count: usize,
}

impl Pages {
    fn last_span(&self, words: &[Word], i: usize) -> Span {
        self.continuations
            .get(&i)
            .and_then(|v| v.last())
            .copied()
            .unwrap_or_else(|| words[i].span())
    }

    /// Whether a page with no words lies between word `first - 1` and word
    /// `next` of `words`, both pages included: the stretch of this side where
    /// the other side's unmatched words would stand. The start of the side
    /// and its end stand in for a word that is not there.
    fn unread_between(&self, words: &[Word], first: usize, next: usize) -> bool {
        if self.unread.is_empty() {
            return false;
        }

        let from = first
            .checked_sub(1)
            .and_then(|i| self.position.get(&self.last_span(words, i).page).copied())
            .unwrap_or(0);
        let to = words
            .get(next)
            .and_then(|word| self.position.get(&word.page).copied())
            .unwrap_or(self.count.saturating_sub(1));
        let first = self
            .unread
            .partition_point(|&(position, _)| position < from);
        self.unread
            .get(first)
            .is_some_and(|&(position, _)| position <= to)
    }
}

/// Each word's id, the same id for the same characters on either side.
fn intern<'w>(ids: &mut HashMap<&'w str, u32>, side: &'w [Word]) -> Vec<u32> {
    side.iter()
        .map(|word| {
            let next = u32::try_from(ids.len()).unwrap_or(u32::MAX);
            *ids.entry(word.text.as_str()).or_insert(next)
        })
        .collect()
}

fn wrapped_separator(codes: &[u32], index: usize) -> bool {
    let (start, end) = match codes[index] {
        13 => (
            index,
            index + 1 + usize::from(codes.get(index + 1) == Some(&10)),
        ),
        10 => (
            index.saturating_sub(usize::from(index > 0 && codes[index - 1] == 13)),
            index + 1,
        ),
        _ => return false,
    };
    start > 0
        && codes[start - 1] == u32::from(SOFT_HYPHEN)
        && codes
            .get(end)
            .and_then(|&c| char::from_u32(c))
            .is_some_and(char::is_alphanumeric)
}

/// The words of one side, in page order as given.
fn append_words(
    out: &mut Vec<Word>,
    page: u32,
    codes: &[u32],
    which: &str,
    limit: usize,
) -> Result<(), String> {
    let mut current: Option<Word> = None;
    for (index, code) in codes.iter().enumerate() {
        // Some producers include a line separator even at a discretionary
        // break. Leave its original index in the span when the word continues.
        if current.is_some() && wrapped_separator(codes, index) {
            continue;
        }
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        // A code that is not a scalar value separates words, as whitespace
        // does: it has no character to compare.
        let ch = char::from_u32(*code).filter(|ch| !ch.is_whitespace() && !ch.is_control());
        match ch {
            Some(SOFT_HYPHEN) => {
                if let Some(word) = current.as_mut() {
                    word.end = index + 1;
                }
            }
            Some(ch) => {
                let word = current.get_or_insert_with(|| Word {
                    page,
                    start: index,
                    end: index,
                    text: String::new(),
                });
                word.text.push(ch);
                word.end = index + 1;
            }
            None => {
                if let Some(word) = current.take() {
                    out.push(word);
                }
            }
        }
        if out.len() > limit {
            return Err(said!(
                "the {which} document has more than {MAX_WORDS} words, which is more \
                     than tpdf compares"
            ));
        }
    }
    if let Some(word) = current.take() {
        out.push(word);
    }
    if out.len() > limit {
        return Err(said!(
            "the {which} document has more than {MAX_WORDS} words, which is more than tpdf \
             compares"
        ));
    }
    Ok(())
}

/// The words of `range` as per-page spans, first word's start to last word's
/// end on each page.
fn spans(side: &ComparisonInput, range: std::ops::Range<usize>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    for i in range {
        let spans = std::iter::once(side.words[i].span()).chain(
            side.pages
                .continuations
                .get(&i)
                .into_iter()
                .flatten()
                .copied(),
        );
        for span in spans {
            match out.last_mut() {
                Some(last) if last.page == span.page => last.end = span.end,
                _ => out.push(span),
            }
        }
    }
    out
}

/// Where a change at word `at` of `side` is: that word's start, or, at the
/// end of the side, just after its last word.
fn place(side: &ComparisonInput, at: usize) -> Place {
    match (side.words.get(at), side.words.len().checked_sub(1)) {
        (Some(word), _) => Place {
            page: word.page,
            index: word.start,
        },
        (None, Some(i)) => {
            let last = side.pages.last_span(&side.words, i);
            Place {
                page: last.page,
                index: last.end,
            }
        }
        (None, None) => Place { page: 0, index: 0 },
    }
}

/// The words of `range` joined by single spaces, cut at `limit` characters.
fn quote(side: &[Word], range: std::ops::Range<usize>, limit: usize) -> String {
    let mut out = String::new();
    let mut chars = 0usize;
    for (index, word) in side[range].iter().enumerate() {
        let separator = (index > 0).then_some(' ');
        for ch in separator.into_iter().chain(word.text.chars()) {
            if chars >= limit {
                out.push('\u{2026}');
                return out;
            }
            out.push(ch);
            chars += 1;
        }
    }
    out
}

fn change(
    left: &ComparisonInput,
    a: std::ops::Range<usize>,
    right: &ComparisonInput,
    b: std::ops::Range<usize>,
    before: std::ops::Range<usize>,
    after: std::ops::Range<usize>,
) -> Change {
    let kind = match (a.is_empty(), b.is_empty()) {
        (false, true) => Kind::Removed,
        (true, false) => Kind::Added,
        _ => Kind::Replaced,
    };
    Change {
        kind,
        left: spans(left, a.clone()),
        right: spans(right, b.clone()),
        left_at: place(left, a.start),
        right_at: place(right, b.start),
        removed: quote(&left.words, a, QUOTE_CHARS),
        added: quote(&right.words, b, QUOTE_CHARS),
        before: quote(&left.words, before, QUOTE_CHARS),
        after: quote(&left.words, after, QUOTE_CHARS),
    }
}

/// The diff's state: the two id sequences, and the pairs of equal words found.
struct Aligner<'s> {
    a: &'s [u32],
    b: &'s [u32],
    /// `(left index, right index)` of each word the two sides share.
    pairs: Vec<(usize, usize)>,
    /// Left start of each unaligned gap, in traversal order.
    fallback_starts: Vec<usize>,
}

impl Aligner<'_> {
    /// Aligns `a` against `b`, adding every shared word to `pairs`.
    fn align(&mut self, mut a: std::ops::Range<usize>, mut b: std::ops::Range<usize>, depth: u32) {
        // Common words at the front and the back.
        while !a.is_empty() && !b.is_empty() && self.a[a.start] == self.b[b.start] {
            self.pairs.push((a.start, b.start));
            a.start += 1;
            b.start += 1;
        }
        while !a.is_empty() && !b.is_empty() && self.a[a.end - 1] == self.b[b.end - 1] {
            self.pairs.push((a.end - 1, b.end - 1));
            a.end -= 1;
            b.end -= 1;
        }
        if a.is_empty() || b.is_empty() {
            return;
        }

        if depth < MAX_DEPTH {
            let anchors = self.anchors(a.clone(), b.clone());
            if !anchors.is_empty() {
                let (mut ai, mut bi) = (a.start, b.start);
                for (pa, pb) in anchors {
                    self.align(ai..pa, bi..pb, depth + 1);
                    self.pairs.push((pa, pb));
                    ai = pa + 1;
                    bi = pb + 1;
                }
                self.align(ai..a.end, bi..b.end, depth + 1);
                return;
            }
        }

        if a.len().saturating_mul(b.len()) <= MAX_TABLE_CELLS {
            self.table(a, b);
        } else {
            self.fallback_starts.push(a.start);
        }
    }

    /// The words unique to both stretches, paired, and cut to the longest run
    /// in the same order on both sides.
    fn anchors(&self, a: std::ops::Range<usize>, b: std::ops::Range<usize>) -> Vec<(usize, usize)> {
        // id -> (count in a, index in a, count in b, index in b)
        let mut seen: HashMap<u32, (u32, usize, u32, usize)> = HashMap::new();
        for i in a.clone() {
            let entry = seen.entry(self.a[i]).or_insert((0, i, 0, 0));
            entry.0 += 1;
            entry.1 = i;
        }
        for j in b {
            if let Some(entry) = seen.get_mut(&self.b[j]) {
                entry.2 += 1;
                entry.3 = j;
            }
        }
        let mut unique: Vec<(usize, usize)> = seen
            .values()
            .filter(|(ca, _, cb, _)| *ca == 1 && *cb == 1)
            .map(|(_, i, _, j)| (*i, *j))
            .collect();
        unique.sort_unstable();
        longest_increasing(&unique)
    }

    /// A longest common subsequence of two small stretches, by table.
    fn table(&mut self, a: std::ops::Range<usize>, b: std::ops::Range<usize>) {
        let (n, m) = (a.len(), b.len());
        let width = m + 1;
        // lengths[i * width + j]: the LCS of a[i..] and b[j..].
        let mut lengths = vec![0u32; (n + 1) * width];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lengths[i * width + j] = if self.a[a.start + i] == self.b[b.start + j] {
                    lengths[(i + 1) * width + j + 1] + 1
                } else {
                    lengths[(i + 1) * width + j].max(lengths[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if self.a[a.start + i] == self.b[b.start + j] {
                self.pairs.push((a.start + i, b.start + j));
                i += 1;
                j += 1;
            } else if lengths[(i + 1) * width + j] >= lengths[i * width + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
}

/// The longest subsequence of `pairs` (sorted by their first member) whose
/// second members increase, by patience sorting.
fn longest_increasing(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    // tails[k]: index into `pairs` of the smallest tail of an increasing run of
    // length k + 1; back[i]: the element before pairs[i] in its run.
    let mut tails: Vec<usize> = Vec::new();
    let mut back: Vec<Option<usize>> = vec![None; pairs.len()];
    for (i, &(_, j)) in pairs.iter().enumerate() {
        let k = tails.partition_point(|&t| pairs[t].1 < j);
        back[i] = k.checked_sub(1).map(|k| tails[k]);
        if k == tails.len() {
            tails.push(i);
        } else {
            tails[k] = i;
        }
    }
    let mut out = Vec::with_capacity(tails.len());
    let mut at = tails.last().copied();
    while let Some(i) = at {
        out.push(pairs[i]);
        at = back[i];
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(page: u32, text: &str) -> PageCodes {
        PageCodes {
            page,
            codes: text.chars().map(u32::from).collect(),
        }
    }

    fn run(left: &[(u32, &str)], right: &[(u32, &str)]) -> Comparison {
        let l: Vec<PageCodes> = left.iter().map(|(p, t)| page(*p, t)).collect();
        let r: Vec<PageCodes> = right.iter().map(|(p, t)| page(*p, t)).collect();
        compare(&l, &r).expect("compares")
    }

    /// `(kind, removed, added)` of each change.
    fn summary(c: &Comparison) -> Vec<(Kind, &str, &str)> {
        c.changes
            .iter()
            .map(|ch| (ch.kind, ch.removed.as_str(), ch.added.as_str()))
            .collect()
    }

    #[test]
    fn identical_text_has_no_change() {
        let c = run(
            &[(0, "The term is 30 days.")],
            &[(0, "The term is 30 days.")],
        );
        assert!(c.changes.is_empty());
        assert_eq!((c.left_words, c.right_words, c.total_changes), (5, 5, 0));
    }

    #[test]
    fn text_that_only_moved_between_lines_and_pages_is_unchanged() {
        let c = run(
            &[(0, "one two\r\nthree"), (1, "four  five")],
            &[(0, "one"), (1, "two three four\nfive")],
        );
        assert!(c.changes.is_empty(), "{:?}", c.changes);
    }

    #[test]
    fn a_replaced_word_is_one_change_with_both_sides_placed() {
        let c = run(
            &[(0, "The term is 30 days.")],
            &[(0, "The term is 45 days.")],
        );
        assert_eq!(summary(&c), vec![(Kind::Replaced, "30", "45")]);
        let ch = &c.changes[0];
        assert_eq!(
            ch.left,
            vec![Span {
                page: 0,
                start: 12,
                end: 14
            }]
        );
        assert_eq!(
            ch.right,
            vec![Span {
                page: 0,
                start: 12,
                end: 14
            }]
        );
        assert_eq!(
            (ch.before.as_str(), ch.after.as_str()),
            ("The term is", "days.")
        );
        assert_eq!((c.removed_words, c.added_words), (1, 1));
    }

    #[test]
    fn an_addition_has_no_left_span_and_a_place_on_the_left() {
        let c = run(&[(0, "pay within days")], &[(0, "pay within 30 days")]);
        assert_eq!(summary(&c), vec![(Kind::Added, "", "30")]);
        let ch = &c.changes[0];
        assert!(ch.left.is_empty());
        // Where "days" starts on the left: the 30 would have stood there.
        assert_eq!(ch.left_at, Place { page: 0, index: 11 });
        assert_eq!(ch.right_at, Place { page: 0, index: 11 });
    }

    #[test]
    fn a_removal_at_the_end_is_placed_after_the_last_right_word() {
        let c = run(&[(0, "a b c")], &[(0, "a b")]);
        assert_eq!(summary(&c), vec![(Kind::Removed, "c", "")]);
        assert_eq!(c.changes[0].right_at, Place { page: 0, index: 3 });
    }

    #[test]
    fn case_is_a_change() {
        let c = run(&[(0, "in Euro")], &[(0, "in euro")]);
        assert_eq!(summary(&c), vec![(Kind::Replaced, "Euro", "euro")]);
    }

    #[test]
    fn a_soft_hyphen_is_not_part_of_the_word_but_is_inside_its_span() {
        let c = run(&[(0, "exam\u{ad}ple x")], &[(0, "example y")]);
        assert_eq!(summary(&c), vec![(Kind::Replaced, "x", "y")]);
    }

    #[test]
    fn only_an_explicit_discretionary_wrap_joins_the_next_line() {
        for end in ["\n", "\r", "\r\n"] {
            let soft = format!("discre\u{ad}{end}tionary");
            let separate = format!("discre{end}tionary");
            assert!(run(&[(0, &soft)], &[(0, "discretionary")])
                .changes
                .is_empty());
            assert_eq!(
                run(&[(0, &separate)], &[(0, "discretionary")]).total_changes,
                1
            );
            assert_eq!(
                run(&[(0, &soft)], &[(0, "replaced")]).changes[0].left[0].end as usize,
                soft.chars().count()
            );
        }
    }

    #[test]
    fn a_change_over_a_page_break_has_one_span_per_page() {
        let c = run(
            &[(0, "keep old"), (1, "words keep")],
            &[(0, "keep"), (1, "keep")],
        );
        assert_eq!(summary(&c), vec![(Kind::Removed, "old words", "")]);
        assert_eq!(
            c.changes[0].left,
            vec![
                Span {
                    page: 0,
                    start: 5,
                    end: 8
                },
                Span {
                    page: 1,
                    start: 0,
                    end: 5
                }
            ]
        );
    }

    #[test]
    fn a_repeated_word_is_not_paired_across_the_document() {
        // A minimum script would keep the left `the` paired with the right
        // `the` and call everything around it changed; the unique words `X`,
        // `Y` and `Z` anchor first, and only the inserted sentence is new.
        let c = run(
            &[(0, "X the a Y b Z")],
            &[(0, "the new start X the a Y b Z")],
        );
        assert_eq!(summary(&c), vec![(Kind::Added, "", "the new start")]);
    }

    #[test]
    fn a_word_repeated_on_the_older_side_is_not_an_anchor() {
        // `the` is on the left twice and on the right once. Paired as an
        // anchor, its last left `the` would hold the right one, out of order
        // with `B`, and `B` would read as removed and added again.
        let c = run(&[(0, "Z the B the Q")], &[(0, "W the B R")]);
        assert_eq!((c.removed_words, c.added_words), (3, 2));
    }

    #[test]
    fn a_moved_paragraph_is_a_removal_and_an_addition() {
        let c = run(&[(0, "A1 A2 B1 B2 C1")], &[(0, "B1 B2 A1 A2 C1")]);
        let removed: usize = c.removed_words as usize;
        let added: usize = c.added_words as usize;
        assert_eq!((removed, added), (2, 2));
        assert_eq!(c.total_changes, 2);
    }

    #[test]
    fn a_stretch_with_no_unique_word_in_common_is_aligned_by_the_table() {
        // Every word but the first occurs twice on some side, so nothing
        // anchors; the LCS of "a a b b a" and "a b b a a" is "a b b a".
        let c = run(&[(0, "x a a b b a")], &[(0, "y a b b a a")]);
        assert_eq!(c.removed_words, 2);
        assert_eq!(c.added_words, 2);
        assert_eq!(c.unaligned, 0);
    }

    #[test]
    fn a_stretch_too_large_for_the_table_is_reported_whole_and_counted() {
        // Every word occurs twice on each side, so nothing anchors, and the
        // stretch is past the table's bound.
        let side = |w: &str| {
            (0..2_100)
                .map(|i| format!("{w}{} {w}{}", i % 1_050, i % 1_050))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let left = side("p");
        let right = side("q");
        let c = run(&[(0, &left)], &[(0, &right)]);
        assert_eq!(c.unaligned, 1);
        assert_eq!(c.total_changes, 1);
        assert_eq!(c.changes[0].kind, Kind::Replaced);
    }

    #[test]
    fn an_unaligned_gap_facing_an_unread_page_is_not_a_replacement() {
        let left = "old ".repeat(2100);
        let right = "new ".repeat(2100);
        let c = run(
            &[(0, "header"), (1, &left)],
            &[(0, "header"), (1, "\u{ad}"), (2, &right)],
        );
        assert_eq!((c.total_changes, c.unaligned), (1, 0));
        assert_eq!((c.left_unread, c.right_unread), (2100, 0));
        assert_eq!(c.changes[0].kind, Kind::Added);
        assert_eq!(c.added_words, 2100);
        assert_eq!(c.right_without_words, vec![1]);
        assert!(c.left_without_words.is_empty());
    }

    #[test]
    fn an_unaligned_passage_beside_a_scan_only_marks_excess_occurrences() {
        let old = format!("{}old", "old shared ".repeat(2100));
        let new = format!("{}new", "new shared ".repeat(2100));
        let result = run(&[(0, &old)], &[(0, ""), (1, &new)]);
        assert_eq!(result.left_unread, 2101);
        let reversed = run(&[(0, ""), (1, &new)], &[(0, &old)]);
        assert_eq!(
            (reversed.removed_words, reversed.right_unread),
            (2101, 2101)
        );
        assert_eq!(result.added_words, 2101);
        assert_eq!(result.removed_words, 0);
        assert!(result.changes.iter().all(|c| c.added == "new"));
    }

    #[test]
    fn repeated_words_beside_a_scan_do_not_get_an_unproven_change_location() {
        let old = "paid due ".repeat(1100);
        let new = format!("due {old}pending");
        let result = run(&[(0, &old)], &[(0, ""), (1, &new)]);
        assert_eq!(result.total_changes, 1);
        assert_eq!(result.changes[0].added, "pending");
        assert_eq!(result.added_words, 1);
        assert!(result.left_unread > 0);
    }

    #[test]
    fn discretionary_wrap_spans_pages_without_crossing_blank_pages() {
        let wrapped = [(5, "discre\u{ad}"), (3, "tion\u{ad}"), (1, "ary")];
        let same = run(&wrapped, &[(0, "discretionary")]);
        assert_eq!((same.total_changes, same.left_words), (0, 1));
        assert!(same.left_without_words.is_empty());
        let changed = run(&wrapped, &[(0, "different")]);
        assert_eq!(
            changed.changes[0]
                .left
                .iter()
                .map(|s| (s.page, s.start, s.end))
                .collect::<Vec<_>>(),
            [(5, 0, 7), (3, 0, 5), (1, 0, 3)]
        );
        for split in [
            vec![(5, "discre"), (1, "tionary")],
            vec![(5, "discre\u{ad}"), (3, ""), (1, "tionary")],
        ] {
            assert_eq!(run(&split, &[(0, "discretionary")]).left_words, 2);
        }
    }

    #[test]
    fn excerpt_ends_before_a_separator_outside_the_budget() {
        let word = "x".repeat(QUOTE_CHARS);
        let added = format!("{word} tail");
        let result = run(&[(0, "old")], &[(0, &added)]);
        assert_eq!(result.changes[0].added, format!("{word}\u{2026}"));
    }

    #[test]
    fn input_stops_at_its_character_budget_even_with_one_word() {
        let mut input = ComparisonInput {
            characters: MAX_CHARACTERS - 1,
            ..Default::default()
        };
        input.add(0, &[97], "first").unwrap();
        assert!(input.add(1, &[97], "first").is_err());
        assert_eq!(input.page_count(), 1);
    }

    #[test]
    fn a_quote_is_cut_with_an_ellipsis() {
        let long = "w".repeat(QUOTE_CHARS + 10);
        let c = run(&[(0, "a b")], &[(0, &format!("a {long} b"))]);
        let added = &c.changes[0].added;
        assert_eq!(added.chars().count(), QUOTE_CHARS + 1);
        assert!(added.ends_with('\u{2026}'));
    }

    #[test]
    fn an_unaligned_replacement_beyond_the_list_limit_is_still_counted() {
        let mut old: String = (0..MAX_CHANGES).map(|i| format!("k{i} x ")).collect();
        let mut new: String = (0..MAX_CHANGES).map(|i| format!("k{i} y ")).collect();
        old.push_str("appendix ");
        new.push_str("appendix ");
        old.push_str(&"old ".repeat(2100));
        new.push_str(&"new ".repeat(2100));
        let c = run(&[(0, &old)], &[(0, &new)]);
        assert_eq!(c.total_changes as usize, MAX_CHANGES + 1);
        assert_eq!(c.changes.len(), MAX_CHANGES);
        assert!(c.changes.iter().all(|ch| ch.removed == "x"));
        assert_eq!(c.unaligned, 1);
    }

    #[test]
    fn the_list_is_cut_at_its_bound_and_the_total_is_not() {
        let left: String = (0..MAX_CHANGES + 5).map(|i| format!("k{i} x ")).collect();
        let right: String = (0..MAX_CHANGES + 5).map(|i| format!("k{i} y ")).collect();
        let c = run(&[(0, &left)], &[(0, &right)]);
        assert_eq!(c.changes.len(), MAX_CHANGES);
        assert_eq!(c.total_changes as usize, MAX_CHANGES + 5);
    }

    #[test]
    fn a_side_with_no_text_compares_nothing_and_a_side_with_no_pages_is_one_change() {
        // A page with no text may hold every word of the other side.
        let c = run(&[(0, "")], &[(0, "new text")]);
        assert!(c.changes.is_empty(), "{:?}", summary(&c));
        assert_eq!((c.left_unread, c.right_unread), (0, 2));
        // No page at all holds nothing, so everything on the other side is added.
        let c = run(&[], &[(0, "new text")]);
        assert_eq!(summary(&c), vec![(Kind::Added, "", "new text")]);
        assert_eq!(c.changes[0].left_at, Place { page: 0, index: 0 });
    }

    #[test]
    fn page_numbers_are_handed_back_as_given() {
        let c = run(&[(7, "a b")], &[(3, "a c")]);
        assert_eq!(c.changes[0].left[0].page, 7);
        assert_eq!(c.changes[0].right[0].page, 3);
    }

    #[test]
    fn longest_increasing_keeps_the_longest_ordered_run() {
        let pairs = [(0, 3), (1, 1), (2, 2), (3, 0), (4, 4)];
        assert_eq!(longest_increasing(&pairs), vec![(1, 1), (2, 2), (4, 4)]);
    }

    /// The inspection report of the user-test kit: the final version's second
    /// page is a scan, and its words, which say the result changed, cannot be
    /// read. The draft's second page is not "removed": it is not compared.
    #[test]
    fn words_facing_a_page_with_no_text_are_not_compared() {
        let c = run(
            &[
                (0, "Sample of 32 parts drawn at random"),
                (1, "Result passed released to stock"),
            ],
            &[(0, "Sample of 32 parts drawn at random"), (1, "")],
        );
        assert!(c.changes.is_empty(), "{:?}", summary(&c));
        assert_eq!((c.total_changes, c.removed_words, c.added_words), (0, 0, 0));
        assert_eq!((c.left_unread, c.right_unread), (5, 0));

        // The other way round: an older scan, a newer digital page.
        let c = run(
            &[(0, "Sample of 32 parts"), (1, "")],
            &[(0, "Sample of 32 parts"), (1, "Result failed blocked")],
        );
        assert!(c.changes.is_empty(), "{:?}", summary(&c));
        assert_eq!((c.left_unread, c.right_unread), (0, 3));

        // A scanned page in the middle, between two read ones.
        let c = run(
            &[(0, "alpha beta"), (1, "gamma delta"), (2, "epsilon zeta")],
            &[(0, "alpha beta"), (1, ""), (2, "epsilon zeta")],
        );
        assert!(c.changes.is_empty(), "{:?}", summary(&c));
        assert_eq!(c.left_unread, 2);
    }

    /// The controls: a change on read pages is still a change when the other
    /// document has a page with no text elsewhere, and with no such page
    /// nothing is held back.
    #[test]
    fn a_change_away_from_a_page_with_no_text_is_still_listed() {
        let c = run(
            &[
                (0, "The term is 30 days net"),
                (1, "Signed by both parties"),
                (2, "Annex"),
            ],
            &[
                (0, "The term is 45 days net"),
                (1, "Signed by both parties"),
                (2, ""),
            ],
        );
        assert_eq!(summary(&c), vec![(Kind::Replaced, "30", "45")]);
        assert_eq!(
            (c.left_unread, c.right_unread),
            (1, 0),
            "only Annex faces the scan"
        );

        let c = run(
            &[(0, "Result passed released to stock")],
            &[(0, "Result failed released to stock")],
        );
        assert_eq!(summary(&c), vec![(Kind::Replaced, "passed", "failed")]);
        assert_eq!((c.left_unread, c.right_unread), (0, 0));

        // A blank page on both sides holds back nothing either.
        let c = run(
            &[(0, "one two"), (1, ""), (2, "three")],
            &[(0, "one two"), (1, ""), (2, "three four")],
        );
        assert_eq!(summary(&c), vec![(Kind::Added, "", "four")]);
        assert_eq!((c.left_unread, c.right_unread), (0, 0));
    }
}
