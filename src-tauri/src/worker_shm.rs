//! The shared mappings a document arrives in and a tile is rendered into.
//!
//! Split out of `worker.rs` when that file had grown to 2,861 lines and four
//! concerns. Nothing changed in the move: `worker.rs` re-exports [`Shm`], so
//! `crate::worker::Shm` still names this type.
//!
//! One type, two implementations, each carrying its own reasoning below --- an
//! unlinked temp file on unix, a nameless section object on Windows. What they
//! share is that neither is reachable by name, which is the property a contained
//! worker needs: it is handed a descriptor or a handle and has no authority to
//! open a file. How either reaches the child is `worker.rs`'s spawn path and
//! `worker_handover.rs`.

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
#[cfg(unix)]
use std::sync::atomic::{AtomicUsize, Ordering};

/// A shared anonymous mapping, created in the parent and inherited by fd.
///
/// Deliberately not `shm_open`: a POSIX shm object lives in a global name space,
/// so a second process can find it by guessing the name, and the worker would
/// need that name space to remain reachable under a sandbox. A temp file
/// unlinked immediately after creation has neither problem --- the descriptor is
/// the only handle that exists, and a descriptor survives a policy that denies
/// opening files.
#[cfg(unix)]
pub struct Shm {
    file: std::fs::File,
    ptr: *mut libc::c_void,
    len: usize,
}

/// A shared mapping backed by a **nameless** Windows section object.
///
/// The macOS note above is about avoiding a global name space, and Windows has
/// the same hazard in a sharper form: a *named* section lives in the object
/// manager's namespace, so a second process can open it by guessing the name.
/// Passing `NULL` for `lpName` makes the section reachable only through a handle,
/// which is the exact property the unlinked temp file buys on the other side.
///
/// It differs from the POSIX version in what backs a document. `map_file` does
/// not map the file: it copies it into a pagefile section, because a section
/// over the file itself would stop every other program rewriting that file
/// while the document is open (`Shm::section` has the account). The child needs
/// only the section handle either way; there is no Windows analogue of passing
/// a file descriptor alongside.
///
/// This replaced a stub whose constructors all refused. The stub's doc recorded a
/// dead end that is still worth keeping: it was an **uninhabited** `enum` first,
/// to carry the impossibility in the type the way `AGENTS.md` recommends and the
/// way [`crate::worker::PreWorker`]/[`crate::worker::WarmWorker`] carry the
/// readiness handshake --- and that fails here, because [`crate::worker::Worker`]
/// holds a `Shm`, so an uninhabited mapping makes
/// the *worker* uninhabited too and the compiler then reports the pool's
/// `retire_idle` loop in `workers.rs` --- ordinary code on a platform that never
/// runs it --- as unreachable. Under `-D warnings` that is fatal.
#[cfg(windows)]
pub struct Shm {
    /// The section. Owned; closed on drop after the view is unmapped.
    mapping: windows_sys::Win32::Foundation::HANDLE,
    ptr: *mut std::ffi::c_void,
    len: usize,
}

// The pointer is an ordinary mapping; moving it between threads is fine.
// Aliasing is disciplined by the protocol: exactly one side writes a given
// buffer at a time, and the parent only reads after the worker's reply.
unsafe impl Send for Shm {}

// Sharing a reference is fine for the same reason, and for one more: every
// `&self` method here reads, and the only writer --- `as_mut_slice` --- takes
// `&mut self`, which no shared handle can produce. This is what lets a document
// mapping be held behind an `Arc` and handed to a replacement worker. It says
// nothing about the *other* process writing the same pages; that race is
// disciplined by the protocol above and predates this impl.
unsafe impl Sync for Shm {}

#[cfg(unix)]
impl Shm {
    /// Creates a mapping of `len` bytes backed by an unlinked temp file.
    ///
    /// # Errors
    ///
    /// Any step of create, unlink, resize or map failing.
    pub fn create(len: usize) -> Result<Self, String> {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "tpdf-shm-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));

        // **`create_new` and an explicit mode, and neither is the other's
        // spelling.** `create_new` refuses to open anything that is already
        // there, which is what stops a planted file or a symlink under this
        // name being what gets mapped --- the name is `pid` plus a counter, so
        // it is guessable. The mode is the other half: without it the file is
        // created at `0o666 & !umask`, and a umask is inherited from whatever
        // launched tpdf rather than chosen here. The window is short --- the
        // unlink is the next statement --- and on macOS `$TMPDIR` is a per-user
        // directory at `0700` besides, so this is not the guard the mapping
        // rests on. It is one syscall argument, and it is what the mapping's
        // contents deserve on a platform whose temp directory is shared.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("shm create failed: {e}"))?;
        std::fs::remove_file(&path).map_err(|e| format!("shm unlink failed: {e}"))?;
        file.set_len(len as u64)
            .map_err(|e| format!("shm resize failed: {e}"))?;
        Self::map(file, len, true)
    }

    /// Maps a whole file, for handing a document to a worker.
    ///
    /// # Errors
    ///
    /// The file not opening, being empty, or not mapping.
    pub fn map_file(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| said!("could not open {path:?}: {e}"))?;
        let len = file
            .metadata()
            .map_err(|e| said!("could not stat {path:?}: {e}"))?
            .len() as usize;
        if len == 0 {
            return Err(said!("{path:?} is empty"));
        }
        // Read-only, and not merely because `File::open` gives a read-only
        // descriptor that `mmap` then refuses `PROT_WRITE` on --- which is how
        // this surfaced. A worker holding a *writable* mapping of the reader's
        // own document could corrupt the file it was asked to display, which is
        // precisely the authority the boundary exists to withhold. The kernel
        // refused it before the threat model did.
        Self::map(file, len, false)
    }

    /// Maps a file that is already open, without naming it again.
    ///
    /// **The seam the save's verification needs, and the pathname is the point.**
    /// `save::append_through` guarantees everything it guarantees about the file
    /// its *handle* holds, never about whatever the name resolves to now, so a
    /// verifier that called [`Shm::map_file`] on the source path would check
    /// whichever file has that name at that moment --- reintroducing exactly the
    /// race that function exists to close.
    ///
    /// The handle is duplicated rather than borrowed, so the caller keeps theirs
    /// and goes on using it: the roll-back after a failed verification writes
    /// through it.
    ///
    /// `len` is passed rather than measured. The caller knows how long the file
    /// it just wrote should be, and asking the filesystem again would map
    /// whatever length something else has since given it.
    ///
    /// # Errors
    ///
    /// A zero length, the handle not duplicating, or the mapping failing.
    pub fn map_open_file(file: &std::fs::File, len: usize) -> Result<Self, String> {
        if len == 0 {
            return Err("the file to map is empty".to_string());
        }
        let dup = file
            .try_clone()
            .map_err(|e| format!("could not duplicate the handle to map it: {e}"))?;
        // Read-only for the reason `map_file` gives, and with one more here: the
        // caller is holding a *writable* handle to this same file, so a writable
        // mapping would be a second way to reach it from inside the sandbox.
        Self::map(dup, len, false)
    }

    /// Adopts a descriptor the parent passed in, and maps it.
    ///
    /// # Errors
    ///
    /// The mapping failing.
    ///
    /// # Safety
    ///
    /// `fd` must be a live descriptor owned by nothing else in this process.
    pub unsafe fn from_fd(fd: i32, len: usize, writable: bool) -> Result<Self, String> {
        use std::os::fd::FromRawFd;
        // SAFETY: the caller guarantees the descriptor is live and unowned.
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        Self::map(file, len, writable)
    }

    /// Maps an open file.
    ///
    /// `writable` must match the descriptor: `mmap` refuses `PROT_WRITE` on a
    /// read-only file with `EACCES`, and the message it gives ("Permission
    /// denied") reads like a sandbox problem rather than a mismatch.
    fn map(file: std::fs::File, len: usize, writable: bool) -> Result<Self, String> {
        use std::os::fd::AsRawFd;
        let prot = if writable {
            libc::PROT_READ | libc::PROT_WRITE
        } else {
            libc::PROT_READ
        };
        // SAFETY: len is non-zero and the descriptor is a regular file of at
        // least that size.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                prot,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(format!(
                "mmap of {len} bytes failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self { file, ptr, len })
    }

    /// How many bytes the mapping covers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the mapping is empty, which `create` and `map_file` both refuse.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The mapping's descriptor, for handing to a child.
    #[must_use]
    pub fn raw_fd(&self) -> i32 {
        use std::os::fd::AsRawFd;
        self.file.as_raw_fd()
    }

    /// How long the mapped file is *now*, which is not always [`len`](Self::len).
    ///
    /// A `MAP_SHARED` mapping does not pin the file's length. Another process
    /// can shorten it while a document is open, and then every page beyond the
    /// new end is unbacked: reading there raises `SIGBUS` at the faulting
    /// instruction rather than returning an error, which kills the worker.
    /// `examples/rewrite_probe.rs` measures exactly that, deterministically.
    ///
    /// **Asked of the descriptor, never of the path**, and the difference is the
    /// whole reason this exists rather than a `metadata()` call at the call
    /// site. The common way to replace a file is to write a temporary and rename
    /// it over --- after which the *path* names a different inode of some
    /// unrelated length, while the inode this mapping holds is alive and intact.
    /// A check on the path reports that healthy document as broken, and reports
    /// it every time the reader's own editor saves. The descriptor keeps naming
    /// the file that was mapped, so the only thing that can shorten it is a
    /// genuine truncation of the bytes underneath us.
    ///
    /// `None` when the length cannot be established, which is not the same as
    /// "unchanged" --- see the caller, which treats it as "no diagnosis" and
    /// leaves the ordinary crash path to do its work.
    #[must_use]
    pub fn backing_len(&self) -> Option<u64> {
        self.file.metadata().ok().map(|m| m.len())
    }

    /// Reads the mapping.
    ///
    /// Not for a mapping a worker also holds: see [`Shm::copy_front`].
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: the mapping is valid for `len` bytes for as long as `self`.
        unsafe { std::slice::from_raw_parts(self.ptr.cast::<u8>(), self.len) }
    }

    /// Writes the mapping.
    ///
    /// Only valid on a mapping created writable --- a document mapping is not,
    /// and writing to one faults. Not for a mapping a worker also holds: see
    /// [`Shm::fill_front`].
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: as above, and `&mut self` excludes concurrent readers here.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.cast::<u8>(), self.len) }
    }

    /// Copies the first `length` bytes out, through the file and not the mapping.
    ///
    /// **The parent's read of anything a worker wrote goes through here.** A
    /// worker is handed a *writable* descriptor to its tile file, and a
    /// `MAP_SHARED` mapping does not pin the file's length: one `ftruncate` from
    /// a taken-over worker leaves every page of the parent's mapping unbacked,
    /// and copying out of the slice then raises `SIGBUS` in the app, which ends
    /// every open tab with its unsaved edits. A positional read of the same file
    /// sees the same bytes (the mapping and the file share one page cache) and
    /// answers a short file with an error instead. That holds whether or not a
    /// platform's sandbox lets the worker truncate an inherited descriptor, so
    /// nothing here rests on that being measured.
    ///
    /// # Errors
    ///
    /// `length` past the mapping, or the file holding fewer than `length` bytes.
    pub fn copy_front(&self, length: usize) -> Result<Vec<u8>, String> {
        use std::os::unix::fs::FileExt;

        if length > self.len {
            return Err(format!(
                "{length} bytes were asked of a {}-byte mapping",
                self.len
            ));
        }
        let mut out = vec![0u8; length];
        self.file
            .read_exact_at(&mut out, 0)
            .map_err(|e| format!("the shared file no longer holds {length} bytes: {e}"))?;
        Ok(out)
    }

    /// Writes `bytes` at the front, through the file and not the mapping.
    ///
    /// The other direction of [`Shm::copy_front`], for a mapping the parent
    /// fills and a running worker reads (the OCR worker's pixels). Writing into
    /// the slice of a file the worker has cut short faults the parent exactly as
    /// reading does; a positional write instead puts the length back.
    ///
    /// # Errors
    ///
    /// `bytes` longer than the mapping, or the write failing.
    pub fn fill_front(&mut self, bytes: &[u8]) -> Result<(), String> {
        use std::os::unix::fs::FileExt;

        if bytes.len() > self.len {
            return Err(format!(
                "{} bytes do not fit a {}-byte mapping",
                bytes.len(),
                self.len
            ));
        }
        self.file
            .write_all_at(bytes, 0)
            .map_err(|e| format!("could not write the shared file: {e}"))
    }

    /// Reborrows the mapping for the process lifetime.
    ///
    /// Only sound because the worker leaks its document `Shm`, which it does:
    /// PDFium holds the document bytes for as long as the document is open, and
    /// the worker's document is open until it exits.
    ///
    /// # Safety
    ///
    /// The caller must guarantee this `Shm` is never dropped.
    #[must_use]
    pub unsafe fn as_static(&self) -> &'static [u8] {
        // SAFETY: caller guarantees the `Shm` outlives every use.
        unsafe { std::slice::from_raw_parts(self.ptr.cast::<u8>(), self.len) }
    }
}

#[cfg(unix)]
impl Drop for Shm {
    fn drop(&mut self) {
        // SAFETY: unmapping exactly what was mapped.
        unsafe { libc::munmap(self.ptr, self.len) };
    }
}

/// The same surface on Windows, so callers compile unchanged.
///
/// `raw_fd` is the one method that cannot be honoured: a `HANDLE` is
/// pointer-sized and an `i32` is not, so returning one would truncate on 64-bit
/// --- silently, and into a value that still looks like a plausible descriptor.
/// [`Shm::raw_handle`] replaces it, and callers that hand a mapping to a child
/// are platform-specific anyway, since Windows has no fixed descriptor numbers
/// for [`crate::worker::DOC_FD`] and [`crate::worker::TILE_FD`] to name.
#[cfg(windows)]
impl Shm {
    /// Creates a pagefile-backed mapping of `len` bytes.
    ///
    /// **The handle is born not inheritable**, and stays so except for the
    /// length of one contained spawn: `sandbox_win::spawn_contained` marks it
    /// for that `CreateProcess` and takes the mark back afterwards. A mark set
    /// here would last the mapping's lifetime, and an inheritable handle is
    /// authority any spawn in this process hands over --- `std::process::Command`
    /// inherits every inheritable handle, so the `explorer.exe` that "Show in
    /// folder" starts would receive a copy of every open document. The handle
    /// list narrows *our* spawns only.
    ///
    /// # Errors
    ///
    /// `len` being zero, or either of the two calls failing.
    pub fn create(len: usize) -> Result<Self, String> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Memory::{CreateFileMappingW, PAGE_READWRITE};

        if len == 0 {
            // Refused rather than passed through: `CreateFileMapping` reads a
            // zero maximum size as "the whole of the backing file", which for
            // the pagefile is not a size at all, and the error it eventually
            // gives describes neither.
            return Err("shm create refused: zero length".into());
        }
        let (high, low) = split_u64(len as u64);
        // SAFETY: null attributes mean the default descriptor and a handle that
        // is not inherited; a null name means an unnamed section;
        // `INVALID_HANDLE_VALUE` selects pagefile backing.
        let mapping = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                high,
                low,
                std::ptr::null(),
            )
        };
        if mapping.is_null() {
            return Err(format!(
                "shm create failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Self::view(mapping, len, true)
    }

    /// Maps a whole file, for handing a document to a worker.
    ///
    /// # Errors
    ///
    /// The file not opening, being empty, or not mapping.
    pub fn map_file(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| said!("could not open {path:?}: {e}"))?;
        let len = file
            .metadata()
            .map_err(|e| said!("could not stat {path:?}: {e}"))?
            .len() as usize;
        if len == 0 {
            return Err(said!("{path:?} is empty"));
        }
        Self::section(file, len).map_err(|e| said!("could not map {path:?}: {e}"))
    }

    /// Maps a file that is already open, without naming it again.
    ///
    /// **The seam the save's verification needs, and the pathname is the point.**
    /// `save::append_through` guarantees everything it guarantees about the file
    /// its *handle* holds, never about whatever the name resolves to now, so a
    /// verifier that called [`Shm::map_file`] on the source path would check
    /// whichever file has that name at that moment --- reintroducing exactly the
    /// race that function exists to close.
    ///
    /// The handle is duplicated rather than borrowed, so the caller keeps theirs
    /// and goes on using it: the roll-back after a failed verification writes
    /// through it.
    ///
    /// `len` is passed rather than measured. The caller knows how long the file
    /// it just wrote should be, and asking the filesystem again would map
    /// whatever length something else has since given it.
    ///
    /// # Errors
    ///
    /// A zero length, the handle not duplicating, or the mapping failing.
    pub fn map_open_file(file: &std::fs::File, len: usize) -> Result<Self, String> {
        if len == 0 {
            return Err("the file to map is empty".to_string());
        }
        let dup = file
            .try_clone()
            .map_err(|e| format!("could not duplicate the handle to map it: {e}"))?;
        Self::section(dup, len)
    }

    /// Copies an open file into a pagefile section, and hands back a read-only one.
    ///
    /// The half [`Shm::map_file`] and [`Shm::map_open_file`] share.
    ///
    /// **A copy, not a section over the file, and the reason is the reader's
    /// other programs.** This used to be `CreateFileMappingW` on the file
    /// itself, and Windows holds a file with a section open on it against
    /// truncation and replacement: every `open(path, "wb")` from another
    /// program failed with `ERROR_USER_MAPPED_FILE` (1224) for as long as the
    /// document was open in tpdf, which Python reports as `EINVAL` (issue #7).
    /// A copy leaves the file free the moment this returns. It also means the
    /// worker never sees bytes a writer puts there later, so a half-written
    /// file is not shown and a valid in-place rewrite does not mix two
    /// revisions in one document --- `rewrite-probe` measured both before this.
    ///
    /// Read through the handle, positionally, and never by name: the
    /// fingerprint and the save's read-back rest on this being the file the
    /// caller opened.
    ///
    /// The cost is one read of the file at open, and the document's length in
    /// committed memory where the section over the file used the file cache.
    ///
    /// # Errors
    ///
    /// The section not being created, the read failing, or the file being
    /// shorter than `len` --- a writer cutting it while it is read, which is
    /// refused rather than handed to a worker as a document of zeroes.
    fn section(file: std::fs::File, len: usize) -> Result<Self, String> {
        use std::os::windows::fs::FileExt;

        let mut copy = Self::create(len)?;
        let into = copy.as_mut_slice();
        let mut at = 0usize;
        while at < len {
            let read = file
                .seek_read(&mut into[at..], at as u64)
                .map_err(|e| format!("could not read the document at byte {at}: {e}"))?;
            if read == 0 {
                return Err(format!(
                    "the document ended at byte {at} of {len} while it was being read"
                ));
            }
            at += read;
        }
        drop(file);
        copy.read_only()
    }

    /// The same section behind a handle that can only map it for reading.
    ///
    /// `create` makes a `PAGE_READWRITE` section, because the parent has to
    /// write the copy into it. The worker must not be able to: the parent reads
    /// the same pages back for the save's verification, so a hostile worker
    /// that could write them could make a refused read-back pass. The handle
    /// is what carries that, since [`duplicate_into`] passes it on with
    /// `DUPLICATE_SAME_ACCESS` --- so the writable handle and its view are
    /// closed here, and the only handle left is one opened with
    /// `FILE_MAP_READ` alone, which a holder cannot upgrade.
    fn read_only(self) -> Result<Self, String> {
        use windows_sys::Win32::Foundation::{DuplicateHandle, HANDLE};
        use windows_sys::Win32::System::Memory::FILE_MAP_READ;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        let mut reader: HANDLE = std::ptr::null_mut();
        // SAFETY: `self.mapping` is live and owned here; the pseudo-handle to
        // this process and the out pointer outlive the call. Not inheritable,
        // for the reason `create` gives.
        let ok = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                self.mapping,
                GetCurrentProcess(),
                &raw mut reader,
                FILE_MAP_READ,
                0,
                0,
            )
        };
        if ok == 0 {
            return Err(format!(
                "could not make the document section read-only: {}",
                std::io::Error::last_os_error()
            ));
        }
        let len = self.len;
        // The writable view and handle go first; the section lives on through
        // `reader`.
        drop(self);
        Self::view(reader, len, false)
    }

    /// Adopts a section handle the parent passed in, and maps it.
    ///
    /// The Windows counterpart of [`Shm::from_fd`]. It takes a `usize` because a
    /// handle arrives through argv as a number, exactly as in
    /// `examples/win_sandbox_probe.rs`.
    ///
    /// # Errors
    ///
    /// The mapping failing.
    ///
    /// # Safety
    ///
    /// `handle` must be a live section handle owned by nothing else in this
    /// process, and `len` must not exceed the section.
    pub unsafe fn from_handle(handle: usize, len: usize, writable: bool) -> Result<Self, String> {
        Self::view(
            handle as windows_sys::Win32::Foundation::HANDLE,
            len,
            writable,
        )
    }

    /// Maps a section that is already open, taking ownership of the handle.
    fn view(
        mapping: windows_sys::Win32::Foundation::HANDLE,
        len: usize,
        writable: bool,
    ) -> Result<Self, String> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Memory::{MapViewOfFile, FILE_MAP_READ, FILE_MAP_WRITE};

        let access = if writable {
            FILE_MAP_READ | FILE_MAP_WRITE
        } else {
            FILE_MAP_READ
        };
        // SAFETY: `mapping` is a live section handle owned here.
        let view = unsafe { MapViewOfFile(mapping, access, 0, 0, len) };
        if view.Value.is_null() {
            let error = std::io::Error::last_os_error();
            // SAFETY: owned here and not yet stored, so this is the only close.
            unsafe { CloseHandle(mapping) };
            return Err(format!("MapViewOfFile of {len} bytes failed: {error}"));
        }
        Ok(Self {
            mapping,
            ptr: view.Value,
            len,
        })
    }

    /// How many bytes the mapping covers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the mapping is empty, which `create` and `map_file` both refuse.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The section handle, for handing to a child.
    ///
    /// Not `raw_fd`: see the impl note. A `HANDLE` does not fit in an `i32`.
    #[must_use]
    pub fn raw_handle(&self) -> usize {
        self.mapping as usize
    }

    /// Always `None` here, and the reason is a real platform difference.
    ///
    /// The POSIX twin exists because a `MAP_SHARED` mapping does not stop
    /// another process shortening the file underneath it. On Windows a document
    /// mapping is a copy in a pagefile section (see `section`), so nothing a
    /// writer does to the file reaches it, and there is no backing file left to
    /// measure. `rewrite-probe` truncates the file under an open document and
    /// records that the worker goes on drawing it.
    #[must_use]
    pub fn backing_len(&self) -> Option<u64> {
        None
    }

    /// Reads the mapping.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: the view is valid for `len` bytes for as long as `self`.
        unsafe { std::slice::from_raw_parts(self.ptr.cast::<u8>(), self.len) }
    }

    /// Writes the mapping.
    ///
    /// Only valid on a mapping created writable --- a document mapping is not,
    /// and writing to one faults.
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: as above, and `&mut self` excludes concurrent readers here.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.cast::<u8>(), self.len) }
    }

    /// Copies the first `length` bytes out.
    ///
    /// The unix twin reads through the file because a worker can shorten it.
    /// A pagefile section cannot be shortened by anyone, and a mapped view
    /// stays backed for as long as it is mapped, so the slice is the read here.
    ///
    /// # Errors
    ///
    /// `length` past the mapping.
    pub fn copy_front(&self, length: usize) -> Result<Vec<u8>, String> {
        self.as_slice()
            .get(..length)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| format!("{length} bytes were asked of a {}-byte mapping", self.len))
    }

    /// Writes `bytes` at the front. See [`Shm::copy_front`] for why this is the
    /// slice here and the file on unix.
    ///
    /// # Errors
    ///
    /// `bytes` longer than the mapping.
    pub fn fill_front(&mut self, bytes: &[u8]) -> Result<(), String> {
        let len = self.len;
        self.as_mut_slice()
            .get_mut(..bytes.len())
            .ok_or_else(|| format!("{} bytes do not fit a {len}-byte mapping", bytes.len()))?
            .copy_from_slice(bytes);
        Ok(())
    }

    /// Reborrows the mapping for the process lifetime.
    ///
    /// # Safety
    ///
    /// The caller must guarantee this `Shm` is never dropped.
    #[must_use]
    pub unsafe fn as_static(&self) -> &'static [u8] {
        // SAFETY: caller guarantees the `Shm` outlives every use.
        unsafe { std::slice::from_raw_parts(self.ptr.cast::<u8>(), self.len) }
    }
}

/// Whether a document mapping is a copy of the file rather than a view of it.
///
/// `true` on Windows, where a section over the file would stop other programs
/// rewriting it (issue #7); `false` on unix, where a shared mapping blocks no
/// writer. What a reader sees after another program rewrites an open document
/// depends on it, which is why `rewrite-probe` reads it rather than `cfg!`.
pub const DOCUMENTS_ARE_COPIED: bool = cfg!(windows);

/// Copies a handle from this process into another one's table.
///
/// The Windows half of the document handover. Returns the value the handle has
/// **in the target**, which is the only form the child can use --- a handle
/// number is meaningful in exactly one process.
///
/// `DUPLICATE_SAME_ACCESS` rather than a named access mask on purpose: the
/// document section's only handle is opened `FILE_MAP_READ` by
/// `Shm::read_only`, so "the same access" is read-only, and re-stating the mask
/// here would be a second place for the worker's read-only guarantee to live.
/// The one that matters is where that handle is made.
///
/// # Safety
///
/// Both handles must be live and owned by this process for the call.
///
/// # Errors
///
/// The duplication failing, which on a target that has exited is the usual case.
#[cfg(windows)]
pub(crate) unsafe fn duplicate_into(
    target: windows_sys::Win32::Foundation::HANDLE,
    handle: usize,
) -> Result<usize, String> {
    use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut out: HANDLE = std::ptr::null_mut();
    // SAFETY: the caller's contract, plus a pseudo-handle to self and an out
    // pointer that outlives the call.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle as HANDLE,
            target,
            &raw mut out,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        return Err(format!(
            "DuplicateHandle: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(out as usize)
}

/// Splits a length into the high and low halves `CreateFileMapping` wants.
///
/// Separate and tested because the shift is the kind of arithmetic that is
/// wrong only above 4 GB, where no ordinary run would notice.
#[cfg(windows)]
fn split_u64(value: u64) -> (u32, u32) {
    #[allow(clippy::cast_possible_truncation)]
    ((value >> 32) as u32, (value & 0xFFFF_FFFF) as u32)
}

#[cfg(windows)]
impl Drop for Shm {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Memory::{UnmapViewOfFile, MEMORY_MAPPED_VIEW_ADDRESS};

        // View first, then the section --- and this order is a convention here,
        // not a correctness requirement. The comment originally claimed that
        // closing the section first leaks the view; a mutation reversing the two
        // left all fifteen checks green, and the claim is simply wrong: Windows
        // keeps a mapped view valid after its section handle is closed, holding
        // the backing open until the last view is unmapped. It is written this
        // way because it mirrors the POSIX side and reads in the order the
        // resources were acquired, and **no test pins it**, which is stated
        // rather than left for the next person to rediscover.
        // SAFETY: unmapping exactly the view that was mapped, then closing the
        // handle that produced it, each exactly once.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.ptr });
            CloseHandle(self.mapping);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Shm;

    #[cfg(unix)]
    use crate::testutil::TempDir;

    /// A new mapping's backing file is created private to this user.
    ///
    /// Asked through the descriptor rather than the pathname, which is the only
    /// way to ask it at all: `create` unlinks the file on the statement after it
    /// opens it, so there is no name left to stat.
    ///
    /// **What this can and cannot fail on.** Without the explicit mode the file
    /// is created at `0o666 & !umask`, and a umask is inherited from whatever
    /// launched tpdf --- so on a machine whose umask is already `0o077` this
    /// test would pass with the mode argument deleted. That is not a reason to
    /// leave the argument out; it is the reason it is an argument rather than an
    /// assumption about the environment. Proved red on this one, whose umask is
    /// `0o022`: without it the file is created `0o644`.
    #[cfg(unix)]
    #[test]
    fn the_backing_file_of_a_new_mapping_is_not_readable_by_anyone_else() {
        use std::os::unix::fs::PermissionsExt;

        let shm = Shm::create(4096).expect("create a mapping");
        let mode = shm
            .file
            .metadata()
            .expect("stat the mapping's own descriptor")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o600,
            "the backing file was created 0o{mode:o}, so its permissions came from the umask"
        );
    }

    /// Truncation is visible through the mapping's own descriptor.
    ///
    /// The condition `workers.rs` diagnoses, asserted at the level that can see
    /// it without a worker or a fault. Both directions, because they fail to
    /// opposite mistakes: a check that never reports a shrink is a diagnosis
    /// that does not exist, and one that reports an untouched file condemns
    /// every document the first time any worker crashes for any reason.
    #[cfg(unix)]
    #[test]
    fn a_mapping_reports_its_file_shrinking_and_not_a_file_that_did_not() {
        let dir = TempDir::new("shrink");
        let path = dir.join("doc.bin");
        std::fs::write(&path, vec![7u8; 4096]).expect("write");

        let shm = Shm::map_file(&path).expect("map");
        assert_eq!(shm.len(), 4096);
        assert_eq!(
            shm.backing_len(),
            Some(4096),
            "before anything is done to it"
        );

        // Growing is not a diagnosis: an incremental save appends a revision and
        // takes nothing away, so every byte the mapping covers still says what
        // it said.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open")
            .set_len(8192)
            .expect("grow");
        assert_eq!(shm.backing_len(), Some(8192));

        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open")
            .set_len(1024)
            .expect("truncate");
        assert_eq!(
            shm.backing_len(),
            Some(1024),
            "after the file was truncated"
        );
    }

    /// A file renamed over the path leaves the mapped inode alone.
    ///
    /// The check that decides whether this whole mechanism is usable, and the
    /// reason `backing_len` asks the descriptor rather than calling
    /// `metadata()` on the path at the call site. Writing a temporary and
    /// renaming it over the original is how nearly everything replaces a file,
    /// the reader's own editor included --- and it leaves the mapping perfectly
    /// healthy, because the old inode is still there underneath it.
    ///
    /// A path-based check reports 64 bytes here, concludes the document has been
    /// truncated from 4096, and condemns a document that is fine. The replacement
    /// file is deliberately *shorter* so that mistake would be caught rather than
    /// merely possible: with a longer one the wrong check passes too.
    #[cfg(unix)]
    #[test]
    fn a_file_renamed_over_the_path_does_not_look_like_a_truncation() {
        let dir = TempDir::new("rename");
        let path = dir.join("doc.bin");
        std::fs::write(&path, vec![7u8; 4096]).expect("write");

        let shm = Shm::map_file(&path).expect("map");

        let staged = dir.join("doc.new");
        std::fs::write(&staged, vec![9u8; 64]).expect("write");
        std::fs::rename(&staged, &path).expect("rename");

        // The control: the path really is a much smaller file now, so a check
        // written against it would have something to go wrong with.
        assert_eq!(std::fs::metadata(&path).expect("stat").len(), 64);
        assert_eq!(
            shm.backing_len(),
            Some(4096),
            "the mapped inode still holds every byte it did"
        );
        // And the mapping still reads what it always did, which is what makes
        // "stale but coherent" a true description rather than a hope.
        assert!(shm.as_slice().iter().all(|b| *b == 7));
    }

    /// A worker that cuts its tile file short costs the parent an error, not a
    /// fault.
    ///
    /// The truncation is the one a taken-over worker can make: `ftruncate` on
    /// its own copy of the descriptor, here a `dup` of the parent's, which is
    /// what the worker inherits. Read through the slice afterwards and the
    /// parent dies of `SIGBUS`, so a regression ends this test binary rather
    /// than failing one assertion; the control first proves the bytes the
    /// worker wrote through its mapping are what the read returns.
    #[cfg(unix)]
    #[test]
    fn a_tile_file_truncated_by_its_worker_is_an_error_and_not_a_fault() {
        let mut shm = Shm::create(8192).expect("create a mapping");
        shm.as_mut_slice().fill(0xAB);
        assert_eq!(
            shm.copy_front(8192)
                .expect("the control: an intact file reads"),
            vec![0xAB; 8192],
            "a read through the file sees what was written through the mapping"
        );

        // SAFETY: duplicating a live descriptor this mapping owns; the copy is
        // closed below and nothing else holds it.
        let theirs = unsafe { libc::dup(shm.raw_fd()) };
        assert!(theirs >= 0, "dup the worker's copy");
        // SAFETY: a live descriptor of a regular file this test owns.
        assert_eq!(unsafe { libc::ftruncate(theirs, 0) }, 0, "truncate");
        // SAFETY: closed once.
        unsafe { libc::close(theirs) };

        let err = shm
            .copy_front(8192)
            .expect_err("a file shorter than the reply's length must be refused");
        assert!(err.contains("no longer holds 8192 bytes"), "{err}");
    }

    /// The parent's write into the OCR pixel file survives the same cut.
    ///
    /// Writing into the slice of a truncated file faults the writer as reading
    /// does; a positional write puts the length back and the worker's mapping
    /// is whole again.
    #[cfg(unix)]
    #[test]
    fn a_pixel_file_truncated_by_its_worker_takes_the_next_write_without_a_fault() {
        let mut shm = Shm::create(8192).expect("create a mapping");
        // SAFETY: as in the test above.
        let theirs = unsafe { libc::dup(shm.raw_fd()) };
        assert!(theirs >= 0, "dup the worker's copy");
        // SAFETY: as above.
        assert_eq!(unsafe { libc::ftruncate(theirs, 0) }, 0, "truncate");
        // SAFETY: closed once.
        unsafe { libc::close(theirs) };

        shm.fill_front(&[7u8; 4096]).expect("a positional write");
        assert_eq!(shm.backing_len(), Some(4096), "the write restored a length");
        assert_eq!(shm.copy_front(4096).expect("read back"), vec![7u8; 4096]);
    }

    /// A length past the mapping is refused on either platform, read or write.
    #[test]
    fn copying_past_the_mapping_is_refused() {
        let mut shm = Shm::create(4096).expect("create a mapping");
        assert!(shm.copy_front(4096).is_ok(), "control: the whole mapping");
        assert!(shm.copy_front(4097).is_err(), "one byte past it");
        assert!(
            shm.fill_front(&[0u8; 4096]).is_ok(),
            "control: a full write"
        );
        assert!(shm.fill_front(&[0u8; 4097]).is_err(), "one byte past it");
    }

    /// Off unix `Shm::create` refuses by design, so there is no mapping to
    /// assert anything about --- the refusal itself is covered below.
    #[cfg(unix)]
    #[test]
    fn a_mapping_is_readable_and_writable_and_unnamed() {
        let mut shm = Shm::create(4096).expect("create");
        assert_eq!(shm.len(), 4096);
        assert!(!shm.is_empty());
        shm.as_mut_slice()[7] = 0xAB;
        assert_eq!(shm.as_slice()[7], 0xAB);
        // Unlinked at creation, so there is no path a second process could open.
        // What is left is the descriptor, which is the whole reason a sandboxed
        // worker can reach it.
        assert!(shm.raw_fd() >= 0);
    }

    /// The same claim on Windows, under the same name **on purpose**.
    ///
    /// `AGENTS.md` records that the stable thing about a suite is its set of
    /// check *names*, not its count --- so a property that holds on both
    /// platforms is asserted under one name on both, and a `--list` diff across
    /// the two shows no gap where there is none.
    ///
    /// This replaced `a_mapping_refuses_off_unix`, which asserted the opposite
    /// and was correct until the mapping existed. That deletion is the point
    /// rather than a casualty: the refusal it pinned was the absence of an
    /// implementation, and keeping it would have meant keeping the absence.
    #[cfg(windows)]
    #[test]
    fn a_mapping_is_readable_and_writable_and_unnamed() {
        let mut shm = Shm::create(4096).expect("create");
        assert_eq!(shm.len(), 4096);
        assert!(!shm.is_empty());
        shm.as_mut_slice()[7] = 0xAB;
        assert_eq!(shm.as_slice()[7], 0xAB);
        // The section was created with a null name, so there is no string a
        // second process could open it by. What is left is the handle, which is
        // the whole reason a contained worker can reach it.
        assert!(shm.raw_handle() != 0);
    }

    /// Zero is refused rather than passed to `CreateFileMapping`.
    ///
    /// Not pedantry about an unreachable input. A zero maximum size means "the
    /// whole backing file" to that API, which for pagefile backing is not a size
    /// at all --- so the call does not fail cleanly, and the error it eventually
    /// produces describes neither the cause nor the length.
    #[cfg(windows)]
    #[test]
    fn a_mapping_refuses_zero_length() {
        let err = Shm::create(0)
            .map(|_| ())
            .expect_err("a zero-length mapping must be refused");
        assert!(err.contains("zero length"), "{err}");
    }

    /// A document mapping reads the file's own bytes back.
    ///
    /// Content, not length: a mapping of the right size full of zeroes passes a
    /// length check identically to a correct one, and that is the failure a
    /// wrong offset or a stale section would actually produce.
    #[cfg(windows)]
    #[test]
    fn a_document_mapping_reads_the_file_back() {
        let path = std::env::temp_dir().join(format!("tpdf-shm-test-{}", std::process::id()));
        std::fs::write(&path, b"%PDF-1.7 not really").expect("write fixture");
        {
            let shm = Shm::map_file(&path).expect("map_file");
            assert_eq!(shm.len(), 19);
            assert_eq!(shm.as_slice(), b"%PDF-1.7 not really");
        }
        // Deleted only after the mapping is dropped, which also asserts the
        // section did not keep the file locked beyond its own lifetime.
        std::fs::remove_file(&path).expect("the mapping must not outlive its Shm");
    }

    /// Another program can rewrite a document while it is open (issue #7).
    ///
    /// The writes the issue names, each against an open mapping: a
    /// share-mode-0 open, a truncating `open(path, "wb")` with a caller's
    /// handle also open, as the app keeps one for the fingerprint, and a
    /// rename over the path. Then the mapping is read again, because the fix is a copy and
    /// a copy is only right if it still holds the bytes it was taken from.
    /// Before the fix the first write fails with `ERROR_USER_MAPPED_FILE`
    /// (1224).
    #[cfg(windows)]
    #[test]
    fn an_open_document_does_not_stop_another_program_rewriting_its_file() {
        use std::io::Write;
        use std::os::windows::fs::OpenOptionsExt;

        let dir = std::env::temp_dir().join(format!("tpdf-shm-rewrite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("doc.pdf");
        let original = vec![7u8; 8192];
        std::fs::write(&path, &original).expect("write fixture");

        let by_name = Shm::map_file(&path).expect("map_file");
        // Exclusive first, while no handle to the file is open in this test:
        // any open handle refuses a share-mode-0 open, whoever holds it, so
        // this asks only whether the mapping holds one. The old section over
        // the file did, and failed this with a sharing violation (32).
        std::fs::OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(&path)
            .expect("an exclusive open must succeed while the document is mapped");

        let held = std::fs::File::open(&path).expect("open as the app does");
        let by_handle = Shm::map_open_file(&held, 8192).expect("map_open_file");

        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(b"x"))
            .expect("a truncating write must succeed while the document is open");
        let staged = dir.join("doc.new");
        std::fs::write(&staged, b"y").expect("write the replacement");
        std::fs::rename(&staged, &path).expect("a rename over the path must succeed");

        // The control: the file really is one byte now.
        assert_eq!(std::fs::metadata(&path).expect("stat").len(), 1);
        assert_eq!(by_name.as_slice(), &original[..], "map_file kept its bytes");
        assert_eq!(
            by_handle.as_slice(),
            &original[..],
            "map_open_file kept its bytes"
        );

        drop((by_name, by_handle, held));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A document section's handle cannot be mapped for writing.
    ///
    /// The section is created writable so the copy can go in, and the parent
    /// reads it back for the save's verification, so a worker able to write it
    /// could make a refused read-back pass. `MapViewOfFile` with
    /// `FILE_MAP_WRITE` on the handle a worker receives is the attempt a
    /// hostile one would make. The control is the same call on a tile mapping,
    /// which is meant to be writable: without it, a call that failed for any
    /// other reason would pass this test.
    #[cfg(windows)]
    #[test]
    fn a_document_section_cannot_be_mapped_for_writing() {
        use windows_sys::Win32::System::Memory::{MapViewOfFile, UnmapViewOfFile, FILE_MAP_WRITE};

        let path = std::env::temp_dir().join(format!("tpdf-shm-ro-{}", std::process::id()));
        std::fs::write(&path, vec![1u8; 4096]).expect("write fixture");
        let doc = Shm::map_file(&path).expect("map_file");
        let tile = Shm::create(4096).expect("create");

        let attempt = |handle: usize| {
            // SAFETY: a live section handle owned by a `Shm` in this scope.
            let view = unsafe { MapViewOfFile(handle as _, FILE_MAP_WRITE, 0, 0, 4096) };
            let mapped = !view.Value.is_null();
            if mapped {
                // SAFETY: unmapping the view just made.
                unsafe { UnmapViewOfFile(view) };
            }
            mapped
        };
        assert!(
            attempt(tile.raw_handle()),
            "control: a tile section maps writable"
        );
        assert!(
            !attempt(doc.raw_handle()),
            "the document section's handle must not grant write access"
        );

        drop(doc);
        let _ = std::fs::remove_file(&path);
    }

    /// No section handle is inheritable outside a spawn.
    ///
    /// Both kinds a worker is handed: the tile (`create`) and a document,
    /// whose only handle is the one `read_only` duplicates. An inheritable one
    /// is handed to every child any code in this process starts with
    /// `bInheritHandles`, and `std::process::Command` always passes it. The
    /// control marks the tile and reads the flag back, so a probe that could
    /// not see the flag would fail there rather than pass here.
    #[cfg(windows)]
    #[test]
    fn a_section_handle_is_not_inheritable_outside_a_spawn() {
        use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT};

        let flag = |handle: usize| {
            let mut flags: u32 = 0;
            // SAFETY: a live section handle owned by a `Shm` in this scope.
            let ok = unsafe { GetHandleInformation(handle as _, &raw mut flags) };
            assert_ne!(ok, 0, "GetHandleInformation");
            flags & HANDLE_FLAG_INHERIT
        };

        let path = std::env::temp_dir().join(format!("tpdf-shm-inherit-{}", std::process::id()));
        std::fs::write(&path, vec![1u8; 4096]).expect("write fixture");
        let doc = Shm::map_file(&path).expect("map_file");
        let tile = Shm::create(4096).expect("create");

        assert_eq!(
            flag(tile.raw_handle()),
            0,
            "a tile section is born inheritable"
        );
        assert_eq!(
            flag(doc.raw_handle()),
            0,
            "a document section is inheritable"
        );

        // SAFETY: the tile's live handle, marked and then read.
        unsafe { crate::sandbox_win::make_inheritable(tile.raw_handle() as _) }
            .expect("mark the tile");
        assert_eq!(
            flag(tile.raw_handle()),
            HANDLE_FLAG_INHERIT,
            "control: a marked handle reads as marked"
        );

        drop(doc);
        let _ = std::fs::remove_file(&path);
    }

    /// A file shorter than the length asked for is refused, not padded.
    ///
    /// `map_open_file` takes the length from the caller, and a writer can cut
    /// the file between that measurement and the copy. Copying what is there
    /// would hand the worker a document whose tail is zeroes.
    #[cfg(windows)]
    #[test]
    fn a_file_shorter_than_its_stated_length_is_refused() {
        let path = std::env::temp_dir().join(format!("tpdf-shm-short-{}", std::process::id()));
        std::fs::write(&path, vec![1u8; 100]).expect("write fixture");
        let file = std::fs::File::open(&path).expect("open");
        let err = Shm::map_open_file(&file, 4096)
            .map(|_| ())
            .expect_err("a short file must be refused");
        drop(file);
        let _ = std::fs::remove_file(&path);
        assert!(err.contains("ended at byte 100 of 4096"), "{err}");
    }

    /// An empty document is refused, since a zero-length view is not a mapping.
    #[cfg(windows)]
    #[test]
    fn a_document_mapping_refuses_an_empty_file() {
        let path = std::env::temp_dir().join(format!("tpdf-shm-empty-{}", std::process::id()));
        std::fs::write(&path, b"").expect("write fixture");
        let err = Shm::map_file(&path)
            .map(|_| ())
            .expect_err("an empty document must be refused");
        let _ = std::fs::remove_file(&path);
        assert!(err.contains("empty"), "{err}");
    }

    /// The 32-bit split is exact where it matters, which is above 4 GB.
    ///
    /// Tested as a function rather than through a mapping because the only
    /// inputs that can expose a wrong shift are ones no test should allocate.
    #[cfg(windows)]
    #[test]
    fn splitting_a_length_is_exact_above_four_gigabytes() {
        assert_eq!(super::split_u64(0), (0, 0));
        assert_eq!(super::split_u64(4096), (0, 4096));
        // The boundary itself: one below is all low, exactly 4 GB is all high.
        assert_eq!(super::split_u64(0xFFFF_FFFF), (0, 0xFFFF_FFFF));
        assert_eq!(super::split_u64(0x1_0000_0000), (1, 0));
        assert_eq!(super::split_u64(0x1_2345_6789), (1, 0x2345_6789));
    }
}
