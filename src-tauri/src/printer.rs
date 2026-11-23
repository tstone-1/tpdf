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
    let queue = cups.destination(printer)?;
    let (path, file) = stage(bytes)?;
    drop(file);
    let sent = cups.print_file(&queue, &path, title);
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
    let output = std::env::var("TPDF_PRINT_OUTPUT")
        .ok()
        .filter(|name| !name.is_empty());
    if output.is_none() {
        if let Err(why) = crate::printers_win::check_unattended_port(&device) {
            // SAFETY: this call owns the DC, and has not begun a job.
            let _ = unsafe { DeleteDC(dc) };
            return Err(why);
        }
    }
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
    let folder = crate::secondlaunch::user_temp_dir().unwrap_or_else(std::env::temp_dir);
    stage_in(&folder, bytes)
}

/// Creates a private print file without claiming a name owned by another job.
#[cfg(unix)]
pub(crate) fn stage_in(
    folder: &std::path::Path,
    bytes: &[u8],
) -> Result<(std::path::PathBuf, std::fs::File), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    stage_named(bytes, || {
        folder.join(format!(
            "tpdf-print-{}-{}.pdf",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    })
}

#[cfg(unix)]
fn stage_named(
    bytes: &[u8],
    mut candidate: impl FnMut() -> std::path::PathBuf,
) -> Result<(std::path::PathBuf, std::fs::File), String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    for _ in 0..128 {
        let path = candidate();
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(said!("the print job could not be written: {e}")),
        };
        if let Err(e) = file.write_all(bytes) {
            let _ = std::fs::remove_file(&path);
            return Err(said!("the print job could not be written: {e}"));
        }
        let reader = std::fs::File::open(&path).map_err(|e| {
            let _ = std::fs::remove_file(&path);
            said!("the print job could not be read back: {e}")
        })?;
        return Ok((path, reader));
    }
    let e = std::io::ErrorKind::AlreadyExists;
    Err(said!("the print job could not be written: {e}"))
}

#[cfg(all(test, unix))]
#[test]
fn staging_keeps_a_colliding_file_and_reads_back_the_owned_file() {
    use std::io::Read as _;
    let dir = crate::testutil::TempDir::new("print-stage");
    let first = dir.join("old.pdf");
    let second = dir.join("new.pdf");
    std::fs::write(&first, b"earlier job").unwrap();
    let mut names = [first.clone(), second.clone()].into_iter();
    let (path, mut file) = stage_named(b"new job", || names.next().unwrap()).unwrap();
    let mut read = Vec::new();
    file.read_to_end(&mut read).unwrap();
    assert_eq!(read, b"new job");
    assert!(std::io::Write::write_all(&mut file, b"x").is_err());
    assert_eq!(path, second);
    assert_eq!(std::fs::read(first).unwrap(), b"earlier job");
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

    pub(super) struct Queue {
        name: CString,
        options: Vec<(CString, CString)>,
    }

    impl Queue {
        // SAFETY: dest comes from the live cupsGetDests array.
        unsafe fn from_dest(dest: &Dest, name: &CStr) -> Self {
            let mut options = Vec::new();
            if !dest.options.is_null() {
                for index in 0..usize::try_from(dest.num_options).unwrap_or(0) {
                    // SAFETY: CUPS supplies num_options live pairs.
                    let option = unsafe { &*dest.options.add(index) };
                    // SAFETY: both strings live until cupsFreeDests.
                    let (key, value) =
                        unsafe { (CStr::from_ptr(option.name), CStr::from_ptr(option.value)) };
                    // The staged PDF already contains exactly the job's pages.
                    if ![b"page-ranges".as_slice(), b"page-set"]
                        .iter()
                        .any(|filter| key.to_bytes().eq_ignore_ascii_case(filter))
                    {
                        options.push((key.to_owned(), value.to_owned()));
                    }
                }
            }
            Self {
                name: name.to_owned(),
                options,
            }
        }
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

        /// Resolve a queue and its local printing preferences before staging.
        pub(super) fn destination(&self, printer: &str) -> Result<Queue, String> {
            let mut dests: *mut Dest = std::ptr::null_mut();
            // SAFETY: CUPS fills `dests` with `count` entries, freed below.
            let count = unsafe { (self.get_dests)(&mut dests) };
            let mut names = Vec::new();
            let mut queue = None;
            if dests.is_null() {
                return Err(super::unknown(printer, &names));
            }
            for i in 0..usize::try_from(count).unwrap_or(0) {
                // SAFETY: `i` is below the count CUPS returned.
                let dest = unsafe { &*dests.add(i) };
                if dest.name.is_null() || !dest.instance.is_null() {
                    continue;
                }
                // SAFETY: a NUL-terminated string CUPS owns until the free.
                let name = unsafe { CStr::from_ptr(dest.name) };
                if name.to_bytes().eq_ignore_ascii_case(printer.as_bytes()) {
                    // SAFETY: dest and all its strings remain live until the free below.
                    queue = Some(unsafe { Queue::from_dest(dest, name) });
                    break;
                }
                let name = name.to_string_lossy().into_owned();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            // SAFETY: the list and count `cupsGetDests` returned, freed once.
            unsafe { (self.free_dests)(count, dests) };
            queue.ok_or_else(|| super::unknown(printer, &names))
        }

        /// Sends the file at `path` to `printer`, answering the job's number.
        ///
        /// # Errors
        ///
        /// CUPS refusing the job, with its own reason.
        pub(super) fn print_file(
            &self,
            queue: &Queue,
            path: &Path,
            title: &str,
        ) -> Result<u32, String> {
            let text = |value: &str| CString::new(value.replace('\0', "")).unwrap_or_default();
            let title_c = text(title);
            let path_c =
                CString::new(path.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
            // libcups borrows the option names and values for this call.
            let mut options: Vec<Opt> = queue
                .options
                .iter()
                .map(|(key, value)| Opt {
                    name: key.as_ptr().cast_mut(),
                    value: value.as_ptr().cast_mut(),
                })
                .collect();
            // SAFETY: all strings and the array outlive this call. The count
            // is bounded by the original CUPS destination's c_int count.
            let job = unsafe {
                (self.print_file)(
                    queue.name.as_ptr(),
                    path_c.as_ptr(),
                    title_c.as_ptr(),
                    options.len() as c_int,
                    options.as_mut_ptr(),
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
            let printer = queue.name.to_string_lossy();
            Err(format!(
                "CUPS did not take the job for \"{printer}\": {reason}"
            ))
        }
    }
    #[cfg(test)]
    #[test]
    fn cups_receives_exact_staging_path() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        unsafe extern "C" fn read_job(
            printer: *const c_char,
            path: *const c_char,
            _: *const c_char,
            count: c_int,
            options: *mut Opt,
        ) -> c_int {
            // SAFETY: these pointers are live for the mocked CUPS call.
            if count != 1
                || unsafe { CStr::from_ptr(printer) } != c"Office"
                || unsafe { CStr::from_ptr((*options).name) } != c"sides"
                || unsafe { CStr::from_ptr((*options).value) } != c"two-sided-long-edge"
            {
                return 0;
            }
            // SAFETY: print_file supplies a live NUL-terminated pathname.
            let bytes = unsafe { CStr::from_ptr(path) }.to_bytes();
            let received = Path::new(OsStr::from_bytes(bytes));
            if !received.ends_with(OsStr::from_bytes(b"print-\xff/job.pdf")) {
                return 0;
            }
            #[cfg(target_os = "linux")]
            {
                i32::from(std::fs::read(received).is_ok_and(|job| job == b"test job"))
            }
            // APFS refuses to create the non-UTF-8 directory. The callback
            // still checks the bytes that would have gone to the library.
            #[cfg(target_os = "macos")]
            1
        }

        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(OsStr::from_bytes(b"print-\xff"));
        let path = directory.join("job.pdf");
        #[cfg(target_os = "linux")]
        {
            std::fs::create_dir(&directory).unwrap();
            std::fs::write(&path, b"test job").unwrap();
        }
        let mut cups = Cups::load().expect("CUPS");
        cups.print_file = read_job;
        let mut prefs = [
            (c"sides", c"two-sided-long-edge"),
            (c"page-ranges", c"2"),
            (c"page-set", c"odd"),
        ]
        .map(|(k, v)| Opt {
            name: k.as_ptr().cast_mut(),
            value: v.as_ptr().cast_mut(),
        });
        let mut dest = Dest {
            name: c"Office".as_ptr().cast_mut(),
            instance: std::ptr::null_mut(),
            is_default: 0,
            num_options: 3,
            options: prefs.as_mut_ptr(),
        };
        // Exercise the name lookup and copy before the simulated CUPS free.
        thread_local! { static DEST: std::cell::Cell<*mut Dest> = const { std::cell::Cell::new(std::ptr::null_mut()) }; }
        unsafe extern "C" fn get(out: *mut *mut Dest) -> c_int {
            // SAFETY: the caller gives CUPS an out-pointer for its list.
            unsafe {
                *out = DEST.with(std::cell::Cell::get);
            }
            1
        }
        unsafe extern "C" fn free(_: c_int, _: *mut Dest) {}
        DEST.set(&raw mut dest);
        cups.get_dests = get;
        cups.free_dests = free;
        for spelling in ["Office", "office"] {
            let queue = cups.destination(spelling).unwrap();
            assert_eq!(cups.print_file(&queue, &path, "test"), Ok(1));
        }
        assert!(cups.destination("Unknown").is_err());
        DEST.set(std::ptr::null_mut());
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
        assert!(cups.destination("tpdf-no-such-printer").is_err());
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
