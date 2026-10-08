//! The walk over a document's pages that [`super::judge`] is asked on, and the
//! sentence the walk ends on.
//!
//! `tpdf hidden` and the window's check are this one loop. What differs between
//! them is who answers for a page --- a worker session the tool opened, or the
//! render service holding the reader's document --- and that is [`Pages`].
//! Nothing here reads a document either: both answers come from a sandboxed
//! worker, and this compares them.
//!
//! **The answer is one-sided, and [`summary`] keeps it so.** A page without
//! text was not compared, a page too large to render at a size text can be
//! judged at was not compared, and a character with no verdict was not decided.
//! All three are in [`Survey`] and in the sentence, so a result with nothing
//! found is never said without what was left out.

use crate::ocr_layer;
use crate::render::PageSize;
use crate::text::PageText;

/// Pixels per point a page is compared at: 144 DPI, `tpdf render`'s default.
const WANTED_SCALE: f32 = 2.0;

/// The lowest scale a page is compared at. Below it ordinary body text is too
/// small to judge, and the page is reported as not checked.
const MIN_SCALE: f32 = 1.0;

/// `tpdf render`'s bounds on one image.
const MAX_SIDE: f32 = 8192.0;
const MAX_PIXELS: f32 = 16_777_216.0;

/// What is outside the comparison whatever it finds, as the window says it
/// under every result and the tool as the last line of its plain output. The
/// page's text is the only thing compared.
pub const NOT_LOOKED_AT: &str = "Not looked at: comments, form values, attachments, metadata and \
                                 earlier versions kept in the file.";

/// Who answers for a page. Pages count from 0 here.
pub trait Pages {
    type Error;

    /// Where the page's characters are, with no crop: [`PageText`] in the
    /// frame of the page as the file displays it.
    ///
    /// # Errors
    ///
    /// Whatever kept the page from being read.
    fn text(&mut self, page: u32) -> Result<PageText, Self::Error>;

    /// The page as `width * height` RGBA, upright with no crop, at `scale`
    /// pixels per point.
    ///
    /// # Errors
    ///
    /// Whatever kept the page from being rendered.
    fn pixels(
        &mut self,
        page: u32,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<Vec<u8>, Self::Error>;
}

/// One passage a page does not show.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Passage {
    /// The page it is on, counted from 1.
    pub page: u32,
    /// The words, in the order the page's text has them.
    pub text: String,
    /// `left, top, right, bottom` in points from the top-left corner of the
    /// page as the file displays it.
    pub rect: [f32; 4],
    /// How many of its characters were judged hidden; spaces are not.
    pub characters: u32,
    /// Whether the words lie outside the page altogether.
    pub off_page: bool,
}

/// What a walk found, and what it did not look at. Page numbers count from 1.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Survey {
    /// The passages, in page order and within a page in the text's own.
    pub found: Vec<Passage>,
    /// Characters whose place on the rendered page was looked at and decided.
    pub compared: u64,
    /// Characters with a place that could not be decided.
    pub unjudged: u64,
    /// Selected pages that have no text, which were therefore not compared.
    pub without_text: Vec<u32>,
    /// Selected pages too large to render at a size text can be judged at.
    pub not_compared: Vec<u32>,
    /// How many pages were selected.
    pub selected: usize,
}

/// The image a page is compared at: its size in pixels and the scale that
/// gives it. `None` for a page with no size, or one so large that it cannot be
/// rendered at [`MIN_SCALE`] within `tpdf render`'s bounds.
#[must_use]
pub fn image_of(size: PageSize) -> Option<(u32, u32, f32)> {
    let (w, h) = (size.width_pt, size.height_pt);
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return None;
    }
    let scale = WANTED_SCALE
        .min(MAX_SIDE / w.max(h))
        .min((MAX_PIXELS / (w * h)).sqrt());
    if scale < MIN_SCALE {
        return None;
    }
    // Rounded as `tpdf render` and the viewer round a page, and floored on
    // the scale side so the product stays inside the bound.
    let width = (w * scale).round().max(1.0) as u32;
    let height = (h * scale).round().max(1.0) as u32;
    Some((width, height, scale))
}

/// Compares every page of `selection` with its rendering.
///
/// `selection` counts from 1 and is walked in its own order. `before` is
/// called ahead of each page with how many are done and the page's number,
/// which is where a caller says how far the walk has got; an error from it
/// stops the walk before the page is asked for, which is how a caller stops.
///
/// The page's size is the text's own ([`PageText::width_pt`]), which is the
/// size the worker reports for the page when a document is opened.
///
/// # Errors
///
/// The first error from `before` or from `pages`. Nothing found so far is
/// returned with it: a walk that did not finish has no result.
pub fn survey<P: Pages>(
    pages: &mut P,
    selection: &[u32],
    before: &mut dyn FnMut(usize, u32) -> Result<(), P::Error>,
) -> Result<Survey, P::Error> {
    let mut out = Survey {
        selected: selection.len(),
        ..Survey::default()
    };
    for (done, n) in selection.iter().enumerate() {
        before(done, *n)?;
        let page = n.saturating_sub(1);
        let text = pages.text(page)?;
        if !ocr_layer::has_text(&text) {
            out.without_text.push(*n);
            continue;
        }
        let size = PageSize {
            width_pt: text.width_pt,
            height_pt: text.height_pt,
        };
        let Some((width, height, scale)) = image_of(size) else {
            out.not_compared.push(*n);
            continue;
        };
        let pixels = pages.pixels(page, width, height, scale)?;
        let judged = super::judge(&text, &pixels, width as usize, height as usize, scale);
        out.compared += judged.judged as u64;
        out.unjudged += judged.unjudged as u64;
        out.found
            .extend(judged.found.into_iter().map(|found| Passage {
                page: *n,
                text: found.text,
                rect: found.rect,
                characters: found.hidden as u32,
                off_page: found.off_page,
            }));
    }
    Ok(out)
}

/// The sentence a walk ends on. What was not checked is in it, so the result
/// is never read without its limits.
#[must_use]
pub fn summary(survey: &Survey) -> String {
    let mut line = if survey.found.is_empty() {
        format!(
            "No hidden text found: {} characters on {} compared with the rendered page",
            survey.compared,
            pages_of(
                survey
                    .selected
                    .saturating_sub(survey.without_text.len() + survey.not_compared.len())
            ),
        )
    } else {
        let on: std::collections::BTreeSet<u32> = survey.found.iter().map(|f| f.page).collect();
        format!(
            "{} in the file and not visible on the page, on {}",
            if survey.found.len() == 1 {
                "1 passage is".to_string()
            } else {
                format!("{} passages are", survey.found.len())
            },
            pages_of(on.len()),
        )
    };
    if survey.unjudged > 0 {
        line.push_str(&format!(
            "; {} characters could not be judged",
            survey.unjudged
        ));
    }
    if !survey.without_text.is_empty() {
        line.push_str(&format!(
            "; {} without text not checked ({})",
            pages_of(survey.without_text.len()),
            numbers(&survey.without_text)
        ));
    }
    if !survey.not_compared.is_empty() {
        line.push_str(&format!(
            "; {} too large to compare ({})",
            pages_of(survey.not_compared.len()),
            numbers(&survey.not_compared)
        ));
    }
    line.push('.');
    line
}

fn pages_of(count: usize) -> String {
    if count == 1 {
        "1 page".into()
    } else {
        format!("{count} pages")
    }
}

fn numbers(pages: &[u32]) -> String {
    pages
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_compared_at_144_dpi_or_as_large_as_the_bounds_allow() {
        let size = |width_pt, height_pt| PageSize {
            width_pt,
            height_pt,
        };
        assert_eq!(image_of(size(595.0, 842.0)), Some((1190, 1684, 2.0)));
        // The longer side decides: 6000 points at 8192 pixels.
        let (width, height, scale) = image_of(size(6000.0, 100.0)).expect("it fits");
        assert!((width, height) == (8192, 137) && scale < 2.0 && scale > 1.0);
        // The area decides: 4000 points square is 16 million pixels at 1.024.
        let (width, height, scale) = image_of(size(4000.0, 4000.0)).expect("it fits");
        assert!(scale >= 1.0 && u64::from(width) * u64::from(height) <= 16_777_216);
        for (w, h) in [
            (9000.0, 100.0),
            (4200.0, 4200.0),
            (0.0, 10.0),
            (-1.0, 10.0),
            (f32::NAN, 10.0),
            (10.0, f32::INFINITY),
        ] {
            assert_eq!(image_of(size(w, h)), None, "{w} by {h}");
        }
    }

    fn found(found: Vec<Passage>) -> Survey {
        Survey {
            found,
            compared: 1200,
            selected: 9,
            ..Survey::default()
        }
    }

    fn passage(page: u32, off_page: bool) -> Passage {
        Passage {
            page,
            text: "Jane Example".into(),
            rect: [1.0, 2.0, 3.0, 4.0],
            characters: 11,
            off_page,
        }
    }

    #[test]
    fn the_last_line_says_what_was_found_and_what_was_not_checked() {
        assert_eq!(
            summary(&found(Vec::new())),
            "No hidden text found: 1200 characters on 9 pages compared with the rendered page."
        );
        assert_eq!(
            summary(&found(vec![passage(2, false)])),
            "1 passage is in the file and not visible on the page, on 1 page."
        );
        assert_eq!(
            summary(&found(vec![
                passage(2, false),
                passage(2, true),
                passage(7, false)
            ])),
            "3 passages are in the file and not visible on the page, on 2 pages."
        );
        // Nothing found is never said without what was left out.
        let mut partial = found(Vec::new());
        partial.unjudged = 40;
        partial.without_text = vec![3, 4];
        partial.not_compared = vec![9];
        assert_eq!(
            summary(&partial),
            "No hidden text found: 1200 characters on 6 pages compared with the rendered page; \
             40 characters could not be judged; 2 pages without text not checked (3, 4); \
             1 page too large to compare (9)."
        );
    }

    /// Side of one character's box, in points.
    const BOX: f32 = 5.0;

    /// A page 100 points square with `words` on one line at `x`, `y`.
    fn page(words: &str, x: f32, y: f32) -> PageText {
        let mut text = PageText {
            width_pt: 100.0,
            height_pt: 100.0,
            ..PageText::default()
        };
        let mut left = x;
        for ch in words.chars() {
            text.codes.push(u32::from(ch));
            text.boxes.extend([left, y, left + BOX, y + BOX]);
            left += BOX + 1.0;
        }
        text
    }

    /// Pages that answer from memory and record what they were asked.
    #[derive(Default)]
    struct Fake {
        /// By page, counted from 0: the text, and whether its words are drawn.
        pages: Vec<(PageText, bool)>,
        asked_text: Vec<u32>,
        asked_pixels: Vec<(u32, u32, u32, f32)>,
        /// The page whose rendering fails, if any.
        broken: Option<u32>,
    }

    impl Pages for Fake {
        type Error = String;

        fn text(&mut self, page: u32) -> Result<PageText, String> {
            self.asked_text.push(page);
            self.pages
                .get(page as usize)
                .map(|(text, _)| text.clone())
                .ok_or_else(|| format!("no page {page}"))
        }

        fn pixels(
            &mut self,
            page: u32,
            width: u32,
            height: u32,
            scale: f32,
        ) -> Result<Vec<u8>, String> {
            self.asked_pixels.push((page, width, height, scale));
            if self.broken == Some(page) {
                return Err(format!("page {page} did not render"));
            }
            let (text, drawn) = &self.pages[page as usize];
            let mut pixels = vec![255; width as usize * height as usize * 4];
            if !drawn {
                return Ok(pixels);
            }
            // A dark stem in the middle of each box: ink with paper beside it.
            for b in text.boxes.chunks_exact(4) {
                let x = ((b[0] + BOX / 2.0) * scale) as usize;
                for y in (b[1] * scale) as usize..(b[3] * scale) as usize {
                    let at = (y * width as usize + x) * 4;
                    if let Some(px) = pixels.get_mut(at..at + 3) {
                        px.fill(0);
                    }
                }
            }
            Ok(pixels)
        }
    }

    fn quiet(_: usize, _: u32) -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn each_selected_page_is_compared_and_a_finding_names_its_page() {
        let mut pages = Fake {
            pages: vec![
                (page("visible words", 4.0, 10.0), true),
                (page("unpainted", 4.0, 30.0), false),
                (page("also __ seen", 4.0, 50.0), true),
            ],
            ..Fake::default()
        };
        let all = survey(&mut pages, &[1, 2, 3], &mut quiet).expect("the walk ends");
        assert_eq!(pages.asked_text, [0, 1, 2]);
        // At the size `image_of` gives a page of 100 points.
        assert_eq!(
            pages.asked_pixels,
            [(0, 200, 200, 2.0), (1, 200, 200, 2.0), (2, 200, 200, 2.0)]
        );
        assert_eq!(all.found.len(), 1, "{all:?}");
        let only = &all.found[0];
        assert_eq!((only.page, only.text.as_str()), (2, "unpainted"));
        assert_eq!((only.characters, only.off_page), (9, false));
        assert_eq!(only.rect, [4.0, 30.0, 4.0 + 9.0 * 6.0 - 1.0, 35.0]);
        // Twelve and eight visible letters, nine unpainted ones; spaces are
        // neither, and the two underscores fill their boxes and are not judged.
        assert_eq!((all.compared, all.unjudged), (12 + 9 + 8, 2));
        assert_eq!(all.selected, 3);
        assert!(all.without_text.is_empty() && all.not_compared.is_empty());

        // A selection is walked in its own order and nothing else is asked.
        let mut pages = Fake {
            pages: pages.pages,
            ..Fake::default()
        };
        let some = survey(&mut pages, &[3, 2], &mut quiet).expect("the walk ends");
        assert_eq!(pages.asked_text, [2, 1]);
        assert_eq!((some.found.len(), some.found[0].page), (1, 2));
        assert_eq!((some.compared, some.selected), (9 + 8, 2));
    }

    #[test]
    fn words_outside_the_page_keep_their_mark() {
        let mut pages = Fake {
            pages: vec![(page("margin", -300.0, 10.0), true)],
            ..Fake::default()
        };
        let all = survey(&mut pages, &[1], &mut quiet).expect("the walk ends");
        assert_eq!(all.found.len(), 1, "{all:?}");
        assert!(all.found[0].off_page);
    }

    #[test]
    fn a_page_without_text_or_too_large_is_named_and_not_rendered() {
        let mut large = page("too large to judge", 4.0, 10.0);
        large.width_pt = 9000.0;
        let mut pages = Fake {
            pages: vec![
                (PageText::default(), true),
                (large, false),
                (page("unpainted", 4.0, 30.0), false),
            ],
            ..Fake::default()
        };
        let all = survey(&mut pages, &[1, 2, 3], &mut quiet).expect("the walk ends");
        assert_eq!(all.without_text, [1]);
        assert_eq!(all.not_compared, [2]);
        // Only the third page was rendered, and the words of the page that
        // could not be compared are in no count.
        assert_eq!(pages.asked_pixels, [(2, 200, 200, 2.0)]);
        assert_eq!((all.found.len(), all.compared), (1, 9));
        assert_eq!(
            summary(&all),
            "1 passage is in the file and not visible on the page, on 1 page; \
             1 page without text not checked (1); 1 page too large to compare (2)."
        );
    }

    #[test]
    fn the_walk_says_where_it_is_and_stops_when_told() {
        let three = || Fake {
            pages: vec![
                (page("one", 4.0, 10.0), true),
                (page("two", 4.0, 10.0), true),
                (page("three", 4.0, 10.0), true),
            ],
            ..Fake::default()
        };
        let mut pages = three();
        let mut seen = Vec::new();
        survey(&mut pages, &[1, 3], &mut |done, page| {
            seen.push((done, page));
            Ok(())
        })
        .expect("the walk ends");
        assert_eq!(seen, [(0, 1), (1, 3)]);

        // Stopped ahead of the second page: it is not asked for, and what the
        // first page gave is not returned as a result.
        let mut pages = three();
        let stopped = survey(&mut pages, &[1, 2, 3], &mut |done, _| {
            if done == 1 {
                Err("stopped".to_string())
            } else {
                Ok(())
            }
        });
        assert_eq!(stopped, Err("stopped".to_string()));
        assert_eq!(pages.asked_text, [0]);
    }

    #[test]
    fn a_page_that_cannot_be_read_or_rendered_ends_the_walk() {
        let mut pages = Fake {
            pages: vec![(page("one", 4.0, 10.0), true)],
            ..Fake::default()
        };
        assert_eq!(
            survey(&mut pages, &[1, 2], &mut quiet),
            Err("no page 1".to_string())
        );
        let mut pages = Fake {
            pages: vec![(page("one", 4.0, 10.0), true)],
            broken: Some(0),
            ..Fake::default()
        };
        assert_eq!(
            survey(&mut pages, &[1], &mut quiet),
            Err("page 0 did not render".to_string())
        );
    }
}
