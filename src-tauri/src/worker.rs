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
#[cfg(not(target_os = "macos"))]
pub const NO_WORKERS: &str = "render workers are implemented on macOS only";

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

/// The argv marker that starts a worker with no document.
pub const PRESPAWN_ARGV: &str = "--prespawn";

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
    Search { page: u32, query: String },
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

/// Off unix, a mapping that is never constructed.
///
/// The private field is the whole of the type: no caller outside this module can
/// build one, and all three constructors refuse, so every `&self` method below
/// is unreachable.
///
/// It was an **uninhabited** `enum` first, to carry the impossibility in the type
/// the way `AGENTS.md` recommends and the way [`PreWorker`]/[`WarmWorker`] carry
/// the readiness handshake. That is worth recording as a dead end: [`Worker`]
/// holds a `Shm`, so an uninhabited mapping makes the *worker* uninhabited too,
/// and the compiler then correctly reports the pool's `retire_idle` loop in
/// `workers.rs` --- ordinary code, on a platform that never runs it --- as
/// unreachable. Under `-D warnings` that is fatal, and the only repairs are
/// `#[allow]`s scattered through production paths. The impossibility is real but
/// it propagates further than the module that declared it.
#[cfg(not(unix))]
pub struct Shm {
    _private: (),
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

/// The same surface off unix, so callers compile unchanged.
///
/// The constructors refuse, so no value exists and the accessors below cannot be
/// reached. They panic rather than returning an empty slice or a `-1` descriptor:
/// a plausible-looking zero value is the silent wrong answer this module refuses
/// everywhere else, and it would be indistinguishable from a real mapping of
/// nothing. No `Drop`: there is nothing to unmap.
#[cfg(not(unix))]
impl Shm {
    /// Always refuses --- see the type note.
    ///
    /// # Errors
    ///
    /// Always.
    pub fn create(_len: usize) -> Result<Self, String> {
        Err(NO_WORKERS.into())
    }

    /// Always refuses --- see the type note.
    ///
    /// # Errors
    ///
    /// Always.
    pub fn map_file(_path: &Path) -> Result<Self, String> {
        Err(NO_WORKERS.into())
    }

    /// Always refuses --- see the type note.
    ///
    /// # Errors
    ///
    /// Always.
    ///
    /// # Safety
    ///
    /// Nothing is dereferenced; the descriptor is ignored.
    pub unsafe fn from_fd(_fd: i32, _len: usize, _writable: bool) -> Result<Self, String> {
        Err(NO_WORKERS.into())
    }

    /// How many bytes the mapping covers.
    #[must_use]
    pub fn len(&self) -> usize {
        unreachable!("{NO_WORKERS}")
    }

    /// Whether the mapping is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        unreachable!("{NO_WORKERS}")
    }

    /// The mapping's descriptor, for handing to a child.
    #[must_use]
    pub fn raw_fd(&self) -> i32 {
        unreachable!("{NO_WORKERS}")
    }

    /// Reads the mapping.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        unreachable!("{NO_WORKERS}")
    }

    /// Writes the mapping.
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unreachable!("{NO_WORKERS}")
    }

    /// Reborrows the mapping for the process lifetime.
    ///
    /// # Safety
    ///
    /// Unreachable.
    #[must_use]
    pub unsafe fn as_static(&self) -> &'static [u8] {
        unreachable!("{NO_WORKERS}")
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
#[cfg(not(unix))]
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

/// A worker process serving one document.
pub struct Worker {
    child: Child,
    stdin: WorkerSender,
    stdout: BufReader<ChildStdout>,
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
pub struct WorkerSender(Arc<Mutex<ChildStdin>>);

impl WorkerSender {
    /// Writes one request line.
    ///
    /// # Errors
    ///
    /// The pipe being closed, i.e. the worker is gone. Reported without an
    /// epitaph, because reaping the child needs the [`Worker`] this was split
    /// from and a caller holding only the sender has no business waiting on it.
    pub fn send(&self, request: &Request) -> Result<(), String> {
        let mut line = serde_json::to_string(request).map_err(|e| e.to_string())?;
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
    /// Gated with the handover itself: off macOS nothing constructs a
    /// `PreWorker` and nothing reads this, so carrying the field there would be
    /// a descriptor that exists only to be warned about.
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

    /// Hands over the document --- refused off macOS.
    ///
    /// Unreachable in practice, since [`Worker::prespawn`] refuses first and is
    /// the only route to a `WarmWorker`. Present so the type's surface does not
    /// change by platform, and refusing rather than panicking so a future caller
    /// that finds another route is told rather than aborted.
    ///
    /// # Errors
    ///
    /// Always.
    #[cfg(not(target_os = "macos"))]
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
    /// platform but macOS, always --- see the module note.
    #[cfg(not(target_os = "macos"))]
    pub fn prespawn(_library_dir: &Path) -> Result<PreWorker, String> {
        Err(NO_WORKERS.into())
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
    #[cfg(not(target_os = "macos"))]
    pub fn spawn_mapped(_doc: Arc<Shm>, _tile: Shm, _library_dir: &Path) -> Result<Self, String> {
        // Not a silent fallback to running unsandboxed. Every containment claim
        // in docs/THREAT-MODEL.md is `sandbox_init` SBPL, so a worker without it
        // is a different thing wearing the same name.
        Err(NO_WORKERS.into())
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
        self.child.id()
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
        epitaph_of(&mut self.child)
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
        phys_footprint(self.child.id())
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

    /// Off unix, the refusal is the containment argument, so it is asserted
    /// rather than assumed.
    ///
    /// A constructor that quietly started succeeding here would hand the worker
    /// path a mapping on a platform with no `sandbox_init`, which is the single
    /// outcome this module exists to prevent --- and it would do it silently,
    /// because every caller already handles a `Result`.
    #[cfg(not(unix))]
    #[test]
    fn a_mapping_refuses_off_unix() {
        // `map(|_| ())` because `expect_err` wants `Debug` on the success type,
        // and neither `Shm` nor `Worker` has it --- a mapping and a live child
        // process are not things to format into a panic message.
        let err = Shm::create(4096)
            .map(|_| ())
            .expect_err("Shm::create must refuse off unix");
        // A literal, not `super::NO_WORKERS`. Comparing against the constant the
        // code returns is a check deriving its expectation from the thing it
        // tests --- it compares a value to itself, and `AGENTS.md` records that
        // shape as one that cannot fail. Substring rather than equality so that
        // rewording the sentence does not break the check, while dropping the
        // platform reason from it does.
        assert!(err.contains("macOS"), "{err}");
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
    #[cfg(not(unix))]
    #[test]
    fn spawning_a_worker_refuses_off_macos() {
        let err = super::Worker::spawn(
            std::path::Path::new("nonexistent.pdf"),
            std::path::Path::new("."),
        )
        .map(|_| ())
        .expect_err("Worker::spawn must refuse off macOS");
        assert!(err.contains("macOS"), "{err}");
    }
}
