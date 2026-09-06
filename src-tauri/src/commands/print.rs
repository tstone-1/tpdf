//! Printing: build the job, read it back, and hand it to the platform.
//!
//! The readback is the point of the shape here. `print_job` builds bytes and
//! `present_job` refuses to show a panel for bytes the platform's own PDF stack
//! cannot read --- PDFKit on macOS, `Windows.Data.Pdf` on Windows --- which is
//! the same standard `docs/PLAN.md` 6 sets for a redaction, and the reason the
//! two halves are separate functions rather than one command body.

use std::path::{Path, PathBuf};

use super::{outside_of, password_for};
use crate::render::RenderService;
use crate::{diag, edits, print, save};

#[cfg(target_os = "macos")]
use crate::print_macos;
#[cfg(windows)]
use crate::print_win;

/// Builds a print job and opens the platform print dialog for it.
///
/// `async` keeps the build off the thread the webview draws on: `print::build`
/// parses the whole document, and on a 337 MB scan that is not something to do
/// there. Only the panel is dispatched back.
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
        Some(doc) => password_for(&service, doc, "print_document").await,
        None => None,
    };
    // **Who parses the reader's document**, chosen the same way every other
    // writer is. Only the `Working` route reaches it --- the passthrough hands
    // the file over byte for byte and parses nothing, and `print::build` still
    // parses a range here (`docs/THREAT-MODEL.md` residual risk 18).
    let writing = outside_of(&app, service.backend());
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
    present_job(&app, bytes, title, expected).map_err(save::Refusal::from)
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
    // Carried across the thread boundary as an integer, not as an `HWND`. A raw
    // handle is a `*mut c_void` and therefore not `Send`, and the compiler is right
    // to say so in general --- but a window handle is a process-wide kernel-managed
    // value with no thread affinity for this use, and `PrintDlgW` only ever reads
    // it to parent a dialog. Reconstructed on the far side rather than smuggled
    // through a wrapper type, so the one unsound-looking step is one line and is
    // where the reasoning is written down.
    let owner = {
        use tauri::Manager;
        app.get_webview_window("main")
            .and_then(|w| w.hwnd().ok())
            .map(|h| h.0 as isize)
    };

    std::thread::spawn(move || {
        let owner = owner.map(|h| windows::Win32::Foundation::HWND(h as *mut std::ffi::c_void));
        if let Err(e) = print_win::present(&bytes, &title, owner) {
            diag::note(&format!("[print] {e}"));
        }
    });
    Ok(())
}

/// The remaining platforms, where nothing is written.
///
/// An error rather than a no-op: a print command that quietly does nothing is the
/// worse of the two failures. Both shipping targets have an implementation above,
/// so this arm exists for a Linux build that does not yet exist.
#[cfg(not(any(target_os = "macos", windows)))]
fn present_job(
    _app: &tauri::AppHandle,
    _bytes: Vec<u8>,
    _title: String,
    _expected: Option<usize>,
) -> Result<(), String> {
    Err("printing is implemented on macOS and Windows only".into())
}
