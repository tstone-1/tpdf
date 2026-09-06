//! Opening a document, closing it, and the geometry of its pages.
//!
//! Everything here is a question for `render::RenderService`, and every answer
//! comes back through the reply channel in [`super`]. The eager open lives here
//! too: it is the same open as [`open_document`]'s, started before the webview
//! can ask for it, and keeping the two in one file is what stops the second
//! drifting from the first.

use std::path::{Path, PathBuf};

use parking_lot::Mutex;

use tauri::Manager;

use super::{await_reply, reply_channel, ReplyRx};
use crate::render::{DocumentInfo, RenderService};
use crate::{edits, progressive, recentdocs, render, startup};

/// A document open that was started before the webview asked for it.
///
/// The path is known at launch --- from a file association, an argument, or
/// `TPDF_STARTUP` here --- and the shell then spends ~95 ms booting a webview
/// that cannot ask for anything. The open can run inside that interval instead
/// of after it. Holding the receiver rather than the result means the frontend
/// blocks only if it beat the render service to the finish.
///
/// The path is kept beside the receiver because this is a *speculative* answer:
/// it is the document one particular path resolved to, and handing it back to
/// whoever asks next would answer a request for file B with file A --- silently,
/// with the right page count for the wrong document. Nothing in the shipped app
/// can ask for a second path first, since the frontend opens what
/// `startup_path()` gave it; that is a precondition of the spike wiring rather
/// than a property of the command, so it is checked rather than relied on.
pub(crate) struct EagerOpen {
    /// What was opened, to be compared against what is asked for.
    pub(crate) path: PathBuf,
    /// The pending result, taken by the first matching request.
    pub(crate) pending: Mutex<Option<ReplyRx<DocumentInfo, progressive::Refusal>>>,
}

/// Whether page geometry should be collected lazily rather than up front.
///
/// Lazy is the default, and it is the reason the Phase 0 startup criterion is
/// met: enumerating every page of the 775-page corpus costs 86 ms on the
/// critical path to buy a scrollbar exactness the scroller estimates anyway
/// (docs/PLAN.md §4). `TPDF_EAGER_GEOMETRY` restores the walk, so the variant
/// that measurement compared against is still reachable.
pub(crate) fn lazy_geometry() -> bool {
    std::env::var_os("TPDF_EAGER_GEOMETRY").is_none()
}

/// Opens a document and returns its page geometry.
///
/// Collects an eager open if one is outstanding, which is why this takes the
/// app handle: the pending receiver is managed state that only exists in that
/// variant.
///
/// `password` is what the reader typed after a previous call came back with
/// [`progressive::Refusal::locked`] set. It is a parameter rather than a second
/// command because opening a locked document and opening any other document
/// differ in one argument, and giving them separate entry points would give the
/// pool two ways to acquire a document to keep in step.
///
/// The refusal is structured for one reason: a locked document is not a damaged
/// one, and the frontend has to be able to ask rather than apologise.
#[tauri::command]
pub async fn open_document(
    app: tauri::AppHandle,
    service: tauri::State<'_, RenderService>,
    edits: tauri::State<'_, edits::Edits>,
    path: String,
    password: Option<String>,
) -> Result<DocumentInfo, progressive::Refusal> {
    let wanted = PathBuf::from(&path);
    // **Opened once, here, and the handle goes two ways.** One clone is what the
    // render service maps for the workers and the other is what is hashed, so
    // the fingerprint is a record of the bytes the reader is looking at. Opening
    // the name twice --- once to map, once to hash on a thread --- is two lookups
    // with a gap between them, and a file replaced in that gap by a revision of
    // the same page count is invisible to every guard there is: the reader sees
    // one document, the fingerprint describes another, and a save applies the
    // plan to whatever the name reaches.
    //
    // A file that will not open is not refused here. The backend is about to
    // meet the same failure and its refusal is the one that names the cause; all
    // this loses is the fingerprint, and a document with none can be read and not
    // saved over, which is the fail-closed half below.
    let handed = std::fs::File::open(&wanted).ok();
    let fingerprinting = handed.as_ref().and_then(|file| {
        let cloned = file.try_clone().ok()?;
        Some(crate::fingerprint::Opened {
            file: cloned,
            what: wanted.clone(),
        })
    });
    // The receiver comes out of the lock before anything is awaited, and it has
    // to: the guard is not `Send`, so holding one across the wait below would
    // not compile.
    let eager = app
        .try_state::<EagerOpen>()
        // Only for the path it was started on. A mismatch falls through to an
        // ordinary open and leaves the eager result where it is: it costs the
        // head start, which is what a speculative optimisation is allowed to
        // lose, rather than returning the wrong document.
        .filter(|eager| eager.path == wanted)
        // And never when a password is being offered. The eager open was started
        // before anyone could type one, so its result is the locked refusal that
        // *prompted* this call --- collecting it here would answer the reader's
        // password with the failure that asked for it, forever.
        .filter(|_| password.is_none())
        .and_then(|eager| eager.pending.lock().take());
    // Both branches end at the same place on purpose. The edit model has to be
    // started for the document that was actually opened, and the eager path
    // returns a `DocumentInfo` produced before this call existed --- registering
    // it in only one of the two would leave a reader who opened a file the fast
    // way with no model and no error, which reads as "rotate does nothing".
    let info = if let Some(rx) = eager {
        startup::mark("eager open collected");
        await_reply("open_document", rx).await?
    } else {
        let (reply, rx) = reply_channel();
        service.open_handed(wanted, handed, lazy_geometry(), password, reply);
        await_reply("open_document", rx).await?
    };
    let pages = u32::try_from(info.page_count).map_err(|_| {
        format!(
            "a document of {} pages is past what tpdf can edit",
            info.page_count
        )
    })?;
    // The handle, not the fingerprint: `edits` starts the hash on a thread and
    // the open does not wait for it. That is a measurement rather than caution ---
    // 452 ms cold for the 337 MB scan fixture, against a 300 ms cold-start
    // priority --- and the wait is moved to `Edits::plan`, which only a save or a
    // print reaches and which is about to read the whole file regardless.
    //
    // A hash that cannot be taken is recorded as "none" rather than as an error.
    // The document opens and can be read; what it cannot do is be saved over,
    // because a save with no fingerprint is refused. Fail closed, and lose the
    // smaller thing: Save a copy still works, and the original is not at risk.
    edits.open(info.id, pages, fingerprinting);

    // After the open succeeded, so a file that failed to parse is not filed as a
    // document the reader had. Every route in reaches here -- a drop on the
    // window, a double-click in Explorer, a path in argv, the single-instance
    // forward and the panel -- which is why it is here rather than in the dialog
    // handler, where four of the five would have missed it.
    recentdocs::note_opened(&app, Path::new(&path));
    Ok(info)
}

/// Releases every document the backend still holds, for a webview that has just
/// started.
///
/// ## The leak this closes
///
/// `close_document` has exactly one caller in the application: `App.svelte`,
/// when a *successful* subsequent open replaces the current document. So the
/// backend's document table is owned entirely by webview state --- and a webview
/// reload resets that state to nothing while the backend keeps everything.
/// Every document opened before the reload is then unreachable, with its worker
/// pool alive, for the life of the process. `App.svelte`'s own comment names the
/// stake: *"without it a session that opens a dozen files holds a dozen sandboxed
/// children"*, and that reasoning holds only while the webview remembers.
///
/// Nothing else reclaims them. There is no timer, no backend-side owner and no
/// reference count; a document lives until somebody names its id.
///
/// ## Why "the webview started" means "nothing is referenced"
///
/// A freshly loaded page holds no document id, by construction --- ids come back
/// from `open_document`, and it has not called it yet. So every id the backend
/// holds at that moment is one nobody can name.
///
/// **That depends on there being one window, and it is worth stating rather than
/// assuming.** tpdf is single-window: a second launch is forwarded by
/// `tauri-plugin-single-instance` to the running process, which opens the file in
/// the window it already has. If tpdf ever grows a second window, this becomes
/// wrong in the worst way --- one window's startup would close the other's
/// document out from under a reader --- and the fix is a per-window table rather
/// than a guard here.
///
/// ## Why it is not silent
///
/// It answers with a count, and a non-zero one is logged. Zero is the ordinary
/// case and means the reader started the application; anything else means a
/// webview reloaded, which is a thing that happened to somebody and which nothing
/// else in the running system reports.
///
/// ## What it adds to the webview's reach: nothing
///
/// Worth stating rather than leaving to be worked out, because a command that
/// closes *every* document sounds like new authority. It is not. Anything able
/// to call this can already call `close_document` in a loop --- the ids are
/// small integers from zero --- so what this adds is one round trip, not a
/// capability. It writes no file, reads no path and touches no network, so it is
/// outside the five commands `docs/THREAT-MODEL.md` §T6.1 enumerates. The worst
/// a caller does with it is close documents, which is the same denial of service
/// already reachable.
///
/// # Errors
///
/// The render service not answering, which is the same failure any command has.
#[tauri::command]
pub async fn release_documents(
    service: tauri::State<'_, RenderService>,
    edits: tauri::State<'_, edits::Edits>,
) -> Result<usize, String> {
    // Before the service call, for `close_document`'s reason and with the same
    // consequence: document numbers are reused, so a model left behind under an
    // id the service is about to hand to another file is one document's journal
    // applied to another's pages.
    let models = edits.release_all();
    let (reply, rx) = reply_channel();
    service.release_all(reply);
    let held = await_reply("release_documents", rx).await?;
    if held > 0 || models > 0 {
        // Not `[render]`: this is not a render failing, and the tag is what a
        // reader greps for. A line here means a webview reloaded and the backend
        // was holding documents nobody could reach.
        crate::diag::note(&format!(
            "[open] a new webview released {held} document(s) and {models} model(s) \
             the previous one left behind"
        ));
    }
    Ok(held)
}

/// Releases a document the reader has finished with.
///
/// Called when the window moves to another file, and it matters more than it
/// looks: under the worker backend an unreleased document is a sandboxed
/// process, not a heap allocation, so a session that opens a dozen files would
/// otherwise be holding a dozen of them.
///
/// It waits for the render service's reply rather than returning as soon as the
/// job is posted, so the promise resolving means the process is really gone and
/// a refusal has somewhere to be reported. Whether the *caller* waits on that
/// promise is its own decision, and `App.svelte` does not: nothing outstanding
/// for the outgoing document can lose its worker to this call, and the guarantee
/// belongs to `Workers::close`, which drains the pool before dropping it. Read
/// the argument there rather than a copy of it here --- holding the reader on
/// this promise would put a process teardown on the path to the first page of
/// the file they asked for, and that is the only decision this end makes.
#[tauri::command]
pub async fn close_document(
    service: tauri::State<'_, RenderService>,
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
) -> Result<(), String> {
    // Before the service call rather than after, and not for tidiness: document
    // numbers are reused, so a model left behind under a handle the service is
    // about to hand to another file is one document's journal applied to
    // another's pages.
    edits.close(doc);
    let (reply, rx) = reply_channel();
    service.close(doc, reply);
    await_reply("close_document", rx).await
}

/// Turns one page of the working document, without touching the file.
///
/// The page is named by the identity a state reply gave it, never by its
/// position --- see `edits.rs` on why. Returns the whole edit state rather than
/// an acknowledgement, so the frontend's copy is replaced by the answer rather
/// than advanced by its own arithmetic.
///
/// Synchronous work in an `async fn`, which the note on
/// [`super::print::print_document`] warns
/// about, and here it is right: this is a `HashMap` lookup, a journal push and a
/// walk of the page order. Nothing parses and nothing touches the disk.
#[tauri::command]
pub async fn page_rotate(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    page: u64,
    turns: i8,
) -> Result<edits::EditState, String> {
    edits.rotate(doc, page, turns)
}

/// Sets or clears one page's visible box, without touching the file.
///
/// Named by identity like [`page_rotate`]. `to` is `[llx, lly, urx, ury]` in the
/// page's own space, y upwards, or absent to put the file's own box back.
///
/// **The reader sees this through PDFium and saves it through `lopdf`, and the
/// two paths never meet.** Every render and every text extraction hands the box
/// to `RawDocument::page_cropped`, which sets it on the loaded page; a save
/// writes `/CropBox` out of the plan in `save.rs`. That is a real duplication and
/// it is the reason a check comparing what is on screen with what comes back out
/// of the saved file can fail at all.
#[tauri::command]
pub async fn page_crop(
    edits: tauri::State<'_, edits::Edits>,
    doc: u32,
    page: u64,
    to: Option<[f64; 4]>,
) -> Result<edits::EditState, String> {
    edits.crop(doc, page, to)
}

/// The box one page's ink occupies, in the page's own space, or `None` if blank.
///
/// `page` is a position in the **baseline file**, not a page id and not a slot:
/// this asks PDFium about the document on disk, which knows nothing about the
/// model's identities. The caller has the source index in the state reply.
///
/// Measured by rendering the page small and finding the bounding box of
/// everything that is not paper --- see `crate::content` for why the object
/// graph cannot answer this for a scan.
#[tauri::command]
pub async fn page_content_box(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
) -> Result<Option<[f64; 4]>, String> {
    let (reply, rx) = reply_channel();
    service.content(doc, page, reply);
    await_reply("page_content_box", rx).await
}

/// One page's displayed size under a crop box, or under the file's own.
///
/// The frontend lays out from this and cannot compute it: a crop is in the
/// page's own space, the layout is in display space, and the turn between them
/// is the page's `/Rotate`, which the frontend is never told.
#[tauri::command]
pub async fn page_geometry(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
    crop: Option<[f32; 4]>,
) -> Result<render::CropGeometry, String> {
    let (reply, rx) = reply_channel();
    service.geometry(doc, page, crop, reply);
    await_reply("page_geometry", rx).await
}

/// The crop box a rectangle the reader dragged out would produce.
///
/// The inverse of [`page_geometry`], and it exists for the same reason: the
/// frontend has the rectangle in the file's **display** space --- which is where
/// every rectangle in the frontend lives --- and a crop box is in the page's own
/// unrotated space. Turning between them needs the page's `/Rotate`, which the
/// frontend is deliberately never told, so a second copy of the rotation table
/// there is the thing this command exists to avoid.
///
/// The answer goes straight into [`page_crop`], which is why it is in exactly
/// the coordinates [`page_content_box`] answers in: a crop the reader dragged and
/// a crop measured from the ink have to be the same kind of thing, or *Reset
/// page crop* would mean two different amounts of undoing.
#[tauri::command]
pub async fn page_crop_box(
    service: tauri::State<'_, RenderService>,
    doc: u32,
    page: u32,
    rect: [f32; 4],
) -> Result<[f32; 4], String> {
    let (reply, rx) = reply_channel();
    service.crop_box(doc, page, rect, reply);
    await_reply("page_crop_box", rx).await
}

/// Starts the document open now, before anything can ask for it.
///
/// Returns `None` unless both a path and the opt-in are set, so the variant is
/// off by default and the baseline stays the baseline.
pub(crate) fn start_eager_open(service: &RenderService) -> Option<EagerOpen> {
    std::env::var_os("TPDF_EAGER_OPEN")?;
    let path = PathBuf::from(std::env::var("TPDF_STARTUP").ok()?);

    let (reply, rx) = reply_channel();
    // No password: nothing has had the chance to ask for one this early, and a
    // locked document simply comes back locked for `open_document` to relay.
    service.open(path.clone(), lazy_geometry(), None, reply);
    startup::mark("eager open requested");
    Some(EagerOpen {
        path,
        pending: Mutex::new(Some(rx)),
    })
}
