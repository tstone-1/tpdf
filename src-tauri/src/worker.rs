//! The process boundary every PDF is parsed behind.
//!
//! PDFium is native C++ parsing attacker-controlled input, and PDF is a format
//! with recursive object graphs, decompression bombs and launch actions in it.
//! Chrome puts it in a separate process; `docs/THREAT-MODEL.md` says tpdf must
//! too, and `AGENTS.md` records why this cannot be a later hardening pass ---
//! retrofitting a process boundary is an architectural rewrite, so it is one
//! now rather than one later.
//!
//! Spike 0.5 measured every piece of this before it was written, and the
//! numbers are the reason the design looks the way it does:
//!
//! - **A control round trip costs 6 µs** and a 4 MB tile costs **0.11 ms**
//!   through shared memory, against 3.0 ms to hand the same tile to the webview.
//!   The boundary is about 1/27th of the UI boundary; isolation is not where the
//!   time goes.
//! - **The document arrives as a mapped descriptor, never a path.** That is what
//!   makes the sandbox possible at all: a descriptor has no name to guess and
//!   survives a policy that denies opening files.
//! - **The sandbox profile is the measured one**, not the one that looks right.
//!   Denying `file-read*` and allowing the font directories back still renders
//!   a different typeface, because font lookup needs *metadata* reads across the
//!   filesystem. See [`SANDBOX_PROFILE`].
//!
//! One worker serves exactly one document. That is a stronger isolation story
//! than multiplexing --- a document that kills its worker takes nothing else
//! with it --- and it is also what makes a worker restartable without a
//! reopening protocol.
//!
//! **Windows has none of this.** `sandbox_init` is SBPL and macOS-only, and no
//! Windows build of this repository has ever run. The module compiles there and
//! [`Worker::spawn`] refuses, rather than silently running unsandboxed.

use std::io::{BufRead, BufReader, Read, Write};
#[cfg(target_os = "macos")]
use std::os::fd::FromRawFd;
#[cfg(unix)]
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::process::{Child, ChildStdin, ChildStdout};
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// The argv marker that turns this executable into a worker.
///
/// Re-exec of `current_exe` rather than a second binary: a bundled `.app` would
/// otherwise need the worker installed beside it and found at runtime, and a
/// path that resolves in development and not in the bundle is exactly the class
/// of defect `AGENTS.md` already records for the PDFium library directory.
pub const WORKER_ARGV: &str = "--render-worker";

/// Why every worker entry point refuses off macOS.
///
/// One constant rather than the same sentence written at each refusal, because
/// the refusals are the containment argument and four copies of it are four
/// things to drift. Not a silent fallback to running unsandboxed: every claim in
/// `docs/THREAT-MODEL.md` is `sandbox_init` SBPL, so a worker without it is a
/// different thing wearing the same name.
#[cfg(not(any(target_os = "macos", windows)))]
pub const NO_WORKERS: &str = "render workers are implemented on macOS and Windows only";

/// Descriptor the document mapping is handed over on.
///
/// Fixed numbers because they must be agreed before `exec`, and there is no
/// channel at that point to negotiate on.
pub const DOC_FD: i32 = 3;
/// Descriptor the tile mapping is handed over on.
pub const TILE_FD: i32 = 4;
/// Descriptor a pre-spawned worker is handed its document over, later.
///
/// A worker started before any file is chosen cannot be given one at `exec`, so
/// it gets a socket instead and receives the mapping as `SCM_RIGHTS` ancillary
/// data once there is something to open. `bin/fdpass_probe.rs` is the standing
/// proof that this crosses a sandboxed boundary --- with the control that the
/// child cannot read `/etc/hosts` at the time, since the transfer works equally
/// well on a process that never sandboxed itself.
pub const SOCK_FD: i32 = 5;

/// The flag the document section's handle arrives on, on Windows.
///
/// Windows has no counterpart to [`DOC_FD`]: handles are inherited by *value*,
/// not by number, so there is nothing for the two sides to agree on in advance
/// and the value has to be told to the child. argv is where every Win32 sandbox
/// does this, and `bin/win_sandbox_probe.rs` measured it working under the
/// containment the worker uses.
///
/// A handle in argv is not authority anyone else can use: the value means nothing
/// in another process, and inheritance is what makes it live here. That is why
/// the *document* may travel this way while a *path* may not --- the path would be
/// authority a low-integrity child could act on, and the handle is not.
#[cfg(windows)]
pub const DOC_HANDLE_ARGV: &str = "--doc-handle";

/// The flag the tile section's handle arrives on, on Windows. See
/// [`DOC_HANDLE_ARGV`].
#[cfg(windows)]
pub const TILE_HANDLE_ARGV: &str = "--tile-handle";

/// The document handed to a Windows worker that was started without one.
///
/// The counterpart of the macOS `SCM_RIGHTS` handover, and it has to be a
/// different mechanism rather than a different encoding: a Windows handle is a
/// number in one process's table and means nothing in another, so there is no
/// value the parent could simply *name*. What crosses is a `DuplicateHandle`
/// into the running child, which is a write the parent performs on the child's
/// handle table --- allowed because the parent is the more privileged of the two,
/// and the direction low integrity does not block. `handle` is therefore already
/// the child's number by the time this message says it.
///
/// **A message of its own rather than a [`Request`] variant**, and the type is
/// the argument. A handover is legal exactly once, before there is a document;
/// `Request` is the vocabulary of a worker that already has one. Folding it in
/// would make "adopt a second document" something the child has to *refuse* at
/// runtime, where keeping it out makes it something that cannot be said. It is
/// read off the same pipe requests later arrive on, at the one point in the
/// child's life where nothing else is reading that pipe.
#[cfg(windows)]
#[derive(Serialize, Deserialize)]
pub struct Handover {
    /// The document section, as a handle in the *child's* table.
    pub handle: usize,
    /// How much of it to map. A handle says nothing about length.
    pub len: usize,
}

/// The argv marker that starts a worker with no document.
pub const PRESPAWN_ARGV: &str = "--prespawn";

/// Whether this build can start a worker before a document has been chosen.
///
/// Exported because callers *outside* this module have to branch on it --- a
/// probe skips its spare checks, a harness stops waiting for a spare that can
/// never appear --- and the alternative is each of them restating the platform
/// list. That is not hypothetical: `backend-probe` restated it, the two copies
/// disagreed for one day, and the half that was missing produced a five-second
/// wait that read as a defect in the pool. See the trap.
///
/// Kept beside [`Worker::prespawn`] and sharing its `cfg` exactly, so the
/// constant and the refusal cannot say different things.
pub const PRESPAWNS: bool = cfg!(any(target_os = "macos", windows));

/// Bytes reserved for one tile payload.
///
/// 2048² RGBA is 16 MB, and `AGENTS.md` measures 1024²--2048² as the useful tile
/// range --- smaller tiles multiply PDFium's ~1 s per-call constant rather than
/// dividing the work. A payload that does not fit is refused rather than
/// truncated.
pub const TILE_CAPACITY: usize = 16 * 1024 * 1024;

/// The largest tile the viewer asks for must fit, checked at compile time.
///
/// This began as a `#[test]`, and clippy was right to reject it: both sides are
/// constants, so it could not fail at runtime any more than `2 + 2 == 4` can.
/// A check nothing can break is a check to delete or to move somewhere it means
/// something --- here that is the compiler, where it also cannot drift.
const _: () = assert!(TILE_CAPACITY >= 2048 * 2048 * 4);

/// The longest reply line the parent will read.
///
/// The worker is ours, but it is the process holding the attacker's document, so
/// its replies are the one thing crossing back from where the hostile input is.
/// `read_line` on a pipe is unbounded: a worker that has been made to emit an
/// endless line takes the *parent* down with it, which is precisely the failure
/// the boundary exists to prevent — the isolation would be perfect and the app
/// would still die.
///
/// Generous rather than tight, because a legitimate reply can be large: a dense
/// page's characters and boxes are hundreds of kilobytes of JSON, and a 10,000-
/// entry outline is a few megabytes. Tile pixels do not travel this way at all.
pub const MAX_REPLY_BYTES: u64 = 32 * 1024 * 1024;

/// The sandbox the worker applies to itself before touching a document.
///
/// Verified **pixel-identical** to an unsandboxed render on base-14, TrueType
/// and CID documents and on the 775-page corpus. That verification is the whole
/// point: an earlier profile returned `ok` and drew a different font, with about
/// the same amount of ink, and nothing in the return value said so.
///
/// `file-read-metadata` is the line that looks wrong and is load-bearing ---
/// font lookup stats paths across the filesystem, and denying that is what makes
/// PDFium substitute silently. The residual is that a hostile document can learn
/// which paths exist; it cannot read one, write one, or open a socket.
pub const SANDBOX_PROFILE: &str = "\
(version 1)
(allow default)
(deny network*)
(deny file-write*)
(deny file-read*)
(allow file-read-metadata)
(allow file-read-data (subpath \"/System/Library/Fonts\") (subpath \"/Library/Fonts\"))
";

// ------------------------------------------------------------------ protocol

/// A request from the parent, one JSON object per line on the worker's stdin.
///
/// Deliberately carries no path, no descriptor and no pointer: everything the
/// worker may touch was handed to it before it dropped its authority.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Request {
    /// Parse the mapped document and report its geometry.
    Open {
        /// Collect only page 1's size instead of the whole table. Enumerating
        /// 775 pages costs 86 ms and buys a scrollbar exactness the scroller
        /// estimates anyway (PLAN §4).
        lazy_geometry: bool,
    },
    /// Render one tile into the shared mapping.
    Tile {
        /// Identity this request may be withdrawn by. Zero is not withdrawable.
        rid: u64,
        page: u32,
        scale: f32,
        turns: u8,
        invert: bool,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
        /// Whether to PNG-encode in the worker rather than send raw pixels.
        png: bool,
    },
    /// Abandon a tile request, whether or not it has started.
    ///
    /// Handled on the worker's reader thread rather than in the request queue,
    /// because the point of it is to reach a render that is *already running* ---
    /// a queued withdrawal would arrive after the thing it withdraws.
    Withdraw { rid: u64 },
    /// Extract one page's characters and their positions.
    Text { page: u32 },
    /// Find a query's occurrences on one page.
    Search {
        page: u32,
        query: String,
        /// How to match. Defaulted so that a request written before the options
        /// existed still parses as the unrestricted search it meant.
        #[serde(default)]
        options: crate::search::Options,
    },
    /// Read the document's outline.
    Outline,
}

/// A reply, one JSON object per line on the worker's stdout.
///
/// Payloads travel through the shared mapping, never inline: measured at
/// 0.11 ms against 0.61 ms down the pipe for 4 MB, and the mapping is where
/// PDFium renders to directly, so the pixel path has no copy in it at all.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Response {
    pub ok: bool,
    #[serde(default)]
    pub error: String,
    /// Bytes written into the tile mapping, for a payload-bearing reply.
    #[serde(default)]
    pub bytes: usize,
    /// Set when a tile was withdrawn rather than rendered.
    ///
    /// Distinct from an error and from an empty tile: there is nothing to draw,
    /// and a caller that painted this as blank would erase content it had.
    #[serde(default)]
    pub abandoned: bool,
    /// JSON for a structured reply --- geometry, text, matches, an outline.
    #[serde(default)]
    pub json: Option<serde_json::Value>,
    /// Time inside PDFium.
    #[serde(default)]
    pub render_us: u64,
    /// Time spent encoding, zero for raw pixels.
    #[serde(default)]
    pub encode_us: u64,
}

impl Response {
    /// A failure carrying a diagnosable message.
    #[must_use]
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: message.into(),
            ..Default::default()
        }
    }

    /// A success carrying a structured payload.
    ///
    /// # Errors
    ///
    /// Serialisation failure, which is a bug rather than a document problem and
    /// is reported as such rather than being unwrapped.
    pub fn json<T: Serialize>(value: &T) -> Self {
        match serde_json::to_value(value) {
            Ok(json) => Self {
                ok: true,
                json: Some(json),
                ..Default::default()
            },
            Err(e) => Self::err(format!("could not serialise a reply: {e}")),
        }
    }
}

// ------------------------------------------------------------- shared memory

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
/// way [`PreWorker`]/[`WarmWorker`] carry the readiness handshake --- and that
/// fails here, because [`Worker`] holds a `Shm`, so an uninhabited mapping makes
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
/// for [`DOC_FD`] and [`TILE_FD`] to name.
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
    /// `bin/win_sandbox_probe.rs`.
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

/// Splits a length into the high and low halves `CreateFileMapping` wants.
///
/// Separate and tested because the shift is the kind of arithmetic that is
/// wrong only above 4 GB, where no ordinary run would notice.
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
unsafe fn duplicate_into(
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

// ------------------------------------------------------- handing over a file

/// How a child died, in words.
///
/// Free rather than a method, because both [`Worker`] and [`PreWorker`] need it
/// and a second copy is a second thing to drift. A signal is named as one:
/// `AGENTS.md` records a crash test that reported "exited with code 9" where a
/// segfault should have said "killed by signal 11", and that difference was the
/// whole tell.
#[cfg(unix)]
fn epitaph_of(child: &mut Child) -> String {
    use std::os::unix::process::ExitStatusExt;
    match child.try_wait() {
        Ok(None) => "still running".into(),
        Ok(Some(status)) => match status.signal() {
            Some(signal) => format!("killed by signal {signal}"),
            None => format!("exited with code {}", status.code().unwrap_or(-1)),
        },
        Err(e) => format!("could not be waited on: {e}"),
    }
}

/// How a child died, in words --- off unix.
///
/// No signal arm, and that is a real difference rather than a stub: Windows does
/// not kill a process with a signal, so there is no equivalent of the "killed by
/// signal 11" tell the unix version exists to preserve. A crash arrives as an
/// exit code, and reporting one as the other would be the exact confusion
/// `AGENTS.md` records this function was written to avoid.
///
/// Excluded on Windows, where [`Worker::epitaph`] delegates to
/// [`Contained::epitaph`](crate::sandbox_win::Contained::epitaph) instead --- a
/// worker there is not a `std::process::Child`, so this arm would have no caller
/// and `-D warnings` would reject it as dead.
#[cfg(not(any(unix, windows)))]
fn epitaph_of(child: &mut Child) -> String {
    match child.try_wait() {
        Ok(None) => "still running".into(),
        Ok(Some(status)) => format!("exited with code {}", status.code().unwrap_or(-1)),
        Err(e) => format!("could not be waited on: {e}"),
    }
}

/// Whether a temporary descriptor from the pre-`exec` shuffle is only that.
///
/// Between `fork` and `exec` each mapping is `dup`'d to a scratch number and
/// then `dup2`'d onto the number the child expects, because the source may
/// already *be* one of those numbers. The scratch copy is closed afterwards ---
/// except when it is not a scratch copy at all.
///
/// `dup` returns the **lowest free** descriptor, and the trap is that "lowest
/// free" can be a number the shuffle is about to install on. With the document
/// mapping on fd 3, the tile mapping on fd 5 and a hole at fd 4, `dup(3)`
/// returns **4**, which is [`TILE_FD`]: the tile's own `dup2` then installs the
/// tile there, correctly, and closing the document's "temporary" afterwards
/// closes the tile the child is about to be handed. The child starts with a
/// descriptor that names nothing, on a number the protocol says is a 16 MB
/// mapping, and every later diagnosis points at the mapping rather than at the
/// fork.
///
/// So a temporary is compared against **every** number the shuffle installs, not
/// only against its own target --- and the list it is compared against is the
/// same array that drives the `dup2` calls, so there is no second copy of the
/// target set to fall out of step with them. A temporary that equals a target
/// *is* the installed descriptor by then; there is nothing left to close, and
/// nothing leaks by keeping it.
#[cfg(target_os = "macos")]
fn is_scratch(fd: i32, shuffle: &[(i32, i32)]) -> bool {
    !shuffle.iter().any(|(_, target)| *target == fd)
}

/// A connected pair, one half of which is handed to a pre-spawned worker.
#[cfg(target_os = "macos")]
fn socket_pair() -> Result<(OwnedFd, OwnedFd), String> {
    let mut fds = [0i32; 2];
    // SAFETY: writes exactly two descriptors into a two-element array.
    let rc = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(format!("socketpair: {}", std::io::Error::last_os_error()));
    }

    // Close-on-exec on **both** ends, and this is not hygiene --- without it a
    // pre-spawned worker never dies.
    //
    // A spare blocks in `recvmsg` on this socket, so unlike a document-serving
    // worker it is not reading stdin and cannot notice the parent going away that
    // way. What should end it is the socket reaching EOF when the parent's end
    // closes. But `socketpair` descriptors are not close-on-exec, so every child
    // spawned afterwards inherits a copy and holds the write end open --- and the
    // spare therefore waits forever, reparented to init, on a socket that will
    // never close because a sibling has it.
    //
    // The symptom is a pile of orphaned `--prespawn` processes that outlive every
    // run, which is what the process table showed: eighteen of them, some seconds
    // old. `Drop` does not help here, because `std::process::exit` runs no
    // destructors and every probe and the app itself exit that way.
    //
    // `dup2` clears the flag on the descriptor it creates, so the child still
    // receives a usable socket on `SOCK_FD`.
    for fd in fds {
        // SAFETY: both descriptors were just created by `socketpair`.
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            let e = std::io::Error::last_os_error();
            // SAFETY: closing descriptors this function owns and is abandoning.
            unsafe {
                libc::close(fds[0]);
                libc::close(fds[1]);
            }
            return Err(format!(
                "could not set FD_CLOEXEC on a handover socket: {e}"
            ));
        }
    }
    // SAFETY: both are fresh descriptors this process owns.
    unsafe { Ok((OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))) }
}

/// Sends a document mapping's descriptor, with its length as the payload.
///
/// The length travels in the ordinary payload rather than in a second message
/// because a descriptor carries no notion of how much of it to map, and two
/// messages could be interleaved by a future caller in a way one cannot.
///
/// A byte of payload is required, not incidental: a `sendmsg` carrying only
/// ancillary data may transfer nothing at all, and the receiver then blocks
/// forever on a message that was never framed.
#[cfg(target_os = "macos")]
fn send_document(socket: i32, fd: i32, len: usize) -> Result<(), String> {
    let mut payload = (len as u64).to_le_bytes();
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    let mut space = [0u8; 32];
    // SAFETY: the control buffer is sized by CMSG_SPACE for one descriptor, and
    // every pointer is into storage that outlives the call.
    unsafe {
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &raw mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = space.as_mut_ptr().cast();
        msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32);

        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if cmsg.is_null() {
            return Err("no control header".into());
        }
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<i32>() as u32);
        std::ptr::copy_nonoverlapping(&raw const fd, libc::CMSG_DATA(cmsg).cast::<i32>(), 1);

        if libc::sendmsg(socket, &raw const msg, 0) < 0 {
            return Err(format!("sendmsg: {}", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

/// Receives a document mapping's descriptor and its length.
///
/// # Errors
///
/// The socket closing --- which is how a pre-spawned worker learns the parent has
/// gone away without ever giving it a file --- or a message that is not the one
/// this protocol sends.
///
/// # Safety
///
/// The caller must own `socket` and must not be reading it concurrently.
#[cfg(target_os = "macos")]
pub unsafe fn recv_document(socket: i32) -> Result<(OwnedFd, usize), String> {
    let mut payload = [0u8; 8];
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    let mut space = [0u8; 32];
    // SAFETY: as `send_document`; the control header is only read once `recvmsg`
    // has reported success.
    unsafe {
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &raw mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = space.as_mut_ptr().cast();
        msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32);

        let read = libc::recvmsg(socket, &raw mut msg, 0);
        if read < 0 {
            return Err(format!("recvmsg: {}", std::io::Error::last_os_error()));
        }
        if read == 0 {
            return Err("the parent closed the handover socket".into());
        }
        // Checked rather than assumed: a short read leaves the rest of `payload`
        // zeroed, and a length of zero is a mapping of nothing that would fail
        // much further along with a far worse message.
        if read as usize != payload.len() {
            return Err(format!("the handover payload was {read} bytes, wanted 8"));
        }
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if cmsg.is_null() {
            return Err("no descriptor arrived with the handover".into());
        }
        if (*cmsg).cmsg_level != libc::SOL_SOCKET || (*cmsg).cmsg_type != libc::SCM_RIGHTS {
            return Err("the handover control message was not SCM_RIGHTS".into());
        }
        let mut fd: i32 = -1;
        std::ptr::copy_nonoverlapping(libc::CMSG_DATA(cmsg).cast::<i32>(), &raw mut fd, 1);
        if fd < 0 {
            return Err("the descriptor that arrived is not valid".into());
        }
        let len = usize::try_from(u64::from_le_bytes(payload))
            .map_err(|_| "the handover length does not fit in this address space".to_string())?;
        if len == 0 {
            return Err("the handover length is zero".into());
        }
        Ok((OwnedFd::from_raw_fd(fd), len))
    }
}

// -------------------------------------------------------------------- parent

/// The child process a [`Worker`] owns.
///
/// Per-platform because the two are not the same object. On unix a worker is an
/// ordinary `std::process::Child` that sandboxed *itself* after `exec`. On
/// Windows it is a [`Contained`](crate::sandbox_win::Contained): a process inside
/// a **job object**, where the job is the containment, the parent applied it
/// before the child ran an instruction, and ending the worker means terminating
/// the job rather than the process.
///
/// Aliases rather than an enum spanning both. An enum would put a runtime match
/// on every call in a struct that can only ever hold one of them, and --- the
/// reason that actually decided it --- adding the Windows arm would have edited
/// every macOS line in this file, so nothing here could be re-verified on macOS
/// by reading a diff.
#[cfg(not(windows))]
type WorkerProcess = Child;
/// The child process a [`Worker`] owns. See the other arm.
#[cfg(windows)]
type WorkerProcess = crate::sandbox_win::Contained;

/// What the parent writes requests to.
///
/// A `File` on Windows, because the child is not a `std::process::Child` and so
/// there is no `ChildStdin` to take from it: the pipe end arrives from
/// `CreatePipe` as a bare handle, and `File` is the standard library's owner for
/// one. It writes, it closes on drop, and closing it is what the child sees as
/// end of input --- all three of which this needs.
#[cfg(not(windows))]
type WorkerStdin = ChildStdin;
/// What the parent writes requests to. See the other arm.
#[cfg(windows)]
type WorkerStdin = std::fs::File;

/// What the parent reads replies from. See [`WorkerStdin`].
#[cfg(not(windows))]
type WorkerStdout = ChildStdout;
/// What the parent reads replies from. See [`WorkerStdin`].
#[cfg(windows)]
type WorkerStdout = std::fs::File;

/// A worker process serving one document.
pub struct Worker {
    child: WorkerProcess,
    stdin: WorkerSender,
    stdout: BufReader<WorkerStdout>,
    /// Where tile payloads arrive.
    pub tile: Shm,
    /// Kept mapped for the worker's lifetime --- it is the document.
    ///
    /// Shared rather than owned so that a caller replacing a dead worker can
    /// hand the replacement the **same bytes**. Re-reading the path would work
    /// and is subtly wrong: a file rewritten in the meantime would become the
    /// document the reader is looking at, silently, under a scroller sized for
    /// the old one.
    ///
    /// `None` only while a [`PreWorker`] is waiting to be given one.
    _doc: Option<Arc<Shm>>,
}

/// The write half of a worker, separable and cheap to clone.
///
/// It exists for exactly one caller: a withdrawal has to reach a render that is
/// **already running**, and the thread that would send it is not the thread
/// waiting on the reply --- that one is blocked inside [`Worker::call`]. So the
/// two halves cannot both live behind the same `&mut`.
///
/// The lock is held for one write and released before any read, which is what
/// keeps a withdrawal from being blocked behind the reply it is trying to
/// pre-empt.
#[derive(Clone)]
pub struct WorkerSender(Arc<Mutex<WorkerStdin>>);

impl WorkerSender {
    /// Writes one request line.
    ///
    /// # Errors
    ///
    /// The pipe being closed, i.e. the worker is gone. Reported without an
    /// epitaph, because reaping the child needs the [`Worker`] this was split
    /// from and a caller holding only the sender has no business waiting on it.
    pub fn send(&self, request: &Request) -> Result<(), String> {
        self.write_line(request)
    }

    /// Writes one JSON line of whatever the child is expecting next.
    ///
    /// Generic over the message because the pipe carries two vocabularies in
    /// sequence: on Windows a pre-spawned worker reads a [`Handover`] first and
    /// [`Request`]s forever after. Sharing the write is what keeps the framing
    /// --- one line, flushed --- from being stated twice and drifting once.
    ///
    /// # Errors
    ///
    /// Serialising, or the pipe being closed.
    fn write_line<T: Serialize>(&self, value: &T) -> Result<(), String> {
        let mut line = serde_json::to_string(value).map_err(|e| e.to_string())?;
        line.push('\n');
        let mut stdin = self.0.lock().unwrap_or_else(|e| e.into_inner());
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|e| format!("worker stdin: {e}"))
    }

    /// Withdraws a tile request.
    ///
    /// # Errors
    ///
    /// As [`WorkerSender::send`].
    pub fn withdraw(&self, rid: u64) -> Result<(), String> {
        self.send(&Request::Withdraw { rid })
    }
}

/// A worker that is running, sandboxed and warmed, and has no document yet.
///
/// This is the whole point of pre-spawning. `bin/prespawn_bench.rs` measures what
/// a worker costs before it can answer anything: a **~6.6 ms floor** --- fork,
/// exec, dyld, mapping libpdfium, `sandbox_init` --- plus **~7.4 ms of system-font
/// enumeration** on any document that does not embed its fonts, plus the page
/// parse. The first two are paid by every worker and depend on no document, so
/// they can be spent while the shell is still coming up (~250 ms, of which none
/// is ours) instead of while a reader waits for their first page.
///
/// Kept as a distinct type rather than a `Worker` with an empty document,
/// because the two cannot do the same things: this one has no document to render
/// from, and a `Worker` cannot be given a second one. Making that a state machine
/// inside one struct would put a runtime check where the compiler is willing to
/// do it.
pub struct PreWorker {
    /// The running process, with no document yet.
    ///
    /// Held as a whole `Worker` rather than as its parts, and that is the fix for
    /// a real defect rather than a tidiness preference. `std::process::Child`
    /// does **not** kill on drop, so an unused spare otherwise outlives the
    /// service that made it -- reparented to init, blocked forever in `recvmsg`,
    /// still holding the stderr it inherited. The symptom is not a stray process
    /// anyone notices: it is that whatever captures the parent's output waits on
    /// a pipe an orphan still holds, so a run that finished cleanly looks hung.
    /// `backend-probe` appeared to wedge on its first corpus exactly this way.
    ///
    /// A second `Drop` here would have worked and could drift from the first.
    /// Containing a `Worker` makes the kill impossible to forget, because it is
    /// not written twice.
    worker: Worker,
    /// Our end of the pair the document descriptor is sent over.
    ///
    /// Gated with the handover itself, and macOS is now the only platform that
    /// needs a field at all: the Windows handover is a `DuplicateHandle` into the
    /// child followed by a line down the request pipe, and both of those are
    /// reachable from `worker` above. Carrying an unused descriptor elsewhere
    /// would be a resource that exists only to be warned about.
    #[cfg(target_os = "macos")]
    socket: OwnedFd,
}

impl PreWorker {
    /// Blocks until the child has linked, sandboxed and warmed itself.
    ///
    /// **Consumes the `PreWorker`, and that is the point.** The readiness line
    /// has to be off the pipe before any real request, or it becomes the answer
    /// to whichever one is asked first --- an `Open` reply that is actually a
    /// readiness notice, carrying geometry nobody sent. Returning a distinct
    /// type is what makes that ordering hold by construction: [`WarmWorker`] is
    /// the only thing that can be handed a document, and this is the only way to
    /// obtain one.
    ///
    /// It was a `&mut self` call inside `adopt` first, guarded by an
    /// `is_it_warm` flag. Deleting that call changed **nothing** anywhere --- no
    /// check, no benchmark --- because `Workers::prewarm` already warms on its
    /// own thread and publishes a spare only if it succeeded. That is
    /// unreachable defence in the sense `AGENTS.md` records, with the extra
    /// wrinkle that the impossibility was enforced in a *different module*, so
    /// simply deleting the line would have made this file's correctness depend
    /// silently on a policy decision in `render.rs`. Encoding it in the type
    /// keeps the guarantee and removes the code.
    ///
    /// Waiting is also deliberately *not* folded into the handover: a pool
    /// filling itself in the background wants to know a worker is ready without
    /// having a document for it, and a benchmark must be able to put the wait
    /// outside its timer --- otherwise the head start a pre-spawned worker gets
    /// is whatever the code before it happened to take, which is not a quantity
    /// anyone chose.
    ///
    /// # Errors
    ///
    /// The worker dying before it was ready, or answering something other than
    /// its readiness.
    pub fn wait_warm(mut self) -> Result<WarmWorker, String> {
        let ready = self
            .worker
            .read_reply()
            .map_err(|e| format!("a pre-spawned worker was not ready: {e}"))?;
        if !ready.ok {
            return Err(format!(
                "a pre-spawned worker failed to warm: {}",
                ready.error
            ));
        }
        let warm = ready
            .json
            .as_ref()
            .and_then(|j| j.get("prespawn"))
            .and_then(serde_json::Value::as_str);
        // Asserted positively. "Not an error" would be satisfied by any reply at
        // all, including a real one if the ordering here ever changed.
        if warm != Some("warm") {
            return Err(format!(
                "a pre-spawned worker sent {warm:?} where its readiness was expected"
            ));
        }
        Ok(WarmWorker { pre: self })
    }

    /// The process id, for a probe that wants to prove one exists.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.worker.pid()
    }
}

/// A pre-spawned worker whose readiness has been consumed, ready for a document.
///
/// The only route here is [`PreWorker::wait_warm`], which is the whole reason
/// the type exists --- see its note.
pub struct WarmWorker {
    pre: PreWorker,
}

impl WarmWorker {
    /// Hands over the document and returns the worker now serving it.
    ///
    /// The mapping is sent as a descriptor rather than a path, for exactly the
    /// reason the whole design exists: the worker has already dropped the
    /// authority to open a file, so a path would be unusable even if it were
    /// trusted. The length travels as the ordinary payload of the same message,
    /// since a descriptor says nothing about how much of it to map.
    ///
    /// # Errors
    ///
    /// The send failing, or the worker having died while it waited --- reported
    /// with its epitaph, because "the pipe is closed" and "killed by signal 11"
    /// are different diagnoses.
    #[cfg(target_os = "macos")]
    pub fn adopt(mut self, doc: Arc<Shm>) -> Result<Worker, String> {
        send_document(self.pre.socket.as_raw_fd(), doc.raw_fd(), doc.len()).map_err(|e| {
            format!(
                "could not hand the document to a pre-spawned worker: {e} --- {}",
                self.pre.worker.epitaph()
            )
        })?;

        // Kept mapped for as long as the worker lives. Until this line the worker
        // had no document; from here it is an ordinary one in every respect.
        self.pre.worker._doc = Some(doc);
        Ok(self.pre.worker)
    }

    /// Hands over the document and returns the worker now serving it.
    ///
    /// The Windows counterpart, and the difference from the macOS arm is where
    /// the authority is exercised. There the parent *sends* a descriptor and the
    /// kernel installs it in the receiver; here the parent **writes into the
    /// child's handle table** with `DuplicateHandle` and then tells the child
    /// which number it wrote. That direction is the one integrity levels permit
    /// --- a medium-integrity parent may reach into a low-integrity child, never
    /// the reverse --- so the handover survives the containment for the same
    /// structural reason the macOS one does, not by coincidence.
    ///
    /// The child's copy is left owned by the child: it is duplicated, not moved,
    /// and closing the parent's own handle is [`Shm`]'s business as before.
    ///
    /// # Errors
    ///
    /// The duplication failing, or the send failing --- reported with the child's
    /// epitaph, since "the pipe is closed" and "died at the loader" are different
    /// diagnoses.
    #[cfg(windows)]
    pub fn adopt(mut self, doc: Arc<Shm>) -> Result<Worker, String> {
        // SAFETY: a live process handle owned by the child struct, and a live
        // section handle owned by `doc`, which outlives this call.
        let handle = unsafe {
            duplicate_into(self.pre.worker.child.process, doc.raw_handle()).map_err(|e| {
                format!(
                    "could not reach a pre-spawned worker's handle table: {e} --- {}",
                    self.pre.worker.epitaph()
                )
            })?
        };
        let len = doc.len();
        self.pre
            .worker
            .stdin
            .write_line(&Handover { handle, len })
            .map_err(|e| {
                format!(
                    "could not hand the document to a pre-spawned worker: {e} --- {}",
                    self.pre.worker.epitaph()
                )
            })?;

        // Kept mapped for as long as the worker lives, as on macOS.
        self.pre.worker._doc = Some(doc);
        Ok(self.pre.worker)
    }

    /// Hands over the document --- refused where there is no worker at all.
    ///
    /// Unreachable in practice, since [`Worker::prespawn`] refuses first and is
    /// the only route to a `WarmWorker`. Present so the type's surface does not
    /// change by platform, and refusing rather than panicking so a future caller
    /// that finds another route is told rather than aborted.
    ///
    /// # Errors
    ///
    /// Always.
    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn adopt(self, _doc: Arc<Shm>) -> Result<Worker, String> {
        Err(NO_WORKERS.into())
    }

    /// The process id, for a probe that wants to prove one exists.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.pre.pid()
    }
}

impl Worker {
    /// Spawns a worker holding this document, sandboxed.
    ///
    /// # Errors
    ///
    /// Mapping the document, creating the tile buffer, or spawning; and on any
    /// platform but macOS, always --- see the module note.
    pub fn spawn(document: &Path, library_dir: &Path) -> Result<Self, String> {
        Self::spawn_shared(Arc::new(Shm::map_file(document)?), library_dir)
    }

    /// Starts a worker with no document, to be given one later.
    ///
    /// # Errors
    ///
    /// Creating the tile buffer, the socket pair, or spawning; and on any
    /// platform but macOS and Windows, always --- see the module note.
    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn prespawn(_library_dir: &Path) -> Result<PreWorker, String> {
        Err(NO_WORKERS.into())
    }

    /// Starts a worker with no document, to be given one later.
    ///
    /// Identical in purpose to the macOS arm and different in one mechanism: the
    /// document arrives by [`Handover`] over the request pipe rather than as
    /// ancillary data on a socket, so there is no socket to make here and the
    /// child is spawned with a tile handle and nothing else.
    ///
    /// What it does **not** change is when containment happens. The child is
    /// created suspended, dropped to low integrity and put in its job before it
    /// runs an instruction, exactly as a document-carrying worker is --- a
    /// pre-spawned worker is not a worker that gets contained later.
    ///
    /// # Errors
    ///
    /// Creating the tile buffer, the pipes, containing or spawning the child.
    #[cfg(windows)]
    pub fn prespawn(library_dir: &Path) -> Result<PreWorker, String> {
        let tile = Shm::create(TILE_CAPACITY)?;
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let worker = Self::spawn_contained_worker(
            &[
                &exe.to_string_lossy(),
                WORKER_ARGV,
                PRESPAWN_ARGV,
                "--lib",
                &library_dir.to_string_lossy(),
                TILE_HANDLE_ARGV,
                &tile.raw_handle().to_string(),
            ],
            &[tile.raw_handle() as windows_sys::Win32::Foundation::HANDLE],
            tile,
            None,
        )?;
        Ok(PreWorker { worker })
    }

    /// Starts a worker with no document, to be given one later.
    ///
    /// Returns as soon as `fork`/`exec` has been issued --- like [`Worker::spawn`],
    /// it waits for nothing. The child then links, maps PDFium, sandboxes itself
    /// and warms the font list, and none of that is on anyone's critical path
    /// unless a document arrives before it finishes.
    ///
    /// # Errors
    ///
    /// Creating the tile buffer, the socket pair, or spawning.
    #[cfg(target_os = "macos")]
    pub fn prespawn(library_dir: &Path) -> Result<PreWorker, String> {
        use std::os::unix::process::CommandExt;

        let tile = Shm::create(TILE_CAPACITY)?;
        let (ours, theirs) = socket_pair()?;

        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let mut command = Command::new(exe);
        command
            .arg(WORKER_ARGV)
            .arg(PRESPAWN_ARGV)
            .arg("--lib")
            .arg(library_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited, as for a document-carrying worker: a sandbox denial or
            // a fatal signal has to be visible.
            .stderr(Stdio::inherit());

        let tile_fd = tile.raw_fd();
        let sock_fd = theirs.as_raw_fd();
        // SAFETY: only dup/dup2/close run between fork and exec, all of which are
        // async-signal-safe. Both sources are dup'd to fresh descriptors first,
        // because either may already occupy the target number.
        unsafe {
            command.pre_exec(move || {
                let t = libc::dup(tile_fd);
                let s = libc::dup(sock_fd);
                if t < 0 || s < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // One array, driving both the installs and the cleanup, so the
                // set of numbers being installed on cannot drift from the set
                // the cleanup protects. See [`is_scratch`] for what closing the
                // wrong one costs --- here it is the handover socket, and a
                // spare that never receives a document waits forever.
                let shuffle = [(t, TILE_FD), (s, SOCK_FD)];
                for (temp, target) in shuffle {
                    if libc::dup2(temp, target) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                for (temp, _) in shuffle {
                    if is_scratch(temp, &shuffle) {
                        libc::close(temp);
                    }
                }
                Ok(())
            });
        }

        let mut child = command.spawn().map_err(|e| format!("spawn failed: {e}"))?;
        let stdin = child.stdin.take().ok_or("worker has no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("worker has no stdout")?);
        Ok(PreWorker {
            worker: Worker {
                child,
                stdin: WorkerSender(Arc::new(Mutex::new(stdin))),
                stdout,
                tile,
                _doc: None,
            },
            socket: ours,
        })
    }

    /// Spawns a worker over a document mapping the caller made and keeps.
    ///
    /// The keeping is the point. A worker that dies is replaced by calling this
    /// again with the same `Arc`, so the replacement parses the bytes the first
    /// one did rather than whatever is at that path now --- and a 337 MB scan is
    /// not read a second time to find that out.
    ///
    /// # Errors
    ///
    /// Creating the tile buffer or spawning; and on any platform but macOS,
    /// always --- see the module note.
    pub fn spawn_shared(doc: Arc<Shm>, library_dir: &Path) -> Result<Self, String> {
        let tile = Shm::create(TILE_CAPACITY)?;
        Self::spawn_mapped(doc, tile, library_dir)
    }

    /// Spawns a worker over mappings the caller already made.
    ///
    /// # Errors
    ///
    /// As [`Worker::spawn`].
    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn spawn_mapped(_doc: Arc<Shm>, _tile: Shm, _library_dir: &Path) -> Result<Self, String> {
        // Not a silent fallback to running unsandboxed. Every containment claim
        // in docs/THREAT-MODEL.md is a named boundary --- `sandbox_init` SBPL on
        // macOS, a low-integrity token inside a job object on Windows --- so a
        // worker without one is a different thing wearing the same name.
        Err(NO_WORKERS.into())
    }

    /// Spawns a worker over mappings the caller already made, contained.
    ///
    /// The Windows counterpart of the macOS arm below, and the differences are
    /// all forced by what the two kernels offer rather than chosen:
    ///
    /// - **The child is contained by its parent, not by itself.** There is no
    ///   `sandbox_init` to call after `exec`, so the token is dropped to low
    ///   integrity and the job object applied *here*, while the child is still
    ///   suspended. That is why it cannot be a `std::process::Command`: nothing
    ///   in the standard library creates a process suspended, and a job applied
    ///   to a process already running is a race the process can win.
    /// - **The mappings travel by inherited handle, named in argv**, because
    ///   Windows inherits handles by value and there is no descriptor number for
    ///   the two sides to agree on in advance. A handle is not authority anyone
    ///   else can use --- see [`DOC_HANDLE_ARGV`].
    /// - **stdin and stdout are pipes we make**, since there is no `Child` to
    ///   take them from. The parent's ends are wrapped in `File` immediately, so
    ///   that every early return from here closes them instead of leaking four
    ///   handles per failed spawn.
    ///
    /// # Errors
    ///
    /// Creating the pipes, containing or spawning the child, or resuming it.
    #[cfg(windows)]
    pub fn spawn_mapped(doc: Arc<Shm>, tile: Shm, library_dir: &Path) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let handles = [
            doc.raw_handle() as windows_sys::Win32::Foundation::HANDLE,
            tile.raw_handle() as windows_sys::Win32::Foundation::HANDLE,
        ];
        Self::spawn_contained_worker(
            &[
                &exe.to_string_lossy(),
                WORKER_ARGV,
                "--doc-len",
                &doc.len().to_string(),
                "--lib",
                &library_dir.to_string_lossy(),
                DOC_HANDLE_ARGV,
                &doc.raw_handle().to_string(),
                TILE_HANDLE_ARGV,
                &tile.raw_handle().to_string(),
            ],
            &handles,
            tile,
            Some(doc),
        )
    }

    /// Spawns a contained child and wraps it as a worker.
    ///
    /// The shared half of [`Worker::spawn_mapped`] and [`Worker::prespawn`] on
    /// Windows, which differ only in their argv and in whether a document is
    /// inherited at `CreateProcess` or arrives later. Everything below --- the
    /// pipes, which end goes where, closing the child's ends in the parent, and
    /// resuming only once all of that is done --- is identical for both, and each
    /// line of it is load-bearing in a way a second copy would eventually get
    /// wrong.
    ///
    /// # Errors
    ///
    /// Creating the pipes, containing or spawning the child, or resuming it.
    #[cfg(windows)]
    fn spawn_contained_worker(
        args: &[&str],
        handles: &[windows_sys::Win32::Foundation::HANDLE],
        tile: Shm,
        doc: Option<Arc<Shm>>,
    ) -> Result<Self, String> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};

        use crate::sandbox_win::{pipe, spawn_contained, Containment, Stdio};

        let command = command_line(args);

        // Two pipes, four ends, and which end goes where is the whole protocol:
        // the child reads requests and writes replies, so it gets the *read* end
        // of one and the *write* end of the other. Handing over the wrong half
        // gives a worker that can read its own answers.
        let (requests_read, requests_write) = pipe()?;
        let (replies_read, replies_write) = pipe()?;
        // SAFETY: four fresh handles from `CreatePipe`, owned by nothing else.
        // Wrapped now rather than after the spawn so that an error between here
        // and there closes them --- `File`'s drop is the only cleanup path that
        // cannot be forgotten on a branch added later.
        let (stdin, stdout, child_stdin, child_stdout) = unsafe {
            (
                std::fs::File::from_raw_handle(requests_write.cast()),
                std::fs::File::from_raw_handle(replies_read.cast()),
                std::fs::File::from_raw_handle(requests_read.cast()),
                std::fs::File::from_raw_handle(replies_write.cast()),
            )
        };

        let stdio = Stdio::with_inherited_stderr(
            child_stdin.as_raw_handle().cast(),
            child_stdout.as_raw_handle().cast(),
        )?;
        let contained = spawn_contained(&command, handles, &Containment::default(), Some(&stdio))?;

        // Closed in the parent *before* the child runs. Not hygiene: while this
        // process holds a copy of the reply pipe's write end, that pipe never
        // reaches end of file, so a worker that dies looks to `read_reply` like
        // one that is taking a long time --- and the epitaph that would name the
        // crash is never asked for. The macOS arm gets this for free, since
        // `Command` closes the child's ends itself.
        drop(child_stdin);
        drop(child_stdout);

        contained.resume()?;
        Ok(Self {
            child: contained,
            stdin: WorkerSender(Arc::new(Mutex::new(stdin))),
            stdout: BufReader::new(stdout),
            tile,
            _doc: doc,
        })
    }

    /// Spawns a worker over mappings the caller already made.
    ///
    /// # Errors
    ///
    /// As [`Worker::spawn`].
    #[cfg(target_os = "macos")]
    pub fn spawn_mapped(doc: Arc<Shm>, tile: Shm, library_dir: &Path) -> Result<Self, String> {
        use std::os::unix::process::CommandExt;

        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let mut command = Command::new(exe);
        command
            .arg(WORKER_ARGV)
            .arg("--doc-len")
            .arg(doc.len().to_string())
            .arg("--lib")
            .arg(library_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited, so a sandbox denial or a fatal signal is visible.
            // A worker that dies silently is the hardest thing here to diagnose.
            .stderr(Stdio::inherit());

        let doc_fd = doc.raw_fd();
        let tile_fd = tile.raw_fd();
        // SAFETY: only dup/dup2/close run between fork and exec, all of which
        // are async-signal-safe. Both sources are dup'd to fresh descriptors
        // first, because either may already occupy the target number --- the
        // parent's own mapping files typically land on exactly fd 3 and 4.
        unsafe {
            command.pre_exec(move || {
                let d = libc::dup(doc_fd);
                let t = libc::dup(tile_fd);
                if d < 0 || t < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // As in `prespawn`: one array drives the installs and the
                // cleanup, so a temporary is never closed on the strength of
                // its own target alone. The parent's mapping files typically
                // land on exactly fd 3 and 4, which is what makes a temporary
                // landing on the *other* target a layout to expect rather than
                // a curiosity --- see [`is_scratch`].
                let shuffle = [(d, DOC_FD), (t, TILE_FD)];
                for (temp, target) in shuffle {
                    if libc::dup2(temp, target) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                for (temp, _) in shuffle {
                    if is_scratch(temp, &shuffle) {
                        libc::close(temp);
                    }
                }
                Ok(())
            });
        }

        let mut child = command.spawn().map_err(|e| format!("spawn failed: {e}"))?;
        let stdin = child.stdin.take().ok_or("worker has no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("worker has no stdout")?);
        Ok(Self {
            child,
            stdin: WorkerSender(Arc::new(Mutex::new(stdin))),
            stdout,
            tile,
            _doc: Some(doc),
        })
    }

    /// A handle another thread can withdraw through. See [`WorkerSender`].
    #[must_use]
    pub fn sender(&self) -> WorkerSender {
        self.stdin.clone()
    }

    /// Sends a request and reads its reply.
    ///
    /// # Errors
    ///
    /// The worker being gone, which is reported with its epitaph rather than as
    /// a bare pipe error --- "killed by signal 11" and "exited with code 9" are
    /// different diagnoses and only one of them is a crash.
    pub fn call(&mut self, request: &Request) -> Result<Response, String> {
        self.send(request)
            .map_err(|e| format!("{e} ({})", self.epitaph()))?;
        self.read_reply()
    }

    /// Reads one reply, for a caller that pipelined its requests.
    ///
    /// # Errors
    ///
    /// The worker being gone, reported with its epitaph; or a reply longer than
    /// [`MAX_REPLY_BYTES`], which leaves the stream mid-line and so kills the
    /// worker rather than trying to resynchronise on a boundary that is no
    /// longer where the protocol says it is.
    pub fn read_reply(&mut self) -> Result<Response, String> {
        match read_reply_line(&mut self.stdout, MAX_REPLY_BYTES) {
            Ok(reply) => {
                serde_json::from_str(&reply).map_err(|e| format!("unreadable reply {reply:?}: {e}"))
            }
            Err(ReplyError::Closed) => {
                Err(format!("worker stopped answering ({})", self.epitaph()))
            }
            Err(ReplyError::TooLong(limit)) => {
                self.kill();
                Err(format!("worker sent a reply longer than {limit} bytes"))
            }
            Err(ReplyError::Io(e)) => Err(format!("worker stdout: {e} ({})", self.epitaph())),
        }
    }

    /// Sends a request without waiting for a reply.
    ///
    /// # Errors
    ///
    /// The pipe being closed.
    pub fn send(&mut self, request: &Request) -> Result<(), String> {
        self.stdin.send(request)
    }

    /// The worker's process id.
    ///
    /// Identity for the parent's own bookkeeping --- which sender belongs to
    /// which worker in a pool --- and not a handle to act on: signalling by pid
    /// races a reaped child whose number has been reused.
    #[must_use]
    pub fn pid(&self) -> u32 {
        #[cfg(not(windows))]
        {
            self.child.id()
        }
        #[cfg(windows)]
        {
            self.child.pid
        }
    }

    /// Whether the process is still there.
    ///
    /// Asked of the kernel rather than inferred from a failed call, because the
    /// two are different diagnoses and only one of them is worth replacing a
    /// worker over: a live worker that answered with an error *answered*, and
    /// restarting it would hide a bug in the protocol behind a fresh process
    /// that gets the next question right.
    ///
    /// Reaps as a side effect, which is deliberate --- `try_wait` is what turns
    /// a zombie into an exit status [`Worker::epitaph`] can name.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// How the worker died, in words.
    ///
    /// A signal is named as one. `AGENTS.md` records a crash test that reported
    /// "exited with code 9" where a segfault should have said "killed by signal
    /// 11", and that difference was the whole tell.
    pub fn epitaph(&mut self) -> String {
        // On Windows the epitaph belongs to `Contained`, which already has to
        // produce one for callers that never reach a `Worker` --- the probe
        // binaries watch a child that has no protocol yet. Delegating rather
        // than growing a third `epitaph_of` arm keeps one implementation of the
        // rule that a live process is *not* diagnosed from its exit code.
        #[cfg(not(windows))]
        {
            epitaph_of(&mut self.child)
        }
        #[cfg(windows)]
        {
            self.child.epitaph()
        }
    }

    /// The worker's physical footprint in bytes, for supervision.
    ///
    /// macOS refuses `RLIMIT_AS`, `RLIMIT_DATA` and `RLIMIT_RSS` outright, so
    /// there is no memory bound available from the kernel and this poll is the
    /// substitute. Note what it cannot do: spike 0.5 measured that a burst
    /// smaller than interval x growth rate is invisible to *any* polling scheme,
    /// so bounding the inputs is the layer that catches those and is not
    /// optional.
    ///
    /// Footprint rather than RSS, because a footprint excludes clean file-backed
    /// pages --- an RSS bound would kill a worker for having its own document
    /// mapped.
    #[must_use]
    pub fn footprint(&self) -> Option<u64> {
        phys_footprint(self.pid())
    }

    /// Kills the worker and reaps it.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Why a reply could not be read.
#[derive(Debug)]
enum ReplyError {
    /// The pipe reached end of file: the worker is gone.
    Closed,
    /// The line exceeded the limit, which is reported rather than truncated ---
    /// a truncated line would deserialise as a *malformed* reply and send the
    /// diagnosis to the protocol rather than to the worker that ran away.
    TooLong(u64),
    Io(std::io::Error),
}

/// Reads one newline-terminated reply, refusing one longer than `limit`.
///
/// Separate from [`Worker::read_reply`] so that it can be tested at all: the
/// thing worth asserting here is what happens on input a live worker will not
/// produce, and the only way to hand that over is to call this with a reader
/// that is not a pipe.
fn read_reply_line(reader: &mut impl BufRead, limit: u64) -> Result<String, ReplyError> {
    let mut line = String::new();
    // `take` bounds the read itself rather than checking the length afterwards,
    // which is the difference between refusing a huge line and allocating it
    // first and then complaining about it.
    match reader.take(limit).read_line(&mut line) {
        Ok(0) => Err(ReplyError::Closed),
        Ok(_) if line.ends_with('\n') => Ok(line),
        // No newline, and the two reasons for that are different diagnoses. At
        // the limit, the line was still going. Short of it, the pipe ended
        // mid-reply --- a worker that died while writing --- and calling that
        // "longer than 32 MB" would send the reader off to look at a size limit
        // when what happened is a crash the epitaph can name.
        Ok(read) if read as u64 >= limit => Err(ReplyError::TooLong(limit)),
        Ok(_) => Err(ReplyError::Closed),
        Err(e) => Err(ReplyError::Io(e)),
    }
}

/// A process's physical footprint in bytes.
#[cfg(target_os = "macos")]
#[must_use]
pub fn phys_footprint(pid: u32) -> Option<u64> {
    // `rusage_info_t` is itself `void *`, so the declared third parameter reads
    // as a pointer to a pointer and is not: every caller in the SDK passes the
    // struct's own address. Passing the address of a pointer type-checks
    // cleanly, returns 0, and has the kernel write the struct over whatever
    // follows on the stack --- which presents as a footprint of zero for every
    // child and sends you off to check entitlements.
    // SAFETY: every field is an integer, so an all-zero value is valid.
    let mut info: libc::rusage_info_v0 = unsafe { std::mem::zeroed() };
    // SAFETY: the struct's address is passed, and RUSAGE_INFO_V0 is the flavour
    // it describes --- the oldest carrying `ri_phys_footprint`, so the least
    // likely to shift under a macOS update.
    let rc = unsafe {
        libc::proc_pid_rusage(
            pid as i32,
            libc::RUSAGE_INFO_V0,
            std::ptr::addr_of_mut!(info).cast::<libc::rusage_info_t>(),
        )
    };
    (rc == 0).then_some(info.ri_phys_footprint)
}

/// A process's physical footprint in bytes.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn phys_footprint(_pid: u32) -> Option<u64> {
    None
}

/// Where the worker's PDFium library lives, given the parent's own.
#[must_use]
pub fn library_dir_arg(args: &[String]) -> Option<PathBuf> {
    value_of(args, "--lib").map(PathBuf::from)
}

/// The document length the parent passed.
#[must_use]
pub fn doc_len_arg(args: &[String]) -> Option<usize> {
    value_of(args, "--doc-len").and_then(|v| v.parse().ok())
}

/// The document section handle the parent passed, on Windows.
///
/// `usize` rather than `i32`, for the reason [`Shm::raw_handle`] gives: a handle
/// is pointer-sized, and an `i32` would truncate one silently into a value that
/// still looks like a plausible handle.
#[cfg(windows)]
#[must_use]
pub fn doc_handle_arg(args: &[String]) -> Option<usize> {
    value_of(args, DOC_HANDLE_ARGV).and_then(|v| v.parse().ok())
}

/// The tile section handle the parent passed, on Windows.
#[cfg(windows)]
#[must_use]
pub fn tile_handle_arg(args: &[String]) -> Option<usize> {
    value_of(args, TILE_HANDLE_ARGV).and_then(|v| v.parse().ok())
}

/// Joins arguments into the single command line `CreateProcess` takes.
///
/// Windows has no `argv`. A process is given one string and **the child** splits
/// it, so quoting is the parent's job --- `std::process::Command` does this and
/// `spawn_contained` cannot use `Command`, so it is done here.
///
/// The rule is the one `CommandLineToArgvW` and the MSVC runtime implement, and
/// it is not "wrap in quotes if it has a space". A backslash is ordinary *except*
/// immediately before a quote, where it escapes; so a run of backslashes that
/// ends the argument must be doubled, or the closing quote we add becomes an
/// escaped quote and the argument swallows the next one. That case is not
/// exotic here: `--lib C:\Program Files\tpdf\` is a directory with a space and a
/// trailing separator, which is exactly the input that breaks a naive quoter.
///
/// The executable is passed through the same way even though argv[0] obeys a
/// simpler rule (quotes delimit, backslashes never escape). The two agree on
/// every string that can be a Windows path, since `"` is not a legal filename
/// character --- so the only divergence is unreachable.
#[cfg(windows)]
fn command_line(parts: &[&str]) -> String {
    let mut line = String::new();
    for part in parts {
        if !line.is_empty() {
            line.push(' ');
        }
        quote_arg(part, &mut line);
    }
    line
}

/// Appends one argument to a command line, quoted if it needs to be.
#[cfg(windows)]
fn quote_arg(arg: &str, out: &mut String) {
    // An empty argument still needs quotes, or it disappears entirely rather
    // than arriving as an empty string.
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        out.push_str(arg);
        return;
    }
    out.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => {
                backslashes += 1;
                out.push(c);
            }
            // The run before a quote is doubled and the quote escaped: one extra
            // backslash per backslash already written, plus one for the quote.
            '"' => {
                for _ in 0..=backslashes {
                    out.push('\\');
                }
                out.push('"');
                backslashes = 0;
            }
            _ => {
                backslashes = 0;
                out.push(c);
            }
        }
    }
    // And the run before the *closing* quote, for the same reason.
    for _ in 0..backslashes {
        out.push('\\');
    }
    out.push('"');
}

/// The value following a flag.
fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::{
        doc_len_arg, library_dir_arg, read_reply_line, value_of, ReplyError, Request, Response, Shm,
    };

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_flag_reads_the_value_after_it() {
        let a = args(&["--render-worker", "--doc-len", "42", "--lib", "/x"]);
        assert_eq!(value_of(&a, "--doc-len"), Some("42"));
        assert_eq!(doc_len_arg(&a), Some(42));
        assert_eq!(
            library_dir_arg(&a).as_deref(),
            Some(std::path::Path::new("/x"))
        );
    }

    #[test]
    fn a_flag_with_nothing_after_it_is_absent_rather_than_panicking() {
        // The worker parses its own argv after `exec`, where a panic is a
        // process that dies before it can say why.
        let a = args(&["--render-worker", "--doc-len"]);
        assert_eq!(value_of(&a, "--doc-len"), None);
        assert_eq!(doc_len_arg(&a), None);
    }

    #[test]
    fn a_flag_that_is_not_there_is_absent() {
        let a = args(&["--render-worker"]);
        assert_eq!(doc_len_arg(&a), None);
        assert_eq!(library_dir_arg(&a), None);
        // And a value is not mistaken for a flag: `--lib` as the *value* of
        // `--doc-len` must not then satisfy a lookup for `--lib`.
        let confusing = args(&["--doc-len", "--lib"]);
        assert_eq!(value_of(&confusing, "--doc-len"), Some("--lib"));
        assert_eq!(doc_len_arg(&confusing), None);
    }

    /// A handle survives argv, including one that does not fit an `i32`.
    ///
    /// The value that matters is the large one. A handle is pointer-sized, and
    /// parsing it into anything narrower is the defect this is aimed at --- it
    /// would not fail, it would produce a *different* handle, and mapping a
    /// wrong-but-valid handle is a far worse outcome than mapping none. The two
    /// flags are also checked not to answer each other's lookups, since they
    /// differ by one word and are passed adjacently.
    #[cfg(windows)]
    #[test]
    fn a_section_handle_survives_argv_at_full_width() {
        use super::{doc_handle_arg, tile_handle_arg};

        let wide = u32::MAX as usize + 4096;
        let a = args(&[
            "--doc-handle",
            &wide.to_string(),
            "--tile-handle",
            "512",
            "--lib",
            "C:\\lib",
        ]);
        assert_eq!(doc_handle_arg(&a), Some(wide));
        assert_eq!(tile_handle_arg(&a), Some(512));

        let neither = args(&["--render-worker"]);
        assert_eq!(doc_handle_arg(&neither), None);
        assert_eq!(tile_handle_arg(&neither), None);

        // A handle is unsigned: a negative value is a parse failure, not a
        // wraparound into a plausible one.
        let negative = args(&["--doc-handle", "-1"]);
        assert_eq!(doc_handle_arg(&negative), None);
    }

    #[test]
    fn a_request_survives_the_wire() {
        // The two sides are separate processes, so a field that fails to
        // round-trip fails at runtime and nowhere else.
        for request in [
            Request::Open {
                lazy_geometry: true,
            },
            Request::Tile {
                rid: 7,
                page: 3,
                scale: 1.5,
                turns: 2,
                invert: true,
                x: -4,
                y: 9,
                width: 1024,
                height: 768,
                png: false,
            },
            Request::Withdraw { rid: 7 },
            Request::Text { page: 0 },
            Request::Search {
                page: 1,
                query: "quartz".into(),
                options: crate::search::Options {
                    match_case: true,
                    whole_word: true,
                },
            },
            Request::Outline,
        ] {
            let line = serde_json::to_string(&request).expect("serialise");
            let back: Request = serde_json::from_str(&line).expect("deserialise");
            assert_eq!(
                format!("{request:?}"),
                format!("{back:?}"),
                "round trip changed {line}"
            );
        }
    }

    #[test]
    fn a_reply_distinguishes_abandoned_from_failed_and_from_empty() {
        // Three states a caller must tell apart: a tile that was withdrawn has
        // nothing to draw, and painting it as blank would erase what was there.
        let abandoned = Response {
            ok: true,
            abandoned: true,
            ..Default::default()
        };
        let empty = Response {
            ok: true,
            bytes: 0,
            ..Default::default()
        };
        let failed = Response::err("no such page");

        assert!(abandoned.ok && abandoned.abandoned);
        assert!(empty.ok && !empty.abandoned);
        assert!(!failed.ok && !failed.abandoned && !failed.error.is_empty());
    }

    #[test]
    fn an_ordinary_reply_is_read_whole() {
        // The control. Without it every assertion below is satisfied by a reader
        // that refuses everything, which is the shape a length bound fails in.
        let mut input = std::io::Cursor::new(b"{\"ok\":true}\n{\"ok\":false}\n".to_vec());
        let first = read_reply_line(&mut input, 64).expect("first line");
        assert_eq!(first, "{\"ok\":true}\n");
        // And the reader is left on the boundary, not somewhere inside the next
        // line: a bounded read that consumed too much would desynchronise the
        // stream and every later reply would be garbage.
        let second = read_reply_line(&mut input, 64).expect("second line");
        assert_eq!(second, "{\"ok\":false}\n");
    }

    #[test]
    fn a_reply_that_fills_the_limit_exactly_is_still_read() {
        // The boundary, from the permitted side. `take` counts the newline, so
        // an off-by-one here rejects the largest legitimate reply --- which
        // would only ever be discovered on a document big enough to produce one.
        let line = b"12345678\n";
        let mut input = std::io::Cursor::new(line.to_vec());
        assert_eq!(
            read_reply_line(&mut input, line.len() as u64).expect("exact fit"),
            "12345678\n"
        );
    }

    #[test]
    fn a_reply_longer_than_the_limit_is_refused_rather_than_truncated() {
        // A *complete* line that is merely too long, not a truncated one: with
        // no newline in it, an unbounded read would run out of input, return
        // without a newline, and be refused for that reason instead --- so the
        // first version of this test passed with the bound deleted.
        let mut line = vec![b'x'; 4096];
        line.push(b'\n');
        let mut input = std::io::Cursor::new(line);

        assert!(matches!(
            read_reply_line(&mut input, 64),
            Err(ReplyError::TooLong(64))
        ));
        // And the property the bound exists for, which no verdict can express:
        // that it stopped reading. The point of a limit is the memory never
        // allocated, so what has to be asserted is the input still waiting.
        assert_eq!(input.position(), 64, "the read was not bounded");
    }

    #[test]
    fn a_pipe_that_ends_mid_reply_is_a_dead_worker_and_not_an_oversized_one() {
        // The two ways a read ends without a newline, and they are different
        // diagnoses: "longer than 32 MB" sends the reader to look for a size
        // limit when what happened is a crash the epitaph can name.
        let mut input = std::io::Cursor::new(b"{\"ok\":tr".to_vec());
        assert!(matches!(
            read_reply_line(&mut input, 64),
            Err(ReplyError::Closed)
        ));
        // And an empty stream is the same answer, reached without reading
        // anything at all.
        let mut empty = std::io::Cursor::new(Vec::new());
        assert!(matches!(
            read_reply_line(&mut empty, 64),
            Err(ReplyError::Closed)
        ));
    }

    /// The layout that provokes it, and it is an ordinary one: the parent's own
    /// mapping files land low, so a hole below the tile's descriptor is exactly
    /// what a process that has opened and closed a file has. With the document
    /// on fd 3, the tile on fd 5 and fd 4 free, `dup` of the document returns 4
    /// --- which is `TILE_FD`, and by the time the cleanup runs it holds the
    /// tile.
    ///
    /// The failure this pins is silent on the parent's side: the child comes up
    /// with a closed descriptor where its tile mapping should be, and says so
    /// as a mapping error rather than as a fork one.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_temporary_that_landed_on_another_installed_number_is_not_closed() {
        let shuffle = [(4, super::DOC_FD), (6, super::TILE_FD)];
        assert!(
            !super::is_scratch(4, &shuffle),
            "fd 4 is TILE_FD and holds the tile mapping by now"
        );
        // And the one that really is a temporary still goes, or the check has
        // been satisfied by refusing to close anything at all.
        assert!(super::is_scratch(6, &shuffle));
    }

    /// The control: the common layout, where both temporaries land above every
    /// number the shuffle installs on and both must be closed.
    #[cfg(target_os = "macos")]
    #[test]
    fn temporaries_above_every_installed_number_are_all_closed() {
        let shuffle = [(7, super::DOC_FD), (8, super::TILE_FD)];
        assert!(super::is_scratch(7, &shuffle));
        assert!(super::is_scratch(8, &shuffle));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_temporary_that_is_already_its_own_target_is_not_closed() {
        // `dup2(n, n)` is a no-op that returns `n`, so the "temporary" and the
        // installed descriptor are the same open file --- closing it would take
        // the mapping with it.
        let shuffle = [(super::DOC_FD, super::DOC_FD), (9, super::TILE_FD)];
        assert!(!super::is_scratch(super::DOC_FD, &shuffle));
        assert!(super::is_scratch(9, &shuffle));
    }

    /// The pre-spawn shuffle installs on different numbers, and the same trap
    /// reaches it: a tile temporary landing on `SOCK_FD` would close the
    /// handover socket, and a spare that never receives a document is not an
    /// error --- it is a process waiting in `recvmsg` for the rest of its life.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_prespawn_shuffle_protects_the_handover_socket_too() {
        let shuffle = [(super::SOCK_FD, super::TILE_FD), (7, super::SOCK_FD)];
        assert!(!super::is_scratch(super::SOCK_FD, &shuffle));
        assert!(super::is_scratch(7, &shuffle));
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

    /// The command line is read back by the parser Windows itself uses.
    ///
    /// `CommandLineToArgvW` rather than a table of expected strings, because a
    /// table would only restate the algorithm above and agree with it about
    /// output that is wrong --- `AGENTS.md` records exactly that failure, where
    /// every check on a generated file went through the library that wrote it.
    /// This is the *consumer's* parser: the same rules the child's own
    /// `std::env::args` implements.
    ///
    /// The awkward argument is the real one. `--lib C:\Program Files\tpdf\` has
    /// a space *and* a trailing separator, so a quoter that handles spaces but
    /// not the backslash run escapes its own closing quote, and the library path
    /// silently swallows the flag that follows it.
    #[cfg(windows)]
    #[test]
    fn a_command_line_survives_the_parser_windows_actually_uses() {
        let parts = [
            r"C:\Program Files\tpdf\tpdf.exe",
            super::WORKER_ARGV,
            "--doc-len",
            "4096",
            "--lib",
            r"C:\Program Files\tpdf\",
            super::DOC_HANDLE_ARGV,
            "312",
        ];
        assert_eq!(parse_command_line(&super::command_line(&parts)), parts);
    }

    /// The control, and it is not optional: it shows the oracle can fail.
    ///
    /// A round trip through a *lenient* parser would pass on any joining rule at
    /// all, and the check above would then be decoration. Joining the same parts
    /// with plain spaces must therefore come back wrong --- which is the naive
    /// implementation, so this also names what the quoting is for.
    #[cfg(windows)]
    #[test]
    fn a_command_line_joined_naively_does_not_survive_it() {
        let parts = [
            r"C:\Program Files\tpdf\tpdf.exe",
            "--lib",
            r"C:\Program Files\tpdf\",
        ];
        let naive = parts.join(" ");
        assert_ne!(parse_command_line(&naive), parts);
    }

    /// Splits a command line the way the child will, using Win32 itself.
    #[cfg(windows)]
    fn parse_command_line(line: &str) -> Vec<String> {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};

        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

        let wide: Vec<u16> = std::ffi::OsStr::new(line)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut count: i32 = 0;
        // SAFETY: `wide` is NUL-terminated and outlives the call; `count` is
        // written by it. The returned array is owned by us until `LocalFree`.
        let argv = unsafe { CommandLineToArgvW(wide.as_ptr(), &raw mut count) };
        assert!(!argv.is_null(), "CommandLineToArgvW rejected {line:?}");
        let mut out = Vec::new();
        for i in 0..count as isize {
            // SAFETY: `i` is below the count the call reported, and each entry
            // is a NUL-terminated wide string it allocated.
            unsafe {
                let arg = *argv.offset(i);
                let len = (0..).take_while(|n| *arg.offset(*n) != 0).count();
                out.push(
                    std::ffi::OsString::from_wide(std::slice::from_raw_parts(arg, len))
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        // SAFETY: the one allocation the call made, freed once.
        unsafe { LocalFree(argv.cast()) };
        out
    }

    /// A worker whose child dies is *reported* dead, not waited on forever.
    ///
    /// Under `cargo test` this is a real spawn with a fake worker: `current_exe`
    /// is the test harness, which has no `--render-worker` dispatch, so the child
    /// exits immediately. That is the point --- what is under test here is the
    /// plumbing, not the protocol, and a child that dies at once exercises it
    /// harder than one that answers.
    ///
    /// The specific defect it pins is the parent keeping its copy of the reply
    /// pipe's *write* end. That pipe then never reaches end of file, so a dead
    /// worker is indistinguishable from a slow one and `read_reply` blocks for
    /// the life of the process. Which is why the read happens on another thread
    /// behind a `recv_timeout`: `AGENTS.md` records that a test whose failure is
    /// a hang reports a pass and a timeout in the same breath, and the whole
    /// value of this check is that the failure it looks for *is* a hang.
    ///
    /// The reader thread is deliberately not joined --- on failure it is still
    /// blocked in `read`, and joining it would reintroduce the hang this exists
    /// to convert into a verdict. Process exit collects it.
    #[cfg(windows)]
    #[test]
    fn a_worker_whose_child_dies_says_so_rather_than_blocking() {
        let path = std::env::temp_dir().join(format!("tpdf-plumbing-{}", std::process::id()));
        std::fs::write(&path, b"%PDF-1.7 not really").expect("write fixture");
        let spawned = super::Worker::spawn(&path, std::path::Path::new("."));
        let _ = std::fs::remove_file(&path);
        // `map_err` first: a live child is not something to format into a panic.
        let mut worker = match spawned {
            Ok(w) => w,
            Err(e) => panic!("Worker::spawn must start a contained child: {e}"),
        };

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let answer = worker.call(&Request::Open {
                lazy_geometry: false,
            });
            let _ = tx.send(answer.map(|_| ()));
        });
        match rx.recv_timeout(std::time::Duration::from_secs(20)) {
            Ok(Ok(())) => panic!("a harness that is not a worker cannot have answered"),
            Ok(Err(e)) => assert!(
                // The epitaph, not merely "the pipe closed": a parent that
                // cannot name how its worker died is the failure this whole
                // module's error handling exists to avoid.
                e.contains("exited with"),
                "the failure must name how the child ended: {e}"
            ),
            Err(_) => panic!("the parent never noticed its child had exited"),
        }
    }

    /// The exported [`PRESPAWNS`] and what `prespawn` actually does agree.
    ///
    /// This replaced a test asserting that `prespawn` refuses on Windows, and
    /// that replacement is itself the evidence the behaviour changed: the old
    /// test went red on its own when the handover landed, which is the strongest
    /// verdict a removed assertion can get.
    ///
    /// What it pins is the thing this repository has now paid for twice --- a
    /// platform fact that callers branch on, restated somewhere else, drifting.
    /// `backend-probe` held its own copy of this predicate for a day; the copies
    /// disagreed, and the wait that followed read as a defect in the pool. A
    /// constant nothing compares against the behaviour it describes is a comment.
    ///
    /// It asserts the **spawn**, not the worker, and that limit is the platform's
    /// rather than a shortcut: under `cargo test` the child is the libtest
    /// harness, which never dispatches `--render-worker`, so it cannot warm or
    /// answer --- the same reason `a harness that is not a worker cannot have
    /// answered` exists above. That a pre-spawned worker warms *inside* its
    /// containment is `backend-probe`'s to say, where the child is a real one.
    /// `prespawn` waits for nothing, so "a process was started" is exactly what
    /// it promises here. The child is killed by `PreWorker`'s drop.
    #[test]
    fn the_prespawn_constant_matches_what_prespawn_does() {
        let started = super::Worker::prespawn(std::path::Path::new(".")).is_ok();
        assert_eq!(
            started,
            super::PRESPAWNS,
            "PRESPAWNS says {}, Worker::prespawn {} --- a caller branching on the \
             constant would do the wrong thing",
            super::PRESPAWNS,
            if started { "started one" } else { "refused" }
        );
    }

    /// And the entry point callers actually reach refuses too.
    ///
    /// Separate from the mapping check because they are different claims: this
    /// one would still have to hold if `Shm` ever grew a Windows implementation,
    /// which is exactly the change that would make the refusal above stop firing
    /// without anyone noticing.
    ///
    /// **It is an end-to-end assertion and pins no single guard**, which is worth
    /// stating because it looks like it pins one. `spawn` chains `map_file`,
    /// `create` and `spawn_mapped`, and all three refuse with the same sentence,
    /// so removing any *one* of them leaves this green --- measured, not assumed:
    /// a mutation making `map_file` succeed changed nothing here. That is the
    /// "an outcome two mechanisms can produce cannot test either one" shape from
    /// `AGENTS.md`, and it is deliberate rather than overlooked: what this check
    /// is for is the property that a worker cannot start, which is exactly the
    /// thing that survives one guard going away. The guard-level claim is the
    /// mapping test above, and that one does go red.
    ///
    /// Gated on `not(unix)` rather than `not(target_os = "macos")` because on a
    /// unix that is not macOS the mapping succeeds and `spawn` refuses one layer
    /// further down with a different message --- a real difference, not a
    /// portability detail, and asserting the wrong one here would make this pass
    /// for the wrong reason.
    ///
    /// **It used to be handed `"nonexistent.pdf"`, and that stopped being valid
    /// the moment `Shm` grew a Windows implementation** --- which the note above
    /// predicted and the fixture did not survive. With every constructor
    /// refusing, any path reached the same sentence; with `map_file` real, a
    /// missing file fails at the *first* step with "could not open", so the check
    /// would have gone red for a reason that has nothing to do with containment.
    /// A real file is written so the call reaches `spawn_mapped`, which is the
    /// guard this is actually about, and the assertion now means what it says.
    ///
    /// **No longer reachable on Windows**, where `spawn_mapped` is implemented
    /// and the check above spawns a real contained child instead. Left in place
    /// rather than deleted, because the claim is still true of a platform with
    /// no boundary and this is where it is written down.
    #[cfg(all(not(unix), not(windows)))]
    #[test]
    fn spawning_a_worker_refuses_off_macos() {
        let path = std::env::temp_dir().join(format!("tpdf-spawn-test-{}", std::process::id()));
        std::fs::write(&path, b"%PDF-1.7 not really").expect("write fixture");
        // `map(|_| ())` because `expect_err` wants `Debug` on the success type,
        // and neither `Shm` nor `Worker` has it --- a mapping and a live child
        // process are not things to format into a panic message.
        let result = super::Worker::spawn(&path, std::path::Path::new(".")).map(|_| ());
        let _ = std::fs::remove_file(&path);
        let err = result.expect_err("Worker::spawn must refuse off macOS");
        // A literal, not `super::NO_WORKERS`. Comparing against the constant the
        // code returns is a check deriving its expectation from the thing it
        // tests --- it compares a value to itself, and `AGENTS.md` records that
        // shape as one that cannot fail. Substring rather than equality so that
        // rewording the sentence does not break the check, while dropping the
        // platform reason from it does.
        assert!(err.contains("macOS"), "{err}");
    }
}
