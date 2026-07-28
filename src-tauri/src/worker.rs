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
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
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

/// Descriptor the document mapping is handed over on.
///
/// Fixed numbers because they must be agreed before `exec`, and there is no
/// channel at that point to negotiate on.
pub const DOC_FD: i32 = 3;
/// Descriptor the tile mapping is handed over on.
pub const TILE_FD: i32 = 4;

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
pub struct Shm {
    file: std::fs::File,
    ptr: *mut libc::c_void,
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

impl Drop for Shm {
    fn drop(&mut self) {
        // SAFETY: unmapping exactly what was mapped.
        unsafe { libc::munmap(self.ptr, self.len) };
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
    _doc: Arc<Shm>,
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
        Err("render workers are implemented on macOS only".into())
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
                if libc::dup2(d, DOC_FD) < 0 || libc::dup2(t, TILE_FD) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if d != DOC_FD {
                    libc::close(d);
                }
                if t != TILE_FD {
                    libc::close(t);
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
            _doc: doc,
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
        use std::os::unix::process::ExitStatusExt;
        match self.child.try_wait() {
            Ok(None) => "still running".into(),
            Ok(Some(status)) => match status.signal() {
                Some(signal) => format!("killed by signal {signal}"),
                None => format!("exited with code {}", status.code().unwrap_or(-1)),
            },
            Err(e) => format!("could not be waited on: {e}"),
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
}
