//! Laying out the text inside a `/FreeText` annotation.
//!
//! **Why this module exists at all.** Every other mark is a shape: the writer is
//! told a rectangle and draws in it. A text box is told a rectangle and a string,
//! and has to decide where each line breaks — which needs the width of every
//! glyph in the font it will be drawn in. Nothing else in this repository
//! measures text, because nothing else had to.
//!
//! ## The font is Helvetica, and that decides most of what follows
//!
//! One of the fourteen standard fonts, so **no font file is embedded and none
//! has to be subsetted**. That side-steps the two traps this repository already
//! records about fonts — a subset that does not contain a glyph draws
//! `.notdef`, and an embedded font must be extended to add one — because a
//! standard font has no subset: every reader is required to have all of it.
//!
//! This writer supports the ASCII and Latin-1 subset of `/WinAnsiEncoding`.
//! Cyrillic, Greek, CJK and characters above U+00FF are refused here; WinAnsi
//! itself also defines some punctuation outside Latin-1. [`encodable`]
//! answers whether a string can be written, and the command refuses one that
//! cannot rather than drawing a box of `.notdef` boxes.
//!
//! ## The widths, and how they were checked
//!
//! [`WIDTHS`] is Helvetica's own advance widths in units of 1/1000 em, from the
//! font's metrics. **They were written out by hand, which is a bad way to
//! produce 95 numbers**, so they are not trusted on that basis: `annot-probe
//! --mode text` renders a line through PDFium — the engine that will actually
//! draw it — and compares the ink's measured width against what [`advance`]
//! predicts. A wrong entry makes the two disagree, and a table agreeing with the
//! reader that shares its assumptions would not be evidence of anything.
//!
//! Latin-1 widths need their own table: umlauts share the base vowel's width,
//! but accented lowercase i advances 278 rather than 222, and sharp s advances
//! 611 rather than s's 500. A short rendered sentence did not detect those
//! errors; the independent metrics check now enumerates the entire supported set.

/// The size a text box's words are set at, in points.
///
/// **Fixed, and the reader cannot change it yet.** Eleven is a readable
/// annotation size against body text without competing with it. Proportional to
/// the box would be wrong in the way `OUTLINE_WIDTH`'s comment describes for a
/// border: nothing about how large a reader dragged the rectangle says how large
/// they want the words.
pub const SIZE: f64 = 11.0;

/// Leading, as a multiple of [`SIZE`].
pub const LEADING: f64 = 1.2;

/// The inset from the rectangle's edge to the first glyph, in points.
///
/// Both sides, and the top. Without it the words sit hard against the border and
/// read as clipped even when every glyph is inside the box.
pub const INSET: f64 = 2.0;

// The three above live here rather than in `save.rs`, where they started,
// because `edits.rs` needs them too: the wire struct carries the wrapped lines
// so that the overlay's breaks are the file's, and wrapping needs the size and
// the inset. A layout parameter belongs with the layout.

/// Advance widths for Helvetica, in units of 1/1000 em, for ASCII 32..=126.
///
/// Indexed by `code - 32`; [`LATIN1_WIDTHS`] covers U+00A0..U+00FF.
const WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // 32..47
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, // 48..63
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, // 64..79
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, // 80..95
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // 96..111
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, // 112..126
];

// Helvetica metrics for WinAnsi codes 160..=255, including space/hyphen aliases.
// Independently checked against ReportLab's Helvetica glyph metrics:
// https://github.com/MrBitBucket/reportlab-mirror/blob/master/src/reportlab/pdfbase/_fontdata_widths_helvetica.py
const LATIN1_WIDTHS: [u16; 96] = [
    278, 333, 556, 556, 556, 556, 260, 556, 333, 737, 370, 556, 584, 333, 737, 333, 400, 584, 333,
    333, 333, 556, 537, 278, 333, 333, 365, 556, 834, 834, 834, 611, 667, 667, 667, 667, 667, 667,
    1000, 722, 667, 667, 667, 667, 278, 278, 278, 278, 722, 722, 778, 778, 778, 778, 778, 584, 778,
    722, 722, 722, 722, 667, 667, 611, 556, 556, 556, 556, 556, 556, 889, 500, 556, 556, 556, 556,
    278, 278, 278, 278, 556, 556, 556, 556, 556, 556, 556, 584, 611, 556, 556, 556, 556, 500, 556,
    500,
];

/// The exact standard-font advance, in units of 1/1000 em. Unsupported
/// characters use n's width only while displaying a draft that cannot be saved.
fn advance_of(ch: char) -> u16 {
    match ch as u32 {
        code @ 32..=126 => WIDTHS[(code - 32) as usize],
        code @ 160..=255 => LATIN1_WIDTHS[(code - 160) as usize],
        _ => 556,
    }
}

/// How wide a string is when set in Helvetica at `size` points.
///
/// Public because `annot-probe --mode text` compares it against the ink PDFium
/// actually lays down, which is the only reason to believe [`WIDTHS`].
#[must_use]
pub fn advance(text: &str, size: f64) -> f64 {
    text.chars()
        .map(|ch| f64::from(advance_of(ch)) * size / 1000.0)
        .sum()
}

/// Most characters a note may carry.
///
/// A bound on work rather than on expression, and generous enough that no reader
/// typing prose meets it --- sixty-four thousand characters is a long chapter.
/// What it stops is a *paste*: [`wrap`] runs for every text box in every edit
/// state the model produces, so one enormous string in one box makes every later
/// rotate, mark or undo re-wrap it, under the lock that serialises edits. The
/// wrap is linear in the note's length now, which turns that from unusable into
/// merely wasteful; this is what keeps it from being unbounded.
///
/// Applied to every kind's note rather than to a text box's, because the cost is
/// in holding and shipping the string, not in drawing it: `EditState` carries
/// every mark's note to the frontend on every edit.
pub const MAX_NOTE_CHARS: usize = 64 * 1024;

/// Whether every character can be written in `/WinAnsiEncoding`.
///
/// **The refusal this makes possible is the point.** Without it a reader pastes
/// a line of Greek into a text box, the writer encodes what it can, and the file
/// contains a row of substituted glyphs that looked fine on screen — the overlay
/// draws with a system font that has them. Saying no is worse for that reader
/// and honest; drawing the wrong glyphs is worse for every reader who opens the
/// file afterwards.
#[must_use]
pub fn encodable(text: &str) -> bool {
    text.chars()
        .all(|ch| ch == '\n' || (' '..='~').contains(&ch) || ('\u{a0}'..='\u{ff}').contains(&ch))
}

/// Breaks `text` into lines that each fit within `width` points.
///
/// Greedy, on spaces, honouring the reader's own newlines first. Returns at
/// least one line for a non-empty string, and an empty vector for an empty one —
/// a box with nothing typed in it draws nothing rather than one blank line,
/// which matters because the appearance stream is rebuilt from this and an empty
/// stream is what "no text yet" should look like.
///
/// **A word wider than the whole box is broken mid-word**, which is the case a
/// greedy wrap silently gets wrong: without it a pasted URL or a long German
/// compound emits one line that runs out past the rectangle and, once the
/// appearance stream's `/BBox` clips, simply disappears at the edge. Breaking it
/// is ugly and visible; overflowing is invisible and loses text.
///
/// A `width` too small for even one character yields one character per line
/// rather than looping forever — the guard is on the *progress* rather than on
/// the width, so there is no threshold to pick.
#[must_use]
pub fn wrap(text: &str, size: f64, width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if advance(&candidate, size) <= width || line.is_empty() && word.is_empty() {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            // The word alone, which may still not fit.
            if advance(word, size) <= width {
                line = word.to_string();
                continue;
            }
            let mut rest = word;
            while advance(rest, size) > width {
                // One pass over the head, accumulating. This used to re-measure
                // every prefix and re-count `rest` on each step --- both O(n)
                // inside a loop that runs n times, so breaking one long word
                // cost O(n^2), and a note has no length bound above this. The
                // result is unchanged because `advance` is a sum over
                // characters, so the running total after `k` of them *is*
                // `advance` of that prefix; the arithmetic is identical, in the
                // same order, on the same values.
                //
                // At least one character, always: `take` of zero would push an
                // empty line and leave `rest` unchanged, which is the loop that
                // does not end.
                let mut used = 0.0;
                let mut cut = 0;
                for ch in rest.chars() {
                    let next = used + f64::from(advance_of(ch)) * size / 1000.0;
                    // `cut > 0` is "at least one character has been taken",
                    // which is the guard this loop needs and the only thing a
                    // separate counter was carrying.
                    if cut > 0 && next > width {
                        break;
                    }
                    used = next;
                    cut += ch.len_utf8();
                }
                let head: String = rest[..cut].to_string();
                lines.push(head);
                rest = &rest[cut..];
            }
            line = rest.to_string();
        }
        if !line.is_empty() {
            lines.push(line);
        } else if paragraph.is_empty() {
            // An empty paragraph is a blank line the reader typed, and it has to
            // survive: two newlines between paragraphs is how anyone separates
            // them, and dropping the empty one closes the gap up.
            lines.push(String::new());
        }
        // **A non-empty paragraph that ends with nothing left over pushes
        // nothing**, and the condition was `!line.is_empty() || !paragraph.is_empty()`
        // until a test found it. A word broken mid-way consumes the whole of
        // `rest`, so `line` ends empty while `paragraph` is not — and the old
        // condition pushed that empty string as a line. One trailing blank in a
        // one-paragraph box, and a whole line of displacement for every
        // paragraph after the first in a longer one.
    }
    if lines.len() == 1 && lines[0].is_empty() {
        lines.clear();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **What these can and cannot say.** They test the *wrapping*, which is
    /// logic, against [`advance`], which is a table. A test comparing the table
    /// against itself would be a writer agreeing with its own reader — so the
    /// table is checked somewhere else entirely, by `helvetica-probe`, which
    /// measures what PDFium actually draws. These take the widths as given and
    /// ask whether the lines come out where they should.
    const SIZE_PT: f64 = 10.0;

    #[test]
    fn an_empty_note_produces_no_lines_at_all() {
        // Not one empty line. The appearance stream is built from this, and one
        // blank line is a `Tj` of nothing — which draws nothing but says the box
        // has content, and is the difference between "no text yet" and "a text
        // box whose text was lost".
        assert!(wrap("", SIZE_PT, 200.0).is_empty());
    }

    #[test]
    fn a_line_that_fits_is_left_alone() {
        assert_eq!(wrap("hello", SIZE_PT, 200.0), vec!["hello".to_string()]);
    }

    #[test]
    fn every_line_produced_fits_the_width_it_was_given() {
        // **The invariant, rather than a fixture's expected output.** A test
        // naming the exact lines would have to be rewritten whenever a width
        // changed and would say nothing about a width nobody tried; this holds
        // for whatever the wrap produces.
        let text = "the quick brown fox jumps over the lazy dog and keeps going";
        for width in [40.0, 60.0, 100.0, 150.0, 300.0] {
            for line in wrap(text, SIZE_PT, width) {
                assert!(
                    advance(&line, SIZE_PT) <= width,
                    "{line:?} is {} wide, over {width}",
                    advance(&line, SIZE_PT)
                );
            }
        }
    }

    #[test]
    fn no_word_is_lost_or_invented_by_wrapping() {
        // The other half of the invariant above, and the half that catches a
        // wrap which satisfies the width by dropping text. Joining the lines
        // back with spaces must give the original word sequence — an assertion
        // about *content* rather than about layout, which is what makes the
        // width test above safe to state loosely.
        let text = "alpha beta gamma delta epsilon zeta eta theta";
        for width in [50.0, 80.0, 120.0] {
            let joined = wrap(text, SIZE_PT, width).join(" ");
            assert_eq!(
                joined.split_whitespace().collect::<Vec<_>>(),
                text.split_whitespace().collect::<Vec<_>>(),
                "at width {width}"
            );
        }
    }

    #[test]
    fn a_word_wider_than_the_box_is_broken_rather_than_overflowing() {
        // The case a greedy wrap silently gets wrong. Without the mid-word break
        // this emits one line far wider than the box, the appearance stream's
        // /BBox clips it, and the text disappears at the edge — invisibly.
        let long = "Donaudampfschifffahrtsgesellschaftskapitaen";
        let lines = wrap(long, SIZE_PT, 40.0);
        assert!(lines.len() > 1, "a long word was not broken: {lines:?}");
        for line in &lines {
            assert!(advance(line, SIZE_PT) <= 40.0, "{line:?} overflows");
        }
        assert_eq!(lines.concat(), long, "breaking it lost or added letters");
    }

    /// Breaking a long word cuts on character boundaries, not byte ones.
    ///
    /// The single-pass break added for the quadratic wrap advances a byte
    /// cursor by `ch.len_utf8()` where the version before it collected chars
    /// and took `head.len()`. Both are right; they are right for different
    /// reasons, and the new one is wrong the moment that arithmetic slips ---
    /// on a multi-byte character it would slice mid-code-point, which is a
    /// panic rather than a wrong answer. Every existing word-breaking test is
    /// pure ASCII, where `len_utf8()` is 1 and any off-by-one in the units
    /// cannot show.
    #[test]
    fn a_long_word_of_multibyte_characters_is_broken_on_character_boundaries() {
        // Two bytes per character in UTF-8, and each is in the accented range
        // `advance_of` maps onto a base letter, so it has a real width.
        let long = "ä".repeat(60);
        let lines = wrap(&long, SIZE_PT, 40.0);
        assert!(lines.len() > 1, "a long word was not broken: {lines:?}");
        for line in &lines {
            assert!(advance(line, SIZE_PT) <= 40.0, "{line:?} overflows");
        }
        assert_eq!(
            lines.concat(),
            long,
            "breaking it lost, added or split a character"
        );
        assert_eq!(
            lines.iter().map(|l| l.chars().count()).sum::<usize>(),
            60,
            "and every character came back exactly once"
        );
    }

    #[test]
    fn a_width_too_small_for_any_character_still_terminates() {
        // The guard is on progress, not on a threshold: taking zero characters
        // would push an empty line and leave the rest unchanged, which is the
        // loop that never ends. One character per line is the floor.
        let lines = wrap("abc", SIZE_PT, 0.5);
        assert_eq!(lines.len(), 3, "{lines:?}");
    }

    #[test]
    fn the_readers_own_blank_lines_survive() {
        // Two newlines is how anyone separates paragraphs, and dropping the
        // empty line between them closes the gap up.
        assert_eq!(
            wrap("one\n\ntwo", SIZE_PT, 200.0),
            vec!["one".to_string(), String::new(), "two".to_string()]
        );
    }

    #[test]
    fn what_helvetica_cannot_write_is_refused_and_what_it_can_is_not() {
        // Both directions, because "refuses everything" satisfies the first half
        // exactly as well as a correct answer does.
        assert!(encodable("plain ASCII"));
        assert!(encodable("Grüße aus München"), "Latin-1 is writable");
        assert!(encodable("line\nbreak"), "a newline is not a glyph");
        assert!(!encodable("Ελληνικά"), "Greek is not in WinAnsi");
        assert!(!encodable("日本語"), "CJK is not in WinAnsi");
        assert!(!encodable("emoji 🙂"), "nor is anything astral");
    }

    #[test]
    fn umlauts_share_base_widths_but_latin1_exceptions_do_not() {
        // Pin the exceptions missed by the former base-letter fallback, along
        // with umlauts that really do keep the base vowel's advance.
        assert_eq!(advance("ßîø", 1000.0), 611.0 + 278.0 + 611.0);
        assert_eq!(advance("©¼°", 1000.0), 737.0 + 834.0 + 400.0);
        assert_eq!(advance("\u{a0}\u{ad}", 1000.0), 278.0 + 333.0);
        assert_eq!(advance("ä", SIZE_PT), advance("a", SIZE_PT));
        assert_eq!(advance("ö", SIZE_PT), advance("o", SIZE_PT));
        assert_eq!(advance("ü", SIZE_PT), advance("u", SIZE_PT));
        assert_eq!(advance("Ü", SIZE_PT), advance("U", SIZE_PT));
        // And a control: two letters that are *not* meant to be equal, so the
        // four above cannot pass by every character having the same width.
        assert_ne!(advance("i", SIZE_PT), advance("m", SIZE_PT));
    }
}
