//! Printing: build the job, read it back, and hand it to the platform.
//!
//! The readback is the point of the shape here. `print_job` builds bytes and
//! `present_job` refuses to show a panel for bytes the platform's own PDF stack
//! cannot read --- PDFKit on macOS, `Windows.Data.Pdf` on Windows, PDFium in a
//! worker on Linux, which has no system parser that fits the licensing rule --- which is
//! the same standard `docs/PLAN.md` 6 sets for a redaction, and the reason the
//! two halves are separate functions rather than one command body.

use std::path::{Path, PathBuf};

use super::{outside_of, password_for};
use crate::diag;
use crate::render::RenderService;
use crate::{edits, print, save};

#[cfg(target_os = "macos")]
use crate::print_macos;
#[cfg(windows)]
use crate::print_win;

/// Builds a print job and opens the platform print dialog for it.
///
/// `async` keeps the build off the thread the webview draws on: a job waits on a
/// worker that parses the whole document, and reads the job back, and on a
/// 337 MB scan that is not something to do there. Only the panel is dispatched
/// back.
///
/// **The build runs on the blocking pool, and that is not the choice the seven
/// render-service commands made.** Being `async` puts this on the runtime rather
/// than the main thread, which was the whole of the original argument and is
/// only half of one: a synchronous parse inside an `async fn` occupies one of
/// the runtime's few worker threads for its entire duration, and it is `await`
/// that yields a thread, not `async`. The render-service bridges --- everything
/// in [`super::document`] and [`super::read`] --- rejected
/// `spawn_blocking` because the work they wait for happens on the render thread,
/// so moving the *wait* to a larger pool raises the bound instead of removing
/// it. Here the work is in this function, CPU-bound and synchronous, which is
/// what the blocking pool is for. The two look like the same fix and are
/// opposite readings of where the time is spent.
///
/// Returns as soon as the panel has been *asked for*, not when it closes. The
/// outcome is deliberately not reported: `runOperation` answers one boolean for
/// both "printed" and "cancelled" (see `print_macos::present`), so a caller
/// waiting for it could only turn a Cancel into an error message.
///
/// **Rejects with a serialised [`save::Refusal`], not with a sentence**, since
/// 2026-09-01. One of the refusals here is about the *file* having been replaced
/// under the reader, and Save a copy and Reload are the two things that answer
/// it --- neither of which a window can offer for a refusal it can only read as
/// prose, and neither of which it may offer for the refusals where reloading
/// would spend the reader's edits for nothing. Every other error path carries
/// `changed: false` through `From<String>`, so the flag is set at the one call
/// site that knows it and nowhere else.
#[tauri::command]
pub async fn print_document(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    path: String,
    doc: Option<u32>,
    pages: Option<Vec<u32>>,
    turns: u8,
) -> Result<(), save::Refusal> {
    let built = build_job(&app, &edits, &service, path, doc, pages, turns).await?;
    present_built(&app, built).await
}

/// A print job built and not yet handed to the platform.
struct Built {
    bytes: Vec<u8>,
    title: String,
    /// How many pages the read-back should find; `None` for every page.
    expected: Option<usize>,
}

/// Reads the job back and opens the platform's dialog for it, off the thread
/// the webview draws on.
async fn present_built(app: &tauri::AppHandle, built: Built) -> Result<(), save::Refusal> {
    // On the blocking pool: every platform reads the job back before the
    // panel, and on Linux that read waits on a worker.
    let presenting = app.clone();
    let Built {
        bytes,
        title,
        expected,
    } = built;
    tauri::async_runtime::spawn_blocking(move || present_job(&presenting, bytes, title, expected))
        .await
        .map_err(|e| save::Refusal::from(format!("the print panel could not be shown: {e}")))?
        .map_err(save::Refusal::from)
}

/// Builds the bytes `print_document` and `print_prepare` print, with the page
/// count the read-back is held to.
async fn build_job(
    app: &tauri::AppHandle,
    edits: &edits::Edits,
    service: &RenderService,
    path: String,
    doc: Option<u32>,
    pages: Option<Vec<u32>>,
    turns: u8,
) -> Result<Built, save::Refusal> {
    let source = PathBuf::from(&path);
    // Read here rather than inside the chooser, so that what decides the shape of
    // the job is a pure function of the plan and the range --- and lives in the
    // module that owns `Pages`, where its tests are under the same filter as the
    // rest of them.
    let plan = doc.map(|doc| edits.plan(doc)).transpose()?;
    let route = print::route(plan.as_ref(), pages, turns);
    // How many pages the readback should find. `None` for the passthrough, where
    // the answer is "whatever the file has" and there is no count to compare
    // against --- see `expect_pages`, which treats `None` as "everything".
    let expected = match (&route, &plan) {
        (print::Route::Passthrough, _) => None,
        (print::Route::Working, Some(plan)) => Some(plan.pages.len()),
        // Unreachable: `route` answers `Working` only with a plan in hand. Kept
        // as the safe arm rather than an `unwrap`, because what a panic would
        // replace is a *count that is not compared*, which is the outcome the
        // passthrough already produces.
        (print::Route::Working, None) => None,
        (print::Route::Range(job), _) => match &job.pages {
            print::Pages::Only(wanted) => Some(wanted.len()),
            // `None` for both, and for one reason each. `All` has no count to
            // compare against; `Unlistable` is unreachable here --- `route`
            // sends a plan carrying one to `Working` --- and the safe arm is the
            // same "do not compare" the passthrough already answers.
            print::Pages::All | print::Pages::Unlistable => None,
        },
    };
    // Read before `source` is moved onto the pool; the name is wanted whether or
    // not the build succeeds, and cloning the path to keep it would be carrying
    // a second copy of the thing that is about to be parsed.
    let title = source.file_name().map_or_else(
        || "Document".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    // **The working document goes through the writer a save uses**, which is what
    // puts the reader's marks and crops on the paper --- see `print::Route`. The
    // plan is moved onto the pool with it, so nothing here holds the model's lock
    // while a 337 MB document is parsed.
    //
    // A panicking build would otherwise surface as a command that returned
    // nothing, which is indistinguishable from a panel the reader dismissed.
    // **For a refusal, not for a print job.** `save::print_bytes` refuses an
    // encrypted document either way --- neither re-encrypting nor decrypting is
    // right for a printer --- but without the key the parse in front of that
    // refusal fails first and tells the reader to open the document with the
    // password it is already open with. See `print_bytes`, which carries the
    // reasoning. `None` for the passthrough and the range, which do not rewrite.
    let password = match doc {
        Some(doc) => password_for(service, doc, "print_document").await,
        None => None,
    };
    // **Who parses the reader's document**, chosen the same way every other
    // writer is. The `Working` route and the range both reach it --- a range goes
    // through `save::print_range_bytes` to `Rewriter::write_range`, so it is parsed
    // where a save is and not here --- and the passthrough hands the file over
    // byte for byte and parses nothing. `docs/THREAT-MODEL.md` residual risk 18
    // is where the parses that left this process are recorded.
    let writing = outside_of(app, service.backend());
    let build = move || {
        print_job(
            &source,
            &route,
            plan.as_ref(),
            turns,
            password.as_deref(),
            &*writing,
        )
    };
    let bytes = tauri::async_runtime::spawn_blocking(build)
        .await
        .map_err(|e| save::Refusal::from(format!("the print job could not be built: {e}")))??;
    Ok(Built {
        bytes,
        title,
        expected,
    })
}

/// The print job the preview shows, held until the reader prints or closes it.
///
/// One at a time: the preview is modal, and a second `print_prepare` replaces
/// the first, whose worker is ended when it is dropped. The worker holds the
/// job's bytes, not the reader's file, so the pictures are of exactly what is
/// handed to the dialog afterwards.
/// Generic over the worker so its tests can hold a stand-in: a real one is a
/// process, and the rules about when one is handed back are what they test.
pub struct Previews<D = Drawer>(std::sync::Mutex<Shown<D>>);

impl<D> Default for Previews<D> {
    fn default() -> Self {
        Self(std::sync::Mutex::new(Shown {
            job: None,
            last: 0,
            built: None,
            drawer: None,
            sizes: Vec::new(),
            #[cfg(windows)]
            printer: None,
        }))
    }
}

/// The preview on screen, if any.
///
/// The job and the worker are held apart because a picture is drawn with the
/// worker taken out of the lock, and Print must still find the job while one
/// is being drawn.
struct Shown<D> {
    /// Names the preview on screen, so a late call for an earlier one is
    /// refused rather than answered from the job that replaced it. `None`
    /// once it is printed or closed.
    job: Option<u32>,
    /// The last number handed out; counts on across closes.
    last: u32,
    built: Option<Built>,
    /// The worker over the job's bytes and every sheet's size; `None` while a
    /// picture is being drawn.
    drawer: Option<D>,
    /// Each page of the job's size in points, which the layout needs while
    /// the worker is out drawing.
    sizes: Vec<crate::render::PageSize>,
    /// The printer the preview has chosen, with its settings; `None` until
    /// the preview asks, and while the driver is being asked something.
    #[cfg(windows)]
    printer: Option<crate::printers_win::Printer>,
}

pub struct Drawer {
    session: crate::save_outside::Session,
    sizes: Vec<crate::render::PageSize>,
}

impl<D> Previews<D> {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Shown<D>>, String> {
        self.0
            .lock()
            .map_err(|_| "the print preview is unavailable".to_owned())
    }

    /// Takes the worker out to draw with, if `job` is the preview on screen.
    fn take_drawer(&self, job: u32) -> Result<D, String> {
        let mut shown = self.lock()?;
        if shown.job != Some(job) {
            return Err(CLOSED.into());
        }
        shown
            .drawer
            .take()
            .ok_or_else(|| "another page of this preview is being drawn".into())
    }

    /// Gives the worker back, unless the preview was printed, closed or
    /// replaced meanwhile, in which case the worker ends here.
    fn give_back(&self, job: u32, drawer: D) {
        if let Ok(mut shown) = self.lock() {
            if shown.job == Some(job) && shown.drawer.is_none() {
                shown.drawer = Some(drawer);
            }
        }
    }

    /// Ends the preview `job`, returning its job when it was the one on
    /// screen. The worker ends with it, or when its picture is finished.
    fn end(&self, job: u32) -> Result<Built, String> {
        let mut shown = self.lock()?;
        if shown.job != Some(job) {
            return Err(CLOSED.into());
        }
        shown.job = None;
        shown.drawer = None;
        shown.sizes = Vec::new();
        #[cfg(windows)]
        {
            shown.printer = None;
        }
        shown.built.take().ok_or_else(|| CLOSED.into())
    }

    /// The job's page sizes and its printer, taken out to ask the driver
    /// with, if `job` is the preview on screen.
    #[cfg(windows)]
    fn take_printer(
        &self,
        job: u32,
    ) -> Result<
        (
            Vec<crate::render::PageSize>,
            Option<crate::printers_win::Printer>,
        ),
        String,
    > {
        let mut shown = self.lock()?;
        if shown.job != Some(job) {
            return Err(CLOSED.into());
        }
        Ok((shown.sizes.clone(), shown.printer.take()))
    }

    /// Gives the printer back, unless the preview has gone meanwhile.
    #[cfg(windows)]
    fn give_printer(&self, job: u32, printer: Option<crate::printers_win::Printer>) {
        if let Ok(mut shown) = self.lock() {
            if shown.job == Some(job) {
                shown.printer = printer;
            }
        }
    }
}

const CLOSED: &str = "this print preview has been closed";

/// What the preview needs to lay out its pages before any is drawn.
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub job: u32,
    /// Each sheet's size in points, after its rotation.
    pub pages: Vec<crate::render::PageSize>,
}

/// The widest picture of a sheet the preview asks for, in pixels.
///
/// Twice the preview's widest page on a large screen. Asked for more, the
/// backend draws this much: a picture is for looking at, and the time PDFium
/// takes grows with its area (`docs/VERIFICATION.md`, *How long Windows takes
/// to rasterise a page for printing*).
pub const PREVIEW_MAX_WIDTH: u32 = 1600;

/// How long one sheet's picture may take. Longer than a viewer tile's: the
/// dense A0 drawing takes PDFium about 2 s at this size, and a page ten times
/// denser is still a page the reader is about to print.
const PREVIEW_DEADLINE: std::time::Duration = std::time::Duration::from_secs(120);

/// Builds the print job and holds it for the preview.
///
/// The job is the one `print_document` would print, built the same way, and
/// a worker is started over its bytes and asked for every sheet's size. The
/// platform's own read-back is not done here: it is done when the reader
/// presses Print, in `print_confirm`, as `print_document` does it.
///
/// # Errors
///
/// As `print_document`; or the worker could not read the job, or found a
/// different number of pages from the one asked for.
// The lint attribute goes first: `check_writers.py` and `ipc.test.ts` find a
// command by `#[tauri::command]` standing directly on its `fn`.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn print_prepare(
    app: tauri::AppHandle,
    edits: tauri::State<'_, edits::Edits>,
    service: tauri::State<'_, RenderService>,
    previews: tauri::State<'_, Previews>,
    path: String,
    doc: Option<u32>,
    pages: Option<Vec<u32>>,
    turns: u8,
) -> Result<Preview, save::Refusal> {
    let built = build_job(&app, &edits, &service, path, doc, pages, turns).await?;
    let library = crate::pdfium_library_dir(&app);
    let (built, session, sizes) =
        tauri::async_runtime::spawn_blocking(move || open_preview(built, &library))
            .await
            .map_err(|e| save::Refusal::from(format!("the print preview could not start: {e}")))?
            .map_err(save::Refusal::from)?;
    let mut shown = previews.lock().map_err(save::Refusal::from)?;
    shown.last = shown.last.wrapping_add(1).max(1);
    let job = shown.last;
    let preview = Preview {
        job,
        pages: sizes.clone(),
    };
    shown.job = Some(job);
    shown.built = Some(built);
    shown.sizes = sizes.clone();
    #[cfg(windows)]
    {
        shown.printer = None;
    }
    shown.drawer = Some(Drawer { session, sizes });
    Ok(preview)
}

/// The printer and paper a preview prints on, and its sheets laid out.
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Setup {
    pub printers: Vec<String>,
    pub device: crate::print_layout::Device,
    pub paper: crate::print_layout::Paper,
    /// Each sheet's pages, placed in points from the printable area's corner.
    pub sheets: Vec<Vec<crate::print_layout::Placement>>,
}

/// Applies the preview's settings to its printer and lays the job out on the
/// paper that gives. The default printer when none is named.
///
/// Windows only: macOS and Linux choose these in their own print dialogs.
///
/// # Errors
///
/// `job` is not the preview held now, no printer is installed, the driver
/// refuses the settings, or the layout is refused.
#[tauri::command]
pub async fn print_setup(
    app: tauri::AppHandle,
    previews: tauri::State<'_, Previews>,
    job: u32,
    settings: crate::print_layout::Settings,
) -> Result<Setup, String> {
    set_up(&app, &previews, job, settings, false).await
}

/// Opens the printer driver's own settings dialog, then lays the job out on
/// the paper it leaves chosen. The settings are those `print_setup` took.
///
/// # Errors
///
/// As `print_setup`, and the driver failing to show its dialog.
#[tauri::command]
pub async fn print_printer_settings(
    app: tauri::AppHandle,
    previews: tauri::State<'_, Previews>,
    job: u32,
    settings: crate::print_layout::Settings,
) -> Result<Setup, String> {
    set_up(&app, &previews, job, settings, true).await
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(clippy::unused_async)]
async fn set_up(
    _app: &tauri::AppHandle,
    _previews: &Previews,
    _job: u32,
    _settings: crate::print_layout::Settings,
    _prompt: bool,
) -> Result<Setup, String> {
    Err("the printer is chosen in the system's print dialog here".into())
}

#[cfg(windows)]
async fn set_up(
    app: &tauri::AppHandle,
    previews: &Previews,
    job: u32,
    settings: crate::print_layout::Settings,
    prompt: bool,
) -> Result<Setup, String> {
    let owner = prompt.then(|| owner_of(app)).flatten();
    let (sizes, held) = previews.take_printer(job)?;
    let (printer, setup) = tauri::async_runtime::spawn_blocking(move || {
        let owner = owner.map(|h| windows::Win32::Foundation::HWND(h as *mut std::ffi::c_void));
        apply(held, &sizes, &settings, prompt, owner)
    })
    .await
    .map_err(|e| format!("the printer could not be asked: {e}"))?;
    previews.give_printer(job, printer);
    setup
}

/// The window's handle as an integer, to cross onto another thread; see
/// `present_job` for why that is sound.
#[cfg(windows)]
fn owner_of(app: &tauri::AppHandle) -> Option<isize> {
    use tauri::Manager;
    app.get_webview_window("main")
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
}

/// Opens the printer the settings name, unless it is the one held, sets it
/// up, and lays the job out. The printer is handed back whatever happens, so
/// a refused setting does not lose the reader's earlier ones.
#[cfg(windows)]
fn apply(
    held: Option<crate::printers_win::Printer>,
    sizes: &[crate::render::PageSize],
    settings: &crate::print_layout::Settings,
    prompt: bool,
    owner: Option<windows::Win32::Foundation::HWND>,
) -> (Option<crate::printers_win::Printer>, Result<Setup, String>) {
    use crate::printers_win::{default_name, list, Printer};
    let printers = match list() {
        Ok(printers) => printers,
        Err(e) => return (held, Err(e)),
    };
    let wanted = settings
        .printer
        .clone()
        .or_else(|| held.as_ref().map(|p| p.name().to_owned()))
        .or_else(default_name)
        .or_else(|| printers.first().cloned());
    let Some(wanted) = wanted else {
        return (held, Err("no printer is installed".into()));
    };
    let mut printer = match held {
        Some(printer) if printer.name() == wanted => printer,
        other => match Printer::open(&wanted) {
            Ok(printer) => printer,
            Err(e) => return (other, Err(e)),
        },
    };
    let chosen = if prompt {
        printer.prompt(owner).map(|_| ())
    } else {
        printer.choose(
            settings.paper,
            settings.landscape,
            settings.copies,
            settings.two_sided,
            settings.colour,
        )
    };
    let setup = chosen.and_then(|()| {
        let paper = printer.paper()?;
        let sheets = crate::print_layout::lay_out(
            sizes,
            &chosen_pages(settings, sizes.len())?,
            &paper,
            settings.layout(),
        )?;
        Ok(Setup {
            printers,
            device: printer.describe(),
            paper,
            sheets,
        })
    });
    (Some(printer), setup)
}

/// The pages the settings name, or every page of a job of `count`.
///
/// # Errors
///
/// No page at all, which is nothing to print.
#[cfg(windows)]
fn chosen_pages(
    settings: &crate::print_layout::Settings,
    count: usize,
) -> Result<Vec<u32>, String> {
    let pages = settings
        .pages
        .clone()
        .unwrap_or_else(|| (0..u32::try_from(count).unwrap_or(u32::MAX)).collect());
    if pages.is_empty() {
        return Err("no page is chosen to print".into());
    }
    Ok(pages)
}

/// Starts a worker over the job and reads every sheet's size.
fn open_preview(
    built: Built,
    library: &Path,
) -> Result<
    (
        Built,
        crate::save_outside::Session,
        Vec<crate::render::PageSize>,
    ),
    String,
> {
    let mut session = save::InWorker::at(library.to_path_buf()).session_over(&built.bytes)?;
    let reply = session
        .ask(crate::worker_proto::Request::Open {
            lazy_geometry: false,
        })
        .map_err(|e| format!("the print job could not be read: {}", e.message()))?;
    let crate::worker_proto::Reply::Open {
        pages, page_count, ..
    } = reply
    else {
        return Err(format!(
            "the worker answered the print job with {}",
            reply.described()
        ));
    };
    if pages.len() != page_count {
        return Err("the worker returned incomplete page sizes for the print job".into());
    }
    print::expect_pages(page_count, built.expected)?;
    Ok((built, session, pages))
}

/// One sheet of the held job as a PNG `data:` URL, `width` pixels wide at
/// most and never wider than [`PREVIEW_MAX_WIDTH`], with its comments and
/// other marks only when `marks` is set.
///
/// # Errors
///
/// `job` is not the preview held now, the sheet is not in it, or the worker
/// could not draw it. A worker that ran out of time is ended, so every later
/// sheet of this preview fails too, and says so.
#[tauri::command]
pub async fn print_preview_page(
    previews: tauri::State<'_, Previews>,
    job: u32,
    index: u32,
    width: u32,
    marks: Option<bool>,
) -> Result<String, String> {
    // Taken out of the state while it draws, so the lock is not held for the
    // seconds a dense sheet takes, and Print still finds the job meanwhile.
    let mut drawer = previews.take_drawer(job)?;
    let (drawer, drawn) = tauri::async_runtime::spawn_blocking(move || {
        let drawn = draw_sheet(&mut drawer, index, width, marks.unwrap_or(true));
        (drawer, drawn)
    })
    .await
    .map_err(|e| format!("the page could not be drawn: {e}"))?;
    previews.give_back(job, drawer);
    drawn
}

/// Draws one sheet of the held job, tiled as `tpdf render` tiles a page.
fn draw_sheet(held: &mut Drawer, index: u32, width: u32, marks: bool) -> Result<String, String> {
    let size = *held
        .sizes
        .get(index as usize)
        .ok_or("that page is not in the print job")?;
    let (scale, wide, high) = preview_scale(size, width)?;
    let mut pixels = vec![0_u8; wide as usize * high as usize * 4];
    for y in (0..high).step_by(1024) {
        for x in (0..wide).step_by(1024) {
            let tile_width = (wide - x).min(1024);
            let tile_height = (high - y).min(1024);
            let tile = held
                .session
                .tile_within(
                    crate::render::TileRequest {
                        annotations: marks,
                        rid: 0,
                        doc: 0,
                        page: index,
                        scale,
                        turns: 0,
                        invert: false,
                        crop: None,
                        x: i32::try_from(x).map_err(|_| "the page is too wide")?,
                        y: i32::try_from(y).map_err(|_| "the page is too tall")?,
                        width: u16::try_from(tile_width).map_err(|_| "a tile is too wide")?,
                        height: u16::try_from(tile_height).map_err(|_| "a tile is too tall")?,
                        format: crate::render::TileFormat::Raw,
                    },
                    PREVIEW_DEADLINE,
                )
                .map_err(|e| format!("the page could not be drawn: {}", e.message()))?;
            let stride = tile_width as usize * 4;
            for row in 0..tile_height as usize {
                let start = ((y as usize + row) * wide as usize + x as usize) * 4;
                pixels[start..start + stride]
                    .copy_from_slice(&tile[row * stride..(row + 1) * stride]);
            }
        }
    }
    let png = crate::render::encode_png(&pixels, wide, high)?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png)
    ))
}

/// The scale that draws `size` at `width` pixels, clamped to
/// [`PREVIEW_MAX_WIDTH`], with the picture's pixel size.
///
/// # Errors
///
/// A width of zero, or a page with no area.
fn preview_scale(size: crate::render::PageSize, width: u32) -> Result<(f32, u32, u32), String> {
    let width = width.min(PREVIEW_MAX_WIDTH);
    if width == 0 || !(size.width_pt > 0.0 && size.height_pt > 0.0) {
        return Err("the page has no size to draw".into());
    }
    #[allow(clippy::cast_precision_loss)]
    let scale = width as f32 / size.width_pt;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let high = (size.height_pt * scale).round().max(1.0) as u32;
    // A very narrow, very tall page: bounded the same way on its long side.
    if high > PREVIEW_MAX_WIDTH * 4 {
        return Err("the page is too tall to preview".into());
    }
    Ok((scale, width, high))
}

/// Prints the held job. The preview is released whatever the outcome.
///
/// With `settings`, which only Windows sends, the job goes to the printer the
/// preview chose, laid out as it showed, with no further dialog. Without, the
/// platform's read-back and then its dialog, as `print_document` does.
///
/// # Errors
///
/// `job` is not the preview held now, or anything `print_document` refuses
/// once the job is built.
#[tauri::command]
pub async fn print_confirm(
    app: tauri::AppHandle,
    previews: tauri::State<'_, Previews>,
    job: u32,
    settings: Option<crate::print_layout::Settings>,
) -> Result<(), save::Refusal> {
    #[cfg(windows)]
    if let Some(settings) = settings {
        let (sizes, printer) = previews.take_printer(job).map_err(save::Refusal::from)?;
        let built = previews.end(job).map_err(save::Refusal::from)?;
        let printer = printer.ok_or("no printer has been chosen for this print")?;
        let app = app.clone();
        return tauri::async_runtime::spawn_blocking(move || {
            print_chosen(&app, built, printer, &sizes, &settings)
        })
        .await
        .map_err(|e| save::Refusal::from(format!("the print could not start: {e}")))?
        .map_err(save::Refusal::from);
    }
    // Only Windows sends settings; the system dialogs choose them elsewhere.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let _ = settings;
    // The worker ends here, or when the picture it is drawing is finished,
    // before the dialog: Windows starts its own over the same bytes to print.
    let built = previews.end(job).map_err(save::Refusal::from)?;
    present_built(&app, built).await
}

/// Reads the job back as `present_job` does, then prints it on the printer
/// the preview chose, on a thread of its own.
///
/// Copies past what the driver makes by itself are made by sending the pages
/// again, each copy whole before the next.
#[cfg(windows)]
fn print_chosen(
    app: &tauri::AppHandle,
    built: Built,
    mut printer: crate::printers_win::Printer,
    sizes: &[crate::render::PageSize],
    settings: &crate::print_layout::Settings,
) -> Result<(), String> {
    let Built {
        bytes,
        title,
        expected,
    } = built;
    let reading = print_win::read(&bytes)
        .ok_or("the print job could not be read back, so it will not be printed")?;
    print::expect_pages(reading.pages.len(), expected)?;
    let mut pages = chosen_pages(settings, reading.pages.len())?;
    let copies = settings.copies.max(1);
    let layout = settings.layout();
    // Laid out here as well as on the print thread, for the count the
    // progress line says, from the sizes the preview's worker read: the
    // read-back's are `Windows.Data.Pdf`'s, in DIPs rather than points.
    let mut sheets = crate::print_layout::lay_out(sizes, &pages, &printer.paper()?, layout)?.len();
    if copies > printer.describe().most_copies {
        pages = pages.repeat(usize::from(copies));
        sheets *= usize::from(copies);
        printer.choose(None, None, 1, None, None)?;
    }
    // Crosses to the print thread as an integer, as the owner window does: a
    // DC is a process-wide handle, and only that thread uses it from here.
    let dc = printer.dc()?.0 as isize;
    let marks = settings.marks;
    let library = crate::pdfium_library_dir(app);
    on_print_thread(app, move |owner, progress| {
        let dc = windows::Win32::Graphics::Gdi::HDC(dc as *mut std::ffi::c_void);
        print_win::print_on(
            dc, &bytes, &title, owner, progress, &library, &pages, layout, sheets, marks,
        )
    });
    Ok(())
}

/// Stops the Windows print that is being drawn. Elsewhere, and when nothing
/// is printing, it does nothing: macOS and Linux print through their own
/// systems, which have their own queues.
#[tauri::command]
pub fn print_cancel() {
    #[cfg(windows)]
    print_win::stop();
}

/// Runs a print on a thread of its own, reporting its progress to the window
/// and saying in a message over it why a print failed.
///
/// `print` answers whether a job was sent; `false` is Cancel, which shows
/// nothing.
#[cfg(windows)]
fn on_print_thread(
    app: &tauri::AppHandle,
    print: impl FnOnce(Option<windows::Win32::Foundation::HWND>, &dyn Fn(u32, u32)) -> Result<bool, String>
        + Send
        + 'static,
) {
    let owner = owner_of(app);
    let app = app.clone();
    std::thread::spawn(move || {
        use tauri::Emitter;
        let owner = owner.map(|h| windows::Win32::Foundation::HWND(h as *mut std::ffi::c_void));
        let last = std::cell::Cell::new((0, 0));
        let report = |sent, of, state| {
            let _ = app.emit(PROGRESS_EVENT, Printing { sent, of, state });
        };
        let printed = print(owner, &|sent, of| {
            last.set((sent, of));
            report(sent, of, "printing");
        });
        let (sent, of) = last.get();
        match printed {
            Ok(true) => report(sent, of, "sent"),
            // Stopped while printing: the line says so instead of "Printing".
            Ok(false) if of > 0 => report(sent, of, "stopped"),
            // Cancel in the dialog: no line was shown, so there is none to clear.
            Ok(false) => {}
            Err(e) => {
                report(sent, of, "failed");
                diag::note(&format!("[print] {e}"));
                // Said in the window too, as on Linux: the dialog has closed, and
                // until 26.10.24 a refused job on Windows was only this log line.
                print_win::tell(owner, &e);
            }
        }
    });
}

/// Closes the preview without printing, ending its worker.
#[tauri::command]
pub fn print_discard(previews: tauri::State<'_, Previews>, job: u32) {
    let _ = previews.end(job);
}

/// The bytes of a print job, whichever route the reader's document takes.
///
/// A function rather than the closure it was until 2026-09-01, and the reason is
/// the error type beside it. Every refusal here now reaches the window whole,
/// including the one bit that decides what the window may offer to do about it,
/// and the way that stops being true is a `map_err` reaching for the message ---
/// which compiles, reads correctly, and delivers a correct sentence with the
/// action missing. That is exactly the shape `docs/TRAPS.md` records under *A
/// refusal flattened to a string across a process boundary loses the action that
/// answers it*, and it was invisible to every test while this was a closure
/// inside a `#[tauri::command]`: nothing can call one.
///
/// # Errors
///
/// The source is not the file the plan was made against, or anything the route's
/// own writer refuses.
pub(crate) fn print_job(
    source: &Path,
    route: &print::Route,
    plan: Option<&edits::Plan>,
    turns: u8,
    password: Option<&str>,
    rewriter: &dyn save::Rewriter,
) -> Result<Vec<u8>, save::Refusal> {
    // **The two routes that do not go through `save::print_bytes`, which asks
    // this itself.** All three read `source` by name, so all three can be
    // handed a file that is no longer the one the reader is looking at ---
    // `Passthrough` prints the whole of it, `Range` prints the page numbers
    // the reader typed against a document that no longer has those pages.
    // Asked once here rather than in each arm, because a second call would
    // hash the file twice for the arm that already did it.
    //
    // `plan` is what carries the fingerprint, and a print with no document
    // open has none to compare: `print::route` sends a plan-less job to
    // `Range`, and nothing about it was made against a file. See
    // `save::print_ready` for why a missing fingerprint prints rather than
    // being refused.
    if !matches!(route, print::Route::Working) {
        if let Some(plan) = plan {
            save::print_ready(source, plan)?;
        }
    }
    match route {
        print::Route::Passthrough => std::fs::read(source)
            .map_err(|e| save::Refusal::from(format!("could not read {source:?}: {e}"))),
        print::Route::Working => {
            let plan = plan.ok_or("the working document has no plan to print")?;
            save::print_bytes(source, plan, turns, password, rewriter)
        }
        print::Route::Range(job) => save::print_range_bytes(source, job, rewriter),
    }
}

/// Hands built bytes to the platform, having first read them back.
#[cfg(target_os = "macos")]
fn present_job(
    app: &tauri::AppHandle,
    bytes: Vec<u8>,
    title: String,
    expected: Option<usize>,
) -> Result<(), String> {
    // Re-parsed by PDFKit before anything is offered to a printer --- a third
    // parser, and the one the print system will use itself. Refusing here costs
    // a dialog; not refusing costs paper.
    let reading = print_macos::read(&bytes)
        .ok_or("the print job could not be read back, so it will not be printed")?;
    print::expect_pages(reading.pages.len(), expected)?;

    app.run_on_main_thread(move || {
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            // Unreachable by construction, and silence here would be a print
            // command that does nothing and says nothing.
            diag::note("[print] dispatched off the main thread; no panel shown");
            return;
        };
        if let Err(e) = print_macos::present(&bytes, &title, mtm) {
            diag::note(&format!("[print] {e}"));
        }
    })
    // The same rule: what failed was the hop to the main thread, and without
    // saying so the reader gets a bare runtime message for a print that silently
    // did not happen.
    .map_err(|e| format!("the print panel could not be shown on the main thread: {e}"))
}

/// Hands built bytes to Windows, having first read them back.
///
/// Structurally the same as the macOS arm above and for the same reasons: an
/// independent parser reads the job, the page count is checked against what was
/// asked for, and only then does a panel open. `Windows.Data.Pdf` stands where
/// PDFKit stands --- the operating system's own PDF stack, independent of the
/// `lopdf` that wrote the job and of the PDFium that drew what the reader saw.
/// Refusing here costs a dialog; not refusing costs paper.
///
/// Two differences from macOS, both real and neither a shortcut. Windows has no
/// in-box PDF print API, so `print_win::present` rasterises each page onto the
/// printer's device context --- see that module for what raster output costs. And
/// the dialog is modal on the calling thread, so it runs on a blocking task rather
/// than through `run_on_main_thread`: `PrintDlgW` pumps its own message loop, and
/// occupying Tauri's main thread with it would freeze the window behind it for as
/// long as the panel is open.
#[cfg(windows)]
fn present_job(
    app: &tauri::AppHandle,
    bytes: Vec<u8>,
    title: String,
    expected: Option<usize>,
) -> Result<(), String> {
    let reading = print_win::read(&bytes)
        .ok_or("the print job could not be read back, so it will not be printed")?;
    print::expect_pages(reading.pages.len(), expected)?;

    // The owner window, so the panel is modal to the document rather than floating
    // free. `None` is degradation and not failure: a print dialog with no owner is
    // still a print dialog, where refusing to print because a window handle could
    // not be found would be a worse outcome than a slightly misplaced panel.
    // Carried across the thread boundary as an integer, not as an `HWND`
    // (`owner_of`). A raw handle is a `*mut c_void` and therefore not `Send`, and
    // the compiler is right to say so in general --- but a window handle is a
    // process-wide kernel-managed value with no thread affinity for this use, and
    // `PrintDlgW` only ever reads it to parent a dialog. Reconstructed on the far
    // side rather than smuggled through a wrapper type, so the one
    // unsound-looking step is one line and is where the reasoning is written down.
    let library = crate::pdfium_library_dir(app);
    on_print_thread(app, move |owner, progress| {
        print_win::present(&bytes, &title, owner, progress, &library)
    });
    Ok(())
}

/// The event that says how far a Windows print has got. `PROGRESS_EVENT` in
/// `printing.ts`, whose test holds the two spellings together.
///
/// Windows only: there tpdf rasterises every sheet itself, which takes minutes
/// on a dense page, and the system shows nothing meanwhile. macOS and Linux
/// hand the PDF to a print system that shows its own progress.
#[cfg_attr(any(target_os = "macos", target_os = "linux"), allow(dead_code))]
pub const PROGRESS_EVENT: &str = "tpdf://print-progress";

/// What [`PROGRESS_EVENT`] carries. `Printing` in `printing.ts`.
#[cfg(windows)]
#[derive(serde::Serialize, Clone)]
struct Printing {
    /// Sheets handed to the printer so far.
    sent: u32,
    /// Sheets in the job.
    of: u32,
    /// `printing`, `sent` or `failed`.
    state: &'static str,
}

/// Hands built bytes to GTK's print dialog on Linux, having first read them back.
///
/// The same order as the two arms above: the job is read back by a parser that
/// did not write it and its pages counted before any dialog opens. The reader
/// is PDFium in a worker (`save::InWorker::printed_pages`), since Linux has no
/// system PDF parser that fits the licensing rule, and the job is written to a
/// private file first because a `GtkPrintJob` sends a file
/// (`print_linux.rs`). The dialog runs on GTK's thread, as the PIN dialog does.
#[cfg(target_os = "linux")]
fn present_job(
    app: &tauri::AppHandle,
    bytes: Vec<u8>,
    title: String,
    expected: Option<usize>,
) -> Result<(), String> {
    let (path, file) = crate::print_linux::stage(&bytes)?;
    let checked = save::InWorker::at(crate::pdfium_library_dir(app))
        .printed_pages(&file, bytes.len())
        .map_err(|e| {
            format!("the print job could not be read back, so it will not be printed: {e}")
        })
        .and_then(|pages| print::expect_pages(pages, expected));
    drop(file);
    if let Err(why) = checked {
        let _ = std::fs::remove_file(&path);
        return Err(why);
    }
    let shown = app.clone();
    let staged = path.clone();
    app.run_on_main_thread(move || {
        use tauri::Manager as _;
        let parent = shown
            .get_webview_window("main")
            .and_then(|window| window.gtk_window().ok());
        let parent = parent
            .as_ref()
            .map(gtk::prelude::Cast::upcast_ref::<gtk::Window>);
        if let Err(e) = crate::print_linux::present(staged, &title, parent) {
            diag::note(&format!("[print] {e}"));
            // Said in the window too: this is after the dialog closed, so the
            // reader pressed Print and nothing else would tell them it did not.
            crate::print_linux::tell(parent, &e);
        }
    })
    .map_err(|e| {
        let _ = std::fs::remove_file(&path);
        format!("the print panel could not be shown on the main thread: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::{preview_scale, Built, Previews, PREVIEW_MAX_WIDTH};
    use crate::render::PageSize;

    const A4: PageSize = PageSize {
        width_pt: 595.0,
        height_pt: 842.0,
    };

    /// The picture is the width asked for, with the page's own proportions.
    #[test]
    fn a_sheet_is_drawn_at_the_width_asked_for() {
        let (scale, wide, high) = preview_scale(A4, 400).expect("a size");
        assert_eq!(wide, 400);
        assert_eq!(high, 566);
        assert!((scale - 400.0 / 595.0).abs() < 1e-6);
    }

    /// Asked for more than the limit, the backend draws the limit.
    #[test]
    fn a_picture_wider_than_the_limit_is_drawn_at_the_limit() {
        let (_, wide, _) = preview_scale(A4, 100_000).expect("a size");
        assert_eq!(wide, PREVIEW_MAX_WIDTH);
    }

    /// Nothing to draw is an answer, not a picture of nothing.
    #[test]
    fn a_page_with_no_size_or_no_width_is_refused() {
        assert!(preview_scale(A4, 0).is_err());
        let flat = PageSize {
            width_pt: 595.0,
            height_pt: 0.0,
        };
        assert!(preview_scale(flat, 400).is_err());
        let sliver = PageSize {
            width_pt: 1.0,
            height_pt: 10_000.0,
        };
        assert!(preview_scale(sliver, 400).is_err());
    }

    /// A preview on screen as `job`, holding a stand-in worker numbered `job`.
    fn held(previews: &Previews<u32>, job: u32) {
        let mut shown = previews.lock().expect("a lock");
        shown.last = job;
        shown.job = Some(job);
        shown.built = Some(Built {
            bytes: b"the job".to_vec(),
            title: "job.pdf".into(),
            expected: None,
        });
        shown.drawer = Some(job);
    }

    /// Print takes the job on screen once; a second Print, a close or a late
    /// call for an earlier preview gets nothing.
    #[test]
    fn a_preview_is_printed_once_and_only_while_it_is_on_screen() {
        let previews = Previews::<u32>::default();
        held(&previews, 7);
        assert!(
            previews.end(6).is_err(),
            "an earlier preview is not this one"
        );
        let built = previews.end(7).expect("the preview on screen");
        assert_eq!(built.bytes, b"the job");
        assert!(previews.end(7).is_err(), "printed already");
    }

    /// A call for an earlier preview does not get the worker of the one that
    /// replaced it, which holds a different job.
    #[test]
    fn an_earlier_preview_does_not_draw_with_the_current_ones_worker() {
        let previews = Previews::<u32>::default();
        held(&previews, 8);
        assert!(previews.take_drawer(7).is_err());
        assert_eq!(previews.take_drawer(8), Ok(8));
    }

    /// While a picture is drawn the worker is out of the state, and Print
    /// must still find the job; the worker coming back afterwards is not
    /// kept, so it ends rather than outliving the preview.
    #[test]
    fn print_finds_the_job_while_a_picture_is_being_drawn() {
        let previews = Previews::<u32>::default();
        held(&previews, 3);
        let drawer = previews.take_drawer(3).expect("the worker");
        let why = previews
            .take_drawer(3)
            .expect_err("no worker to take while one is drawing");
        assert!(why.contains("being drawn"), "{why}");
        assert!(previews.end(3).is_ok());
        previews.give_back(3, drawer);
        assert!(
            previews.lock().expect("a lock").drawer.is_none(),
            "kept after Print"
        );
    }

    /// A picture finished while the preview is still on screen gives the
    /// worker back for the next one.
    #[test]
    fn a_worker_comes_back_to_the_preview_it_drew_for() {
        let previews = Previews::<u32>::default();
        held(&previews, 4);
        let drawer = previews.take_drawer(4).expect("the worker");
        previews.give_back(4, drawer);
        assert_eq!(previews.take_drawer(4), Ok(4));
    }
}
