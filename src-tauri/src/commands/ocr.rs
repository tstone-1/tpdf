//! Text recognition from the window: a searchable copy of the open document.
//!
//! `tpdf ocr` with the render service in place of a worker session. What is
//! decided --- which pages are read, how finely, what a recognition becomes ---
//! is [`crate::ocr_layer`]'s and is the command-line tool's too. What is here
//! is the walk over the open document's pages, the read-back of the staged
//! file, and the two things only a window has: a line saying how far it has
//! got, and a way to stop.
//!
//! **The open document is not changed.** Nothing is journalled: the result is a
//! new file, which the window then opens. And it is read from the file, not
//! from the journal --- a document with unsaved changes is refused, because the
//! words are placed on the pages the render service holds, and a plan that
//! turns, moves or drops pages would put them somewhere else.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::Emitter;

use super::{outside_at, password_for};
use crate::ocr::Pixels;
use crate::ocr_layer;
use crate::ocr_worker::{OcrWorker, PIXELS_CAPACITY};
use crate::render::{RenderService, TileFormat, TileOutcome, TileRequest};
use crate::textlayer::Layer;
use crate::worker::TILE_CAPACITY;
use crate::{edits, pdfium_library_dir, save};

/// The event that says which page is being read. Its payload is [`Progress`].
pub const PROGRESS_EVENT: &str = "tpdf://ocr-progress";

/// What a cancelled recognition answers.
pub const CANCELLED: &str = "Text recognition was stopped. No copy was written.";

/// What a document with unsaved changes is told.
pub const UNSAVED: &str = "Save your changes first. Text is recognised on the pages of the saved \
                           file.";

/// The page being read, counted from 1, and how many there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Progress {
    pub page: u32,
    pub of: u32,
}

/// Whether the reader asked the running recognition to stop.
///
/// One flag for the application, not one per document: recognition blocks every
/// other document command while it runs, so there is one at a time.
#[derive(Default)]
pub struct Cancel(Arc<AtomicBool>);

/// One page that was given a text layer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LayerPage {
    /// Counted from 1.
    pub page: u32,
    pub words: usize,
}

/// What became of every page of the document. Page numbers count from 1.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recognised {
    /// The pages given a text layer.
    pub pages: Vec<LayerPage>,
    /// Pages that have text of their own and were left as they are.
    pub already_text: Vec<u32>,
    /// Pages the engine read and found no words on.
    pub nothing_read: Vec<u32>,
    /// Pages too large to render finely enough to read.
    pub too_large: Vec<u32>,
    /// The engine that read them.
    pub engine: String,
}

/// The longest any single answer from the render service is waited for.
///
/// [`crate::ocr_gate`]'s bound and its reason: a wait that never ends cannot be
/// reported, and the slowest legitimate answer is the open of a large file.
const ANSWER_BOUND: std::time::Duration = std::time::Duration::from_secs(60);

/// Drives one of the render service's callback-shaped calls to an answer.
fn wait<T: Send + 'static, E: Send + 'static + From<String>>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, E>) + Send>),
) -> Result<T, E> {
    let (tx, rx) = std::sync::mpsc::channel();
    call(Box::new(move |result| {
        let _ = tx.send(result);
    }));
    match rx.recv_timeout(ANSWER_BOUND) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(E::from(format!(
            "the render service did not answer within {} s while recognising text",
            ANSWER_BOUND.as_secs()
        ))),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(E::from("the render service stopped".to_string()))
        }
    }
}

/// The layers to write, and the account of the pages that got none.
struct Read {
    layers: Vec<Layer>,
    report: Recognised,
}

/// Recognises every page of `doc` that has no text of its own.
fn read(
    service: &RenderService,
    doc: u32,
    pages: u32,
    languages: Vec<String>,
    cancelled: &AtomicBool,
    progress: &dyn Fn(Progress),
) -> Result<Read, String> {
    let options = ocr_layer::options(languages);
    let capacity = PIXELS_CAPACITY.min(TILE_CAPACITY);
    let mut engine: Option<OcrWorker> = None;
    let mut layers = Vec::new();
    let mut report = Recognised::default();
    for page in 0..pages {
        if cancelled.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        let n = page + 1;
        progress(Progress { page: n, of: pages });
        let text: crate::text::PageText = wait(|reply| service.text(doc, page, None, reply))?;
        if ocr_layer::has_text(&text) {
            report.already_text.push(n);
            continue;
        }
        let Some((width, height, scale)) =
            ocr_layer::render_size(text.width_pt, text.height_pt, capacity)
        else {
            report.too_large.push(n);
            continue;
        };
        // One tile for the page: `render_size` keeps it inside the tile budget
        // and under `MAX_EDGE` a side, which fits the request's 16 bits.
        let request = TileRequest {
            rid: 0,
            doc,
            page,
            scale,
            turns: 0,
            invert: false,
            crop: None,
            x: 0,
            y: 0,
            width: width as u16,
            height: height as u16,
            format: TileFormat::Raw,
        };
        let rgba = match wait(|reply| service.tile(request, reply))? {
            TileOutcome::Rendered(tile) => tile.bytes,
            TileOutcome::Abandoned => {
                return Err(format!("the render of page {n} was abandoned"));
            }
        };
        if rgba.len() != width as usize * height as usize * 4 {
            return Err(format!("page {n} was rendered at the wrong size"));
        }
        // Spawned at the first page that needs it, so a document whose pages
        // all have text never starts an engine.
        let worker = match &mut engine {
            Some(worker) => worker,
            None => engine.insert(
                OcrWorker::spawn().map_err(|why| format!("The text could not be read. {why}"))?,
            ),
        };
        let pixels = Pixels {
            rgba: &rgba,
            width,
            height,
            scale,
        };
        let (id, items) = worker
            .recognise(pixels, &options)
            .map_err(|why| format!("Page {n} could not be read: {why}"))?;
        if report.engine.is_empty() {
            report.engine = id.to_string();
        }
        match ocr_layer::layer_of(page, items) {
            Some(layer) => {
                report.pages.push(LayerPage {
                    page: n,
                    words: layer.words.len(),
                });
                layers.push(layer);
            }
            None => report.nothing_read.push(n),
        }
    }
    // A stop asked for during the last page is still a stop.
    if cancelled.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    Ok(Read { layers, report })
}

/// Why a document none of whose pages got a layer is not written.
fn nothing_to_add(report: &Recognised) -> String {
    let read = !report.nothing_read.is_empty();
    let had = !report.already_text.is_empty();
    let large = !report.too_large.is_empty();
    match (had, read, large) {
        (true, false, false) => "Every page already has text, so there is nothing to add.".into(),
        (false, true, false) => "No text was recognised on any page.".into(),
        (false, false, true) => {
            "Every page is too large to render finely enough to read its text.".into()
        }
        _ => "No page was given text: each one either has text already, or none was recognised \
              on it."
            .into(),
    }
}

/// Whether the staged file has the pages and reads back the layers.
///
/// Opened in the render service as a document of its own, which is how the
/// reader's next search of it will read it.
fn reads_back(
    service: &RenderService,
    staged: &Path,
    password: Option<String>,
    pages: u32,
    layers: &[Layer],
) -> Result<(), String> {
    let opened = wait(|reply| service.open(staged.to_path_buf(), true, password, reply)).map_err(
        |refusal| {
            format!(
                "The copy could not be opened to check it, so it was not kept: {}",
                refusal.reason
            )
        },
    )?;
    let checked = (|| {
        if opened.page_count != pages as usize {
            return Err(format!(
                "The copy has {} pages where the document has {pages}, so it was not kept.",
                opened.page_count
            ));
        }
        for layer in layers {
            let text: crate::text::PageText =
                wait(|reply| service.text(opened.id, layer.page, None, reply))?;
            if !ocr_layer::reads_back(layer, &text) {
                return Err(format!(
                    "Page {} of the copy did not read back with the text that was recognised, so \
                     the copy was not kept.",
                    layer.page + 1
                ));
            }
        }
        Ok(())
    })();
    let _: Result<(), String> = wait(|reply| service.close(opened.id, reply));
    checked
}

/// The window's *Recognise text*, without the window.
///
/// Blocks: it waits on renders and on the recognition engine. `plan` is the
/// open document's and must be the file itself --- see the module docs.
///
/// # Errors
///
/// [`UNSAVED`] for a plan that changes the file; [`CANCELLED`]; no engine on
/// this platform; no page given a layer; everything
/// [`save::write_checked_copy`] refuses; or a copy that did not read back. No
/// path leaves a file under `out` that was not there before.
#[allow(clippy::too_many_arguments)]
pub fn ocr_copy_asked(
    service: &RenderService,
    doc: u32,
    mut plan: edits::Plan,
    source: &Path,
    out: &Path,
    password: Option<String>,
    library: std::path::PathBuf,
    languages: Vec<String>,
    cancelled: &AtomicBool,
    progress: &dyn Fn(Progress),
) -> Result<Recognised, String> {
    if !plan.is_identity() {
        return Err(UNSAVED.into());
    }
    let pages = plan.baseline;
    let read = read(service, doc, pages, languages, cancelled, progress)?;
    if read.layers.is_empty() {
        return Err(format!(
            "{} No copy was written.",
            nothing_to_add(&read.report)
        ));
    }
    plan.text_layers = read.layers.clone();
    // Who parses the reader's document: `save_copy`'s choice, for its reason.
    let writing = outside_at(library, service.backend());
    save::write_checked_copy(
        source,
        &plan,
        out,
        password.as_deref(),
        &*writing,
        &|staged| {
            reads_back(service, staged, password.clone(), pages, &read.layers)
                .map_err(save::Refusal::from)
        },
    )
    .map_err(|why| why.message)?;
    Ok(read.report)
}

/// Writes a copy of the open document in which scanned pages can be searched.
///
/// Progress arrives as [`PROGRESS_EVENT`], and [`ocr_cancel`] stops it before
/// the next page.
#[tauri::command]
pub async fn ocr_copy(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    cancel: tauri::State<'_, Cancel>,
    doc: u32,
    source: String,
    path: String,
) -> Result<Recognised, String> {
    let plan = edits.plan(doc)?;
    let password = password_for(&service, doc, "ocr_copy").await;
    let library = pdfium_library_dir(&app);
    let service = service.inner().clone();
    let cancelled = Arc::clone(&cancel.0);
    // A stop asked for before this recognition began was for an earlier one.
    cancelled.store(false, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        ocr_copy_asked(
            &service,
            doc,
            plan,
            Path::new(&source),
            Path::new(&path),
            password,
            library,
            // No languages: the engine's own choice, which is `tpdf ocr`'s default.
            Vec::new(),
            &cancelled,
            &|at| {
                let _ = app.emit(PROGRESS_EVENT, at);
            },
        )
    })
    .await
    .map_err(|e| format!("Text recognition did not run: {e}"))?
}

/// Asks the running recognition to stop before its next page.
#[tauri::command]
pub fn ocr_cancel(cancel: tauri::State<'_, Cancel>) {
    cancel.0.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(already: &[u32], nothing: &[u32], large: &[u32]) -> Recognised {
        Recognised {
            already_text: already.to_vec(),
            nothing_read: nothing.to_vec(),
            too_large: large.to_vec(),
            ..Recognised::default()
        }
    }

    #[test]
    fn the_reason_nothing_was_added_names_the_one_cause_when_there_is_one() {
        assert!(nothing_to_add(&report(&[1, 2], &[], &[])).contains("already has text"));
        assert!(nothing_to_add(&report(&[], &[1], &[])).contains("No text was recognised"));
        assert!(nothing_to_add(&report(&[], &[], &[1])).contains("too large"));
        let mixed = nothing_to_add(&report(&[1], &[2], &[]));
        assert!(mixed.contains("either"), "{mixed}");
    }

    #[test]
    fn the_report_reaches_the_window_in_its_own_spelling() {
        let sent = serde_json::to_value(Recognised {
            pages: vec![LayerPage { page: 2, words: 7 }],
            already_text: vec![1],
            nothing_read: vec![3],
            too_large: vec![4],
            engine: "vision".into(),
        })
        .unwrap();
        assert_eq!(
            sent,
            serde_json::json!({
                "pages": [{ "page": 2, "words": 7 }],
                "alreadyText": [1],
                "nothingRead": [3],
                "tooLarge": [4],
                "engine": "vision",
            })
        );
    }
}
