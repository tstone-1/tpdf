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
#[must_use]
pub fn layer_of(page: u32, items: Vec<RecognisedItem>) -> Option<Layer> {
    let words: Vec<Word> = items
        .into_iter()
        .filter(|item| !item.text.trim().is_empty())
        .map(|item| Word {
            text: item.text,
            rect: item.rect,
        })
        .collect();
    (!words.is_empty()).then_some(Layer { page, words })
}

/// Whether a written page reads back with the characters its layer was given.
///
/// Compared as the two sets of characters with their counts, white space left
/// out and order ignored. That is deliberately weaker than comparing words:
/// the reader decides where one word ends from the gap between two, and two
/// boxes an engine reported touching read back as one word without anything
/// having been lost. What this does catch is what can go wrong in the
/// writing --- a layer that is not there, on the wrong page, or in a font
/// whose codes do not map back to the characters.
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
        .filter_map(|code| char::from_u32(*code))
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
    fn a_missing_layer_and_a_wrong_character_do_not_read_back() {
        let layer = layer_of(0, vec![item("black"), item("quartz")]).expect("words");
        assert!(!reads_back(&layer, &page("")));
        assert!(!reads_back(&layer, &page("black quart")));
        assert!(!reads_back(&layer, &page("black quartz z")));
        assert!(!reads_back(&layer, &page("black quartx")));
    }
}
