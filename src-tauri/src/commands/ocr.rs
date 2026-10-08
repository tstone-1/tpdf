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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tauri::Emitter;

use super::{outside_at, password_for};
use crate::ocr::Pixels;
use crate::ocr_layer::{self, Outcome};
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

/// Which recognition the reader asked to stop: the number of that run, or 0.
///
/// **A run's number and not a flag.** The window shows Stop before `ocr_copy`
/// has been sent --- the signed-document warning is asked in between --- so a
/// stop can arrive before the command it is for. A flag the command cleared as
/// it started lost that stop, which the window phase found by pressing Stop as
/// soon as it was offered. A number needs no clearing: a stop names its run,
/// the run compares, and which of the two arrived first changes nothing.
///
/// One for the application, not one per document: recognition blocks every
/// other document command while it runs, so there is one at a time.
#[derive(Default)]
pub struct Cancel(Arc<AtomicU64>);

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
    /// Pages whose image the engine would not read ([`Outcome::Refused`]).
    pub refused: Vec<u32>,
    /// Pages too large to render finely enough to read.
    pub too_large: Vec<u32>,
    /// The engine that read them.
    pub engine: String,
    /// The language the reader had chosen, when this machine no longer offers
    /// it and the engine chose for itself instead. Not sent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_unavailable: Option<String>,
}

/// What the reader can choose between. The reply of [`ocr_languages`].
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Offered {
    /// The languages this machine's engine can be asked to expect, as BCP-47
    /// tags in the engine's own spelling and order.
    pub languages: Vec<String>,
    /// Whether the reader can add to them, which is Windows: a language there
    /// is installed in Settings, where macOS has a fixed list.
    pub installable: bool,
}

/// The languages this machine's engine offers.
///
/// Asked in the app process, which recognition never is: no image and no
/// document goes with the question ([`crate::ocr_vision::Vision::languages`]).
///
/// # Errors
///
/// The engine's reason for not answering.
pub fn offered_languages() -> Result<Vec<String>, String> {
    #[cfg(target_os = "macos")]
    let listed = crate::ocr_vision::Vision::languages();
    #[cfg(windows)]
    let listed = crate::ocr_windows::installed_languages();
    #[cfg(not(any(target_os = "macos", windows)))]
    let listed: Result<Vec<String>, crate::ocr::RecogniseError> = Ok(Vec::new());
    listed.map_err(|why| why.to_string())
}

/// [`offered_languages`], with whether more can be installed.
///
/// A list that cannot be read is an empty one: the reader is then offered the
/// engine's own choice, which needs no list.
fn offered() -> Offered {
    Offered {
        languages: offered_languages().unwrap_or_default(),
        installable: cfg!(windows),
    }
}

/// Drives one of the render service's callback-shaped calls to an answer.
fn wait<T: Send + 'static, E: Send + 'static + From<String>>(
    call: impl FnOnce(Box<dyn FnOnce(Result<T, E>) + Send>),
) -> Result<T, E> {
    super::answered("recognising text", call)
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
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(Progress),
) -> Result<Read, String> {
    let options = ocr_layer::options(languages);
    let capacity = PIXELS_CAPACITY.min(TILE_CAPACITY);
    let mut engine: Option<OcrWorker> = None;
    let mut layers = Vec::new();
    let mut report = Recognised::default();
    for page in 0..pages {
        if cancelled() {
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
        // A page the engine will not read is this page's alone; anything else
        // it reports stops the run (`ocr_layer::outcome_of`).
        let answer = worker.recognise_page(pixels, &options).map(|(id, items)| {
            if report.engine.is_empty() {
                report.engine = id.to_string();
            }
            items
        });
        let outcome = ocr_layer::outcome_of(page, answer)
            .map_err(|why| format!("Page {n} could not be read: {why}"))?;
        match outcome {
            Outcome::Layer(layer) => {
                report.pages.push(LayerPage {
                    page: n,
                    words: layer.words.len(),
                });
                layers.push(layer);
            }
            Outcome::Nothing => report.nothing_read.push(n),
            Outcome::Refused => report.refused.push(n),
        }
    }
    // A stop asked for during the last page is still a stop.
    if cancelled() {
        return Err(CANCELLED.into());
    }
    Ok(Read { layers, report })
}

/// Why a document none of whose pages got a layer is not written.
///
/// A page the recogniser refused is named with what that usually means,
/// whatever became of the others: it is the cause a reader can act on.
fn nothing_to_add(report: &Recognised) -> String {
    if !report.refused.is_empty() {
        return format!(
            "No page was given text. The recogniser refused {}, {}.",
            ocr_layer::pages_named(&report.refused),
            ocr_layer::REFUSED_MEANS
        );
    }
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
/// `language` is the one the reader chose, or `None` for the engine's own
/// choice, and `offered` is what the machine offers now
/// ([`offered_languages`]). A language that is not among them is not an error:
/// the engine chooses, and the answer names what was asked
/// ([`Recognised::language_unavailable`]).
///
/// # Errors
///
/// [`UNSAVED`] for a plan that changes the file; [`CANCELLED`]; no engine on
/// this platform; an engine that died, did not answer in time or was handed a
/// malformed request, on any page; no page given a layer; everything
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
    language: Option<&str>,
    offered: &[String],
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(Progress),
) -> Result<Recognised, String> {
    if !plan.is_identity() {
        return Err(UNSAVED.into());
    }
    let pages = plan.baseline;
    // A language the machine has stopped offering is not sent to the engine.
    let choice = ocr_layer::choose(language, offered);
    let mut read = read(service, doc, pages, choice.languages, cancelled, progress)?;
    read.report.language_unavailable = choice.unavailable;
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
/// Progress arrives as [`PROGRESS_EVENT`]. `run` is a number the window chose
/// for this recognition, never 0, and [`ocr_cancel`] with the same number stops
/// it before the next page, whichever of the two calls arrives first.
// The lint attribute goes first: `check_writers.py` and `ipc.test.ts` find a
// command by `#[tauri::command]` standing directly on its `fn`.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ocr_copy(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    cancel: tauri::State<'_, Cancel>,
    doc: u32,
    source: String,
    path: String,
    run: u64,
    language: Option<String>,
) -> Result<Recognised, String> {
    let plan = edits.plan(doc)?;
    let password = password_for(&service, doc, "ocr_copy").await;
    let library = pdfium_library_dir(&app);
    let service = service.inner().clone();
    let stopped = Arc::clone(&cancel.0);
    tauri::async_runtime::spawn_blocking(move || {
        ocr_copy_asked(
            &service,
            doc,
            plan,
            Path::new(&source),
            Path::new(&path),
            password,
            library,
            language.as_deref(),
            // Asked only for a reader who chose a language. With none chosen
            // the engine decides, which is `tpdf ocr`'s default.
            &language
                .as_ref()
                .map(|_| offered_languages().unwrap_or_default())
                .unwrap_or_default(),
            &|| stopped.load(Ordering::Relaxed) == run,
            &|at| {
                let _ = app.emit(PROGRESS_EVENT, at);
            },
        )
    })
    .await
    .map_err(|e| format!("Text recognition did not run: {e}"))?
}

/// The languages the reader can choose between for *Recognise text*.
///
/// Takes nothing and names no document. On the pool because the answer is the
/// operating system's and how long it takes to give it is not ours.
#[tauri::command]
pub async fn ocr_languages() -> Result<Offered, String> {
    tauri::async_runtime::spawn_blocking(offered)
        .await
        .map_err(|e| format!("The languages could not be listed: {e}"))
}

/// Asks recognition number `run` to stop before its next page.
#[tauri::command]
pub fn ocr_cancel(cancel: tauri::State<'_, Cancel>, run: u64) {
    cancel.0.store(run, Ordering::Relaxed);
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
    fn the_reason_nothing_was_added_names_the_pages_the_recogniser_refused() {
        let refused = |already: &[u32], pages: &[u32]| Recognised {
            refused: pages.to_vec(),
            ..report(already, &[], &[])
        };
        assert_eq!(
            nothing_to_add(&refused(&[], &[1])),
            "No page was given text. The recogniser refused page 1, which usually means a \
             script it cannot read or a scan too unclear to tell the script."
        );
        let mixed = nothing_to_add(&refused(&[1], &[2, 3]));
        assert!(mixed.contains("refused pages 2, 3, which"), "{mixed}");
        // Without one, nothing is said about refusing.
        assert!(!nothing_to_add(&report(&[1], &[2], &[])).contains("refused"));
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
            refused: vec![5],
            too_large: vec![4],
            engine: "vision".into(),
            language_unavailable: Some("de-DE".into()),
        })
        .unwrap();
        assert_eq!(
            sent,
            serde_json::json!({
                "pages": [{ "page": 2, "words": 7 }],
                "alreadyText": [1],
                "nothingRead": [3],
                "refused": [5],
                "tooLarge": [4],
                "engine": "vision",
                "languageUnavailable": "de-DE",
            })
        );
        let plain = serde_json::to_value(Recognised::default()).unwrap();
        assert!(plain.get("languageUnavailable").is_none(), "{plain}");
    }

    #[test]
    fn only_windows_tells_the_reader_more_languages_can_be_installed() {
        let listed = offered();
        assert_eq!(listed.installable, cfg!(windows));
        assert_eq!(
            listed.languages,
            offered_languages().unwrap_or_default(),
            "the reply is the engine's list"
        );
    }
}
