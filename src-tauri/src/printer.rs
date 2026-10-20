//! Sending a print job to a printer named on the command line, with no dialog
//! (`tpdf print --printer NAME`).
//!
//! The job is built and read back before it reaches here, as the window builds
//! and reads back the job its dialog prints (`commands/print.rs`). What differs
//! is only the last step, where the window asks the reader for a printer:
//!
//! - **macOS and Linux** hand the PDF to CUPS, which both use for printing,
//!   through `cupsPrintFile`. CUPS is loaded when a job is sent rather than
//!   linked, so neither the build nor a machine without it needs the library;
//!   macOS has it at a fixed path, and on Linux it is `libcups.so.2`, which a
//!   desktop with printing has. The PDF goes to the printer as it is, as the
//!   macOS dialog sends it (`print.rs`): any conversion is CUPS's own.
//! - **Windows** opens a device context for the printer by name and draws each
//!   page on it, through the same `print_win::spool` the dialog's Print button
//!   reaches; only the dialog is skipped. That is a picture of each page, drawn
//!   by PDFium in a worker, as the window prints on Windows (`print_win.rs` says
//!   why).
//!
//! A printer name nothing knows is refused with the names there are, so a
//! script's author can correct it from the message alone.

/// What was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    /// The print system's number for the job, where it gives one: CUPS does,
    /// a Windows device context does not.
    pub job: Option<u32>,
}

/// Sends `bytes`, a PDF already read back as `pages` pages, to the printer
/// called `printer`.
///
/// # Errors
///
/// No printer by that name, the print system not reachable, or it refusing
/// the job.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn send(
    printer: &str,
    bytes: &[u8],
    title: &str,
    _pages: u32,
    _library_dir: &std::path::Path,
) -> Result<Sent, String> {
    let cups = cups::Cups::load()?;
    let names = cups.printers();
    if !names.iter().any(|name| name == printer) {
        return Err(unknown(printer, &names));
    }
    let (path, file) = stage(bytes)?;
    drop(file);
    let sent = cups.print_file(printer, &path, title);
    let _ = std::fs::remove_file(&path);
    sent.map(|job| Sent { job: Some(job) })
}

/// Sends `bytes`, a PDF already read back as `pages` pages, to the printer
/// called `printer`.
///
/// `output` names a file for the driver to write instead of the port, as a
/// "Print to file" does; only the checks use it, through
/// `TPDF_PRINT_OUTPUT`, with "Microsoft Print to PDF", which otherwise asks
/// where to save. The pages are drawn by PDFium in a worker started from
/// `library_dir`.
///
/// # Errors
///
/// No printer by that name, the worker refusing the job, or GDI or the driver
/// refusing a page.
#[cfg(windows)]
pub fn send(
    printer: &str,
    bytes: &[u8],
    title: &str,
    pages: u32,
    library_dir: &std::path::Path,
) -> Result<Sent, String> {
    use windows::core::HSTRING;
    use windows::Win32::Graphics::Gdi::{CreateDCW, DeleteDC};

    let sheets: Vec<u32> = (0..pages).collect();
    let driver = HSTRING::from("WINSPOOL");
    let device = HSTRING::from(printer);
    // SAFETY: two valid wide strings that outlive the call; the remaining two
    // arguments are optional and null selects the printer's defaults.
    let dc = unsafe { CreateDCW(&driver, &device, None, None) };
    if dc.is_invalid() {
        return Err(format!(
            "there is no printer called \"{printer}\" that Windows could open ({}); \
             `Get-Printer` lists the printers there are",
            std::io::Error::last_os_error()
        ));
    }
    let output = std::env::var("TPDF_PRINT_OUTPUT").ok();
    // A stuck queue blocks inside GDI and reports nothing; stderr says so
    // rather than leaving a script waiting on silence (`print::STALL`).
    let spooled = crate::print::watched(
        crate::print::STALL,
        |_| (),
        |beat| {
            crate::print_win::spool(
                dc,
                bytes,
                title,
                output.as_deref(),
                &sheets,
                Some(beat),
                library_dir,
            )
        },
        |sent| eprintln!("tpdf: {}", crate::print::stalled_message(sent)),
    );
    // SAFETY: the DC `CreateDCW` returned above, released once.
    let _ = unsafe { DeleteDC(dc) };
    spooled.map(|_| Sent { job: None })
}

/// The refusal for a printer nothing knows, naming the ones there are.
#[cfg_attr(windows, allow(dead_code))]
fn unknown(printer: &str, names: &[String]) -> String {
    if names.is_empty() {
        format!("there is no printer called \"{printer}\", and this computer has none")
    } else {
        let quoted: Vec<String> = names.iter().map(|name| format!("\"{name}\"")).collect();
        format!(
            "there is no printer called \"{printer}\"; this computer has {}",
            quoted.join(", ")
        )
    }
}

/// The job, written to a file only this user can read, which CUPS reads from.
#[cfg(target_os = "linux")]
fn stage(bytes: &[u8]) -> Result<(std::path::PathBuf, std::fs::File), String> {
    crate::print_linux::stage(bytes)
}

/// The job, written to a file only this user can read, which CUPS reads from:
/// in the user's own temporary folder, under a name made new each time.
#[cfg(target_os = "macos")]
fn stage(bytes: &[u8]) -> Result<(std::path::PathBuf, std::fs::File), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let folder = crate::secondlaunch::user_temp_dir().unwrap_or_else(std::env::temp_dir);
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

/// CUPS, loaded by name when a job is sent.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod cups {
    use std::ffi::{c_char, c_int, c_void, CStr, CString};
    use std::path::Path;

    /// `cups_option_t`.
    #[repr(C)]
    struct Opt {
        name: *mut c_char,
        value: *mut c_char,
    }

    /// `cups_dest_t`.
    #[repr(C)]
    struct Dest {
        name: *mut c_char,
        instance: *mut c_char,
        is_default: c_int,
        num_options: c_int,
        options: *mut Opt,
    }

    type GetDests = unsafe extern "C" fn(*mut *mut Dest) -> c_int;
    type FreeDests = unsafe extern "C" fn(c_int, *mut Dest);
    type PrintFile =
        unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, c_int, *mut Opt) -> c_int;
    type LastErrorString = unsafe extern "C" fn() -> *const c_char;

    /// Where the library is: a fixed path on macOS, the soname on Linux.
    #[cfg(target_os = "macos")]
    const LIBRARY: &CStr = c"/usr/lib/libcups.2.dylib";
    #[cfg(target_os = "linux")]
    const LIBRARY: &CStr = c"libcups.so.2";

    /// The library and the four functions used. The handle is never closed:
    /// the process is a command that ends after one job.
    pub struct Cups {
        get_dests: GetDests,
        free_dests: FreeDests,
        print_file: PrintFile,
        last_error: LastErrorString,
    }

    impl Cups {
        /// Loads CUPS.
        ///
        /// # Errors
        ///
        /// The library or one of the four functions is not there.
        pub fn load() -> Result<Self, String> {
            // SAFETY: a NUL-terminated name; `dlopen` returns null on failure,
            // which is checked.
            let handle =
                unsafe { libc::dlopen(LIBRARY.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if handle.is_null() {
                return Err(format!(
                    "printing needs CUPS, and {} could not be loaded",
                    LIBRARY.to_string_lossy()
                ));
            }
            let symbol = |name: &CStr| -> Result<*mut c_void, String> {
                // SAFETY: a live handle and a NUL-terminated name.
                let found = unsafe { libc::dlsym(handle, name.as_ptr()) };
                if found.is_null() {
                    Err(format!("CUPS has no {}", name.to_string_lossy()))
                } else {
                    Ok(found)
                }
            };
            // SAFETY: each symbol is the CUPS function of that name, whose C
            // signature the type above spells out (cups/cups.h).
            unsafe {
                Ok(Cups {
                    get_dests: std::mem::transmute::<*mut c_void, GetDests>(symbol(
                        c"cupsGetDests",
                    )?),
                    free_dests: std::mem::transmute::<*mut c_void, FreeDests>(symbol(
                        c"cupsFreeDests",
                    )?),
                    print_file: std::mem::transmute::<*mut c_void, PrintFile>(symbol(
                        c"cupsPrintFile",
                    )?),
                    last_error: std::mem::transmute::<*mut c_void, LastErrorString>(symbol(
                        c"cupsLastErrorString",
                    )?),
                })
            }
        }

        /// The names of the printers CUPS knows, without instances.
        pub fn printers(&self) -> Vec<String> {
            let mut dests: *mut Dest = std::ptr::null_mut();
            // SAFETY: CUPS fills `dests` with `count` entries, freed below.
            let count = unsafe { (self.get_dests)(&mut dests) };
            let mut names = Vec::new();
            if dests.is_null() {
                return names;
            }
            for i in 0..usize::try_from(count).unwrap_or(0) {
                // SAFETY: `i` is below the count CUPS returned.
                let dest = unsafe { &*dests.add(i) };
                if dest.name.is_null() || !dest.instance.is_null() {
                    continue;
                }
                // SAFETY: a NUL-terminated string CUPS owns until the free.
                let name = unsafe { CStr::from_ptr(dest.name) }
                    .to_string_lossy()
                    .into_owned();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            // SAFETY: the list and count `cupsGetDests` returned, freed once.
            unsafe { (self.free_dests)(count, dests) };
            names
        }

        /// Sends the file at `path` to `printer`, answering the job's number.
        ///
        /// # Errors
        ///
        /// CUPS refusing the job, with its own reason.
        pub fn print_file(&self, printer: &str, path: &Path, title: &str) -> Result<u32, String> {
            let text = |value: &str| CString::new(value.replace('\0', "")).unwrap_or_default();
            let (printer_c, path_c, title_c) =
                (text(printer), text(&path.to_string_lossy()), text(title));
            // SAFETY: three NUL-terminated strings that outlive the call, and
            // no options.
            let job = unsafe {
                (self.print_file)(
                    printer_c.as_ptr(),
                    path_c.as_ptr(),
                    title_c.as_ptr(),
                    0,
                    std::ptr::null_mut(),
                )
            };
            if job > 0 {
                return Ok(job.unsigned_abs());
            }
            // SAFETY: CUPS returns a string it owns, or null.
            let reason = unsafe {
                let raw = (self.last_error)();
                if raw.is_null() {
                    "no reason given".to_string()
                } else {
                    CStr::from_ptr(raw).to_string_lossy().into_owned()
                }
            };
            Err(format!(
                "CUPS did not take the job for \"{printer}\": {reason}"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::unknown;

    #[test]
    fn a_printer_nothing_knows_is_refused_with_the_names_there_are() {
        assert_eq!(
            unknown("Ofice", &["Office".into(), "PDF".into()]),
            "there is no printer called \"Ofice\"; this computer has \"Office\", \"PDF\""
        );
        assert_eq!(
            unknown("Office", &[]),
            "there is no printer called \"Office\", and this computer has none"
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn cups_loads_and_answers_a_list_and_a_refusal() {
        // Every machine the tests run on has CUPS: macOS always, Ubuntu with
        // the desktop. The list may be empty; the call must answer.
        let cups = super::cups::Cups::load().expect("CUPS");
        let _ = cups.printers();
        let refused = super::send(
            "tpdf-no-such-printer",
            b"%PDF-1.4",
            "t",
            1,
            std::path::Path::new("."),
        )
        .unwrap_err();
        assert!(
            refused.starts_with("there is no printer called \"tpdf-no-such-printer\""),
            "{refused}"
        );
    }
}
