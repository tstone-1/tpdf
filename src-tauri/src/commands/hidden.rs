//! *Find text the pages do not show* from the window: `tpdf hidden` on the
//! open document.
//!
//! The walk and its last sentence are [`crate::hidden::survey`]'s and are the
//! command-line tool's too. What is here is who answers for a page --- the
//! render service, which holds the reader's document in a sandboxed worker ---
//! and the two things only a window has: a line saying how far the check has
//! got, and a way to stop it.
//!
//! **The file is checked, not the reader's unsaved work.** The render service
//! holds the document as it was opened. Marks, regions marked for removal and
//! page edits that are not saved are in the journal and are not asked about;
//! [`HiddenText::unsaved`] says when there were any, so the window can say so.
//!
//! Nothing is written and nothing is journalled.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tauri::Emitter;

use super::answered;
use crate::edits;
use crate::hidden::survey::{self, Pages, Passage, Survey, NOT_LOOKED_AT};
use crate::render::{RenderService, TileFormat, TileOutcome, TileRequest};
use crate::text::PageText;
use crate::worker::TILE_CAPACITY;

/// The event that says which page is being compared. Its payload is [`Progress`].
pub const PROGRESS_EVENT: &str = "tpdf://hidden-progress";

/// What a stopped check answers. A walk that did not reach the last page has
/// no result: a list from half a document would read as the whole of it.
pub const CANCELLED: &str = "The check was stopped before the last page, so there is no result.";

/// The side of one tile a page is asked for in, in pixels: the largest whose
/// pixels fit a worker's reply.
const TILE: u32 = 2048;
const _: () = assert!(TILE as usize * TILE as usize * 4 <= TILE_CAPACITY);

/// The page being compared, counted from 1, and how many there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Progress {
    pub page: u32,
    pub of: u32,
}

/// Which check the reader asked to stop: the number of that run, or 0.
///
/// A run's number and not a flag, for [`super::ocr::Cancel`]'s reason: Stop is
/// shown before the command has been sent, and a number needs no clearing.
#[derive(Default)]
pub struct Cancel(Arc<AtomicU64>);

/// What the window is told. The reply of [`hidden_text`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiddenText {
    /// The passages, in page order.
    pub found: Vec<Passage>,
    /// [`survey::summary`]: what was found, and what was not compared.
    pub summary: String,
    /// [`NOT_LOOKED_AT`]: what the comparison never covers.
    pub not_looked_at: String,
    /// Whether the document has changes that are not saved, which were
    /// therefore not part of what was checked.
    pub unsaved: bool,
}

impl HiddenText {
    /// The reply for a finished walk.
    #[must_use]
    pub fn of(survey: Survey, unsaved: bool) -> HiddenText {
        HiddenText {
            summary: survey::summary(&survey),
            found: survey.found,
            not_looked_at: NOT_LOOKED_AT.into(),
            unsaved,
        }
    }
}

/// Whether a plan holds changes the file does not: marks, regions marked for
/// removal, page edits. They are in the journal, and the check reads the file.
fn unsaved(plan: &edits::Plan) -> bool {
    !plan.is_identity()
}

/// One page as RGBA, asked for in tiles of at most `side` pixels a side.
///
/// `tile` is handed a tile's left, top, width and height and answers its
/// pixels; `wrong` makes the error for an answer of another size, which is
/// refused before it is copied.
fn tiled<E>(
    width: u32,
    height: u32,
    side: u32,
    tile: &mut dyn FnMut(u32, u32, u16, u16) -> Result<Vec<u8>, E>,
    wrong: &dyn Fn() -> E,
) -> Result<Vec<u8>, E> {
    let mut pixels = vec![0; width as usize * height as usize * 4];
    for y in (0..height).step_by(side as usize) {
        for x in (0..width).step_by(side as usize) {
            let tw = (width - x).min(side) as u16;
            let th = (height - y).min(side) as u16;
            let part = tile(x, y, tw, th)?;
            let stride = usize::from(tw) * 4;
            if part.len() != stride * usize::from(th) {
                return Err(wrong());
            }
            for row in 0..usize::from(th) {
                let start = ((y as usize + row) * width as usize + x as usize) * 4;
                pixels[start..start + stride]
                    .copy_from_slice(&part[row * stride..(row + 1) * stride]);
            }
        }
    }
    Ok(pixels)
}

/// The pages of an open document, answered by the render service.
struct Held<'a> {
    service: &'a RenderService,
    doc: u32,
}

impl Pages for Held<'_> {
    type Error = String;

    fn text(&mut self, page: u32) -> Result<PageText, String> {
        answered("comparing text with the pages", |reply| {
            self.service.text(self.doc, page, None, reply)
        })
    }

    fn pixels(
        &mut self,
        page: u32,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<Vec<u8>, String> {
        let (service, doc) = (self.service, self.doc);
        tiled(
            width,
            height,
            TILE,
            &mut |x, y, tw, th| {
                let request = TileRequest {
                    rid: 0,
                    doc,
                    page,
                    scale,
                    turns: 0,
                    invert: false,
                    crop: None,
                    x: x as i32,
                    y: y as i32,
                    width: tw,
                    height: th,
                    format: TileFormat::Raw,
                };
                match answered("comparing text with the pages", |reply| {
                    service.tile(request, reply)
                })? {
                    TileOutcome::Rendered(tile) => Ok(tile.bytes),
                    TileOutcome::Abandoned => {
                        Err(format!("the render of page {} was abandoned", page + 1))
                    }
                }
            },
            &|| format!("page {} was rendered at the wrong size", page + 1),
        )
    }
}

/// Every page of a document of `count` pages, with a stop asked about ahead
/// of each page and `progress` told which one is next.
fn walk<P: Pages<Error = String>>(
    pages: &mut P,
    count: u32,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(Progress),
) -> Result<Survey, String> {
    let selection: Vec<u32> = (1..=count).collect();
    survey::survey(pages, &selection, &mut |_, page| {
        if cancelled() {
            return Err(CANCELLED.into());
        }
        progress(Progress { page, of: count });
        Ok(())
    })
}

/// The window's check, without the window: every page of `doc` as the render
/// service holds it, `pages` of them.
///
/// Blocks: it waits on a text extraction and a render for each page.
/// `cancelled` is asked ahead of every page and `progress` told which one.
///
/// # Errors
///
/// [`CANCELLED`]; a page that could not be read or rendered; a render service
/// that did not answer in time.
pub fn hidden_text_asked(
    service: &RenderService,
    doc: u32,
    pages: u32,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(Progress),
) -> Result<Survey, String> {
    walk(&mut Held { service, doc }, pages, cancelled, progress)
}

/// Lists the text of the open document that its pages do not show.
///
/// Progress arrives as [`PROGRESS_EVENT`]. `run` is a number the window chose
/// for this check, never 0, and [`hidden_text_cancel`] with the same number
/// stops it before the next page, whichever of the two calls arrives first.
#[tauri::command]
pub async fn hidden_text(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    cancel: tauri::State<'_, Cancel>,
    doc: u32,
    run: u64,
) -> Result<HiddenText, String> {
    let plan = edits.plan(doc)?;
    let service = service.inner().clone();
    let stopped = Arc::clone(&cancel.0);
    tauri::async_runtime::spawn_blocking(move || {
        hidden_text_asked(
            &service,
            doc,
            plan.baseline,
            &|| stopped.load(Ordering::Relaxed) == run,
            &|at| {
                let _ = app.emit(PROGRESS_EVENT, at);
            },
        )
        .map(|survey| HiddenText::of(survey, unsaved(&plan)))
    })
    .await
    .map_err(|e| format!("The check did not run: {e}"))?
}

/// Asks check number `run` to stop before its next page.
#[tauri::command]
pub fn hidden_text_cancel(cancel: tauri::State<'_, Cancel>, run: u64) {
    cancel.0.store(run, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pixel a picture of `width` has at `x`, `y`: each one different.
    fn pixel(width: u32, x: u32, y: u32) -> [u8; 4] {
        let n = y * width + x;
        [n as u8, (n >> 8) as u8, x as u8, y as u8]
    }

    #[test]
    fn a_page_is_put_together_from_its_tiles() {
        let (width, height) = (5, 3);
        let whole: Vec<u8> = (0..height)
            .flat_map(|y| (0..width).flat_map(move |x| pixel(width, x, y)))
            .collect();
        let mut asked = Vec::new();
        let got = tiled(
            width,
            height,
            2,
            &mut |x, y, tw, th| {
                asked.push((x, y, tw, th));
                Ok::<_, String>(
                    (y..y + u32::from(th))
                        .flat_map(|row| {
                            (x..x + u32::from(tw)).flat_map(move |col| pixel(5, col, row))
                        })
                        .collect(),
                )
            },
            &|| "wrong".to_string(),
        );
        assert_eq!(got.as_deref(), Ok(whole.as_slice()));
        // Row by row, and the last of each row and the last row are what is
        // left over.
        assert_eq!(
            asked,
            [
                (0, 0, 2, 2),
                (2, 0, 2, 2),
                (4, 0, 1, 2),
                (0, 2, 2, 1),
                (2, 2, 2, 1),
                (4, 2, 1, 1)
            ]
        );
        // A page that fits one tile is one request.
        let mut count = 0;
        let one = tiled(
            width,
            height,
            TILE,
            &mut |_, _, tw, th| {
                count += 1;
                Ok::<_, String>(vec![7; usize::from(tw) * usize::from(th) * 4])
            },
            &|| "wrong".to_string(),
        );
        assert_eq!((count, one.map(|p| p.len())), (1, Ok(5 * 3 * 4)));
    }

    #[test]
    fn a_tile_of_the_wrong_size_or_one_that_failed_ends_the_page() {
        let short = tiled(
            4,
            4,
            2,
            &mut |_, _, tw, th| Ok::<_, String>(vec![0; usize::from(tw) * usize::from(th) * 4 - 4]),
            &|| "wrong size".to_string(),
        );
        assert_eq!(short, Err("wrong size".to_string()));
        let long = tiled(
            4,
            4,
            2,
            &mut |_, _, tw, th| Ok::<_, String>(vec![0; usize::from(tw) * usize::from(th) * 4 + 4]),
            &|| "wrong size".to_string(),
        );
        assert_eq!(long, Err("wrong size".to_string()));
        let mut asked = 0;
        let failed = tiled(
            4,
            4,
            2,
            &mut |_, _, _, _| {
                asked += 1;
                Err::<Vec<u8>, _>("abandoned".to_string())
            },
            &|| "wrong size".to_string(),
        );
        assert_eq!((failed, asked), (Err("abandoned".to_string()), 1));
    }

    /// Pages with no text, which records the pages it was asked for.
    #[derive(Default)]
    struct Blank(Vec<u32>);

    impl Pages for Blank {
        type Error = String;

        fn text(&mut self, page: u32) -> Result<PageText, String> {
            self.0.push(page);
            Ok(PageText::default())
        }

        fn pixels(&mut self, _: u32, _: u32, _: u32, _: f32) -> Result<Vec<u8>, String> {
            Err("a page without text is not rendered".into())
        }
    }

    #[test]
    fn every_page_is_walked_and_reported_before_it_is_read() {
        let mut pages = Blank::default();
        let seen = std::cell::RefCell::new(Vec::new());
        let all = walk(&mut pages, 3, &|| false, &|at| seen.borrow_mut().push(at))
            .expect("the walk ends");
        assert_eq!(pages.0, [0, 1, 2]);
        assert_eq!(all.without_text, [1, 2, 3]);
        assert_eq!(
            seen.into_inner(),
            [1, 2, 3].map(|page| Progress { page, of: 3 })
        );
    }

    #[test]
    fn a_stop_ends_the_walk_before_the_next_page_and_leaves_no_result() {
        // Asked to stop from the start: no page is read or reported.
        let mut pages = Blank::default();
        let seen = std::cell::Cell::new(0);
        let stopped = walk(&mut pages, 3, &|| true, &|_| seen.set(seen.get() + 1));
        assert_eq!(stopped, Err(CANCELLED.to_string()));
        assert_eq!((pages.0.len(), seen.get()), (0, 0));

        // Asked to stop while the second page is being read.
        let mut pages = Blank::default();
        let seen = std::cell::Cell::new(0);
        let stopped = walk(&mut pages, 3, &|| seen.get() == 2, &|_| {
            seen.set(seen.get() + 1);
        });
        assert_eq!(stopped, Err(CANCELLED.to_string()));
        assert_eq!(pages.0, [0, 1]);
    }

    #[test]
    fn a_document_with_changes_that_are_not_saved_is_told_so() {
        let edits = edits::Edits::default();
        edits.open(7, 2, None);
        assert!(!unsaved(&edits.plan(7).expect("a plan")));
        edits.rotate(7, 1, 1).expect("the page turns");
        assert!(unsaved(&edits.plan(7).expect("a plan")));
    }

    #[test]
    fn the_reply_reaches_the_window_in_its_own_spelling() {
        let survey = Survey {
            found: vec![Passage {
                page: 2,
                text: "Jane Example".into(),
                rect: [1.0, 2.0, 3.0, 4.0],
                characters: 11,
                off_page: true,
            }],
            compared: 40,
            unjudged: 3,
            without_text: vec![1],
            not_compared: Vec::new(),
            selected: 2,
        };
        let sent = serde_json::to_value(HiddenText::of(survey.clone(), true)).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({
                "found": [{
                    "page": 2,
                    "text": "Jane Example",
                    "rect": [1.0, 2.0, 3.0, 4.0],
                    "characters": 11,
                    "offPage": true,
                }],
                "summary": "1 passage is in the file and not visible on the page, on 1 page; \
                            3 characters could not be judged; \
                            1 page without text not checked (1).",
                "notLookedAt": "Not looked at: comments, form values, attachments, metadata and \
                                earlier versions kept in the file.",
                "unsaved": true,
            })
        );
        assert_eq!(
            serde_json::to_value(HiddenText::of(survey, false)).unwrap()["unsaved"],
            false
        );
    }
}
