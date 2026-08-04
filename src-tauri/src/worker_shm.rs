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
/// It is simpler than the POSIX version in one way worth knowing. A section holds
/// a reference to whatever backs it, so once `map_file` has created the section
/// the *file* handle can be closed and the child needs only the section handle.
/// There is no Windows analogue of passing a file descriptor alongside.
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

        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
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
        let file =
            std::fs::File::open(path).map_err(|e| format!("could not open {path:?}: {e}"))?;
        let len = file
            .metadata()
            .map_err(|e| format!("could not stat {path:?}: {e}"))?
            .len() as usize;
        if len == 0 {
            return Err(format!("{path:?} is empty"));
        }
        // Read-only, and not merely because `File::open` gives a read-only
        // descriptor that `mmap` then refuses `PROT_WRITE` on --- which is how
        // this surfaced. A worker holding a *writable* mapping of the reader's
        // own document could corrupt the file it was asked to display, which is
        // precisely the authority the boundary exists to withhold. The kernel
        // refused it before the threat model did.
        Self::map(file, len, false)
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
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: the mapping is valid for `len` bytes for as long as `self`.
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
    /// The handle is inheritable, because handing it to a child is the only
    /// reason to make one. Inheritable is *permission*, not delivery: a child
    /// receives it only if it is also named in the spawn's handle list, which is
    /// what keeps a hostile worker from receiving every other inheritable handle
    /// this process happens to hold.
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
        let mut attributes = inheritable_attributes();
        let (high, low) = split_u64(len as u64);
        // SAFETY: `attributes` outlives the call; a null name means an unnamed
        // section; `INVALID_HANDLE_VALUE` selects pagefile backing.
        let mapping = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                &raw mut attributes,
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
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Memory::{CreateFileMappingW, PAGE_READONLY};

        let file =
            std::fs::File::open(path).map_err(|e| format!("could not open {path:?}: {e}"))?;
        let len = file
            .metadata()
            .map_err(|e| format!("could not stat {path:?}: {e}"))?
            .len() as usize;
        if len == 0 {
            return Err(format!("{path:?} is empty"));
        }

        let mut attributes = inheritable_attributes();
        // `PAGE_READONLY`, for the reason the POSIX side gives: a worker holding
        // a writable mapping of the reader's own document could corrupt the file
        // it was asked to display, which is precisely the authority the boundary
        // exists to withhold.
        //
        // A zero maximum size is correct *here* and wrong in `create` above ---
        // backed by a real file it means "as large as the file", which is what a
        // document mapping wants.
        // SAFETY: `file` is live for the call and `attributes` outlives it.
        let mapping = unsafe {
            CreateFileMappingW(
                file.as_raw_handle().cast(),
                &raw mut attributes,
                PAGE_READONLY,
                0,
                0,
                std::ptr::null(),
            )
        };
        if mapping.is_null() {
            return Err(format!(
                "could not map {path:?}: {}",
                std::io::Error::last_os_error()
            ));
        }
        // Dropped deliberately: the section holds its own reference to the file,
        // so the mapping outlives this handle and the child never needs it.
        drop(file);
        Self::view(mapping, len, false)
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
    /// another process shortening the file underneath it. Windows does stop it:
    /// while a section object exists over a file, the file is held against
    /// truncation, and `SetEndOfFile` fails with `ERROR_USER_MAPPED_FILE`. So
    /// the condition the twin diagnoses is believed unreachable here rather than
    /// merely undiagnosed.
    ///
    /// **Believed, not measured.** Nothing in this repository has provoked it on
    /// Windows, and `AGENTS.md` is explicit that a refusal existing because
    /// nobody wrote the code is not a guarantee. `None` is therefore the honest
    /// answer and not a claim: the caller reads it as "no diagnosis available"
    /// and falls through to the ordinary crash path, which is what would happen
    /// anyway if the belief turns out to be wrong. There is a second reason it
    /// could not answer even if we wanted it to --- `map_file` closes the file
    /// handle once the section holds a reference to it, so there is nothing left
    /// here to ask.
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

/// Security attributes whose only content is "this handle may be inherited".
#[cfg(windows)]
fn inheritable_attributes() -> windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
    windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: u32::try_from(std::mem::size_of::<
            windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
        >())
        .unwrap_or(0),
        // Null means the default descriptor, which is what a handle passed to a
        // child of the same user wants. A section this process created is not
        // reachable by name, so there is nothing for a descriptor to guard.
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    }
}

/// Copies a handle from this process into another one's table.
///
/// The Windows half of the document handover. Returns the value the handle has
/// **in the target**, which is the only form the child can use --- a handle
/// number is meaningful in exactly one process.
///
/// `DUPLICATE_SAME_ACCESS` rather than a named access mask on purpose: the
/// document section is created `PAGE_READONLY` by `Shm::map_file`, so "the same
/// access" is read-only, and re-stating the mask here would be a second place
/// for the worker's read-only guarantee to live. The one that matters is where
/// the section is made.
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

    /// A directory that removes itself, so a failing test cannot leave litter.
    #[cfg(unix)]
    struct TempDir(std::path::PathBuf);

    #[cfg(unix)]
    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("tpdf-shm-{name}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }
    }

    #[cfg(unix)]
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
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
        let path = dir.0.join("doc.bin");
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
        let path = dir.0.join("doc.bin");
        std::fs::write(&path, vec![7u8; 4096]).expect("write");

        let shm = Shm::map_file(&path).expect("map");

        let staged = dir.0.join("doc.new");
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
