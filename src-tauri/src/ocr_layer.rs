//! Deciding which pages get a text layer, and turning a recognition into one.
//!
//! The pieces between the recogniser ([`crate::ocr`]) and the writer
//! ([`crate::textlayer`]) that are the same whoever is asking: the
//! command-line tool, which holds a worker session, and the window, which
//! holds the render service. Each of them renders the page and calls the
//! engine in its own way; neither decides anything this module decides.
//!
//! Nothing here is a safety gate. [`crate::ocr`] is built around the one caller
//! for whom an empty recognition is a claim --- redaction verification. This
//! caller is the other one that module's docs name: it wants recall, an engine
//! that misses a word makes the copy worse, and nothing is unsafe.

use crate::ocr::{Options, RecognisedItem};
use crate::text::PageText;
use crate::textlayer::{Layer, Word};

/// The resolution a page is rendered at for recognition, in pixels per point,
/// when it fits: 300 DPI.
pub const WANTED_SCALE: f32 = 300.0 / 72.0;

/// The lowest resolution worth recognising at: 100 DPI.
///
/// Below it 10 pt type is under 14 px tall, where both engines start
/// misreading. A page too large to render even this finely inside the image
/// budget is reported rather than read badly.
pub const MIN_SCALE: f32 = 100.0 / 72.0;

/// The longest side of an image handed to an engine, in pixels.
///
/// `Windows.Media.Ocr` refuses anything longer than its `MaxImageDimension`,
/// measured at 10,000 (`BUILD.md`, `win-ocr-probe`). 8,192 is the bound
/// `tpdf render` already applies and is under it.
pub const MAX_EDGE: u32 = 8192;

/// What the engine is asked for: words, with its language model on.
///
/// Correction is on here and off for the redaction gate, for the reason
/// [`Options::language_correction`] gives turned round: a corrector makes a
/// doubtful mark into a plausible word, which is wrong when the question is
/// whether anything is legible and right when the question is what it says.
#[must_use]
pub fn options(languages: Vec<String>) -> Options {
    Options {
        languages,
        language_correction: true,
        deadline_ms: 30_000,
        words: true,
    }
}

/// Whether `raw` has the shape of a language tag as the engines take one:
/// letters, digits and hyphens, starting with a letter.
///
/// The shape only. Whether an engine offers the language is asked of the
/// engine; this is what keeps anything else out of a request and out of the
/// session file.
#[must_use]
pub fn is_language_tag(raw: &str) -> bool {
    (2..=35).contains(&raw.len())
        && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && raw.starts_with(|c: char| c.is_ascii_alphabetic())
}

/// What the engine is asked to expect for a reader's remembered language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// What goes into [`options`]: the language, or nothing for the engine's
    /// own choice.
    pub languages: Vec<String>,
    /// The language that was remembered and is not offered any more.
    pub unavailable: Option<String>,
}

/// Holds a remembered language against what the machine offers now.
///
/// A language is remembered across launches, and a machine can stop offering
/// one: a Windows language pack is removed, a session file is carried to
/// another computer. That must not reach the engine, where it would be an
/// engine's error on some page or a silent reading in another language. So the
/// recognition goes ahead with the engine's own choice and
/// [`Choice::unavailable`] names what was asked, for the caller to say.
#[must_use]
pub fn choose(remembered: Option<&str>, offered: &[String]) -> Choice {
    let Some(asked) = remembered else {
        return Choice {
            languages: Vec::new(),
            unavailable: None,
        };
    };
    match crate::ocr::first_offered(&[asked.to_string()], offered) {
        Some(tag) => Choice {
            languages: vec![tag],
            unavailable: None,
        },
        None => Choice {
            languages: Vec::new(),
            unavailable: Some(asked.to_string()),
        },
    }
}

/// Whether a page already has text of its own.
///
/// Any character that is not white space. A page with one is left alone: a
/// layer over text that is already there would make every word findable twice.
#[must_use]
pub fn has_text(page: &PageText) -> bool {
    page.codes
        .iter()
        .any(|code| !char::from_u32(*code).is_some_and(char::is_whitespace))
}

/// The size in pixels to render a page at, and the scale that gives it.
///
/// The largest scale up to [`WANTED_SCALE`] whose image is at most `capacity`
/// bytes at four per pixel and at most [`MAX_EDGE`] on its longer side. `None`
/// when the page has no size, or when the image would not fit even at
/// [`MIN_SCALE`].
#[must_use]
pub fn render_size(width_pt: f32, height_pt: f32, capacity: usize) -> Option<(u32, u32, f32)> {
    if !(width_pt.is_finite() && height_pt.is_finite() && width_pt > 0.0 && height_pt > 0.0) {
        return None;
    }
    let pixels = (capacity / 4) as f32;
    let by_area = (pixels / (width_pt * height_pt)).sqrt();
    let by_edge = MAX_EDGE as f32 / width_pt.max(height_pt);
    let mut scale = WANTED_SCALE.min(by_area).min(by_edge);
    // Rounding the two sides can put the product a row over the budget, so the
    // scale is stepped down until the rounded image fits.
    for _ in 0..16 {
        if scale < MIN_SCALE {
            return None;
        }
        let width = (width_pt * scale).round().max(1.0) as u32;
        let height = (height_pt * scale).round().max(1.0) as u32;
        let fits = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .is_some_and(|bytes| bytes <= capacity);
        if fits && width.max(height) <= MAX_EDGE {
            return Some((width, height, scale));
        }
        scale *= 0.995;
    }
    None
}

/// One page's recognition as the layer to write, or `None` if nothing was read.
///
/// A word the engine reports twice in one place is written once. Vision did
/// that for one word of 180 on a measured page, the two boxes a point or two
/// apart. Written twice, the word would be found twice by a search, and the
/// page does not read back: PDFium takes a character drawn over the same
/// character for emboldening and reads one of the two.
#[must_use]
pub fn layer_of(page: u32, items: Vec<RecognisedItem>) -> Option<Layer> {
    let mut words: Vec<Word> = Vec::new();
    for item in items {
        if item.text.trim().is_empty() || words.iter().any(|word| repeats(word, &item)) {
            continue;
        }
        words.push(Word {
            text: item.text,
            rect: item.rect,
        });
    }
    (!words.is_empty()).then_some(Layer { page, words })
}

/// Whether `item` is `word` again: the same text over at least half of the
/// smaller of the two boxes.
fn repeats(word: &Word, item: &RecognisedItem) -> bool {
    let span = |rect: [f32; 4]| {
        (
            rect[0].min(rect[2]),
            rect[1].min(rect[3]),
            rect[0].max(rect[2]),
            rect[1].max(rect[3]),
        )
    };
    let (a, b) = (span(word.rect), span(item.rect));
    let shared = (a.2.min(b.2) - a.0.max(b.0)).max(0.0) * (a.3.min(b.3) - a.1.max(b.1)).max(0.0);
    let smaller = ((a.2 - a.0) * (a.3 - a.1)).min((b.2 - b.0) * (b.3 - b.1));
    word.text == item.text && smaller > 0.0 && shared >= smaller * 0.5
}

/// What PDFium reads in place of a hyphen that ends a line when the word goes
/// on in the next: U+0002, its mark for a break inside a word. The page holds
/// the hyphen. Dropped with the other control characters, it made every such
/// page read back one character short, and the layer was refused: 9 of 46
/// picture pages measured on 2026-10-05.
const LINE_END_HYPHEN: u32 = 2;

/// Whether a written page reads back with the characters its layer was given.
///
/// Compared as the two sets of characters with their counts, white space left
/// out and order ignored. That is deliberately weaker than comparing words:
/// the reader decides where one word ends from the gap between two, and two
/// boxes an engine reported touching read back as one word without anything
/// having been lost. What this does catch is what can go wrong in the
/// writing --- a layer that is not there, on the wrong page, or in a font
/// whose codes do not map back to the characters.
///
/// A hyphen that ends a line is read as [`LINE_END_HYPHEN`] and counted as
/// the hyphen it is.
#[must_use]
pub fn reads_back(written: &Layer, read: &PageText) -> bool {
    let mut wanted: Vec<char> = written
        .words
        .iter()
        .flat_map(|word| word.text.chars())
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect();
    let mut got: Vec<char> = read
        .codes
        .iter()
        .map(|code| {
            if *code == LINE_END_HYPHEN {
                u32::from('-')
            } else {
                *code
            }
        })
        .filter_map(char::from_u32)
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect();
    wanted.sort_unstable();
    got.sort_unstable();
    wanted == got
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(text: &str) -> PageText {
        PageText {
            codes: text.chars().map(u32::from).collect(),
            ..PageText::default()
        }
    }

    fn item(text: &str) -> RecognisedItem {
        RecognisedItem {
            text: text.into(),
            rect: [0.0, 0.0, 10.0, 10.0],
            confidence: None,
        }
    }

    #[test]
    fn the_engine_is_asked_for_words_with_its_corrector_on() {
        let asked = options(vec!["de-DE".into()]);
        assert!(asked.words, "a layer is written word by word");
        assert!(asked.language_correction, "recall, not a safety gate");
        assert_eq!(asked.languages, ["de-DE"]);
    }

    #[test]
    fn a_language_tag_is_letters_digits_and_hyphens_after_a_letter() {
        for tag in ["en-US", "de", "zh-Hans", "yue-Hant", "es-419"] {
            assert!(is_language_tag(tag), "{tag}");
        }
        for not in ["", "x", "de_DE", "-de", "1de", "de DE", "de/../x"] {
            assert!(!is_language_tag(not), "{not}");
        }
        assert!(is_language_tag(&"a".repeat(35)));
        assert!(!is_language_tag(&"a".repeat(36)));
    }

    #[test]
    fn a_remembered_language_is_asked_for_only_while_the_machine_offers_it() {
        let offered = ["en-US".to_string(), "de-DE".to_string()];
        assert_eq!(
            choose(None, &offered),
            Choice {
                languages: vec![],
                unavailable: None
            }
        );
        assert_eq!(
            choose(Some("de-de"), &offered),
            Choice {
                languages: vec!["de-DE".into()],
                unavailable: None
            },
            "in the engine's own spelling"
        );
        assert_eq!(
            choose(Some("fr-FR"), &offered),
            Choice {
                languages: vec![],
                unavailable: Some("fr-FR".into())
            },
            "the engine chooses, and what was asked is named"
        );
        assert_eq!(
            choose(Some("de-DE"), &[]).unavailable.as_deref(),
            Some("de-DE")
        );
    }

    #[test]
    fn a_page_with_only_white_space_has_no_text() {
        assert!(!has_text(&page("")));
        assert!(!has_text(&page(" \r\n\t")));
        assert!(has_text(&page(" \r\n.")));
    }

    #[test]
    fn an_a4_page_is_rendered_as_finely_as_sixteen_megabytes_allow() {
        let capacity = 16 * 1024 * 1024;
        let (width, height, scale) = render_size(595.0, 842.0, capacity).expect("fits");
        assert!(width as usize * height as usize * 4 <= capacity);
        // 2.89 is where 595 x 842 points fills the budget exactly.
        assert!((2.85..=2.90).contains(&scale), "{scale}");
        assert_eq!(width, (595.0 * scale).round() as u32);
        assert_eq!(height, (842.0 * scale).round() as u32);
    }

    #[test]
    fn a_small_page_stops_at_300_dpi() {
        let (_, _, scale) = render_size(200.0, 100.0, 16 * 1024 * 1024).expect("fits");
        assert_eq!(scale, WANTED_SCALE);
    }

    #[test]
    fn a_long_narrow_page_is_bounded_by_its_longer_side() {
        let (width, height, _) = render_size(100.0, 5000.0, 64 * 1024 * 1024).expect("fits");
        assert!(height <= MAX_EDGE, "{height}");
        assert!(width >= 1);
    }

    #[test]
    fn a_page_too_large_to_read_at_100_dpi_is_refused() {
        // An A0 sheet at 100 DPI is 3311 x 4681 px: 62 MB.
        assert_eq!(render_size(2384.0, 3370.0, 16 * 1024 * 1024), None);
        assert_eq!(render_size(0.0, 100.0, 16 * 1024 * 1024), None);
        assert_eq!(render_size(f32::NAN, 100.0, 16 * 1024 * 1024), None);
    }

    #[test]
    fn a_recognition_with_no_words_makes_no_layer() {
        assert_eq!(layer_of(3, Vec::new()), None);
        assert_eq!(layer_of(3, vec![item("  ")]), None);
        let layer = layer_of(3, vec![item("  "), item("word")]).expect("one word");
        assert_eq!(layer.page, 3);
        assert_eq!(layer.words.len(), 1);
    }

    #[test]
    fn two_words_read_back_as_one_still_read_back() {
        let layer = layer_of(0, vec![item("black"), item("quartz")]).expect("words");
        assert!(reads_back(&layer, &page("blackquartz")));
        assert!(reads_back(&layer, &page("quartz\r\nblack")));
    }

    #[test]
    fn a_word_reported_twice_in_one_place_is_written_once() {
        let at = |text: &str, rect: [f32; 4]| RecognisedItem {
            text: text.into(),
            rect,
            confidence: None,
        };
        let texts = |items: Vec<RecognisedItem>| -> Vec<String> {
            let layer = layer_of(0, items).expect("words");
            layer.words.into_iter().map(|word| word.text).collect()
        };
        // The measured shape: the second box a point or two inside the first.
        let first = at("quartz", [300.0, 272.0, 368.0, 292.0]);
        let again = at("quartz", [302.0, 273.0, 368.0, 291.0]);
        assert_eq!(texts(vec![first.clone(), again]), ["quartz"]);
        // The same word further along the line is a second word.
        let later = at("quartz", [380.0, 272.0, 448.0, 292.0]);
        assert_eq!(texts(vec![first.clone(), later]), ["quartz", "quartz"]);
        // Just under half of the smaller box shared is still two words, and
        // half is one.
        let under = at("quartz", [335.0, 272.0, 403.0, 292.0]);
        assert_eq!(texts(vec![first.clone(), under]), ["quartz", "quartz"]);
        let half = at("quartz", [334.0, 272.0, 402.0, 292.0]);
        assert_eq!(texts(vec![first.clone(), half]), ["quartz"]);
        // Another word in the same place is not a repeat.
        let other = at("quarts", [300.0, 272.0, 368.0, 292.0]);
        assert_eq!(texts(vec![first.clone(), other]), ["quartz", "quarts"]);
        // A small box inside a large one of the same text is measured by the small one.
        let inside = at("quartz", [310.0, 275.0, 330.0, 285.0]);
        assert_eq!(texts(vec![first, inside]), ["quartz"]);
    }

    #[test]
    fn a_hyphen_that_ends_a_line_reads_back_as_the_hyphen_it_is() {
        let layer = layer_of(0, vec![item("quar-"), item("tz")]).expect("words");
        assert!(reads_back(&layer, &page("quar\u{2}\r\ntz")));
        // The mark stands for one hyphen and for nothing else.
        assert!(!reads_back(&layer, &page("quar\r\ntz")));
        assert!(!reads_back(&layer, &page("quar-\u{2}\r\ntz")));
        let plain = layer_of(0, vec![item("quartz")]).expect("words");
        assert!(!reads_back(&plain, &page("quartz\u{2}")));
    }

    #[test]
    fn a_missing_layer_and_a_wrong_character_do_not_read_back() {
        let layer = layer_of(0, vec![item("black"), item("quartz")]).expect("words");
        assert!(!reads_back(&layer, &page("")));
        assert!(!reads_back(&layer, &page("black quart")));
        assert!(!reads_back(&layer, &page("black quartz z")));
        assert!(!reads_back(&layer, &page("black quartx")));
    }
}
