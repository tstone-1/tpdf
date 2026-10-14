//! Printing on Linux: GTK's print dialog, and the PDF itself handed to CUPS.
//!
//! `print.rs` says why a print job is a PDF and never pixels, and Linux keeps
//! that: the dialog is `GtkPrintUnixDialog`, and the job is a `GtkPrintJob`
//! whose source is the job's file, which GTK sends to CUPS as it is. CUPS does
//! any conversion a printer needs. `GtkPrintOperation`, the part of GTK that
//! the `gtk` crate binds, was the other choice and was not taken: it asks the
//! application to draw each page with cairo, which means pixels from a worker,
//! as on Windows.
//!
//! **The app process parses no document here**, unlike macOS and Windows,
//! where the system's PDF parser reads the job in this process. The read-back
//! is PDFium in a worker (`save::InWorker::printed_pages`), and GTK only copies
//! the file's bytes to the print system.
//!
//! The unix print dialog is part of `libgtk-3.so.0` but has a header of its
//! own (`gtk+-unix-print-3.0`), which `gtk-sys` does not bind, so the few
//! functions used are declared here.
//!
//! The dialog's page range applies to the job, as the range in the macOS panel
//! does: the job already holds only the pages the reader asked tpdf for.

use std::ffi::{c_char, c_uint, c_void, CStr, CString};
use std::path::{Path, PathBuf};

use gtk::ffi::{GtkPageSetup, GtkPrintSettings, GtkWidget, GtkWindow};
use gtk::glib::ffi::{gboolean, gpointer, GError};

#[link(name = "gtk-3")]
extern "C" {
    fn gtk_print_unix_dialog_new(title: *const c_char, parent: *mut GtkWindow) -> *mut GtkWidget;
    fn gtk_print_unix_dialog_set_manual_capabilities(dialog: *mut GtkWidget, capabilities: c_uint);
    fn gtk_print_unix_dialog_get_selected_printer(dialog: *mut GtkWidget) -> *mut c_void;
    fn gtk_print_unix_dialog_get_settings(dialog: *mut GtkWidget) -> *mut GtkPrintSettings;
    fn gtk_print_unix_dialog_get_page_setup(dialog: *mut GtkWidget) -> *mut GtkPageSetup;
    fn gtk_printer_accepts_pdf(printer: *mut c_void) -> gboolean;
    fn gtk_printer_get_name(printer: *mut c_void) -> *const c_char;
    fn gtk_print_job_new(
        title: *const c_char,
        printer: *mut c_void,
        settings: *mut GtkPrintSettings,
        page_setup: *mut GtkPageSetup,
    ) -> *mut c_void;
    fn gtk_print_job_set_source_file(
        job: *mut c_void,
        filename: *const c_char,
        error: *mut *mut GError,
    ) -> gboolean;
    fn gtk_enumerate_printers(
        func: unsafe extern "C" fn(*mut c_void, gpointer) -> gboolean,
        data: gpointer,
        destroy: Option<unsafe extern "C" fn(gpointer)>,
        wait: gboolean,
    );
    fn gtk_print_job_send(
        job: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void, gpointer, *const GError),
        user_data: gpointer,
        dnotify: Option<unsafe extern "C" fn(gpointer)>,
    );
}

/// Writes a print job to a file only this user can read, and opens it again
/// for reading.
///
/// In `$XDG_RUNTIME_DIR`, which is the user's own and in memory, or in the
/// temporary folder when there is none. The name is new each time and created
/// with `create_new`, so nothing else's file is written through.
///
/// # Errors
///
/// The file could not be written.
pub fn stage(bytes: &[u8]) -> Result<(PathBuf, std::fs::File), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let folder = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from);
    let path = folder.join(format!(
        "tpdf-print-{}-{}.pdf",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .and_then(|mut file| file.write_all(bytes));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&path);
        return Err(format!("the print job could not be written: {e}"));
    }
    let file = std::fs::File::open(&path).map_err(|e| {
        let _ = std::fs::remove_file(&path);
        format!("the print job could not be read back: {e}")
    })?;
    Ok((path, file))
}

/// Shows the print dialog over `parent` and, on Print, sends the file at
/// `path` to the chosen printer. The file is removed once the job has been
/// sent, or at once if the reader cancels.
///
/// Must run on GTK's thread: the dialog runs a nested main loop. Returns when
/// the dialog closes; the job is sent afterwards, on the main loop, and its
/// outcome is noted in the diagnostics log, as on the other two platforms.
///
/// # Errors
///
/// The chosen printer cannot take a PDF, or GTK would not make the job. The
/// file is removed in both cases.
pub fn present(path: PathBuf, title: &str, parent: Option<&gtk::Window>) -> Result<(), String> {
    use gtk::glib::translate::ToGlibPtr;
    use gtk::prelude::*;

    let answer = (|| {
        let heading = CString::new("Print").expect("no NUL");
        let parent_ptr: *mut GtkWindow =
            parent.map_or(std::ptr::null_mut(), |w| w.to_glib_none().0);
        // SAFETY: both arguments are valid for the call; the dialog is a new
        // toplevel, owned by GTK until destroyed below.
        let raw = unsafe { gtk_print_unix_dialog_new(heading.as_ptr(), parent_ptr) };
        if raw.is_null() {
            return Err("GTK would not make a print dialog".to_string());
        }
        // SAFETY: `raw` is a GtkPrintUnixDialog, which is a GtkDialog.
        let dialog: gtk::Dialog =
            unsafe { gtk::glib::translate::from_glib_none(raw.cast::<gtk::ffi::GtkDialog>()) };
        // No preview and nothing laid out by tpdf: the job is the finished PDF.
        // SAFETY: `raw` is the dialog.
        unsafe { gtk_print_unix_dialog_set_manual_capabilities(raw, 0) };
        let chosen = if dialog.run() == gtk::ResponseType::Ok {
            send(raw, &path, title)
        } else {
            Ok(false)
        };
        // SAFETY: the dialog is ours and not used after this.
        unsafe { dialog.destroy() };
        chosen
    })();
    match answer {
        Ok(true) => Ok(()),
        Ok(false) => {
            let _ = std::fs::remove_file(&path);
            Ok(())
        }
        Err(why) => {
            let _ = std::fs::remove_file(&path);
            Err(why)
        }
    }
}

/// What the job's completion needs: the file to remove, the job to let go,
/// and, for [`print_to`], where to say how it went.
struct Sending {
    path: PathBuf,
    job: *mut c_void,
    outcome: Option<std::sync::mpsc::Sender<Result<(), String>>>,
}

/// Makes the job for the dialog's choices and sends it. `Ok(true)` once it is
/// on its way, after which the file belongs to [`released`].
fn send(dialog: *mut GtkWidget, path: &Path, title: &str) -> Result<bool, String> {
    // SAFETY: `dialog` is a live GtkPrintUnixDialog; the printer and page
    // setup are borrowed from it (transfer none), the settings are ours
    // (transfer full) and released below.
    let printer = unsafe { gtk_print_unix_dialog_get_selected_printer(dialog) };
    if printer.is_null() {
        return Err("no printer was chosen".to_string());
    }
    let settings = unsafe { gtk_print_unix_dialog_get_settings(dialog) };
    let setup = unsafe { gtk_print_unix_dialog_get_page_setup(dialog) };
    let submitted = submit(printer, settings, setup, path, title, None);
    if !settings.is_null() {
        // SAFETY: the settings were ours, and a job holds its own reference.
        unsafe { gtk::glib::gobject_ffi::g_object_unref(settings.cast()) };
    }
    submitted.map(|()| true)
}

/// Sends the file at `path` to `printer` as a PDF, with `settings` and
/// `setup`. The one place a job is made, for the dialog and for [`print_to`].
fn submit(
    printer: *mut c_void,
    settings: *mut GtkPrintSettings,
    setup: *mut GtkPageSetup,
    path: &Path,
    title: &str,
    outcome: Option<std::sync::mpsc::Sender<Result<(), String>>>,
) -> Result<(), String> {
    // SAFETY: `printer` is live for the call.
    if unsafe { gtk_printer_accepts_pdf(printer) } == 0 {
        // SAFETY: as above; GTK owns the name.
        let name = unsafe { CStr::from_ptr(gtk_printer_get_name(printer)) };
        return Err(format!(
            "the printer {:?} cannot take a PDF, and tpdf prints only the PDF itself",
            name.to_string_lossy()
        ));
    }
    let title = CString::new(title.replace('\0', "")).unwrap_or_default();
    // SAFETY: every argument is live; the job takes its own references.
    let job = unsafe { gtk_print_job_new(title.as_ptr(), printer, settings, setup) };
    if job.is_null() {
        return Err("GTK would not make a print job".to_string());
    }
    let file = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| "the print job's path has a NUL in it".to_string())?;
    let mut error: *mut GError = std::ptr::null_mut();
    // SAFETY: `job` is live and `error` is a valid out-pointer.
    if unsafe { gtk_print_job_set_source_file(job, file.as_ptr(), &mut error) } == 0 {
        let why = gerror_message(error);
        if !error.is_null() {
            // SAFETY: GTK set it for us to free.
            unsafe { gtk::glib::ffi::g_error_free(error) };
        }
        // SAFETY: the job is ours and was never sent.
        unsafe { gtk::glib::gobject_ffi::g_object_unref(job.cast()) };
        return Err(format!("GTK would not take the print job: {why}"));
    }
    let state = Box::into_raw(Box::new(Sending {
        path: path.to_path_buf(),
        job,
        outcome,
    }));
    // SAFETY: `job` is live; `state` stays valid until `released` frees it,
    // which GTK calls once, after `sent`.
    unsafe { gtk_print_job_send(job, sent, state.cast(), Some(released)) };
    Ok(())
}

/// GTK's answer when the job has gone to the print system, or failed to.
unsafe extern "C" fn sent(_job: *mut c_void, state: gpointer, error: *const GError) {
    let answer = if error.is_null() {
        Ok(())
    } else {
        Err(gerror_message(error))
    };
    if let Err(why) = &answer {
        crate::diag::note(&format!("[print] {why}"));
    }
    // SAFETY: `state` is the box `submit` leaked; `released` has not run yet.
    if let Some(outcome) = unsafe { &(*state.cast::<Sending>()).outcome } {
        let _ = outcome.send(answer);
    }
}

/// Called once after [`sent`]: removes the file and lets go of the job.
unsafe extern "C" fn released(state: gpointer) {
    // SAFETY: `state` is the box `submit` leaked, and GTK calls this once.
    let state = unsafe { Box::from_raw(state.cast::<Sending>()) };
    let _ = std::fs::remove_file(&state.path);
    // SAFETY: the reference `gtk_print_job_new` gave `submit`.
    unsafe { gtk::glib::gobject_ffi::g_object_unref(state.job.cast()) };
}

/// Sends the file at `path` to the printer named `printer`, with its default
/// settings, without the dialog, and waits up to `within` for GTK to say the
/// job reached the print system. The file is removed either way.
///
/// **For `print-probe`, not the application**, which always asks the reader
/// through [`present`]. It drives everything after the dialog, the way the
/// Windows half of the probe drives a printer DC: the job is made by the same
/// [`submit`] the dialog's Print button reaches. GTK must be initialised, on
/// this thread.
///
/// # Errors
///
/// No printer by that name, the printer refusing a PDF, GTK refusing the job,
/// or no answer within `within`.
pub fn print_to(
    printer: &str,
    path: &Path,
    title: &str,
    within: std::time::Duration,
) -> Result<(), String> {
    use gtk::glib::translate::ToGlibPtr;

    let answer = (|| {
        let found = find_printer(printer)?;
        let settings = gtk::PrintSettings::new();
        let setup = gtk::PageSetup::new();
        let (tx, rx) = std::sync::mpsc::channel();
        let submitted = submit(
            found,
            settings.to_glib_none().0,
            setup.to_glib_none().0,
            path,
            title,
            Some(tx),
        );
        // SAFETY: the reference `find_printer` took; the job holds its own.
        unsafe { gtk::glib::gobject_ffi::g_object_unref(found.cast()) };
        submitted?;
        let context = gtk::glib::MainContext::default();
        let until = std::time::Instant::now() + within;
        loop {
            while context.iteration(false) {}
            match rx.try_recv() {
                Ok(answer) => {
                    // Until `released` has removed the file, so the caller can
                    // tell the job let go of it.
                    while path.exists() && std::time::Instant::now() < until {
                        context.iteration(false);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    return answer;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err("GTK dropped the print job without an answer".to_string());
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            if std::time::Instant::now() >= until {
                return Err(format!(
                    "GTK did not say the job was sent within {} s",
                    within.as_secs()
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    })();
    if answer.is_err() {
        let _ = std::fs::remove_file(path);
    }
    answer
}

/// The printer named `name`, with a reference the caller releases.
fn find_printer(name: &str) -> Result<*mut c_void, String> {
    struct Search {
        name: String,
        found: *mut c_void,
        seen: Vec<String>,
    }
    unsafe extern "C" fn each(printer: *mut c_void, data: gpointer) -> gboolean {
        // SAFETY: `data` is the `Search` below, alive for the whole enumeration.
        let search = unsafe { &mut *data.cast::<Search>() };
        // SAFETY: GTK hands a live printer with its name.
        let name = unsafe { CStr::from_ptr(gtk_printer_get_name(printer)) }
            .to_string_lossy()
            .into_owned();
        if name == search.name && search.found.is_null() {
            // SAFETY: kept past the enumeration, so a reference of our own.
            unsafe { gtk::glib::gobject_ffi::g_object_ref(printer.cast()) };
            search.found = printer;
            return 1;
        }
        search.seen.push(name);
        0
    }
    let mut search = Search {
        name: name.to_string(),
        found: std::ptr::null_mut(),
        seen: Vec::new(),
    };
    // SAFETY: `wait` is true, so GTK calls `each` only before this returns,
    // while `search` is alive.
    unsafe { gtk_enumerate_printers(each, std::ptr::from_mut(&mut search).cast(), None, 1) };
    if search.found.is_null() {
        return Err(format!(
            "no printer named {name:?}; GTK listed {:?}",
            search.seen
        ));
    }
    Ok(search.found)
}

/// A `GError`'s message. Freeing it is the caller's business: GTK owns the
/// one [`sent`] is given.
fn gerror_message(error: *const GError) -> String {
    if error.is_null() {
        return "no reason was given".to_string();
    }
    // SAFETY: a set GError has a valid message.
    unsafe { CStr::from_ptr((*error).message) }
        .to_string_lossy()
        .into_owned()
}
