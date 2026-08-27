//! What this process has actually mapped.
//!
//! The loader's own table --- dyld's on macOS, Toolhelp's on Windows --- rather
//! than a mark of our own. A milestone says what our code believes it did, and
//! every claim in this repository about *where* something runs is a claim about
//! what a process is, which only the loader can answer. Same reason `print.rs`
//! reads its output back with a parser that did not write it.
//!
//! Read of **this** process. `scripts/win_modules.py` reads the app from outside
//! it and is the stronger oracle for that reason; this one is what a probe that
//! is its own subject can use, and the two do not replace each other.
//!
//! One home rather than a copy per probe: `backend-probe` asks whether the app
//! process mapped the PDF parser and `ocr-worker-probe` asks whether it mapped
//! the OCR engine, and two copies of an FFI declaration is the shape this
//! repository keeps finding drifted.

/// Every dynamic library this process has mapped, by path.
///
/// Toolhelp's module list, which is what the loader itself holds --- the Windows
/// counterpart of dyld's image table below, and used for the same reason: a
/// milestone says what our code believes it did, and the question here is what
/// the process actually is.
///
/// Read of *this* process, unlike `scripts/win_modules.py`, which reads the app
/// from outside it. Both exist and neither replaces the other: the script is the
/// stronger oracle and can only watch a real application, while this one is
/// available to a probe that is its own subject.
#[cfg(windows)]
pub fn mapped() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
        TH32CS_SNAPMODULE32,
    };

    // SAFETY: our own pid; the snapshot is closed on every path out.
    let snapshot = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, std::process::id())
    };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let mut found = Vec::new();
    // SAFETY: zeroed is the documented initial state; `dwSize` is set as the API
    // requires and the entry outlives every call it is passed to.
    let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = u32::try_from(std::mem::size_of::<MODULEENTRY32W>()).unwrap_or(0);
    // SAFETY: a live snapshot handle and an initialised entry.
    if unsafe { Module32FirstW(snapshot, &raw mut entry) } != 0 {
        loop {
            let len = entry
                .szExePath
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExePath.len());
            found.push(String::from_utf16_lossy(&entry.szExePath[..len]));
            // SAFETY: as above.
            if unsafe { Module32NextW(snapshot, &raw mut entry) } == 0 {
                break;
            }
        }
    }
    // SAFETY: the snapshot handle, closed once.
    unsafe { CloseHandle(snapshot) };
    found
}

/// Every dynamic library this process has mapped, by path.
///
/// The dynamic linker's own table, rather than a mark of our own: a milestone
/// says what our code believes it did, and the question here is what the process
/// actually is. Same reason `print.rs` reads its output back with a parser that
/// did not write it.
#[cfg(target_os = "macos")]
pub fn mapped() -> Vec<String> {
    // Declared here rather than taken from `libc`, which deprecates both in
    // favour of the `mach2` crate --- a dependency this repository would be
    // adding for two symbols, against a licensing rule that makes every new
    // crate a decision. The signatures are dyld's own and have not changed.
    extern "C" {
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_name(index: u32) -> *const std::os::raw::c_char;
    }

    // SAFETY: the count bounds the index, and every name dyld returns is a live
    // NUL-terminated string for as long as the image stays loaded --- nothing
    // here unloads one.
    unsafe {
        (0.._dyld_image_count())
            .filter_map(|i| {
                let name = _dyld_get_image_name(i);
                (!name.is_null()).then(|| {
                    std::ffi::CStr::from_ptr(name)
                        .to_string_lossy()
                        .into_owned()
                })
            })
            .collect()
    }
}
